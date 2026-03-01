//! Parakeet TDT ONNX inference engine.
//!
//! Implements the full NeMo Parakeet TDT (Token-and-Duration Transducer)
//! speech-recognition pipeline in pure Rust:
//!
//!   audio file → load/resample 16 kHz mono → pre-emphasis → dither
//!   → 128-band log-mel spectrogram → encoder ONNX → TDT greedy decode
//!   (decoder + joiner in a loop) → text
//!
//! The model consists of three ONNX files:
//!   - `encoder.int8.onnx` — FastConformer audio encoder
//!   - `decoder.int8.onnx` — Prediction network (LSTM)
//!   - `joiner.int8.onnx`  — Joint network (token + duration logits)
//!
//! Designed to be stored inside an `InMemoryHandle` by the Parakeet backend.

use ort::session::Session;
use realfft::num_complex::Complex;
use realfft::RealFftPlanner;
use serde::Deserialize;
use std::path::Path;
use std::sync::Mutex;

// ═══════════════════════════════════════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════════════════════════════════════

/// Preprocessing + model configuration.
///
/// If `config.json` exists in the model directory it is loaded;
/// otherwise defaults matching NeMo Parakeet TDT 0.6B are used.
#[derive(Debug, Clone, Deserialize)]
pub struct ParakeetConfig {
    /// Encoder ONNX filename.
    #[serde(default = "defaults::encoder_file")]
    pub encoder_file: String,

    /// Decoder ONNX filename.
    #[serde(default = "defaults::decoder_file")]
    pub decoder_file: String,

    /// Joiner ONNX filename.
    #[serde(default = "defaults::joiner_file")]
    pub joiner_file: String,

    /// Vocabulary filename (space-separated `token id` per line, or JSON array).
    #[serde(default = "defaults::vocab_file")]
    pub vocab_file: String,

    /// Target sample rate in Hz (audio will be resampled to this).
    #[serde(default = "defaults::sample_rate")]
    pub sample_rate: u32,

    /// FFT size.
    #[serde(default = "defaults::n_fft")]
    pub n_fft: usize,

    /// Hop length in samples (10 ms at 16 kHz = 160).
    #[serde(default = "defaults::hop_length")]
    pub hop_length: usize,

    /// Analysis window length in samples (25 ms at 16 kHz = 400).
    #[serde(default = "defaults::win_length")]
    pub win_length: usize,

    /// Number of mel filter-bank bands (Parakeet TDT uses 128).
    #[serde(default = "defaults::n_mels")]
    pub n_mels: usize,

    /// Minimum frequency for the mel filterbank (Hz).
    #[serde(default)]
    pub fmin: f32,

    /// Maximum frequency for the mel filterbank (Hz). 0 = sr/2.
    #[serde(default)]
    pub fmax: f32,

    /// Token index used as the TDT blank. Default: 8192 for Parakeet.
    #[serde(default = "defaults::blank_id")]
    pub blank_id: usize,

    /// Total vocabulary size (including blank). Default: 8193.
    #[serde(default = "defaults::vocab_size")]
    pub vocab_size: usize,

    /// Number of TDT duration bins. Default: 5 (durations 0–4).
    #[serde(default = "defaults::num_durations")]
    pub num_durations: usize,

    /// The duration values corresponding to each duration bin index.
    #[serde(default = "defaults::durations")]
    pub durations: Vec<usize>,

    /// FastConformer encoder output dimension.
    #[serde(default = "defaults::encoder_dim")]
    pub encoder_dim: usize,

    /// LSTM decoder / prediction-network hidden dimension.
    #[serde(default = "defaults::decoder_dim")]
    pub decoder_dim: usize,

    /// Pre-emphasis coefficient (0 to disable).
    #[serde(default = "defaults::preemphasis")]
    pub preemphasis: f32,

    /// Dither magnitude added before feature extraction (0 to disable).
    #[serde(default = "defaults::dither")]
    pub dither: f32,

    /// Max symbols emitted per encoder frame (prevents infinite loop).
    #[serde(default = "defaults::max_symbols_per_step")]
    pub max_symbols_per_step: usize,
}

mod defaults {
    pub fn encoder_file() -> String {
        "encoder.int8.onnx".into()
    }
    pub fn decoder_file() -> String {
        "decoder.int8.onnx".into()
    }
    pub fn joiner_file() -> String {
        "joiner.int8.onnx".into()
    }
    pub fn vocab_file() -> String {
        "tokens.txt".into()
    }
    pub fn sample_rate() -> u32 {
        16_000
    }
    pub fn n_fft() -> usize {
        512
    }
    pub fn hop_length() -> usize {
        160
    }
    pub fn win_length() -> usize {
        400
    }
    pub fn n_mels() -> usize {
        128
    }
    pub fn blank_id() -> usize {
        8192
    }
    pub fn vocab_size() -> usize {
        8193
    }
    pub fn num_durations() -> usize {
        5
    }
    pub fn durations() -> Vec<usize> {
        vec![0, 1, 2, 3, 4]
    }
    pub fn encoder_dim() -> usize {
        1024
    }
    pub fn decoder_dim() -> usize {
        640
    }
    pub fn preemphasis() -> f32 {
        0.97
    }
    pub fn dither() -> f32 {
        1e-5
    }
    pub fn max_symbols_per_step() -> usize {
        10
    }
}

