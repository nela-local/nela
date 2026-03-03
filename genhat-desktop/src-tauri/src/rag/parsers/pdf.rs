//! PDF parser — text extraction via pdf-extract, image/table extraction via pdfium-render.
//!
//! Text is extracted using `pdf_extract` (fast, reliable for text-heavy PDFs).
//! Images are pulled via pdfium-render's `PdfPageImageObject` API.
//! Tables are detected as dense clusters of line/path objects and rendered
//! as PNG screenshots of the relevant page region.
//!
//! If libpdfium is not available on the system, media extraction is gracefully
//! skipped and only text is returned (no crash).

use std::path::Path;
use super::{ParsedDocument, ParsedElement, TextBlock, MIN_IMAGE_WIDTH, MIN_IMAGE_HEIGHT, MIN_IMAGE_BYTES};

/// Normalize common Unicode ligatures and problematic codepoints that
/// `pdf-extract` produces from embedded fonts.
///
/// This cleans up:
///   - fi/fl/ffi/ffl ligatures (U+FB01..U+FB04)
///   - Long-s variants, typographic hyphens, etc.
fn normalize_pdf_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            // Soft hyphen → remove
            '\u{00AD}' => {}
            // Non-breaking space → regular space
            '\u{00A0}' => out.push(' '),
            // En-dash / Em-dash → ASCII hyphen (optional, keeps text searchable)
            '\u{2013}' | '\u{2014}' => out.push('-'),
            // Horizontal ellipsis → three dots
            '\u{2026}' => out.push_str("..."),
            // Private-use area chars (U+F8xx) from symbol fonts → skip them
            ch if ('\u{F800}'..='\u{F8FF}').contains(&ch) => {}
            other => out.push(other),
        }
    }
    out
}

/// Parse a PDF document, extracting text and optionally images/tables.
///
/// - `path`: path to the .pdf file
/// - `media_dir`: if `Some`, extracted images and tables are saved here as PNG
pub fn parse(path: &Path, media_dir: Option<&Path>) -> Result<ParsedDocument, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Failed to read PDF: {e}"))?;

    let title = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("document.pdf")
        .to_string();

    // ── 1. Text extraction (pdf-extract) ──
    let raw_text = pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|e| format!("PDF extraction error: {e}"))?;

    // Normalize ligatures and problematic Unicode codepoints
    let text = normalize_pdf_text(&raw_text);

    log::debug!("PDF raw text length: {} chars", text.len());

    let sections: Vec<TextBlock> = if text.contains('\u{000C}') {
        text.split('\u{000C}')
            .enumerate()
            .filter(|(_, page)| !page.trim().is_empty())
            .map(|(i, page)| TextBlock {
                text: page.trim().to_string(),
                metadata: format!("page:{}", i + 1),
            })
            .collect()
    } else {
        text.split("\n\n")
            .filter(|p| !p.trim().is_empty())
            .enumerate()
            .map(|(i, para)| TextBlock {
                text: para.trim().to_string(),
                metadata: format!("paragraph:{}", i + 1),
            })
            .collect()
    };

    log::debug!("PDF parsed into {} text sections", sections.len());

    // ── 2. Build text elements ──
    let mut elements: Vec<ParsedElement> = sections
        .iter()
        .map(|s| ParsedElement::text(&s.text, &s.metadata))
        .collect();

    // ── 3. Media extraction (pdfium-render) — only if media_dir is provided ──
    if let Some(media_dir) = media_dir {
        if let Err(e) = std::fs::create_dir_all(media_dir) {
            log::error!("❌ Failed to create media dir {}: {e}", media_dir.display());
        } else {
            match extract_media_pdfium(&bytes, media_dir, &sections) {
                Ok(media_elements) => {
                    log::info!(
                        "Extracted {} media elements from PDF (dir={})",
                        media_elements.len(),
                        media_dir.display()
                    );
                    elements.extend(media_elements);
                }
                Err(e) => {
                    log::error!(
                        "❌ PDF media extraction FAILED — images/tables will NOT be available: {e}"
                    );
                }
            }
        }
    } else {
        log::debug!("No media_dir provided — skipping image/table extraction");
    }

    Ok(ParsedDocument {
        title,
        elements,
        sections,
    })
}

