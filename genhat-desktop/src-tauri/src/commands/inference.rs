//! Inference commands — the main entry point for task routing.

use crate::registry::types::{TaskRequest, TaskResponse, TaskType};
use crate::router::TaskRouter;
use crate::backends::llama_cli::execute_vision_streaming;
use crate::commands::models::get_models_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, State};

fn parse_bool_param(value: &str, default: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

/// Managed state wrapper for the TaskRouter.
pub struct TaskRouterState(pub Arc<TaskRouter>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContextMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCompactionRequest {
    pub messages: Vec<ChatContextMessage>,
    pub context_window_tokens: Option<u32>,
    pub reserved_output_tokens: Option<u32>,
    pub threshold_percent: Option<f32>,
    pub allow_auto_compaction: Option<bool>,
    pub force_compaction: Option<bool>,
    pub preserve_recent_messages: Option<usize>,
    pub model_override: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContextUsage {
    pub context_window_tokens: u32,
    pub used_tokens: u32,
    pub reserved_output_tokens: u32,
    pub projected_tokens: u32,
    pub remaining_tokens: u32,
    pub remaining_after_reserve_tokens: u32,
    pub used_percent: f32,
    pub projected_percent: f32,
    pub threshold_percent: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCompactionResult {
    pub messages: Vec<ChatContextMessage>,
    pub usage: ChatContextUsage,
    pub compacted: bool,
    pub summary_applied: bool,
    pub dropped_messages: usize,
    pub reason: String,
    pub kept_indices: Vec<usize>,
    pub summary_insert_index: Option<usize>,
}

fn normalize_chat_role(role: &str) -> String {
    let normalized = role.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "user" | "assistant" | "system" => normalized,
        _ => "assistant".to_string(),
    }
}

fn estimate_text_tokens(content: &str) -> u32 {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return 1;
    }

    // Heuristic approximation tuned for local context budgeting:
    // - char estimate works well for punctuation-heavy text/code
    // - word estimate works better for natural language
    let chars_estimate = ((trimmed.chars().count() as f32) / 4.0).ceil() as u32;
    let words_estimate = ((trimmed.split_whitespace().count() as f32) * 1.25).ceil() as u32;
    chars_estimate.max(words_estimate).max(1)
}

fn estimate_message_tokens(message: &ChatContextMessage) -> u32 {
    // Include lightweight message framing overhead.
    estimate_text_tokens(&message.content) + 4
}

fn calc_context_usage(
    messages: &[ChatContextMessage],
    context_window_tokens: u32,
    reserved_output_tokens: u32,
    threshold_percent: f32,
) -> ChatContextUsage {
    let used_tokens: u32 = messages.iter().map(estimate_message_tokens).sum();
    let projected_tokens = used_tokens.saturating_add(reserved_output_tokens);
    let remaining_tokens = context_window_tokens.saturating_sub(used_tokens);
    let remaining_after_reserve_tokens = context_window_tokens.saturating_sub(projected_tokens);
    let denom = context_window_tokens.max(1) as f32;

    ChatContextUsage {
        context_window_tokens,
        used_tokens,
        reserved_output_tokens,
        projected_tokens,
        remaining_tokens,
        remaining_after_reserve_tokens,
        used_percent: ((used_tokens as f32 / denom) * 100.0).min(100.0),
        projected_percent: ((projected_tokens as f32 / denom) * 100.0).min(100.0),
        threshold_percent: threshold_percent * 100.0,
    }
}

fn is_generated_compaction_summary(content: &str) -> bool {
    let lowered = content.to_ascii_lowercase();
    lowered.starts_with("conversation summary (auto-compacted):")
        || lowered.starts_with("conversation summary (manual compaction):")
}

fn messages_equal(a: &[ChatContextMessage], b: &[ChatContextMessage]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(left, right)| left.role == right.role && left.content == right.content)
}

#[tauri::command]
pub async fn compact_chat_context(
    req: ContextCompactionRequest,
    router_state: State<'_, TaskRouterState>,
) -> Result<ContextCompactionResult, String> {
    let original = req
        .messages
        .iter()
        .map(|message| ChatContextMessage {
            role: normalize_chat_role(&message.role),
            content: message.content.trim().to_string(),
        })
        .filter(|message| !message.content.is_empty())
        .collect::<Vec<_>>();

    if original.is_empty() {
        let usage = calc_context_usage(&[], 4096, 512, 0.9);
        return Ok(ContextCompactionResult {
            messages: Vec::new(),
            usage,
            compacted: false,
            summary_applied: false,
            dropped_messages: 0,
            reason: "no-messages".to_string(),
            kept_indices: Vec::new(),
            summary_insert_index: None,
        });
    }

    let context_window_tokens = req.context_window_tokens.unwrap_or(4096).clamp(1024, 262_144);
    let max_reservable = context_window_tokens.saturating_sub(32).max(64);
    let reserved_output_tokens = req
        .reserved_output_tokens
        .unwrap_or(1024)
        .clamp(64, max_reservable);
    let threshold_percent = req.threshold_percent.unwrap_or(0.9).clamp(0.5, 0.99);
    let allow_auto_compaction = req.allow_auto_compaction.unwrap_or(true);
    let force_compaction = req.force_compaction.unwrap_or(false);
    let preserve_recent_messages = req.preserve_recent_messages.unwrap_or(8).clamp(2, 24);

    let usage_before = calc_context_usage(
        &original,
        context_window_tokens,
        reserved_output_tokens,
        threshold_percent,
    );

    if !force_compaction {
        if !allow_auto_compaction {
            return Ok(ContextCompactionResult {
                messages: original.clone(),
                usage: usage_before,
                compacted: false,
                summary_applied: false,
                dropped_messages: 0,
                reason: "analysis-only".to_string(),
                kept_indices: (0..original.len()).collect(),
                summary_insert_index: None,
            });
        }

        if usage_before.projected_percent < threshold_percent * 100.0 {
            return Ok(ContextCompactionResult {
                messages: original.clone(),
                usage: usage_before,
                compacted: false,
                summary_applied: false,
                dropped_messages: 0,
                reason: "below-threshold".to_string(),
                kept_indices: (0..original.len()).collect(),
                summary_insert_index: None,
            });
        }
    }

    let mut keep_indices = Vec::new();
    let mut non_system_indices = Vec::new();
    for (index, message) in original.iter().enumerate() {
        if message.role == "system" {
            if !is_generated_compaction_summary(&message.content) {
                keep_indices.push(index);
            }
        } else {
            non_system_indices.push(index);
        }
    }

    let keep_recent_count = preserve_recent_messages.min(non_system_indices.len()).max(2);
    let split_at = non_system_indices.len().saturating_sub(keep_recent_count);
    let older_indices = non_system_indices[..split_at].to_vec();
    let recent_indices = non_system_indices[split_at..].to_vec();
    keep_indices.extend(recent_indices.iter().copied());
    keep_indices.sort_unstable();
    keep_indices.dedup();

    let mut summary_applied = false;
    let mut summary_message: Option<ChatContextMessage> = None;

    if !older_indices.is_empty() {
        // Build a bounded transcript for summarization.
        let mut transcript = String::new();
        for idx in &older_indices {
            let message = &original[*idx];
            let label = if message.role == "user" { "User" } else { "Assistant" };
            let next_line = format!("{label}: {}\n", message.content);
            if transcript.len().saturating_add(next_line.len()) > 14_000 {
                break;
            }
            transcript.push_str(&next_line);
        }

        if !transcript.trim().is_empty() {
            let summary_prompt = format!(
                "Summarize the following previous conversation so it can be used as compact model context.\n\
                Keep it factual and concise. Preserve: user goals, key constraints, decisions, unresolved questions, and critical entities/values.\n\
                Do not include markdown or bullet symbols.\n\
                Limit to at most 8 short lines.\n\n\
                Conversation:\n{transcript}"
            );

            let mut extra = HashMap::new();
            extra.insert("max_tokens".to_string(), "320".to_string());
            extra.insert("temperature".to_string(), "0.2".to_string());

            let summarize_req = TaskRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                task_type: TaskType::Summarize,
                input: summary_prompt,
                model_override: req
                    .model_override
                    .clone()
                    .and_then(|v| if v.trim().is_empty() { None } else { Some(v) }),
                extra,
            };

            let summary_text = match router_state.0.route(&summarize_req).await {
                Ok(TaskResponse::Text(text)) => Some(text.trim().to_string()),
                Ok(TaskResponse::ChatWithThinking { content, .. }) => {
                    Some(content.trim().to_string())
                }
                Ok(other) => {
                    log::warn!("Unexpected response while compacting context: {:?}", other);
                    None
                }
                Err(err) => {
                    log::warn!("Summary generation failed during context compaction: {}", err);
                    None
                }
            };

            if let Some(summary) = summary_text {
                if !summary.is_empty() {
                    summary_applied = true;
                    let summary_prefix = if force_compaction {
                        "Conversation summary (manual compaction):"
                    } else {
                        "Conversation summary (auto-compacted):"
                    };
                    summary_message = Some(ChatContextMessage {
                        role: "system".to_string(),
                        content: format!("{summary_prefix}\n{summary}"),
                    });
                }
            }
        }
    }

    let mut compacted = keep_indices
        .iter()
        .filter_map(|idx| original.get(*idx).cloned())
        .collect::<Vec<_>>();
    let mut compacted_origins = keep_indices.iter().map(|idx| Some(*idx)).collect::<Vec<_>>();

    let mut summary_insert_index = None;
    if let Some(summary) = summary_message {
        let insert_at = compacted
            .iter()
            .position(|message| message.role != "system")
            .unwrap_or(compacted.len());
        compacted.insert(insert_at, summary);
        compacted_origins.insert(insert_at, None);
        summary_insert_index = Some(insert_at);
    }

    // If still near/exceeding threshold, trim oldest non-system turns while
    // preserving at least two recent non-system messages.
    loop {
        let usage_now = calc_context_usage(
            &compacted,
            context_window_tokens,
            reserved_output_tokens,
            threshold_percent,
        );

        if usage_now.projected_percent < threshold_percent * 100.0 {
            break;
        }

        let non_system_positions = compacted
            .iter()
            .enumerate()
            .filter_map(|(idx, message)| if message.role == "system" { None } else { Some(idx) })
            .collect::<Vec<_>>();

        if non_system_positions.len() <= 2 {
            break;
        }

        // Drop the oldest non-system message.
        let remove_at = non_system_positions[0];
        compacted.remove(remove_at);
        compacted_origins.remove(remove_at);

        if let Some(summary_idx) = summary_insert_index {
            if remove_at < summary_idx {
                summary_insert_index = Some(summary_idx - 1);
            } else if remove_at == summary_idx {
                summary_insert_index = None;
                summary_applied = false;
            }
        }
    }

    let mut final_kept_indices = compacted_origins
        .iter()
        .filter_map(|origin| *origin)
        .collect::<Vec<_>>();
    final_kept_indices.sort_unstable();
    final_kept_indices.dedup();

    let dropped_messages = original
        .len()
        .saturating_sub(final_kept_indices.len());

    let compacted_changed = !messages_equal(&original, &compacted);
    let reason = if !compacted_changed {
        if force_compaction {
            "manual-noop".to_string()
        } else {
            "below-threshold".to_string()
        }
    } else if summary_applied {
        "summary-and-trim".to_string()
    } else {
        "trim-only".to_string()
    };

    let usage_after = calc_context_usage(
        &compacted,
        context_window_tokens,
        reserved_output_tokens,
        threshold_percent,
    );

    Ok(ContextCompactionResult {
        messages: compacted,
        usage: usage_after,
        compacted: compacted_changed,
        summary_applied,
        dropped_messages,
        reason,
        kept_indices: final_kept_indices,
        summary_insert_index,
    })
}

