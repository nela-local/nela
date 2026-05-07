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
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

const PODCAST_MIN_TOP_K: usize = 6;
const PODCAST_MAX_TOP_K: usize = 24;
const PODCAST_FEEDBACK_WINDOW: usize = 8;

#[derive(Debug, Clone, Copy)]
struct PodcastRunFeedback {
    requested_turns: usize,
    produced_lines: usize,
}

fn podcast_feedback_store() -> &'static Mutex<VecDeque<PodcastRunFeedback>> {
    static PODCAST_FEEDBACK: OnceLock<Mutex<VecDeque<PodcastRunFeedback>>> = OnceLock::new();
    PODCAST_FEEDBACK.get_or_init(|| Mutex::new(VecDeque::with_capacity(PODCAST_FEEDBACK_WINDOW)))
}

fn record_podcast_feedback(requested_turns: usize, produced_lines: usize) {
    if let Ok(mut store) = podcast_feedback_store().lock() {
        if store.len() >= PODCAST_FEEDBACK_WINDOW {
            let _ = store.pop_front();
        }
        store.push_back(PodcastRunFeedback {
            requested_turns,
            produced_lines,
        });
    }
}

fn adaptive_adjustment_from_feedback(
    runs: impl DoubleEndedIterator<Item = PodcastRunFeedback>,
    target_turns: usize,
) -> i32 {
    let mut weighted_shortfall = 0.0f32;
    let mut weight_sum = 0.0f32;
    let mut success_weight = 0.0f32;

    for (idx, run) in runs.rev().take(PODCAST_FEEDBACK_WINDOW).enumerate() {
        let recency_weight = 1.0f32 / (idx as f32 + 1.0);
        let turn_gap = run.requested_turns.abs_diff(target_turns);
        let similarity_weight = if turn_gap <= 6 {
            1.0
        } else if turn_gap <= 12 {
            0.7
        } else {
            0.45
        };
        let weight = recency_weight * similarity_weight;

        let denom = run.requested_turns.max(1) as f32;
        let shortfall = (run.requested_turns.saturating_sub(run.produced_lines)) as f32 / denom;

        weighted_shortfall += shortfall * weight;
        weight_sum += weight;
        if run.produced_lines >= run.requested_turns {
            success_weight += weight;
        }
    }

    if weight_sum <= f32::EPSILON {
        return 0;
    }

    let avg_shortfall = weighted_shortfall / weight_sum;
    let success_ratio = success_weight / weight_sum;

    if avg_shortfall >= 0.35 {
        4
    } else if avg_shortfall >= 0.22 {
        3
    } else if avg_shortfall >= 0.12 {
        2
    } else if avg_shortfall >= 0.05 {
        1
    } else if success_ratio >= 0.95 {
        -1
    } else {
        0
    }
}

fn dynamic_top_k_with_feedback(query: &str, max_turns: usize) -> usize {
    let base_top_k = dynamic_top_k_for_podcast(query, max_turns) as i32;
    let adjustment = podcast_feedback_store()
        .lock()
        .map(|store| adaptive_adjustment_from_feedback(store.iter().copied(), max_turns))
        .unwrap_or(0);

    (base_top_k + adjustment)
        .clamp(PODCAST_MIN_TOP_K as i32, PODCAST_MAX_TOP_K as i32)
        as usize
}

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
    let uses_auto_top_k = request.top_k.is_none();
    let top_k = request
        .top_k
        .unwrap_or_else(|| dynamic_top_k_with_feedback(&request.query, request.max_turns));

    // ─── STAGE 1: RAG Retrieval ───────────────────────────────────────────
    emit_progress(
        app,
        "rag",
        &format!("Retrieving context from documents (top_k={top_k})..."),
        0.05,
    );

    let rag_result = rag_pipeline
        .query(&request.query, top_k)
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

    if uses_auto_top_k {
        record_podcast_feedback(request.max_turns, lines.len());
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

    // ─── STAGE 3: TTS for each line (concurrent) ────────────────────────────
    emit_progress(
        app,
        "tts",
        &format!("Synthesizing {} dialogue lines concurrently...", lines.len()),
        0.35,
    );

    // Dispatch all TTS synthesis tasks simultaneously. KittenTTS is an
    // in-process ONNX backend: each call runs in a spawn_blocking thread and
    // the ONNX session supports concurrent inference, so N lines take roughly
    // the time of the single slowest line instead of N × average line time.
    let tts_tasks: Vec<_> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let router = Arc::clone(&router);
            let speaker = line.speaker.clone();
            let voice = line.voice.clone();
            let text = line.text.clone();
            let line_clone = line.clone();
            async move {
                let mut extra = HashMap::new();
                extra.insert("voice".to_string(), voice);

                let tts_request = TaskRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    task_type: TaskType::Tts,
                    input: text,
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
                        return Err(format!(
                            "TTS failed for line {} ({}): {}",
                            i, speaker, e
                        ))
                    }
                };

                let wav_bytes = std::fs::read(&wav_path)
                    .map_err(|e| format!("Failed to read WAV for line {}: {e}", i))?;
                let _ = std::fs::remove_file(&wav_path);

                Ok::<(usize, PodcastLine, Vec<u8>), String>((i, line_clone, wav_bytes))
            }
        })
        .collect();

    let tts_results = futures_util::future::join_all(tts_tasks).await;

    // Validate all tasks succeeded and restore original script ordering.
    // join_all preserves insertion order, but sort_unstable_by_key is cheap
    // and makes the ordering guarantee explicit.
    let mut indexed: Vec<(usize, PodcastLine, Vec<u8>)> = Vec::with_capacity(lines.len());
    for result in tts_results {
        indexed.push(result?);
    }
    indexed.sort_unstable_by_key(|(i, _, _)| *i);

    let mut segments: Vec<PodcastSegment> = Vec::with_capacity(indexed.len());
    let mut all_wav_bytes: Vec<Vec<u8>> = Vec::with_capacity(indexed.len());

    for (_, line, wav_bytes) in indexed {
        let b64 = STANDARD.encode(&wav_bytes);
        let data_url = format!("data:audio/wav;base64,{b64}");
        all_wav_bytes.push(wav_bytes);
        segments.push(PodcastSegment {
            line,
            audio_data_url: data_url,
        });
    }

    emit_progress(
        app,
        "tts",
        &format!("All {} lines synthesized", lines.len()),
        0.90,
    );

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