/// Resolve the bundled libpdfium shared library.
///
/// Walks up from `current_exe()` probing the same subdirectories used by the
/// llama-server and espeak-ng resolvers (`src-tauri/bin`, `bin`, `resources/bin`),
/// looking inside the platform-specific `pdfium-{os}/` folder.
fn resolve_pdfium_library() -> Result<Box<dyn pdfium_render::prelude::PdfiumLibraryBindings>, String> {
    use pdfium_render::prelude::Pdfium;

    let os_folder = if cfg!(windows) {
        "pdfium-win"
    } else if cfg!(target_os = "macos") {
        "pdfium-mac"
    } else {
        "pdfium-lin"
    };

    let lib_name = if cfg!(windows) {
        "pdfium.dll"
    } else if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else {
        "libpdfium.so"
    };

    log::debug!("Resolving pdfium: looking for {os_folder}/{lib_name}");

    match crate::paths::resolve_bundled_library(os_folder, lib_name) {
        Ok(candidate) => {
            let dir = candidate.parent().unwrap().to_str().unwrap();
            log::info!("Found bundled pdfium at: {}", candidate.display());
            let lib_path = Pdfium::pdfium_platform_library_name_at_path(dir);
            log::debug!("Binding to pdfium library: {}", lib_path.display());
            Pdfium::bind_to_library(&lib_path)
                .map_err(|e| format!("Failed to bind to bundled pdfium at {}: {e}", candidate.display()))
        }
        Err(e) => {
            log::warn!("Bundled pdfium not found: {e}");
            Err(format!("Bundled libpdfium not found. {e}"))
        }
    }
}

/// Extract images and table regions from a PDF using pdfium-render.
///
/// For each page:
///   1. Iterate `PdfPageImageObject`s → extract embedded images
///   2. Detect table regions (clusters of line objects) → render as PNG
///
/// Each media element gets a context-aware caption from surrounding text.
///
/// Returns `Err` if libpdfium cannot be found — the caller treats this as a
/// soft failure and continues with text-only output.
fn extract_media_pdfium(
    pdf_bytes: &[u8],
    media_dir: &Path,
    text_sections: &[TextBlock],
) -> Result<Vec<ParsedElement>, String> {
    use pdfium_render::prelude::*;

    // Try bundled library first, then fall back to system library
    let bindings = resolve_pdfium_library()
        .or_else(|bundled_err| {
            log::debug!("Bundled pdfium not found ({bundled_err}), trying system library");
            Pdfium::bind_to_system_library()
                .map_err(|sys_err| format!(
                    "libpdfium not available. Bundled: {bundled_err}. System: {sys_err}"
                ))
        })?;

    let pdfium = Pdfium::new(bindings);

    let doc = pdfium
        .load_pdf_from_byte_slice(pdf_bytes, None)
        .map_err(|e| format!("pdfium load error: {e}"))?;

    let mut media_elements = Vec::new();
    let mut image_counter = 0u32;
    let mut table_counter = 0u32;

    for (page_idx, page) in doc.pages().iter().enumerate() {
        let page_num = page_idx + 1;

        // Context text for this page (from the text sections)
        let page_context = text_sections
            .iter()
            .find(|s| s.metadata == format!("page:{page_num}"))
            .map(|s| s.text.as_str())
            .unwrap_or("");

        // ── Extract embedded images ──
        for obj in page.objects().iter() {
            if let Some(image_obj) = obj.as_image_object() {
                match extract_image_object(
                    &image_obj,
                    media_dir,
                    page_num,
                    &mut image_counter,
                    page_context,
                ) {
                    Ok(Some(element)) => media_elements.push(element),
                    Ok(None) => {} // filtered out by size heuristic
                    Err(e) => log::debug!("Skip image on page {page_num}: {e}"),
                }
            }
        }

        // ── Detect and render table regions ──
        let table_rects = detect_table_regions(&page);
        for rect in &table_rects {
            match render_table_region(
                &page,
                rect,
                media_dir,
                page_num,
                &mut table_counter,
                page_context,
            ) {
                Ok(Some(element)) => media_elements.push(element),
                Ok(None) => {}
                Err(e) => log::debug!("Skip table on page {page_num}: {e}"),
            }
        }
    }

    Ok(media_elements)
}