impl Default for ParakeetConfig {
    fn default() -> Self {
        Self {
            encoder_file: defaults::encoder_file(),
            decoder_file: defaults::decoder_file(),
            joiner_file: defaults::joiner_file(),
            vocab_file: defaults::vocab_file(),
            sample_rate: defaults::sample_rate(),
            n_fft: defaults::n_fft(),
            hop_length: defaults::hop_length(),
            win_length: defaults::win_length(),
            n_mels: defaults::n_mels(),
            fmin: 0.0,
            fmax: 0.0, // 0 → sr / 2
            blank_id: defaults::blank_id(),
            vocab_size: defaults::vocab_size(),
            num_durations: defaults::num_durations(),
            durations: defaults::durations(),
            encoder_dim: defaults::encoder_dim(),
            decoder_dim: defaults::decoder_dim(),
            preemphasis: defaults::preemphasis(),
            dither: defaults::dither(),
            max_symbols_per_step: defaults::max_symbols_per_step(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Engine
// ═══════════════════════════════════════════════════════════════════════════════

/// The fully-loaded Parakeet TDT inference engine.
///
/// Thread-safe: each ONNX `Session` is behind a `Mutex` so multiple
/// requests can safely share the same `Arc<ParakeetEngine>`.
pub struct ParakeetEngine {
    encoder: Mutex<Session>,
    decoder: Mutex<Session>,
    joiner: Mutex<Session>,
    vocab: Vec<String>,
    config: ParakeetConfig,
    /// Pre-computed mel filterbank matrix [n_mels][n_fft/2+1].
    mel_filterbank: Vec<Vec<f32>>,
    /// Pre-computed Hann window [win_length].
    hann_window: Vec<f32>,
}

impl std::fmt::Debug for ParakeetEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParakeetEngine")
            .field("vocab_size", &self.vocab.len())
            .field("config", &self.config)
            .finish()
    }
}

impl ParakeetEngine {
    /// Load the Parakeet TDT model (3 ONNX files), vocabulary, and
    /// pre-compute DSP tables.
    pub fn load(model_dir: &Path) -> Result<Self, String> {
        // ── 1. Configuration ──────────────────────────────────────────────
        let config_path = model_dir.join("config.json");
        let config: ParakeetConfig = if config_path.exists() {
            let text = std::fs::read_to_string(&config_path)
                .map_err(|e| format!("Failed to read config.json: {e}"))?;
            serde_json::from_str(&text)
                .map_err(|e| format!("Failed to parse config.json: {e}"))?
        } else {
            log::info!("[Parakeet] No config.json found, using defaults for TDT 0.6B");
            ParakeetConfig::default()
        };

        // ── 2. Load 3 ONNX sessions ──────────────────────────────────────
        let encoder_path = model_dir.join(&config.encoder_file);
        let decoder_path = model_dir.join(&config.decoder_file);
        let joiner_path = model_dir.join(&config.joiner_file);

        for (name, path) in [
            ("Encoder", &encoder_path),
            ("Decoder", &decoder_path),
            ("Joiner", &joiner_path),
        ] {
            if !path.exists() {
                return Err(format!(
                    "{name} model not found: {}",
                    path.display()
                ));
            }
        }

        log::info!("[Parakeet] Loading encoder: {}", encoder_path.display());
        let encoder = Session::builder()
            .map_err(|e| format!("ORT session builder (encoder): {e}"))?
            .with_intra_threads(4)
            .map_err(|e| format!("ORT intra threads (encoder): {e}"))?
            .commit_from_file(&encoder_path)
            .map_err(|e| format!("ORT load encoder: {e}"))?;

        log::info!("[Parakeet] Loading decoder: {}", decoder_path.display());
        let decoder = Session::builder()
            .map_err(|e| format!("ORT session builder (decoder): {e}"))?
            .with_intra_threads(2)
            .map_err(|e| format!("ORT intra threads (decoder): {e}"))?
            .commit_from_file(&decoder_path)
            .map_err(|e| format!("ORT load decoder: {e}"))?;

        log::info!("[Parakeet] Loading joiner: {}", joiner_path.display());
        let joiner = Session::builder()
            .map_err(|e| format!("ORT session builder (joiner): {e}"))?
            .with_intra_threads(2)
            .map_err(|e| format!("ORT intra threads (joiner): {e}"))?
            .commit_from_file(&joiner_path)
            .map_err(|e| format!("ORT load joiner: {e}"))?;

        // ── 3. Vocabulary ─────────────────────────────────────────────────
        let vocab = load_vocab(model_dir, &config.vocab_file)?;
        log::info!("[Parakeet] Vocabulary loaded: {} tokens", vocab.len());

        // ── 4. Pre-compute DSP tables ─────────────────────────────────────
        let effective_fmax = if config.fmax <= 0.0 {
            config.sample_rate as f32 / 2.0
        } else {
            config.fmax
        };
        let mel_filterbank = compute_mel_filterbank(
            config.n_fft,
            config.n_mels,
            config.sample_rate,
            config.fmin,
            effective_fmax,
        );
        let hann_window = hann(config.win_length);

        log::info!(
            "[Parakeet] Engine ready — vocab={}, n_mels={}, sr={}, blank_id={}, durations={:?}",
            vocab.len(),
            config.n_mels,
            config.sample_rate,
            config.blank_id,
            config.durations,
        );

        Ok(Self {
            encoder: Mutex::new(encoder),
            decoder: Mutex::new(decoder),
            joiner: Mutex::new(joiner),
            vocab,
            config,
            mel_filterbank,
            hann_window,
        })
    }

    /// Transcribe an audio file to text.
    ///
    /// Accepts WAV natively; other formats are converted via `ffmpeg`.
    pub fn transcribe(&self, audio_path: &Path) -> Result<String, String> {
        let cfg = &self.config;

        // ── 1. Load & resample audio ──
        let samples = load_audio(audio_path, cfg.sample_rate)?;
        if samples.is_empty() {
            return Ok(String::new());
        }
        let duration_secs = samples.len() as f64 / cfg.sample_rate as f64;
        log::info!(
            "[Parakeet] Loaded {} samples ({:.1}s) from {}",
            samples.len(),
            duration_secs,
            audio_path.display(),
        );

        // ── 2. Pre-emphasis ──
        let samples = apply_preemphasis(&samples, cfg.preemphasis);

        // ── 3. Dither ──
        let samples = if cfg.dither > 0.0 {
            apply_dither(&samples, cfg.dither)
        } else {
            samples
        };

        // ── 4. Mel spectrogram [n_mels, n_frames] ──
        let mut mel = self.compute_mel(&samples);
        let n_frames = if mel.is_empty() { 0 } else { mel[0].len() };
        if n_frames == 0 {
            return Ok(String::new());
        }

        // ── 4b. Per-feature normalization (NeMo normalize="per_feature") ──
        normalize_per_feature(&mut mel);

        log::info!("[Parakeet] Mel spectrogram: {}×{}", cfg.n_mels, n_frames);

        // ── 5. Encoder forward pass ──
        let (encoder_out, encoded_len) = self.run_encoder(&mel, n_frames)?;
        let encoded_frames = encoded_len as usize;
        log::info!("[Parakeet] Encoder output: {} frames", encoded_frames);

        // ── 6. TDT greedy decode ──
        let text = self.tdt_greedy_decode(&encoder_out, encoded_frames)?;

        log::info!(
            "[Parakeet] Decoded: \"{}\"",
            truncate(&text, 120),
        );

        Ok(text)
    }

    // ─────────────────────────────────────────────────────────────────────
    // Encoder
    // ─────────────────────────────────────────────────────────────────────

    /// Run the FastConformer encoder.
    ///
    /// Input  : mel spectrogram [1, n_mels, n_frames]
    /// Output : (encoded_flat [1024 × T_enc row-major], encoded_length)
    fn run_encoder(
        &self,
        mel: &[Vec<f32>],
        n_frames: usize,
    ) -> Result<(Vec<f32>, i64), String> {
        let n_mels = self.config.n_mels;

        // Flatten [n_mels][n_frames] → row-major [1, n_mels, n_frames]
        let mut mel_flat = Vec::with_capacity(n_mels * n_frames);
        for band in mel.iter().take(n_mels) {
            mel_flat.extend_from_slice(band);
        }

        let audio_value =
            ort::value::Value::from_array(([1usize, n_mels, n_frames], mel_flat))
                .map_err(|e| format!("ORT create encoder audio tensor: {e}"))?;

        let len_value =
            ort::value::Value::from_array(([1usize], vec![n_frames as i64]))
                .map_err(|e| format!("ORT create encoder length tensor: {e}"))?;

        let mut session = self
            .encoder
            .lock()
            .map_err(|e| format!("Encoder lock poisoned: {e}"))?;

        let outputs = session
            .run(ort::inputs![
                "audio_signal" => audio_value,
                "length" => len_value,
            ])
            .map_err(|e| format!("ORT encoder run: {e}"))?;

        // outputs  → shape [1, 1024, T_enc]  (row-major flat)
        let enc_val = outputs
            .get("outputs")
            .ok_or("Encoder missing 'outputs' tensor")?;
        let enc_tensor = enc_val
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Extract encoder outputs: {e}"))?;
        let enc_data: Vec<f32> = enc_tensor.1.to_vec();

        // encoded_lengths → [1]  i64
        let len_val = outputs
            .get("encoded_lengths")
            .ok_or("Encoder missing 'encoded_lengths' tensor")?;
        let len_tensor = len_val
            .try_extract_tensor::<i64>()
            .map_err(|e| format!("Extract encoded_lengths: {e}"))?;
        let encoded_len = len_tensor.1[0];

        Ok((enc_data, encoded_len))
    }

    // ─────────────────────────────────────────────────────────────────────
    // Decoder  (prediction network)
    // ─────────────────────────────────────────────────────────────────────

    /// Run the LSTM prediction network for a single token.
    ///
    /// Returns (decoder_out [decoder_dim], new_lstm_states [2 * decoder_dim]).
    fn run_decoder(
        &self,
        token: i32,
        lstm_state: &[f32],
        init_state: &[f32],
    ) -> Result<(Vec<f32>, Vec<f32>), String> {
        let dec_dim = self.config.decoder_dim;

        // targets: [1, 1] int32
        let targets =
            ort::value::Value::from_array(([1usize, 1usize], vec![token]))
                .map_err(|e| format!("ORT decoder targets: {e}"))?;

        // target_length: [1] int32
        let target_length =
            ort::value::Value::from_array(([1usize], vec![1i32]))
                .map_err(|e| format!("ORT decoder target_length: {e}"))?;

        // states.1: [2, 1, decoder_dim] float  (LSTM h + c)
        let states_val =
            ort::value::Value::from_array(([2usize, 1usize, dec_dim], lstm_state.to_vec()))
                .map_err(|e| format!("ORT decoder states: {e}"))?;

        // onnx::Slice_3: [2, 1, decoder_dim] float  (constant init)
        let init_val =
            ort::value::Value::from_array(([2usize, 1usize, dec_dim], init_state.to_vec()))
                .map_err(|e| format!("ORT decoder init: {e}"))?;

        let mut session = self
            .decoder
            .lock()
            .map_err(|e| format!("Decoder lock poisoned: {e}"))?;

        let outputs = session
            .run(ort::inputs![
                "targets" => targets,
                "target_length" => target_length,
                "states.1" => states_val,
                "onnx::Slice_3" => init_val,
            ])
            .map_err(|e| format!("ORT decoder run: {e}"))?;

        // decoder prediction: shape [1, 640, 1]  →  flat [640]
        let dec_val = outputs
            .get("outputs")
            .ok_or("Decoder missing 'outputs' tensor")?;
        let dec_tensor = dec_val
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Extract decoder outputs: {e}"))?;
        let dec_out: Vec<f32> = dec_tensor.1.to_vec();

        // new LSTM state: shape [2, 1, 640]  →  flat [2 * 640]
        let st_val = outputs
            .get("states")
            .ok_or("Decoder missing 'states' tensor")?;
        let st_tensor = st_val
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Extract decoder states: {e}"))?;
        let new_states: Vec<f32> = st_tensor.1.to_vec();

        Ok((dec_out, new_states))
    }

    // ─────────────────────────────────────────────────────────────────────
    // Joiner  (joint network)
    // ─────────────────────────────────────────────────────────────────────

    /// Run the joint network on a single encoder frame + decoder output.
    ///
    /// Returns logits `[vocab_size + num_durations]`.
    fn run_joiner(
        &self,
        enc_frame: &[f32],
        dec_out: &[f32],
    ) -> Result<Vec<f32>, String> {
        let enc_dim = self.config.encoder_dim;
        let dec_dim = self.config.decoder_dim;

        // encoder_outputs: [1, encoder_dim, 1]
        let enc_val =
            ort::value::Value::from_array(([1usize, enc_dim, 1usize], enc_frame.to_vec()))
                .map_err(|e| format!("ORT joiner encoder: {e}"))?;

        // decoder_outputs: [1, decoder_dim, 1]
        let dec_val =
            ort::value::Value::from_array(([1usize, dec_dim, 1usize], dec_out.to_vec()))
                .map_err(|e| format!("ORT joiner decoder: {e}"))?;

        let mut session = self
            .joiner
            .lock()
            .map_err(|e| format!("Joiner lock poisoned: {e}"))?;

        let outputs = session
            .run(ort::inputs![
                "encoder_outputs" => enc_val,
                "decoder_outputs" => dec_val,
            ])
            .map_err(|e| format!("ORT joiner run: {e}"))?;

        let logits_val = outputs
            .get("outputs")
            .ok_or("Joiner missing 'outputs' tensor")?;
        let logits_tensor = logits_val
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Extract joiner logits: {e}"))?;

        Ok(logits_tensor.1.to_vec())
    }

    // ─────────────────────────────────────────────────────────────────────
    // TDT greedy decode
    // ─────────────────────────────────────────────────────────────────────

    /// Greedy TDT decoding loop.
    ///
    /// For each encoder frame the inner loop runs until a blank is
    /// predicted (which also carries a duration Δ telling us how many
    /// frames to skip).
    fn tdt_greedy_decode(
        &self,
        encoder_out: &[f32],
        encoded_frames: usize,
    ) -> Result<String, String> {
        let cfg = &self.config;
        let blank_id = cfg.blank_id;
        let vocab_size = cfg.vocab_size;
        let num_dur = cfg.num_durations;
        let max_sym = cfg.max_symbols_per_step;
        let enc_dim = cfg.encoder_dim;
        let dec_dim = cfg.decoder_dim;

        // Constant initial-state slice (never changes).
        let init_state = vec![0.0f32; 2 * dec_dim];

        // Mutable LSTM state (updated on every decoder call).
        let mut lstm_state = vec![0.0f32; 2 * dec_dim];

        // Seed the decoder with the blank token.
        let (mut decoder_out, new_lstm) =
            self.run_decoder(blank_id as i32, &lstm_state, &init_state)?;
        lstm_state = new_lstm;

        let mut output_tokens: Vec<usize> = Vec::new();
        let mut t: usize = 0;

        while t < encoded_frames {
            // Extract encoder frame at time t.
            // encoder_out is [1, enc_dim, T_enc] row-major →
            //   element [0, c, t] = encoder_out[c * T_enc + t]
            let enc_frame: Vec<f32> = (0..enc_dim)
                .map(|c| {
                    let idx = c * encoded_frames + t;
                    if idx < encoder_out.len() {
                        encoder_out[idx]
                    } else {
                        0.0
                    }
                })
                .collect();

            for _ in 0..max_sym {
                // ── joiner ──
                let logits = self.run_joiner(&enc_frame, &decoder_out)?;

                if logits.len() < vocab_size + num_dur {
                    return Err(format!(
                        "Joiner output {} < expected {} + {}",
                        logits.len(),
                        vocab_size,
                        num_dur,
                    ));
                }

                let best_token = argmax(&logits[..vocab_size]);
                let best_dur_idx = argmax(&logits[vocab_size..vocab_size + num_dur]);
                let duration = cfg
                    .durations
                    .get(best_dur_idx)
                    .copied()
                    .unwrap_or(best_dur_idx);

                if best_token == blank_id {
                    // Blank → advance encoder by at least 1 frame.
                    t += duration.max(1);
                    break;
                } else {
                    // Non-blank → emit token, re-run decoder.
                    output_tokens.push(best_token);

                    let (new_dec, new_st) = self.run_decoder(
                        best_token as i32,
                        &lstm_state,
                        &init_state,
                    )?;
                    decoder_out = new_dec;
                    lstm_state = new_st;
                }
            }
        }

        let text = decode_tokens(&output_tokens, &self.vocab);
        Ok(text)
    }

    // ─────────────────────────────────────────────────────────────────────
    // Mel spectrogram
    // ─────────────────────────────────────────────────────────────────────

    fn compute_mel(&self, samples: &[f32]) -> Vec<Vec<f32>> {
        let cfg = &self.config;
        let n_fft = cfg.n_fft;
        let hop = cfg.hop_length;
        let win_len = cfg.win_length;
        let n_mels = cfg.n_mels;
        let n_freqs = n_fft / 2 + 1;

        // NeMo exact_pad=True: pad by (n_fft - hop) / 2 on each side.
        let pad = (n_fft - hop) / 2;
        let padded = reflect_pad(samples, pad);

        let n_frames = if padded.len() >= win_len {
            (padded.len() - win_len) / hop + 1
        } else {
            0
        };
        if n_frames == 0 {
            return vec![vec![]; n_mels];
        }

        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(n_fft);

        let mut mel = vec![vec![0.0f32; n_frames]; n_mels];
        let mut fft_in = vec![0.0f32; n_fft];
        let mut fft_out = vec![Complex::new(0.0f32, 0.0f32); n_freqs];

        for frame in 0..n_frames {
            let start = frame * hop;

            // Apply window + zero-pad to n_fft.
            fft_in.fill(0.0);
            let copy_len = win_len.min(padded.len().saturating_sub(start));
            for i in 0..copy_len {
                fft_in[i] = padded[start + i] * self.hann_window[i];
            }

            // FFT → power spectrum → mel → log.
            fft.process(&mut fft_in, &mut fft_out)
                .map_err(|e| {
                    log::warn!("[Parakeet] FFT error at frame {frame}: {e}");
                    e
                })
                .ok();

            for band in 0..n_mels {
                let mut energy = 0.0f32;
                for f in 0..n_freqs {
                    let power =
                        fft_out[f].re * fft_out[f].re + fft_out[f].im * fft_out[f].im;
                    energy += self.mel_filterbank[band][f] * power;
                }
                // NeMo log_zero_guard: add 2^-24 before log.
                mel[band][frame] = (energy + 5.960_464_478e-8).ln();
            }
        }

        mel
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Per-feature normalization
// ═══════════════════════════════════════════════════════════════════════════════

/// Per-feature (per mel band) normalization: zero mean, unit variance.
///
/// Matches NeMo's `normalize = "per_feature"`:
///   `x = (x - mean) / (std + 1e-5)` computed per band across time.
fn normalize_per_feature(mel: &mut [Vec<f32>]) {
    const EPS: f32 = 1e-5;
    for band in mel.iter_mut() {
        let n = band.len() as f32;
        if n < 1.0 {
            continue;
        }
        let mean = band.iter().sum::<f32>() / n;
        let var = band.iter().map(|&x| (x - mean) * (x - mean)).sum::<f32>() / n;
        let std = var.sqrt() + EPS;
        for val in band.iter_mut() {
            *val = (*val - mean) / std;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Token decoding
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert output token IDs to human-readable text.
///
/// Handles SentencePiece-style ▁ (U+2581) word boundaries and filters
/// out control / special tokens (anything wrapped in `<…>`).
fn decode_tokens(token_ids: &[usize], vocab: &[String]) -> String {
    let mut pieces: Vec<&str> = Vec::new();

    for &id in token_ids {
        if let Some(tok) = vocab.get(id) {
            // Skip special tokens like <unk>, <blk>, <|nospeech|>, …
            if tok.starts_with('<') && tok.ends_with('>') {
                continue;
            }
            pieces.push(tok.as_str());
        }
    }

    // SentencePiece tokens: ▁ marks word start → replace with space.
    pieces
        .join("")
        .replace('\u{2581}', " ")
        .trim()
        .to_string()
}

/// Argmax over a float slice.
fn argmax(slice: &[f32]) -> usize {
    slice
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Audio loading
// ═══════════════════════════════════════════════════════════════════════════════

/// Load an audio file and return mono f32 samples at `target_sr`.
fn load_audio(path: &Path, target_sr: u32) -> Result<Vec<f32>, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    if ext == "wav" {
        load_wav(path, target_sr)
    } else {
        // Convert to WAV via ffmpeg then load.
        let temp_dir =
            tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {e}"))?;
        let wav_path = temp_dir.path().join("converted.wav");
        convert_to_wav(path, &wav_path, target_sr)?;
        load_wav(&wav_path, target_sr)
    }
}

/// Read a WAV file with `hound`, convert to mono float, resample if needed.
fn load_wav(path: &Path, target_sr: u32) -> Result<Vec<f32>, String> {
    let reader =
        hound::WavReader::open(path).map_err(|e| format!("Failed to open WAV: {e}"))?;

    let spec = reader.spec();
    let channels = spec.channels as usize;
    let source_sr = spec.sample_rate;

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .collect(),
        hound::SampleFormat::Int => {
            let max_val = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .into_samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / max_val)
                .collect()
        }
    };

    // Downmix to mono.
    let mono: Vec<f32> = if channels <= 1 {
        samples
    } else {
        samples
            .chunks(channels)
            .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
            .collect()
    };

    // Resample if source rate differs.
    if source_sr == target_sr {
        Ok(mono)
    } else {
        Ok(resample_linear(&mono, source_sr, target_sr))
    }
}

/// Convert any audio format to 16-bit mono WAV at `target_sr` via ffmpeg.
fn convert_to_wav(input: &Path, output: &Path, target_sr: u32) -> Result<(), String> {
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-i",
            &input.to_string_lossy(),
            "-ar",
            &target_sr.to_string(),
            "-ac",
            "1",
            "-sample_fmt",
            "s16",
            "-f",
            "wav",
            "-y",
            &output.to_string_lossy(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()
        .map_err(|e| {
            format!(
                "Failed to run ffmpeg (is it installed?): {e}. \
                 Non-WAV audio files require ffmpeg for format conversion."
            )
        })?;

    if !status.success() {
        return Err(format!("ffmpeg conversion failed (exit {})", status));
    }

    Ok(())
}

/// Simple linear-interpolation resampling.
fn resample_linear(samples: &[f32], from_sr: u32, to_sr: u32) -> Vec<f32> {
    if from_sr == to_sr || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = from_sr as f64 / to_sr as f64;
    let output_len = (samples.len() as f64 / ratio).ceil() as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_pos = i as f64 * ratio;
        let src_idx = src_pos as usize;
        let frac = (src_pos - src_idx as f64) as f32;

        let sample = if src_idx + 1 < samples.len() {
            samples[src_idx] * (1.0 - frac) + samples[src_idx + 1] * frac
        } else if src_idx < samples.len() {
            samples[src_idx]
        } else {
            0.0
        };

        output.push(sample);
    }

    output
}

// ═══════════════════════════════════════════════════════════════════════════════
// DSP helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Apply pre-emphasis filter: y[n] = x[n] − α·x[n−1].
fn apply_preemphasis(samples: &[f32], coef: f32) -> Vec<f32> {
    if samples.is_empty() || coef == 0.0 {
        return samples.to_vec();
    }
    let mut out = Vec::with_capacity(samples.len());
    out.push(samples[0]);
    for i in 1..samples.len() {
        out.push(samples[i] - coef * samples[i - 1]);
    }
    out
}

/// Add small random dither noise (deterministic per-sample hash for
/// reproducibility in tests).
fn apply_dither(samples: &[f32], magnitude: f32) -> Vec<f32> {
    samples
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            // Simple deterministic hash → uniform-ish noise in [-1, 1].
            let noise = ((i as u64)
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1)
                >> 33) as f32
                / u32::MAX as f32
                * 2.0
                - 1.0;
            s + noise * magnitude
        })
        .collect()
}

/// Reflect-pad a signal (matches numpy `reflect` mode).
fn reflect_pad(samples: &[f32], pad: usize) -> Vec<f32> {
    let n = samples.len();
    if n == 0 {
        return Vec::new();
    }
    let pad = pad.min(n - 1);
    let mut padded = Vec::with_capacity(n + 2 * pad);

    for i in (1..=pad).rev() {
        padded.push(samples[i]);
    }
    padded.extend_from_slice(samples);
    for i in 0..pad {
        padded.push(samples[n - 2 - i]);
    }

    padded
}

/// Compute a Hann window of given length.
fn hann(size: usize) -> Vec<f32> {
    if size <= 1 {
        return vec![1.0; size];
    }
    (0..size)
        .map(|i| {
            0.5 * (1.0
                - (2.0 * std::f32::consts::PI * i as f32 / (size - 1) as f32).cos())
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Mel filterbank
// ═══════════════════════════════════════════════════════════════════════════════

fn hz_to_mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10.0_f32.powf(mel / 2595.0) - 1.0)
}

/// Triangular mel filterbank matrix `[n_mels][n_fft/2+1]`.
fn compute_mel_filterbank(
    n_fft: usize,
    n_mels: usize,
    sample_rate: u32,
    fmin: f32,
    fmax: f32,
) -> Vec<Vec<f32>> {
    let sr = sample_rate as f32;
    let n_freqs = n_fft / 2 + 1;

    let mel_min = hz_to_mel(fmin);
    let mel_max = hz_to_mel(fmax.min(sr / 2.0));

    let n_points = n_mels + 2;
    let mel_points: Vec<f32> = (0..n_points)
        .map(|i| mel_min + (mel_max - mel_min) * i as f32 / (n_points - 1) as f32)
        .collect();

    let bin_points: Vec<f32> = mel_points
        .iter()
        .map(|&m| mel_to_hz(m) * n_fft as f32 / sr)
        .collect();

    let mut filterbank = vec![vec![0.0f32; n_freqs]; n_mels];

    for i in 0..n_mels {
        let left = bin_points[i];
        let center = bin_points[i + 1];
        let right = bin_points[i + 2];

        for j in 0..n_freqs {
            let freq = j as f32;
            if freq >= left && freq <= center && (center - left).abs() > 1e-10 {
                filterbank[i][j] = (freq - left) / (center - left);
            } else if freq > center && freq <= right && (right - center).abs() > 1e-10 {
                filterbank[i][j] = (right - freq) / (right - center);
            }
        }
    }

    filterbank
}

// ═══════════════════════════════════════════════════════════════════════════════
// Vocabulary loading
// ═══════════════════════════════════════════════════════════════════════════════

/// Load the token vocabulary from a file in the model directory.
///
/// Supports:
///   - Space-separated `token id` per line (NeMo / sherpa-onnx format)
///   - Tab-separated `token\tid` per line
///   - JSON array of strings
fn load_vocab(model_dir: &Path, vocab_file: &str) -> Result<Vec<String>, String> {
    let vocab_path = model_dir.join(vocab_file);
    if vocab_path.exists() {
        return parse_vocab_file(&vocab_path);
    }

    // Auto-discover common names.
    for name in &["tokens.txt", "vocab.txt", "tokenizer.txt"] {
        let path = model_dir.join(name);
        if path.exists() {
            log::info!("[Parakeet] Auto-discovered vocab: {}", path.display());
            return parse_vocab_file(&path);
        }
    }

    Err(format!(
        "Vocabulary file not found. Expected '{}' in {}",
        vocab_file,
        model_dir.display()
    ))
}

fn parse_vocab_file(path: &Path) -> Result<Vec<String>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read vocab: {e}"))?;

    // Try JSON array first.
    if content.trim_start().starts_with('[') {
        let tokens: Vec<String> = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse vocab JSON: {e}"))?;
        if tokens.is_empty() {
            return Err("Vocabulary JSON array is empty".into());
        }
        return Ok(tokens);
    }

    // Space-or-tab separated "token id" format.
    // Build an index-ordered vector keyed by the integer ID.
    let mut max_id: usize = 0;
    let mut entries: Vec<(String, usize)> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Split on *last* space (token itself may contain spaces, though rare).
        if let Some(last_space) = line.rfind(' ') {
            let token_part = &line[..last_space];
            let id_part = &line[last_space + 1..];
            if let Ok(id) = id_part.parse::<usize>() {
                max_id = max_id.max(id);
                entries.push((token_part.to_string(), id));
                continue;
            }
        }

        // Fallback: try tab separator.
        if let Some(tab_pos) = line.find('\t') {
            let token_part = &line[..tab_pos];
            let id_part = &line[tab_pos + 1..];
            if let Ok(id) = id_part.parse::<usize>() {
                max_id = max_id.max(id);
                entries.push((token_part.to_string(), id));
                continue;
            }
        }

        // Last resort: line-order based (no ID column).
        let id = entries.len();
        max_id = max_id.max(id);
        entries.push((line.to_string(), id));
    }

    if entries.is_empty() {
        return Err("Vocabulary file is empty".into());
    }

    let mut vocab = vec![String::new(); max_id + 1];
    for (token, id) in entries {
        if id < vocab.len() {
            vocab[id] = token;
        }
    }

    Ok(vocab)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Utility
// ═══════════════════════════════════════════════════════════════════════════════

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hann_window() {
        let w = hann(5);
        assert_eq!(w.len(), 5);
        assert!((w[0]).abs() < 1e-6);
        assert!((w[4]).abs() < 1e-6);
        assert!((w[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_reflect_pad() {
        let s = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let padded = reflect_pad(&s, 2);
        assert_eq!(padded, vec![2.0, 1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0]);
    }

    #[test]
    fn test_preemphasis() {
        let s = vec![1.0, 2.0, 3.0, 4.0];
        let out = apply_preemphasis(&s, 0.97);
        assert_eq!(out.len(), 4);
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert!((out[1] - (2.0 - 0.97)).abs() < 1e-6);
    }

    #[test]
    fn test_resample_linear() {
        let s = vec![0.0, 1.0, 2.0, 3.0];
        let out = resample_linear(&s, 16000, 8000);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn test_mel_filterbank_128() {
        let fb = compute_mel_filterbank(512, 128, 16000, 0.0, 8000.0);
        assert_eq!(fb.len(), 128);
        assert_eq!(fb[0].len(), 257); // 512/2 + 1
    }

    #[test]
    fn test_argmax() {
        assert_eq!(argmax(&[0.1, 0.9, 0.3]), 1);
        assert_eq!(argmax(&[0.5, 0.1, 0.2]), 0);
        assert_eq!(argmax(&[0.1, 0.2, 0.7]), 2);
    }

    #[test]
    fn test_decode_tokens_sentencepiece() {
        let vocab = vec![
            "<blk>".to_string(),
            "▁he".to_string(),
            "llo".to_string(),
            "▁world".to_string(),
        ];
        let tokens = vec![1, 2, 3];
        let text = decode_tokens(&tokens, &vocab);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn test_decode_tokens_skips_special() {
        let vocab = vec![
            "<blk>".to_string(),
            "<unk>".to_string(),
            "▁hi".to_string(),
        ];
        let tokens = vec![0, 1, 2];
        let text = decode_tokens(&tokens, &vocab);
        assert_eq!(text, "hi");
    }

    #[test]
    fn test_parse_vocab_space_separated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.txt");
        std::fs::write(
            &path,
            "<blk> 0\n<unk> 1\n▁hello 2\n▁world 3\n",
        )
        .unwrap();

        let vocab = parse_vocab_file(&path).unwrap();
        assert_eq!(vocab.len(), 4);
        assert_eq!(vocab[0], "<blk>");
        assert_eq!(vocab[1], "<unk>");
        assert_eq!(vocab[2], "▁hello");
        assert_eq!(vocab[3], "▁world");
    }

    #[test]
    fn test_hz_mel_roundtrip() {
        let hz = 1000.0;
        let mel = hz_to_mel(hz);
        let back = mel_to_hz(mel);
        assert!((hz - back).abs() < 0.1);
    }

    #[test]
    fn test_default_config() {
        let cfg = ParakeetConfig::default();
        assert_eq!(cfg.n_mels, 128);
        assert_eq!(cfg.blank_id, 8192);
        assert_eq!(cfg.vocab_size, 8193);
        assert_eq!(cfg.num_durations, 5);
        assert_eq!(cfg.durations, vec![0, 1, 2, 3, 4]);
        assert_eq!(cfg.encoder_dim, 1024);
        assert_eq!(cfg.decoder_dim, 640);
    }
}
