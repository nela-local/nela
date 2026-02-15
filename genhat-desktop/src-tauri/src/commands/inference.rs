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
/// * `task_type` — One of: "chat", "summarize", "mindmap", "tts", "podcast_script",
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
        "podcast_audio" => Ok(TaskType::PodcastAudio),
        "podcast_script" => Ok(TaskType::PodcastScript),
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
#[tauri::command]
pub async fn get_vision_port(
    router_state: State<'_, TaskRouterState>,
) -> Result<u16, String> {
    // Use the router to find the vision model and ensure it's running
    let pm = &router_state.0.process_manager;
    
    // Ensure the vision model is running
    let instance_id = pm.ensure_running("lfm-2_5-vl", false).await?;
    
    // Get the port from the running instance
    let port: Option<u16> = pm.get_instance_port("lfm-2_5-vl", &instance_id).await;
    port.ok_or_else(|| "Vision model has no port assigned".to_string())
}

/// Streaming vision chat command — emits "vision-stream" events as output arrives.
/// Frontend should listen for these events to display streaming output.
#[tauri::command]
pub async fn vision_chat_stream(
    image_path: String,
    prompt: String,
    max_tokens: Option<String>,
    app: AppHandle,
) -> Result<(), String> {
    let models_dir = get_models_dir();
    let max_tokens = max_tokens.unwrap_or_else(|| "256".to_string());
    
    // Vision model config (from models.toml)
    let model_file = "LFM2.5-VL-1.6B-Q4_0.gguf";
    let mmproj_file = "mmproj-LFM2.5-VL-1.6b-Q8_0.gguf";
    
    execute_vision_streaming(
        model_file,
        mmproj_file,
        &image_path,
        &prompt,
        &max_tokens,
        &models_dir,
        app,
    ).await
}