/// Route a task request to the appropriate model and return the result.
///
/// This is the primary command for all inference — the frontend sends the
/// task type and input, and the router handles model selection and execution.
///
/// # Arguments
/// * `task_type` — One of: "chat", "summarize", "mindmap", "tts",
///   "transcribe", "embed", "classify", "enrich", "grade", "hyde"
/// * `input` — The input text/prompt
/// * `model_override` — Optional: force a specific model ID
/// * `extra` — Optional: additional key-value parameters (e.g., model_path for TTS)
#[tauri::command]
pub async fn route_request(
    task_type: String,
    input: String,
    model_override: Option<String>,
    extra: Option<HashMap<String, String>>,
    router_state: State<'_, TaskRouterState>,
) -> Result<TaskResponse, String> {
    let task = parse_task_type(&task_type)?;

    let request = TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: task,
        input,
        model_override,
        extra: extra.unwrap_or_default(),
    };

    router_state.0.route(&request).await
}

fn parse_task_type(s: &str) -> Result<TaskType, String> {
    match s {
        "chat" => Ok(TaskType::Chat),
        "vision_chat" => Ok(TaskType::VisionChat),
        "summarize" => Ok(TaskType::Summarize),
        "mindmap" => Ok(TaskType::Mindmap),
        "tts" => Ok(TaskType::Tts),
        "transcribe" => Ok(TaskType::Transcribe),
        "stt" => Ok(TaskType::Stt),
        "embed" => Ok(TaskType::Embed),
        "classify" => Ok(TaskType::Classify),
        "enrich" => Ok(TaskType::Enrich),
        "grade" => Ok(TaskType::Grade),
        "hyde" => Ok(TaskType::Hyde),
        other => Ok(TaskType::Custom(other.to_string())),
    }
}