/// Extract a single image object from a PDF page.
/// Returns `None` if the image is too small (icon/decoration).
fn extract_image_object(
    image_obj: &pdfium_render::prelude::PdfPageImageObject,
    media_dir: &Path,
    page_num: usize,
    counter: &mut u32,
    page_context: &str,
) -> Result<Option<ParsedElement>, String> {
    use image::ImageFormat;

    // Get the raw image from the PDF object
    let dyn_image = image_obj
        .get_raw_image()
        .map_err(|e| format!("get_raw_image: {e}"))?;

    let (w, h) = (dyn_image.width(), dyn_image.height());

    // Size heuristic filter — skip tiny icons/decorations
    if w < MIN_IMAGE_WIDTH || h < MIN_IMAGE_HEIGHT {
        return Ok(None);
    }

    *counter += 1;
    let filename = format!("pdf_p{page_num}_img{counter}.png");
    let out_path = media_dir.join(&filename);

    dyn_image
        .save_with_format(&out_path, ImageFormat::Png)
        .map_err(|e| format!("save image: {e}"))?;

    // Check file size heuristic
    let file_size = std::fs::metadata(&out_path)
        .map(|m| m.len() as usize)
        .unwrap_or(0);
    if file_size < MIN_IMAGE_BYTES {
        let _ = std::fs::remove_file(&out_path);
        return Ok(None);
    }

    // Build context-aware caption from surrounding ~400 chars of page text
    let caption = build_context_caption(page_context, *counter as usize);

    log::debug!(
        "Extracted image: {filename} ({w}x{h}, {:.1}KB)",
        file_size as f64 / 1024.0
    );

    Ok(Some(ParsedElement::image(
        caption,
        out_path,
        format!("page:{page_num}:image:{counter}"),
    )))
}

/// Detect table regions on a page by finding dense clusters of line/path objects.
///
/// Heuristic: if a rectangular region contains ≥4 horizontal lines and ≥2 vertical
/// lines in close proximity, it's likely a table. Returns bounding rectangles.
fn detect_table_regions(
    page: &pdfium_render::prelude::PdfPage,
) -> Vec<(f32, f32, f32, f32)> {
    use pdfium_render::prelude::*;

    let mut h_lines: Vec<(f32, f32, f32, f32)> = Vec::new(); // (x1, y1, x2, y2)
    let mut v_lines: Vec<(f32, f32, f32, f32)> = Vec::new();

    for obj in page.objects().iter() {
        if obj.as_path_object().is_some() {
            // Use the object's bounding box to detect lines
            let bounds = obj.bounds();
            if let Ok(bounds) = bounds {
                let bw = bounds.right().value - bounds.left().value;
                let bh = bounds.top().value - bounds.bottom().value;

                // Horizontal line: wide and thin
                if bw > 50.0 && bh < 3.0 {
                    h_lines.push((
                        bounds.left().value,
                        bounds.bottom().value,
                        bounds.right().value,
                        bounds.top().value,
                    ));
                }
                // Vertical line: tall and thin
                else if bh > 20.0 && bw < 3.0 {
                    v_lines.push((
                        bounds.left().value,
                        bounds.bottom().value,
                        bounds.right().value,
                        bounds.top().value,
                    ));
                }
            }
        }
    }

    // Cluster horizontal lines that are spatially close into table regions
    if h_lines.len() < 4 {
        return vec![];
    }

    // Sort by Y position
    h_lines.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut table_rects: Vec<(f32, f32, f32, f32)> = Vec::new();
    let mut cluster_start = 0;

    for i in 1..h_lines.len() {
        let gap = (h_lines[i].1 - h_lines[i - 1].1).abs();
        // If gap between consecutive lines is too large, break the cluster
        if gap > 100.0 {
            if i - cluster_start >= 3 {
                // Found a cluster of ≥3 lines — compute bounding box
                let min_x = h_lines[cluster_start..i]
                    .iter()
                    .map(|l| l.0)
                    .fold(f32::MAX, f32::min);
                let min_y = h_lines[cluster_start..i]
                    .iter()
                    .map(|l| l.1)
                    .fold(f32::MAX, f32::min);
                let max_x = h_lines[cluster_start..i]
                    .iter()
                    .map(|l| l.2)
                    .fold(f32::MIN, f32::max);
                let max_y = h_lines[cluster_start..i]
                    .iter()
                    .map(|l| l.3)
                    .fold(f32::MIN, f32::max);

                table_rects.push((min_x, min_y, max_x, max_y));
            }
            cluster_start = i;
        }
    }
    // Final cluster
    if h_lines.len() - cluster_start >= 3 {
        let min_x = h_lines[cluster_start..]
            .iter()
            .map(|l| l.0)
            .fold(f32::MAX, f32::min);
        let min_y = h_lines[cluster_start..]
            .iter()
            .map(|l| l.1)
            .fold(f32::MAX, f32::min);
        let max_x = h_lines[cluster_start..]
            .iter()
            .map(|l| l.2)
            .fold(f32::MIN, f32::max);
        let max_y = h_lines[cluster_start..]
            .iter()
            .map(|l| l.3)
            .fold(f32::MIN, f32::max);

        table_rects.push((min_x, min_y, max_x, max_y));
    }

    table_rects
}

