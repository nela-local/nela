//! Document parsers — extract text from various file formats.

pub mod pdf;
pub mod docx;
pub mod pptx;
pub mod text;
pub mod audio;

use std::path::Path;

/// Parsed output from any document parser.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ParsedDocument {
    /// Human-readable title (filename or extracted title).
    pub title: String,
    /// Extracted text blocks (paragraphs, pages, slides, etc.)
    pub sections: Vec<TextBlock>,
}

/// A single block of text from a document.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TextBlock {
    /// The text content.
    pub text: String,
    /// Optional metadata (page number, slide number, timestamp, etc.)
    pub metadata: String,
}

/// Parse a document at the given path, dispatching to the correct parser
/// based on file extension.
pub fn parse_document(path: &Path) -> Result<ParsedDocument, String> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "pdf" => pdf::parse(path),
        "docx" => docx::parse(path),
        "pptx" => pptx::parse(path),
        "txt" | "md" | "csv" | "json" | "toml" | "yaml" | "yml" | "rs" | "py" | "js" | "ts"
        | "c" | "cpp" | "h" | "java" | "go" | "rb" | "sh" | "bat" | "html" | "css" | "xml"
        | "log" => text::parse(path),
        "mp3" | "wav" | "m4a" | "ogg" | "flac" | "aac" | "wma" | "webm" => {
            // Audio files need async transcription — return a placeholder.
            // The pipeline handles transcription via the TaskRouter.
            Ok(ParsedDocument {
                title: path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("audio")
                    .to_string(),
                sections: vec![TextBlock {
                    text: format!("[Audio file pending transcription: {}]", path.display()),
                    metadata: "audio:pending".into(),
                }],
            })
        }
        _ => Err(format!("Unsupported file type: .{ext}")),
    }
}