/// Convenience command for vision chat — accepts image path and prompt directly.
///
/// # Arguments
/// * `image_path` — Absolute path to the image file
/// * `prompt` — The question about the image
/// * `max_tokens` — Optional: max output tokens (default: 256)
#[tauri::command]
pub async fn vision_chat(
    image_path: String,
    prompt: String,
    max_tokens: Option<String>,
    router_state: State<'_, TaskRouterState>,
) -> Result<String, String> {
    let mut extra = HashMap::new();
    extra.insert("image_path".to_string(), image_path);
    if let Some(tokens) = max_tokens {
        extra.insert("max_tokens".to_string(), tokens);
    }

    let request = TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::VisionChat,
        input: prompt,
        model_override: None,
        extra,
    };

    match router_state.0.route(&request).await? {
        TaskResponse::Text(text) => Ok(text),
        other => Err(format!("Unexpected response type: {:?}", other)),
    }
}

/// Get the port for the vision model server (triggers lazy load).
/// Returns the port number so frontend can stream directly.

/// Streaming vision chat command — emits "vision-stream" events as output arrives.
/// Frontend should listen for these events to display streaming output.
///
/// Looks up the vision model definition via the TaskRouter so that model_file
/// and mmproj_file come from config (models.toml or dynamic registration)
/// rather than being hardcoded.
#[tauri::command]
pub async fn vision_chat_stream(
    image_path: Option<String>,
    prompt: String,
    max_tokens: Option<String>,
    model_id: Option<String>,
    app: AppHandle,
    router_state: State<'_, TaskRouterState>,
) -> Result<(), String> {
    let models_dir = get_models_dir();
    let image_path = image_path
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());

    if image_path.is_none() {
        return Err("Vision mode requires an image. Please attach an image and try again.".to_string());
    }

    // Look up the vision model definition — use override if provided
    let def = if let Some(ref id) = model_id {
        router_state
            .0
            .get_model_def_by_id(id)
            .await
            .ok_or_else(|| format!("Vision model '{id}' not found"))?
    } else {
        router_state
            .0
            .get_model_def_for_task(&TaskType::VisionChat)
            .await
            .ok_or_else(|| "No vision model registered for task 'vision_chat'".to_string())?
    };

    let model_file = &def.model_file;
    let mmproj_file = def.param_or("mmproj_file", "");
    if mmproj_file.is_empty() {
        return Err(format!(
            "Vision model '{}' is missing 'mmproj_file' in its params",
            def.id
        ));
    }

    let max_tokens = max_tokens
        .or_else(|| {
            def.params
                .get("max_tokens")
                .cloned()
                .filter(|v| !v.trim().is_empty())
        })
        .unwrap_or_else(|| "1024".to_string());

    let ctx_size = def
        .params
        .get("ctx_size")
        .cloned()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "8192".to_string());

    let image_min_tokens = def.params.get("image_min_tokens").map(|s| s.as_str());
    let image_max_tokens = def.params.get("image_max_tokens").map(|s| s.as_str());
    let use_jinja = parse_bool_param(&def.param_or("use_jinja", "true"), true);

    execute_vision_streaming(
        model_file,
        &mmproj_file,
        image_path.as_deref(),
        &prompt,
        &max_tokens,
        &ctx_size,
        image_min_tokens,
        image_max_tokens,
        use_jinja,
        &models_dir,
        app,
    )
    .await
}
