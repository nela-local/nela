//! Script generation — builds LLM prompt and parses dialogue JSON.

use crate::podcast::types::PodcastLine;

/// Build a single prompt string for the LLM to generate a podcast script.
///
/// Returns a single string (not messages array) because the llama-server
/// backend wraps the input in its own system/user message structure.
/// We embed all instructions directly so the LLM gets a clear, self-contained
/// prompt that asks for JSON output.
pub fn build_script_prompt(
    query: &str,
    rag_context: &str,
    speaker_a: &str,
    speaker_b: &str,
    max_turns: usize,
) -> String {
    format!(
        r#"Write a podcast dialogue between two hosts: {speaker_a} and {speaker_b}.

RULES:
1. {speaker_a} is the curious interviewer. {speaker_b} is the knowledgeable expert.
2. You MUST write EXACTLY {max_turns} dialogue lines. Not fewer, not more.
3. Alternate speakers: {speaker_a}, {speaker_b}, {speaker_a}, {speaker_b}, ...
4. Base ALL content on the CONTEXT below. Do not invent facts.
5. Keep each line conversational, 1-3 sentences.
6. Start with {speaker_a} introducing the topic. End with {speaker_b} wrapping up.
7. Respond with ONLY a valid JSON array. No markdown, no extra text.

CONTEXT:
{rag_context}

TOPIC: {query}

IMPORTANT: Output EXACTLY {max_turns} items in the JSON array.
JSON format (produce {max_turns} entries):
[{{"speaker":"{speaker_a}","text":"..."}},{{"speaker":"{speaker_b}","text":"..."}}]"#
    )
}

/// Parse the raw LLM response into structured `PodcastLine` entries.
///
/// Handles common LLM output quirks — markdown-wrapped JSON, trailing text, etc.
pub fn parse_script_response(
    raw_response: &str,
    speaker_a: &str,
    speaker_b: &str,
    voice_a: &str,
    voice_b: &str,
) -> Result<Vec<PodcastLine>, String> {
    log::info!("[podcast] Raw LLM script output ({} chars): {}",
        raw_response.len(),
        &raw_response[..raw_response.len().min(500)]
    );

    let json_str = extract_json_array(raw_response)?;
    log::debug!("[podcast] Extracted JSON ({} chars): {}",
        json_str.len(),
        &json_str[..json_str.len().min(300)]
    );

    let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str)
        .map_err(|e| format!("Failed to parse script JSON: {e}\nExtracted: {}",
            &json_str[..json_str.len().min(200)]
        ))?;

    if parsed.is_empty() {
        return Err("LLM returned an empty script".to_string());
    }

    let mut lines = Vec::new();
    for (i, entry) in parsed.iter().enumerate() {
        let speaker = entry["speaker"]
            .as_str()
            .ok_or_else(|| format!("Missing 'speaker' at line {i}"))?
            .to_string();

        let text = entry["text"]
            .as_str()
            .ok_or_else(|| format!("Missing 'text' at line {i}"))?
            .to_string();

        // Map speaker name → voice
        let voice = if speaker == speaker_a {
            voice_a.to_string()
        } else if speaker == speaker_b {
            voice_b.to_string()
        } else {
            // Unknown speaker — default to voice_a
            log::warn!("Unknown speaker '{}' at line {}, defaulting to voice_a", speaker, i);
            voice_a.to_string()
        };

        lines.push(PodcastLine {
            speaker,
            voice,
            text,
            index: i,
        });
    }

    Ok(lines)
}

