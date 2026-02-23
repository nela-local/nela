//! Minimal WAV file writer for TTS audio output.
//!
//! Writes a mono 16-bit PCM WAV file at 24 kHz (the KittenTTS sample rate).
//! No external crate needed — the WAV header is just 44 bytes.

use std::io::Write;
use std::path::Path;

/// Default sample rate for KittenTTS output.
pub const SAMPLE_RATE: u32 = 24_000;

/// Write f32 audio samples to a WAV file (mono, 16-bit PCM).
///
/// Samples are expected in the range [-1.0, 1.0]. Values outside that
/// range are clamped.
pub fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<(), String> {
    let num_samples = samples.len() as u32;
    let bits_per_sample: u16 = 16;
    let num_channels: u16 = 1;
    let byte_rate = sample_rate * (bits_per_sample as u32 / 8) * num_channels as u32;
    let block_align = num_channels * (bits_per_sample / 8);
    let data_size = num_samples * (bits_per_sample as u32 / 8);
    let file_size = 36 + data_size; // RIFF chunk size = file size - 8, but the "ChunkSize" field = file_size - 8... let's just do it right:

    let mut buf: Vec<u8> = Vec::with_capacity(44 + data_size as usize);

    // RIFF header
    buf.write_all(b"RIFF").map_err(|e| e.to_string())?;
    buf.write_all(&file_size.to_le_bytes()).map_err(|e| e.to_string())?;
    buf.write_all(b"WAVE").map_err(|e| e.to_string())?;

    // fmt sub-chunk
    buf.write_all(b"fmt ").map_err(|e| e.to_string())?;
    buf.write_all(&16u32.to_le_bytes()).map_err(|e| e.to_string())?; // Sub-chunk size
    buf.write_all(&1u16.to_le_bytes()).map_err(|e| e.to_string())?;  // PCM format
    buf.write_all(&num_channels.to_le_bytes()).map_err(|e| e.to_string())?;
    buf.write_all(&sample_rate.to_le_bytes()).map_err(|e| e.to_string())?;
    buf.write_all(&byte_rate.to_le_bytes()).map_err(|e| e.to_string())?;
    buf.write_all(&block_align.to_le_bytes()).map_err(|e| e.to_string())?;
    buf.write_all(&bits_per_sample.to_le_bytes()).map_err(|e| e.to_string())?;

    // data sub-chunk
    buf.write_all(b"data").map_err(|e| e.to_string())?;
    buf.write_all(&data_size.to_le_bytes()).map_err(|e| e.to_string())?;

    // Convert f32 → i16 and write
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_val = (clamped * 32767.0) as i16;
        buf.write_all(&i16_val.to_le_bytes()).map_err(|e| e.to_string())?;
    }

    std::fs::write(path, &buf)
        .map_err(|e| format!("Failed to write WAV to {}: {e}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_wav_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wav");
        let samples = vec![0.0f32; 1000];
        write_wav(&path, &samples, SAMPLE_RATE).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
    }
}
