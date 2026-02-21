//! Audio file parser stub.
//!
//! Audio transcription is async and requires the TaskRouter (Whisper backend).
//! This module provides the placeholder parser; actual transcription is handled
//! by the ingestion pipeline which calls `transcribe_audio` via the router.

use std::path::Path;
use super::{ParsedDocument, TextBlock};

/// Returns a placeholder document. The pipeline should detect the
/// `audio:pending` metadata and trigger async transcription.
pub fn parse_placeholder(path: &Path) -> ParsedDocument {
    let title = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("audio")
        .to_string();

    ParsedDocument {
        title,
        sections: vec![TextBlock {
            text: format!("[Audio file: {}]", path.display()),
            metadata: "audio:pending".into(),
        }],
    }
}

/// Build a ParsedDocument from transcription segments returned by the
/// Whisper backend (TaskResponse::Transcription).
pub fn from_transcription(
    title: &str,
    segments: &[(String, u64, u64)], // (text, start_ms, end_ms)
) -> ParsedDocument {
    let sections = segments
        .iter()
        .map(|(text, start, end)| TextBlock {
            text: text.clone(),
            metadata: format!("audio:{}ms-{}ms", start, end),
        })
        .collect();

    ParsedDocument {
        title: title.to_string(),
        sections,
    }
}
