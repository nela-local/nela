//! Podcast engine — orchestrates RAG → Script → TTS → Merge.
//!
//! Uses the existing `RagPipeline` for retrieval and the `TaskRouter`
//! for both LLM script generation and KittenTTS voice synthesis.

use crate::podcast::script::{build_script_prompt, parse_script_response};
use crate::podcast::types::*;
use crate::rag::pipeline::RagPipeline;
use crate::registry::types::{TaskRequest, TaskResponse, TaskType};
use crate::router::TaskRouter;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

/// Run the complete podcast generation pipeline.
///
/// # Stages
/// 1. **RAG** — retrieve relevant context from ingested documents
/// 2. **Scripting** — LLM generates a two-person dialogue
/// 3. **TTS** — each line is synthesized with the assigned voice
/// 4. **Merging** — all WAV segments are concatenated
pub async fn generate_podcast(
    app: &AppHandle,
    request: PodcastRequest,
    rag_pipeline: Arc<RagPipeline>,
    router: Arc<TaskRouter>,
) -> Result<PodcastResult, String> {
    // ─── STAGE 1: RAG Retrieval ───────────────────────────────────────────
    emit_progress(app, "rag", "Retrieving context from documents...", 0.05);

    let rag_result = rag_pipeline
        .query(&request.query, request.top_k)
        .await
        .map_err(|e| format!("RAG query failed: {e}"))?;

    // Collect source texts
    let source_chunks: Vec<String> = rag_result
        .sources
        .iter()
        .map(|s| s.text.clone())
        .collect();

    if source_chunks.is_empty() {
        return Err(
            "No relevant documents found. Please ingest documents first.".to_string(),
        );
    }

    let rag_context = source_chunks.join("\n\n---\n\n");
    emit_progress(
        app,
        "rag",
        &format!("Retrieved {} relevant chunks", source_chunks.len()),
        0.15,
    );

    // ─── STAGE 2: Script Generation ───────────────────────────────────────
    emit_progress(app, "scripting", "Generating podcast script...", 0.20);

    let prompt = build_script_prompt(
        &request.query,
        &rag_context,
        &request.speaker_a_name,
        &request.speaker_b_name,
        request.max_turns,
    );

    // Send as a PodcastScript task — routes to the highest-priority model
    // for podcast generation (separate from general chat priority).
    // Set high max_tokens so the LLM can produce all requested dialogue turns.
    // ~60 tokens per turn × max_turns, plus JSON overhead. Minimum 1024.
    let needed_tokens = (request.max_turns * 80).max(1024);
    let mut extra = HashMap::new();
    extra.insert("max_tokens".to_string(), needed_tokens.to_string());
    extra.insert("temperature".to_string(), "0.7".to_string());

    let chat_request = TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::PodcastScript,
        input: prompt,
        model_override: None,
        extra,
    };

    let script_text = match router.route(&chat_request).await {
        Ok(TaskResponse::Text(text)) => text,
        Ok(other) => {
            return Err(format!(
                "Unexpected response type from LLM: {:?}",
                other
            ))
        }
        Err(e) => return Err(format!("Script generation failed: {e}")),
    };

    emit_progress(app, "scripting", "Parsing generated script...", 0.30);

    let lines = parse_script_response(
        &script_text,
        &request.speaker_a_name,
        &request.speaker_b_name,
        &request.voice_a,
        &request.voice_b,
    )?;

    if lines.is_empty() {
        return Err("LLM generated an empty script".to_string());
    }

    if lines.len() < request.max_turns {
        log::warn!(
            "[podcast] Requested {} dialogue lines but LLM only produced {} (output may have been truncated)",
            request.max_turns,
            lines.len()
        );
    }

    let script = PodcastScript {
        title: format!("Podcast: {}", truncate_str(&request.query, 60)),
        lines: lines.clone(),
        source_chunks: source_chunks.clone(),
    };

    emit_progress(
        app,
        "scripting",
        &format!("Script ready — {} dialogue lines", lines.len()),
        0.35,
    );

    // ─── STAGE 3: TTS for each line ───────────────────────────────────────
    let mut segments: Vec<PodcastSegment> = Vec::new();
    let mut all_wav_bytes: Vec<Vec<u8>> = Vec::new();
    let total_lines = lines.len() as f32;

    for (i, line) in lines.iter().enumerate() {
        let progress = 0.35 + (0.55 * (i as f32 / total_lines));
        emit_progress(
            app,
            "tts",
            &format!(
                "Synthesizing line {}/{} ({})...",
                i + 1,
                lines.len(),
                line.speaker
            ),
            progress,
        );

        // Build TTS request with the voice parameter
        let mut extra = HashMap::new();
        extra.insert("voice".to_string(), line.voice.clone());

        let tts_request = TaskRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            task_type: TaskType::Tts,
            input: line.text.clone(),
            model_override: None,
            extra,
        };

        let wav_path = match router.route(&tts_request).await {
            Ok(TaskResponse::FilePath(path)) => path,
            Ok(other) => {
                return Err(format!(
                    "Unexpected TTS response for line {}: {:?}",
                    i, other
                ))
            }
            Err(e) => {
                return Err(format!("TTS failed for line {} ({}): {}", i, line.speaker, e))
            }
        };

        // Read the WAV bytes
        let wav_bytes = std::fs::read(&wav_path)
            .map_err(|e| format!("Failed to read WAV for line {}: {e}", i))?;

        // Clean up temp file
        let _ = std::fs::remove_file(&wav_path);

        // Create base64 data URL for individual segment
        let b64 = STANDARD.encode(&wav_bytes);
        let data_url = format!("data:audio/wav;base64,{b64}");

        all_wav_bytes.push(wav_bytes);

        segments.push(PodcastSegment {
            line: line.clone(),
            audio_data_url: data_url,
        });
    }

    // ─── STAGE 4: Merge audio segments ────────────────────────────────────
    emit_progress(app, "merging", "Combining audio segments...", 0.92);

    let combined_bytes = merge_wav_bytes(&all_wav_bytes)?;
    let combined_b64 = STANDARD.encode(&combined_bytes);
    let combined_data_url = format!("data:audio/wav;base64,{combined_b64}");

    emit_progress(app, "done", "Podcast ready!", 1.0);

    Ok(PodcastResult {
        script,
        segments,
        combined_audio_data_url: combined_data_url,
    })
}