/// Render a rectangular region of a PDF page to a PNG image.
fn render_table_region(
    page: &pdfium_render::prelude::PdfPage,
    rect: &(f32, f32, f32, f32),
    media_dir: &Path,
    page_num: usize,
    counter: &mut u32,
    page_context: &str,
) -> Result<Option<ParsedElement>, String> {
    use pdfium_render::prelude::*;

    let (min_x, min_y, max_x, max_y) = *rect;
    let region_w = (max_x - min_x).max(1.0);
    let region_h = (max_y - min_y).max(1.0);

    // Skip tiny regions
    if region_w < 100.0 || region_h < 40.0 {
        return Ok(None);
    }

    // Render the entire page at high DPI, then crop the table region
    let page_width = page.width().value;
    let page_height = page.height().value;

    let render_dpi = 200; // Render at 200 DPI for readable tables
    let scale = render_dpi as f32 / 72.0;

    let render_config = PdfRenderConfig::new()
        .set_target_width((page_width * scale) as i32)
        .set_target_height((page_height * scale) as i32);

    let bitmap = page
        .render_with_config(&render_config)
        .map_err(|e| format!("render page: {e}"))?;

    let dyn_image = bitmap
        .as_image();

    // Calculate crop coordinates in pixel space
    let px_x = ((min_x / page_width) * dyn_image.width() as f32).max(0.0) as u32;
    // PDF coordinate system has origin at bottom-left, image at top-left
    let px_y = (((page_height - max_y) / page_height) * dyn_image.height() as f32).max(0.0) as u32;
    let px_w = ((region_w / page_width) * dyn_image.width() as f32).min(dyn_image.width() as f32 - px_x as f32) as u32;
    let px_h = ((region_h / page_height) * dyn_image.height() as f32).min(dyn_image.height() as f32 - px_y as f32) as u32;

    if px_w < MIN_IMAGE_WIDTH || px_h < MIN_IMAGE_HEIGHT {
        return Ok(None);
    }

    let cropped = image::DynamicImage::from(dyn_image.to_owned()).crop_imm(px_x, px_y, px_w, px_h);

    *counter += 1;
    let filename = format!("pdf_p{page_num}_tbl{counter}.png");
    let out_path = media_dir.join(&filename);

    cropped
        .save(&out_path)
        .map_err(|e| format!("save table image: {e}"))?;

    let file_size = std::fs::metadata(&out_path)
        .map(|m| m.len() as usize)
        .unwrap_or(0);
    if file_size < MIN_IMAGE_BYTES {
        let _ = std::fs::remove_file(&out_path);
        return Ok(None);
    }

    let caption = build_context_caption(page_context, *counter as usize);

    log::debug!(
        "Rendered table: {filename} ({px_w}x{px_h}, {:.1}KB)",
        file_size as f64 / 1024.0
    );

    Ok(Some(ParsedElement::table(
        caption,
        out_path,
        format!("page:{page_num}:table:{counter}"),
    )))
}

/// Build a context-aware caption from the surrounding page text.
/// Takes ~200 chars before and ~200 chars after the estimated position
/// of the media element within the page text.
fn build_context_caption(page_text: &str, element_index: usize) -> String {
    if page_text.is_empty() {
        return format!("(embedded media #{element_index})");
    }

    let chars: Vec<char> = page_text.chars().collect();
    let total = chars.len();
    let context_radius = 200;

    // Estimate position as a fraction of the page (crude but effective)
    let est_pos = total / 2; // default: middle of page text

    let start = est_pos.saturating_sub(context_radius);
    let end = (est_pos + context_radius).min(total);

    let snippet: String = chars[start..end].iter().collect();
    snippet.trim().to_string()
}