/// Extract a JSON array from potentially wrapped LLM output.
///
/// Handles:
/// - Raw JSON: `[{...}, ...]`
/// - Markdown fences: ` ```json\n[...]\n``` `
/// - Prefix/suffix text around the array
/// - Nested brackets (finds the outermost balanced pair)
/// - **Truncated output** — if the LLM ran out of tokens mid-JSON,
///   we find the last complete object `}` and close the array there.
fn extract_json_array(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Err("LLM returned empty response".to_string());
    }

    // Find the first '[' character
    let start = match trimmed.find('[') {
        Some(pos) => pos,
        None => return Err(format!(
            "No JSON array found in LLM response (first 200 chars): {}",
            &trimmed[..trimmed.len().min(200)]
        )),
    };

    // Find the matching closing bracket by tracking bracket depth
    let mut depth = 0;
    let mut end_pos = None;
    for (i, ch) in trimmed[start..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end_pos = Some(start + i);
                    break;
                }
            }
            _ => {}
        }
    }

    match end_pos {
        Some(end) => Ok(trimmed[start..=end].to_string()),
        None => {
            // The JSON array is truncated (LLM ran out of tokens).
            // Try to salvage all complete objects by finding the last '}' that
            // closes top-level array entries, then appending ']'.
            log::warn!(
                "[podcast] JSON array is truncated — attempting repair. Raw length={}",
                trimmed.len()
            );
            try_repair_truncated_array(&trimmed[start..])
        }
    }
}

/// Attempt to repair a truncated JSON array by keeping only complete objects.
///
/// Walks through the array tracking brace depth. Each time we return to
/// depth-1 (i.e. top-level array) after a `}`, we record that as the last
/// safe cut point. We slice up to there and close with `]`.
fn try_repair_truncated_array(array_str: &str) -> Result<String, String> {
    let mut depth = 0i32;          // overall bracket/brace depth
    let mut last_complete_end = None; // byte index of last complete top-level `}`
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in array_str.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }

        match ch {
            '[' | '{' => depth += 1,
            '}' => {
                depth -= 1;
                // depth == 1 means we just closed a top-level object inside the array
                if depth == 1 {
                    last_complete_end = Some(i);
                }
            }
            ']' => {
                depth -= 1;
            }
            _ => {}
        }
    }

    if let Some(end) = last_complete_end {
        // Slice everything up to and including the last complete `}`
        let repaired = array_str[..=end].to_string();
        // Remove any trailing comma + whitespace before closing
        let trimmed_tail = repaired.trim_end().trim_end_matches(',').to_string();
        let result = format!("{}]", trimmed_tail);
        log::info!(
            "[podcast] Repaired truncated JSON — kept {} chars out of {}",
            result.len(),
            array_str.len()
        );
        Ok(result)
    } else {
        Err(format!(
            "No complete dialogue entries found in truncated LLM output (first 300 chars): {}",
            &array_str[..array_str.len().min(300)]
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_array_raw() {
        let input = r#"[{"speaker":"A","text":"Hello"},{"speaker":"B","text":"Hi"}]"#;
        let result = extract_json_array(input).unwrap();
        assert!(result.starts_with('['));
        assert!(result.ends_with(']'));
    }

    #[test]
    fn test_extract_json_array_with_markdown() {
        let input = "Here is the script:\n```json\n[{\"speaker\":\"A\",\"text\":\"Hello\"}]\n```\nDone.";
        let result = extract_json_array(input).unwrap();
        assert!(result.starts_with('['));
        assert!(result.ends_with(']'));
    }

    #[test]
    fn test_parse_script_response() {
        let raw = r#"[{"speaker":"Alex","text":"Welcome!"},{"speaker":"Sam","text":"Thanks for having me."}]"#;
        let lines = parse_script_response(raw, "Alex", "Sam", "Leo", "Bella").unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].speaker, "Alex");
        assert_eq!(lines[0].voice, "Leo");
        assert_eq!(lines[1].speaker, "Sam");
        assert_eq!(lines[1].voice, "Bella");
    }

    #[test]
    fn test_extract_truncated_json_repairs() {
        // Simulates LLM running out of tokens mid-sentence
        let input = r#"[{"speaker":"Alex","text":"Hello there!"},{"speaker":"Sam","text":"Welcome to the show!"},{"speaker":"Alex","text":"So tell me about thi"#;
        let result = extract_json_array(input).unwrap();
        // Should keep the first two complete objects
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["speaker"], "Alex");
        assert_eq!(parsed[1]["speaker"], "Sam");
    }

    #[test]
    fn test_extract_truncated_with_trailing_comma() {
        let input = r#"[{"speaker":"A","text":"One"},{"speaker":"B","text":"Two"},"#;
        let result = extract_json_array(input).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 2);
    }
}