/// Concatenate multiple WAV files (raw bytes) into a single WAV.
///
/// Assumes all WAVs share the same format (sample rate, channels, bit depth)
/// which is guaranteed since they all come from KittenTTS (24kHz mono 16-bit).
fn merge_wav_bytes(wav_files: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    if wav_files.is_empty() {
        return Err("No audio segments to merge".to_string());
    }

    if wav_files.len() == 1 {
        return Ok(wav_files[0].clone());
    }

    let first = &wav_files[0];
    if first.len() < 44 {
        return Err("First WAV file is too small (missing header)".to_string());
    }

    // Collect all raw PCM data (skip 44-byte WAV header from each file)
    let mut all_pcm: Vec<u8> = Vec::new();
    for (i, wav) in wav_files.iter().enumerate() {
        if wav.len() <= 44 {
            log::warn!("WAV segment {} is empty or header-only, skipping", i);
            continue;
        }
        all_pcm.extend_from_slice(&wav[44..]);
    }

    if all_pcm.is_empty() {
        return Err("All audio segments were empty".to_string());
    }

    // Build a new WAV header with the combined data length
    let mut header = first[..44].to_vec();
    let data_size = all_pcm.len() as u32;
    let file_size = data_size + 36;

    // Update RIFF chunk size (offset 4, little-endian u32)
    header[4..8].copy_from_slice(&file_size.to_le_bytes());
    // Update data sub-chunk size (offset 40, little-endian u32)
    header[40..44].copy_from_slice(&data_size.to_le_bytes());

    let mut result = header;
    result.extend_from_slice(&all_pcm);
    Ok(result)
}

/// Emit a progress event to the frontend.
fn emit_progress(app: &AppHandle, stage: &str, detail: &str, progress: f32) {
    log::info!("[podcast] [{stage}] {detail} ({:.0}%)", progress * 100.0);
    let _ = app.emit(
        "podcast-progress",
        PodcastProgress {
            stage: stage.to_string(),
            detail: detail.to_string(),
            progress,
        },
    );
}

fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
