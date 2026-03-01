//! Parakeet ASR — Pure-Rust automatic speech recognition via ONNX Runtime.
//!
//! This module implements the full NeMo Parakeet TDT inference pipeline
//! in native Rust, using `ort` (ONNX Runtime) for the neural network
//! forward pass and hand-written DSP for audio preprocessing.
//!
//! Pipeline:
//! 1. **Audio loading** (`inference.rs`) — read WAV via `hound`, convert
//!    non-WAV formats to WAV via `ffmpeg` subprocess.
//! 2. **Resampling** — linear interpolation to 16 kHz mono.
//! 3. **Pre-emphasis** — high-pass filter (coeff 0.97).
//! 4. **Mel spectrogram** — STFT with Hann window → power spectrum →
//!    80-band mel filterbank → natural log.
//! 5. **ONNX inference** — run the Parakeet encoder model.
//! 6. **CTC greedy decode** — argmax → collapse repeats → remove blanks
//!    → join BPE tokens into text.
//!
//! The entire pipeline is orchestrated by `ParakeetEngine` in `inference.rs`.

pub mod inference;
