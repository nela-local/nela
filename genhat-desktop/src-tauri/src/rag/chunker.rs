//! Recursive character chunker.
//!
//! Splits text into overlapping chunks using a hierarchy of separators:
//!   `["\n\n", "\n", ". ", " "]`
//!
//! Target: 384 tokens (~1536 chars at ~4 chars/token).
//! Overlap: 50 tokens (~200 chars) for context bleed.

/// A chunk produced by the splitter.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Byte offset in the original text (start).
    pub offset: usize,
    /// The chunk text.
    pub text: String,
    /// Index within the document (0-based).
    pub index: usize,
}

/// Chunker configuration.
pub struct ChunkerConfig {
    /// Target chunk size in characters.
    pub chunk_size: usize,
    /// Number of overlap characters between consecutive chunks.
    pub overlap: usize,
    /// Separators to try, in order of preference.
    pub separators: Vec<String>,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            chunk_size: 1536,  // ~384 tokens at 4 chars/token
            overlap: 200,     // ~50 tokens
            separators: vec![
                "\n\n".to_string(),
                "\n".to_string(),
                ". ".to_string(),
                " ".to_string(),
            ],
        }
    }
}

/// Split a document into overlapping chunks using recursive character splitting.
pub fn chunk_text(text: &str, config: &ChunkerConfig) -> Vec<Chunk> {
    let raw_splits = recursive_split(text, &config.separators, config.chunk_size);

    // Merge tiny splits and apply overlap
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_offset: usize = 0;
    let mut byte_pos: usize = 0;

    for split in &raw_splits {
        if current.is_empty() {
            current_offset = byte_pos;
        }

        if current.len() + split.len() <= config.chunk_size {
            current.push_str(split);
        } else {
            if !current.trim().is_empty() {
                chunks.push(Chunk {
                    offset: current_offset,
                    text: current.clone(),
                    index: chunks.len(),
                });
            }

            // Start new chunk with overlap from previous
            let overlap_text = get_overlap_suffix(&current, config.overlap);
            current_offset = byte_pos.saturating_sub(overlap_text.len());
            current = format!("{}{}", overlap_text, split);
        }

        byte_pos += split.len();
    }

    // Final chunk
    if !current.trim().is_empty() {
        chunks.push(Chunk {
            offset: current_offset,
            text: current,
            index: chunks.len(),
        });
    }

    chunks
}

/// Convenience: chunk with default config.
pub fn chunk_text_default(text: &str) -> Vec<Chunk> {
    chunk_text(text, &ChunkerConfig::default())
}

/// Recursively split text using separators.
fn recursive_split(text: &str, separators: &[String], chunk_size: usize) -> Vec<String> {
    if text.len() <= chunk_size || separators.is_empty() {
        return vec![text.to_string()];
    }

    let sep = &separators[0];
    let parts: Vec<&str> = text.split(sep.as_str()).collect();

    let mut result = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let piece = if i < parts.len() - 1 {
            format!("{}{}", part, sep) // re-attach separator
        } else {
            part.to_string()
        };

        if piece.len() > chunk_size && separators.len() > 1 {
            // Try finer-grained splitting
            let sub = recursive_split(&piece, &separators[1..], chunk_size);
            result.extend(sub);
        } else {
            result.push(piece);
        }
    }

    result
}

/// Get the last `max_len` characters of a string (for overlap).
fn get_overlap_suffix(text: &str, max_len: usize) -> &str {
    if text.len() <= max_len {
        text
    } else {
        let start = text.len() - max_len;
        // Find the next valid char boundary at or after `start`
        let start = (start..text.len())
            .find(|&i| text.is_char_boundary(i))
            .unwrap_or(text.len());
        &text[start..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_text_single_chunk() {
        let text = "Hello world, this is short.";
        let chunks = chunk_text_default(text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, text);
        assert_eq!(chunks[0].index, 0);
    }

    #[test]
    fn test_paragraph_splitting() {
        let para1 = "A".repeat(1000);
        let para2 = "B".repeat(1000);
        let text = format!("{}\n\n{}", para1, para2);

        let config = ChunkerConfig {
            chunk_size: 1200,
            overlap: 100,
            separators: vec!["\n\n".to_string(), " ".to_string()],
        };
        let chunks = chunk_text(&text, &config);
        assert!(chunks.len() >= 2, "Should split on paragraph boundary");
    }

    #[test]
    fn test_overlap_present() {
        // Create text that forces splitting
        let words: Vec<String> = (0..500).map(|i| format!("word{}", i)).collect();
        let text = words.join(" ");

        let config = ChunkerConfig {
            chunk_size: 200,
            overlap: 50,
            separators: vec![" ".to_string()],
        };
        let chunks = chunk_text(&text, &config);
        assert!(chunks.len() > 1);

        // Check overlap: end of chunk[0] should appear at start of chunk[1]
        if chunks.len() >= 2 {
            let end_of_first = &chunks[0].text[chunks[0].text.len().saturating_sub(30)..];
            assert!(
                chunks[1].text.starts_with(end_of_first)
                    || chunks[1].text.contains(&end_of_first[..end_of_first.len().min(15)]),
                "Chunks should overlap"
            );
        }
    }

    #[test]
    fn test_indices_sequential() {
        let text = "A\n\nB\n\nC\n\nD\n\nE";
        let config = ChunkerConfig {
            chunk_size: 3,
            overlap: 0,
            separators: vec!["\n\n".to_string()],
        };
        let chunks = chunk_text(&text, &config);
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.index, i);
        }
    }
}
