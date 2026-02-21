//! Plain text / source code file parser.

use std::path::Path;
use super::{ParsedDocument, TextBlock};

pub fn parse(path: &Path) -> Result<ParsedDocument, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read text file: {e}"))?;

    let title = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("text")
        .to_string();

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    // For Markdown, split on headers; for source code, keep as one block;
    // for plain text, split on double-newlines.
    let sections = match ext.as_str() {
        "md" => split_markdown(&content),
        _ => split_by_paragraphs(&content),
    };

    Ok(ParsedDocument { title, sections })
}

/// Split markdown by headings (# lines).
fn split_markdown(content: &str) -> Vec<TextBlock> {
    let mut sections = Vec::new();
    let mut current = String::new();
    let mut heading = String::from("intro");

    for line in content.lines() {
        if line.starts_with('#') {
            // Flush previous section
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                sections.push(TextBlock {
                    text: trimmed,
                    metadata: format!("section:{heading}"),
                });
            }
            heading = line.trim_start_matches('#').trim().to_string();
            current.clear();
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }

    // Flush last section
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        sections.push(TextBlock {
            text: trimmed,
            metadata: format!("section:{heading}"),
        });
    }

    sections
}

/// Split by double-newlines into paragraphs.
fn split_by_paragraphs(content: &str) -> Vec<TextBlock> {
    content
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .enumerate()
        .map(|(i, para)| TextBlock {
            text: para.trim().to_string(),
            metadata: format!("paragraph:{}", i + 1),
        })
        .collect()
}
