//! LlamaCli backend: runs llama-cli for each inference request.
//!
//! Unlike LlamaServer, this backend spawns a new process per request,
//! making it suitable for vision models that use --image flags.
//! No persistent server is maintained.

use crate::registry::types::{ModelDef, ModelHandle, ProcessHandle, TaskRequest, TaskResponse};
use async_trait::async_trait;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::process::Command as TokioCommand;
use tauri::{AppHandle, Emitter};

#[derive(Debug)]
pub struct LlamaCliBackend;

impl LlamaCliBackend {
    pub fn new() -> Self {
        Self
    }

    /// Get the path to the llama-mtmd-cli executable for the current platform.
    /// This is the multimodal CLI that supports --image flag with local GGUF files.
    fn get_cli_path() -> &'static str {
        #[cfg(target_os = "windows")]
        {
            "bin/llama-win/llama-mtmd-cli.exe"
        }
        #[cfg(target_os = "macos")]
        {
            "bin/llama-mac/llama-mtmd-cli"
        }
        #[cfg(target_os = "linux")]
        {
            "bin/llama-lin/llama-mtmd-cli"
        }
    }
}

#[async_trait]
impl super::ModelBackend for LlamaCliBackend {
    /// For CLI backend, "start" is a no-op since we spawn per request.
    /// We return a dummy handle to indicate the model is "ready".
    async fn start(&self, def: &ModelDef, _models_dir: &Path) -> Result<ModelHandle, String> {
        log::info!("LlamaCli backend ready for model: {}", def.id);
        
        // Create a minimal process handle (no actual process yet)
        // We use a dummy child process that immediately exits
        #[cfg(target_os = "windows")]
        let child = Command::new("cmd")
            .args(["/C", "echo ready"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to create dummy process: {e}"))?;

        #[cfg(not(target_os = "windows"))]
        let child = Command::new("true")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to create dummy process: {e}"))?;

        let pid = child.id();

        Ok(ModelHandle::Process(ProcessHandle {
            child,
            pid,
            port: None,
            started_at: Instant::now(),
            work_dir: std::env::current_dir().unwrap_or_default(),
        }))
    }

    /// CLI backend is always "healthy" since there's no persistent process.
    async fn is_healthy(&self, _handle: &ModelHandle) -> bool {
        true
    }

    /// Execute a vision/chat request by spawning llama-cli.
    async fn execute(
        &self,
        _handle: &ModelHandle,
        request: &TaskRequest,
        models_dir: &Path,
    ) -> Result<TaskResponse, String> {
        // Get the model file path from the request's extra params or use default
        let model_file = request
            .extra
            .get("model_file")
            .cloned()
            .unwrap_or_else(|| "LFM2.5-VL-1.6B-Q4_0.gguf".to_string());

        let model_path = models_dir.join(&model_file);

        // Build the command - resolve exe path relative to CARGO_MANIFEST_DIR in dev
        let cli_path = Self::get_cli_path();
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = manifest_dir.join(cli_path);

        if !exe_path.exists() {
            return Err(format!("llama-mtmd-cli not found at: {}", exe_path.display()));
        }

        // Set working directory for DLLs
        let work_dir = exe_path.parent().unwrap_or(&manifest_dir);

        let mut cmd = Command::new(&exe_path);
        cmd.current_dir(work_dir);
        
        // Always use local model file
        if model_path.exists() {
            cmd.arg("-m").arg(&model_path);
        } else {
            return Err(format!("Model file not found: {}", model_path.display()));
        }

        // Add mmproj (multimodal projector) - required for vision models
        if let Some(mmproj_file) = request.extra.get("mmproj_file") {
            let mmproj_path = models_dir.join(mmproj_file);
            if mmproj_path.exists() {
                cmd.arg("--mmproj").arg(&mmproj_path);
            } else {
                return Err(format!("mmproj file not found: {}", mmproj_path.display()));
            }
        }

        // Add image if provided in extra params
        if let Some(image_path) = request.extra.get("image_path") {
            let img = Path::new(image_path);
            if !img.exists() {
                return Err(format!("Image file not found: {image_path}"));
            }
            cmd.arg("--image").arg(image_path);

            // Image max tokens (optional, default 64)
            let max_img_tokens = request.extra.get("image_max_tokens").map(|s| s.as_str()).unwrap_or("64");
            cmd.arg("--image-max-tokens").arg(max_img_tokens);
        }

        // Prompt
        cmd.arg("-p").arg(&request.input);

        // Max output tokens
        let max_tokens = request.extra.get("max_tokens").map(|s| s.as_str()).unwrap_or("256");
        cmd.arg("-n").arg(max_tokens);

        // Log the full command for debugging
        log::info!(
            "Running llama-mtmd-cli for request {}: model={}, prompt_len={}, has_image={}",
            request.request_id,
            model_path.display(),
            request.input.len(),
            request.extra.contains_key("image_path")
        );

        // Run the command and capture output
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run llama-cli: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            log::error!("llama-cli failed. stderr: {}", stderr);
            log::error!("llama-cli stdout: {}", stdout);
            return Err(format!("llama-cli failed (exit {}): {}", output.status, stderr));
        }

        let response_text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        log::info!(
            "llama-cli completed for request {}: output_len={}",
            request.request_id,
            response_text.len()
        );

        Ok(TaskResponse::Text(response_text))
    }

    /// No persistent process to stop.
    async fn stop(&self, _handle: &ModelHandle) -> Result<(), String> {
        Ok(())
    }
}

/// Execute vision chat with streaming output via Tauri events.
/// Emits "vision-stream" events with { chunk: String, done: bool }.
pub async fn execute_vision_streaming(
    model_file: &str,
    mmproj_file: &str,
    image_path: &str,
    prompt: &str,
    max_tokens: &str,
    models_dir: &Path,
    app: AppHandle,
) -> Result<(), String> {
    let cli_path = LlamaCliBackend::get_cli_path();
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let exe_path = manifest_dir.join(cli_path);

    if !exe_path.exists() {
        return Err(format!("llama-mtmd-cli not found at: {}", exe_path.display()));
    }

    let model_path = models_dir.join(model_file);
    if !model_path.exists() {
        return Err(format!("Model file not found: {}", model_path.display()));
    }

    let mmproj_path = models_dir.join(mmproj_file);
    if !mmproj_path.exists() {
        return Err(format!("mmproj file not found: {}", mmproj_path.display()));
    }

    let img = Path::new(image_path);
    if !img.exists() {
        return Err(format!("Image file not found: {image_path}"));
    }

    let work_dir = exe_path.parent().unwrap_or(&manifest_dir);

    log::info!("Starting streaming vision chat: model={}, image={}", model_file, image_path);

    let mut child = TokioCommand::new(&exe_path)
        .current_dir(work_dir)
        .arg("-m").arg(&model_path)
        .arg("--mmproj").arg(&mmproj_path)
        .arg("--image").arg(image_path)
        .arg("--image-max-tokens").arg("64")
        .arg("-p").arg(prompt)
        .arg("-n").arg(max_tokens)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn llama-mtmd-cli: {e}"))?;

    let mut stdout = child.stdout.take()
        .ok_or("Failed to capture stdout")?;

    // Read output in small chunks for token-by-token streaming
    let mut buffer = [0u8; 64];  // Small buffer for responsive streaming
    
    loop {
        match stdout.read(&mut buffer).await {
            Ok(0) => break,  // EOF
            Ok(n) => {
                // Convert bytes to string and emit
                if let Ok(chunk) = String::from_utf8(buffer[..n].to_vec()) {
                    let _ = app.emit("vision-stream", serde_json::json!({
                        "chunk": chunk,
                        "done": false
                    }));
                }
            }
            Err(e) => {
                log::error!("Error reading stdout: {}", e);
                break;
            }
        }
    }

    // Wait for process to finish
    let status = child.wait().await
        .map_err(|e| format!("Process error: {e}"))?;

    if !status.success() {
        log::error!("llama-mtmd-cli exited with: {}", status);
    }

    // Emit done event
    let _ = app.emit("vision-stream", serde_json::json!({
        "chunk": "",
        "done": true
    }));

    log::info!("Streaming vision chat completed");

    Ok(())
}
