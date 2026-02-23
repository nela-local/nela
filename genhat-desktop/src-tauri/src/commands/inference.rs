//! Inference commands — the main entry point for task routing.

use crate::registry::types::{TaskRequest, TaskResponse, TaskType};
use crate::router::TaskRouter;
use crate::backends::llama_cli::execute_vision_streaming;
use crate::commands::models::get_models_dir;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, State};

/// Managed state wrapper for the TaskRouter.
pub struct TaskRouterState(pub Arc<TaskRouter>);

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
    image_path: String,
    prompt: String,
    max_tokens: Option<String>,
    model_id: Option<String>,
    app: AppHandle,
    router_state: State<'_, TaskRouterState>,
) -> Result<(), String> {
    let models_dir = get_models_dir();
    let max_tokens = max_tokens.unwrap_or_else(|| "256".to_string());

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

    execute_vision_streaming(
        model_file,
        &mmproj_file,
        &image_path,
        &prompt,
        &max_tokens,
        &models_dir,
        app,
    )
    .await
}
