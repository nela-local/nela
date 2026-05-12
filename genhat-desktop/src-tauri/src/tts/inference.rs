//! KittenTTS ONNX inference engine.
//!
//! Orchestrates the full TTS pipeline:
//!   text → preprocess → phonemize → tokenize → ONNX inference → audio
//!
//! The engine holds the loaded ONNX session, voice bank, and config.
//! Stored inside an `Arc<KittenTtsEngine>` for thread-safe concurrent inference:
//! ort 2.x `Session::run()` takes `&self` and is safe to call from multiple
//! threads simultaneously, so no `Mutex` is required.

use crate::tts::{
    phonemizer,
    preprocess::TextPreprocessor,
    text_cleaner::{basic_english_tokenize, TextCleaner},
    voice::{load_voices, VoiceBank},
    wav,
};
use ort::session::Session;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

// ─────────────────────────────────────────────────────────────────────────────
// Config (parsed from config.json in the model directory)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct KittenTtsConfig {
    #[allow(dead_code)]
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub model_type: Option<String>,
    pub model_file: String,
    pub voices: String,
    #[serde(default)]
    pub speed_priors: HashMap<String, f32>,
    #[serde(default)]
    pub voice_aliases: HashMap<String, String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Loaded engine (stored inside InMemoryHandle.model via Arc)
// ─────────────────────────────────────────────────────────────────────────────

/// The fully loaded KittenTTS inference engine.
///
/// `Session::run()` takes `&mut self`, so the session is wrapped in a
/// `Mutex` to allow concurrent access from an `Arc<KittenTtsEngine>`.
pub struct KittenTtsEngine {
    session: std::sync::Mutex<Session>,
    voice_bank: VoiceBank,
    text_cleaner: TextCleaner,
    preprocessor: TextPreprocessor,
    speed_priors: HashMap<String, f32>,
    voice_aliases: HashMap<String, String>,
    available_voices: Vec<String>,
}

impl KittenTtsEngine {
    /// Load the engine from a model directory containing config.json, .onnx model, and voices.npz.
    pub fn load(model_dir: &Path) -> Result<Self, String> {
        // 1. Parse config.json
        let config_path = model_dir.join("config.json");
        let config_text = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read KittenTTS config: {e}"))?;
        let config: KittenTtsConfig = serde_json::from_str(&config_text)
            .map_err(|e| format!("Failed to parse KittenTTS config: {e}"))?;

        log::info!(
            "[KittenTTS] Loading model '{}' from {}",
            config.name.as_deref().unwrap_or("unknown"),
            model_dir.display()
        );

        // 2. Load ONNX model
        let model_path = model_dir.join(&config.model_file);
        let session = Session::builder()
            .map_err(|e| format!("ORT session builder: {e}"))?
            .with_intra_threads(4)
            .map_err(|e| format!("ORT intra threads: {e}"))?
            .commit_from_file(&model_path)
            .map_err(|e| format!("ORT load KittenTTS model: {e}"))?;

        log::info!("[KittenTTS] ONNX session loaded from {}", model_path.display());

        // 3. Load voice embeddings
        let voices_path = model_dir.join(&config.voices);
        let voice_bank = load_voices(&voices_path)?;

        let available_voices: Vec<String> = voice_bank.keys().cloned().collect();
        log::info!("[KittenTTS] Loaded {} voices: {:?}", available_voices.len(), available_voices);

        // 4. Check espeak-ng availability
        if !phonemizer::is_available() {
            log::warn!("[KittenTTS] espeak-ng not found — TTS will fail at inference time");
        }

        Ok(Self {
            session: std::sync::Mutex::new(session),
            voice_bank,
            text_cleaner: TextCleaner::new(),
            preprocessor: TextPreprocessor::new(),
            speed_priors: config.speed_priors,
            voice_aliases: config.voice_aliases,
            available_voices,
        })
    }

