//! DOCX text extraction using docx-rs.

use std::path::Path;
use super::{ParsedDocument, TextBlock};

pub fn parse(path: &Path) -> Result<ParsedDocument, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Failed to read DOCX: {e}"))?;
    let docx = docx_rs::read_docx(&bytes).map_err(|e| format!("DOCX parse error: {e}"))?;

    let title = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("document.docx")
        .to_string();

    let mut sections = Vec::new();
    let mut para_idx = 0u32;

    // Walk the document body collecting paragraph text
    for child in docx.document.children.iter() {
        if let docx_rs::DocumentChild::Paragraph(para) = child {
            let mut text = String::new();
            for child in &para.children {
                if let docx_rs::ParagraphChild::Run(run) = child {
                    for child in &run.children {
                        if let docx_rs::RunChild::Text(t) = child {
                            text.push_str(&t.text);
                        }
                    }
                }
            }
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                para_idx += 1;
                sections.push(TextBlock {
                    text: trimmed,
                    metadata: format!("paragraph:{para_idx}"),
                });
            }
        }
    }

    Ok(ParsedDocument { title, sections })
}
