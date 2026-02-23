//! KittenTTS — Pure-Rust text-to-speech inference engine.
//!
//! This module ports the Python KittenTTS package to Rust, running ONNX
//! inference natively via `ort` (already part of the GenHat backend).
//!
//! Pipeline:
//! 1. **Text preprocessing** (`preprocess.rs`) — normalize numbers, currency,
//!    contractions, etc. into speakable English words.
//! 2. **Phonemization** (`phonemizer.rs`) — convert English text to IPA via
//!    `espeak-ng` subprocess.
//! 3. **Text cleaning / tokenization** (`text_cleaner.rs`) — map IPA characters
//!    to integer token IDs expected by the ONNX model.
//! 4. **Voice loading** (`voice.rs`) — load speaker embeddings from `.npz` files.
//! 5. **ONNX inference** (`inference.rs`) — run the KittenTTS ONNX model and
//!    produce raw audio samples.
//! 6. **WAV writing** (`wav.rs`) — write audio samples to a `.wav` file.
//!
//! The entire pipeline is orchestrated by `KittenTtsEngine` in `inference.rs`.

pub mod preprocess;
pub mod phonemizer;
pub mod text_cleaner;
pub mod voice;
pub mod inference;
pub mod wav;
