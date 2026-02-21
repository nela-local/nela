//! PPTX text extraction (ZIP + XML parsing).

use std::path::Path;
use std::io::Read;
use super::{ParsedDocument, TextBlock};

pub fn parse(path: &Path) -> Result<ParsedDocument, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Failed to open PPTX: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("ZIP error: {e}"))?;

    let title = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("presentation.pptx")
        .to_string();

    let mut sections = Vec::new();

    // PPTX stores slides in ppt/slides/slide{N}.xml
    // Collect slide filenames and sort numerically
    let mut slide_names: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            let name = entry.name().to_string();
            if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    slide_names.sort_by(|a, b| {
        let num_a = extract_slide_number(a);
        let num_b = extract_slide_number(b);
        num_a.cmp(&num_b)
    });

    for (idx, slide_name) in slide_names.iter().enumerate() {
        if let Ok(mut entry) = archive.by_name(slide_name) {
            let mut xml = String::new();
            if entry.read_to_string(&mut xml).is_ok() {
                let text = extract_text_from_xml(&xml);
                if !text.trim().is_empty() {
                    sections.push(TextBlock {
                        text: text.trim().to_string(),
                        metadata: format!("slide:{}", idx + 1),
                    });
                }
            }
        }
    }

    Ok(ParsedDocument { title, sections })
}

/// Extract slide number from path like "ppt/slides/slide3.xml" -> 3
fn extract_slide_number(name: &str) -> u32 {
    name.trim_start_matches("ppt/slides/slide")
        .trim_end_matches(".xml")
        .parse()
        .unwrap_or(0)
}

/// Extract text content from XML by stripping all tags.
/// Simple approach: find all text between `<a:t>...</a:t>` tags (OOXML text elements).
fn extract_text_from_xml(xml: &str) -> String {
    let mut result = String::new();
    let mut in_text = false;

    // Simple state machine to extract text from <a:t> elements
    let mut chars = xml.chars().peekable();
    let mut tag_buf = String::new();

    while let Some(ch) = chars.next() {
        if ch == '<' {
            tag_buf.clear();
            while let Some(&next) = chars.peek() {
                chars.next();
                if next == '>' {
                    break;
                }
                tag_buf.push(next);
            }
            if tag_buf == "a:t" || tag_buf.starts_with("a:t ") {
                in_text = true;
            } else if tag_buf == "/a:t" {
                in_text = false;
                result.push(' ');
            }
        } else if in_text {
            result.push(ch);
        }
    }

    result
}