    /// Get list of user-friendly voice names (aliases).
    pub fn voice_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.voice_aliases.keys().cloned().collect();
        names.sort();
        names
    }

    /// Generate speech from text and save as WAV file.
    ///
    /// Returns the absolute path to the generated WAV.
    pub fn generate_to_file(
        &self,
        text: &str,
        voice: &str,
        speed: f32,
        output_path: &Path,
    ) -> Result<String, String> {
        let audio = self.generate(text, voice, speed)?;
        wav::write_wav(output_path, &audio, wav::SAMPLE_RATE)?;

        log::info!(
            "[KittenTTS] Generated {:.1}s audio → {}",
            audio.len() as f32 / wav::SAMPLE_RATE as f32,
            output_path.display()
        );

        Ok(output_path.to_string_lossy().to_string())
    }

    /// Generate speech from text, returning raw f32 audio samples.
    pub fn generate(&self, text: &str, voice: &str, speed: f32) -> Result<Vec<f32>, String> {
        if text.trim().is_empty() {
            return Err("Empty text input".into());
        }

        // Preprocess text (expand numbers, currency, etc.)
        let cleaned = self.preprocessor.process(text);
        log::debug!("[KittenTTS] Preprocessed: '{}' → '{}'", truncate(text, 60), truncate(&cleaned, 80));

        // Chunk long text
        let chunks = chunk_text(&cleaned, 400);
        log::debug!("[KittenTTS] Split into {} chunk(s)", chunks.len());

        let mut all_audio = Vec::new();
        for chunk in &chunks {
            let chunk_audio = self.generate_single_chunk(chunk, voice, speed)?;
            all_audio.extend_from_slice(&chunk_audio);
        }

        Ok(all_audio)
    }

    /// Generate audio for a single text chunk (≤400 chars).
    fn generate_single_chunk(&self, text: &str, voice: &str, speed: f32) -> Result<Vec<f32>, String> {
        // Resolve voice alias
        let voice_id = self
            .voice_aliases
            .get(voice)
            .cloned()
            .unwrap_or_else(|| voice.to_string());

        let voice_emb = self
            .voice_bank
            .get(&voice_id)
            .ok_or_else(|| {
                format!(
                    "Voice '{}' not found. Available: {:?}",
                    voice, self.available_voices
                )
            })?;

        // Apply speed prior if configured
        let mut effective_speed = speed;
        if let Some(&prior) = self.speed_priors.get(&voice_id) {
            effective_speed *= prior;
        }

        // Phonemize
        let ipa = phonemizer::phonemize(text)?;
        if ipa.is_empty() {
            return Err(format!("Phonemization produced empty output for: '{}'", truncate(text, 40)));
        }

        // Tokenize: basic_english_tokenize → join → TextCleaner encode
        let tokenized = basic_english_tokenize(&ipa);
        let mut tokens = self.text_cleaner.encode(&tokenized);

        // Add start (0) and end (10, 0) tokens
        tokens.insert(0, self.text_cleaner.pad_id());
        tokens.push(self.text_cleaner.end_id());
        tokens.push(self.text_cleaner.pad_id());

        let seq_len = tokens.len();

        // Select voice style reference
        let style = voice_emb.select_style(text.len());
        let style_dim = voice_emb.embed_dim;

        log::debug!(
            "[KittenTTS] Chunk: {} tokens, style [{} × 1], speed={:.2}",
            seq_len, style_dim, effective_speed
        );

        // Build ONNX input tensors
        let input_ids_data: Vec<i64> = tokens;
        let input_ids = ort::value::Value::from_array(([1usize, seq_len], input_ids_data))
            .map_err(|e| format!("ORT input_ids: {e}"))?;

        let style_value = ort::value::Value::from_array(([1usize, style_dim], style))
            .map_err(|e| format!("ORT style: {e}"))?;

        let speed_value = ort::value::Value::from_array(([1usize], vec![effective_speed]))
            .map_err(|e| format!("ORT speed: {e}"))?;

        // Run inference — lock the session mutex for the duration of this call.
        let mut session_guard = self.session.lock().unwrap();
        let outputs = session_guard
            .run(ort::inputs![
                "input_ids" => input_ids,
                "style" => style_value,
                "speed" => speed_value,
            ])
            .map_err(|e| format!("ORT inference failed: {e}"))?;

        // Extract audio output — outputs[0] is the waveform tensor
        let output_keys: Vec<String> = outputs.keys().map(|k| k.to_string()).collect();
        let audio_value = outputs
            .iter()
            .next()
            .ok_or_else(|| format!("No output from KittenTTS model. Keys: {output_keys:?}"))?
            .1;

        let audio_tensor = audio_value
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Extract audio tensor: {e}"))?;

        let mut audio: Vec<f32> = audio_tensor.1.to_vec();

        // Trim last 5000 samples (padding artifact, same as Python: output[..., :-5000])
        let trim = 5000.min(audio.len());
        audio.truncate(audio.len() - trim);

        Ok(audio)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Text chunking (mirrors Python chunk_text)
// ─────────────────────────────────────────────────────────────────────────────

/// Ensure text ends with sentence punctuation.
fn ensure_punctuation(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return text.to_string();
    }
    if text.ends_with(|c: char| ".!?,;:".contains(c)) {
        text.to_string()
    } else {
        format!("{text},")
    }
}

/// Split text into chunks of at most `max_len` characters.
/// Splits on sentence boundaries first, then on words.
fn chunk_text(text: &str, max_len: usize) -> Vec<String> {
    let sentences: Vec<&str> = text.split(|c: char| ".!?".contains(c)).collect();
    let mut chunks = Vec::new();

    for sentence in sentences {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            continue;
        }

        if sentence.len() <= max_len {
            chunks.push(ensure_punctuation(sentence));
        } else {
            // Split long sentences by words
            let words: Vec<&str> = sentence.split_whitespace().collect();
            let mut temp = String::new();
            for word in words {
                if temp.len() + word.len() + 1 <= max_len {
                    if !temp.is_empty() {
                        temp.push(' ');
                    }
                    temp.push_str(word);
                } else {
                    if !temp.is_empty() {
                        chunks.push(ensure_punctuation(&temp));
                    }
                    temp = word.to_string();
                }
            }
            if !temp.is_empty() {
                chunks.push(ensure_punctuation(&temp));
            }
        }
    }

    if chunks.is_empty() && !text.trim().is_empty() {
        // Fallback: return the whole text as one chunk
        chunks.push(ensure_punctuation(text));
    }

    chunks
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_text_short() {
        let chunks = chunk_text("Hello world.", 400);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_chunk_text_multi_sentence() {
        let text = "First sentence. Second sentence. Third sentence.";
        let chunks = chunk_text(text, 400);
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn test_ensure_punctuation() {
        assert_eq!(ensure_punctuation("hello"), "hello,");
        assert_eq!(ensure_punctuation("hello."), "hello.");
        assert_eq!(ensure_punctuation("hello!"), "hello!");
    }
}