/// Dynamically size RAG retrieval for podcasts.
///
/// Larger requested conversations need broader context coverage. We scale with
/// requested turns and query complexity, then clamp to a safe range
/// to avoid over-retrieval.
fn dynamic_top_k_for_podcast(query: &str, max_turns: usize) -> usize {
    let turns = max_turns.clamp(4, 60);
    let query_words = query.split_whitespace().count();

    // Aggressive scaling for longer episodes: roughly one context chunk per
    // two requested turns, plus an extra boost for very long conversations.
    let turn_component = turns.div_ceil(2);
    let long_conversation_boost = if turns >= 30 {
        3
    } else if turns >= 18 {
        2
    } else {
        0
    };

    let query_component = if query_words > 30 {
        4
    } else if query_words > 18 {
        3
    } else if query_words > 8 {
        2
    } else {
        1
    };

    (turn_component + long_conversation_boost + query_component)
        .clamp(PODCAST_MIN_TOP_K, PODCAST_MAX_TOP_K)
}

#[cfg(test)]
mod tests {
    use super::{adaptive_adjustment_from_feedback, dynamic_top_k_for_podcast, PodcastRunFeedback};

    #[test]
    fn dynamic_top_k_scales_with_turns() {
        let short = dynamic_top_k_for_podcast("summarize transformers", 6);
        let long = dynamic_top_k_for_podcast("summarize transformers", 24);
        assert!(long > short, "top_k should grow with requested turns");
    }

    #[test]
    fn dynamic_top_k_scales_with_query_complexity() {
        let simple = dynamic_top_k_for_podcast("quantization", 10);
        let complex = dynamic_top_k_for_podcast(
            "compare quantization and pruning versus distillation for retrieval augmented generation tradeoffs in edge deployment scenarios",
            10,
        );
        assert!(complex > simple, "top_k should grow for more complex queries");
    }

    #[test]
    fn dynamic_top_k_is_bounded() {
        let low = dynamic_top_k_for_podcast("a", 1);
        let high = dynamic_top_k_for_podcast("word ".repeat(100).trim(), 200);
        assert_eq!(low, 6);
        assert_eq!(high, 24);
    }

    #[test]
    fn adaptive_feedback_increases_top_k_after_shortfalls() {
        let history = vec![
            PodcastRunFeedback {
                requested_turns: 20,
                produced_lines: 11,
            },
            PodcastRunFeedback {
                requested_turns: 18,
                produced_lines: 10,
            },
            PodcastRunFeedback {
                requested_turns: 22,
                produced_lines: 13,
            },
        ];

        let adjustment = adaptive_adjustment_from_feedback(history.into_iter(), 20);
        assert!(adjustment >= 2, "shortfalls should increase future top_k");
    }

    #[test]
    fn adaptive_feedback_can_reduce_top_k_after_consistent_success() {
        let history = vec![
            PodcastRunFeedback {
                requested_turns: 12,
                produced_lines: 12,
            },
            PodcastRunFeedback {
                requested_turns: 10,
                produced_lines: 11,
            },
            PodcastRunFeedback {
                requested_turns: 14,
                produced_lines: 14,
            },
            PodcastRunFeedback {
                requested_turns: 12,
                produced_lines: 12,
            },
        ];

        let adjustment = adaptive_adjustment_from_feedback(history.into_iter(), 12);
        assert_eq!(adjustment, -1);
    }
}
