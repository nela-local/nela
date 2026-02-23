//! Voice embedding loader for KittenTTS.
//!
//! KittenTTS voices are stored as NumPy `.npz` archives — a ZIP file where
//! each entry is a `.npy` array.  Each voice key (e.g. `expr-voice-2-f`)
//! maps to a 2-D f32 array of shape `[N, embed_dim]`.
//!
//! We read the ZIP using the `zip` crate and parse the `.npy` binary format
//! manually (it's 10 bytes of magic + a header + raw f32 data).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// A loaded voice embedding: shape [num_refs, embed_dim].
#[derive(Debug, Clone)]
pub struct VoiceEmbedding {
    pub data: Vec<f32>,
    pub num_refs: usize,
    pub embed_dim: usize,
}

impl VoiceEmbedding {
    /// Select a style reference vector based on text length (Python logic).
    /// `ref_id = min(len(text), shape[0] - 1)`, then `ref_s = voices[voice][ref_id:ref_id+1]`.
    pub fn select_style(&self, text_len: usize) -> Vec<f32> {
        let ref_id = text_len.min(self.num_refs.saturating_sub(1));
        let start = ref_id * self.embed_dim;
        let end = start + self.embed_dim;
        self.data[start..end].to_vec()
    }
}

/// All loaded voices keyed by internal voice ID (e.g. "expr-voice-2-f").
pub type VoiceBank = HashMap<String, VoiceEmbedding>;

/// Load all voice embeddings from a `.npz` file.
pub fn load_voices(npz_path: &Path) -> Result<VoiceBank, String> {
    let file = std::fs::File::open(npz_path)
        .map_err(|e| format!("Cannot open voices file {}: {e}", npz_path.display()))?;

    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Cannot read NPZ archive: {e}"))?;

    let mut voices = HashMap::new();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("NPZ entry read failed: {e}"))?;

        let name = entry.name().to_string();

        // Each file in the NPZ is "<key>.npy"
        let voice_key = name.trim_end_matches(".npy").to_string();
        if voice_key == name {
            continue; // Not a .npy file, skip
        }

        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| format!("Failed to read NPZ entry '{name}': {e}"))?;

        let embedding = parse_npy_f32(&buf, &voice_key)?;
        log::info!(
            "[KittenTTS] Loaded voice '{}': [{} × {}]",
            voice_key, embedding.num_refs, embedding.embed_dim
        );
        voices.insert(voice_key, embedding);
    }

    if voices.is_empty() {
        return Err("No voice embeddings found in NPZ file".into());
    }

    Ok(voices)
}

/// Parse a raw `.npy` byte buffer (v1.0 format) into a VoiceEmbedding.
///
/// NumPy `.npy` format:
///   bytes 0-5:   magic  \x93NUMPY
///   byte  6:     major version (1)
///   byte  7:     minor version (0)
///   bytes 8-9:   header_len (little-endian u16)
///   bytes 10..:  ASCII header dict, then raw data
fn parse_npy_f32(buf: &[u8], name: &str) -> Result<VoiceEmbedding, String> {
    if buf.len() < 10 {
        return Err(format!("NPY entry '{name}' too short"));
    }

    // Check magic
    if &buf[0..6] != b"\x93NUMPY" {
        return Err(format!("NPY entry '{name}' has invalid magic bytes"));
    }

    let major = buf[6];
    let (header_len, header_start) = if major == 1 {
        let hl = u16::from_le_bytes([buf[8], buf[9]]) as usize;
        (hl, 10usize)
    } else if major == 2 {
        if buf.len() < 12 {
            return Err(format!("NPY v2 entry '{name}' too short"));
        }
        let hl = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]) as usize;
        (hl, 12usize)
    } else {
        return Err(format!("NPY entry '{name}' unsupported version {major}"));
    };

    let data_start = header_start + header_len;
    if data_start > buf.len() {
        return Err(format!("NPY entry '{name}' header extends past EOF"));
    }

    // Parse ASCII header to extract shape and dtype
    let header = std::str::from_utf8(&buf[header_start..data_start])
        .map_err(|_| format!("NPY entry '{name}' header is not valid UTF-8"))?;

    // Verify it's float32 little-endian
    if !header.contains("'<f4'") && !header.contains("'<f8'") && !header.contains("float") {
        // Be lenient: if it's <f4 or float32 or f4, accept
        if !header.contains("f4") && !header.contains("float32") {
            return Err(format!("NPY entry '{name}' is not float32: {header}"));
        }
    }

    let is_f8 = header.contains("'<f8'") || header.contains("float64");

    // Extract shape from header, e.g. 'shape': (128, 256)
    let shape = parse_shape(header, name)?;
    if shape.is_empty() || shape.len() > 2 {
        return Err(format!("NPY entry '{name}' unexpected shape: {shape:?}"));
    }

    let num_refs = shape[0];
    let embed_dim = if shape.len() == 2 { shape[1] } else { 1 };
    let expected_elements = num_refs * embed_dim;

    let raw_data = &buf[data_start..];

    let data: Vec<f32> = if is_f8 {
        // float64 → f32 conversion
        let expected_bytes = expected_elements * 8;
        if raw_data.len() < expected_bytes {
            return Err(format!(
                "NPY entry '{name}' data too short: need {expected_bytes}, got {}",
                raw_data.len()
            ));
        }
        (0..expected_elements)
            .map(|i| {
                let bytes = &raw_data[i * 8..(i + 1) * 8];
                f64::from_le_bytes(bytes.try_into().unwrap()) as f32
            })
            .collect()
    } else {
        let expected_bytes = expected_elements * 4;
        if raw_data.len() < expected_bytes {
            return Err(format!(
                "NPY entry '{name}' data too short: need {expected_bytes}, got {}",
                raw_data.len()
            ));
        }
        (0..expected_elements)
            .map(|i| {
                let bytes = &raw_data[i * 4..(i + 1) * 4];
                f32::from_le_bytes(bytes.try_into().unwrap())
            })
            .collect()
    };

    Ok(VoiceEmbedding {
        data,
        num_refs,
        embed_dim,
    })
}

/// Parse shape tuple from NPY header string.
/// E.g. "'shape': (128, 256)," → [128, 256]
fn parse_shape(header: &str, name: &str) -> Result<Vec<usize>, String> {
    // Find 'shape': (...)
    let shape_key = "'shape':";
    let start = header
        .find(shape_key)
        .ok_or_else(|| format!("NPY entry '{name}' header missing shape"))?;
    let rest = &header[start + shape_key.len()..];

    let paren_open = rest
        .find('(')
        .ok_or_else(|| format!("NPY entry '{name}' shape has no '('"))?;
    let paren_close = rest
        .find(')')
        .ok_or_else(|| format!("NPY entry '{name}' shape has no ')'"))?;

    let inner = &rest[paren_open + 1..paren_close];
    let dims: Vec<usize> = inner
        .split(',')
        .filter_map(|s| {
            let s = s.trim();
            if s.is_empty() {
                None
            } else {
                s.parse::<usize>().ok()
            }
        })
        .collect();

    Ok(dims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_shape() {
        let s = "{'descr': '<f4', 'fortran_order': False, 'shape': (128, 256), }";
        assert_eq!(parse_shape(s, "test").unwrap(), vec![128, 256]);
    }

    #[test]
    fn test_parse_shape_1d() {
        let s = "{'descr': '<f4', 'fortran_order': False, 'shape': (10,), }";
        assert_eq!(parse_shape(s, "test").unwrap(), vec![10]);
    }
}
