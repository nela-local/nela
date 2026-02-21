//! LlamaCli backend: runs llama-cli for each inference request.
//!
//! Unlike LlamaServer, this backend spawns a new process per request,
//! making it suitable for vision models that use --image flags.
//! No persistent server is maintained.
//!
//! Supports Windows, macOS, and Linux. On Unix platforms, shared libraries
//! are located via LD_LIBRARY_PATH / DYLD_LIBRARY_PATH, and the executable
//! is ensured to have +x permissions before spawning.

use crate::registry::types::{ModelDef, ModelHandle, ProcessHandle, TaskRequest, TaskResponse};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::process::Command as TokioCommand;
use tauri::{AppHandle, Emitter};

#[derive(Debug)]
pub struct LlamaCliBackend;

/// Platform-specific constants.
const OS_FOLDER: &str = if cfg!(target_os = "windows") {
    "llama-win"
} else if cfg!(target_os = "macos") {
    "llama-mac"
} else {
    "llama-lin"
};

const EXE_NAME: &str = if cfg!(target_os = "windows") {
    "llama-mtmd-cli.exe"
} else {
    "llama-mtmd-cli"
};

impl LlamaCliBackend {
    pub fn new() -> Self {
        Self
    }
}

/// Resolve the llama-mtmd-cli executable path.
/// Walks up from `current_exe()` checking known subdirectories, mirroring
/// the strategy used by `llama_server.rs` so it works in dev and production.
fn resolve_cli_exe() -> Result<PathBuf, String> {
    let exe_path =
        std::env::current_exe().map_err(|e| format!("Cannot get current_exe: {e}"))?;
    let mut checked = Vec::new();

    exe_path
        .ancestors()
        .find_map(|dir| {
            for sub in &["src-tauri/bin", "bin", "resources/bin"] {
                let candidate = dir.join(sub).join(OS_FOLDER).join(EXE_NAME);
                checked.push(candidate.clone());
                if candidate.exists() {
                    return Some(candidate);
                }
            }
            None
        })
        .ok_or_else(|| {
            format!(
                "llama-mtmd-cli not found. Checked:\n{}",
                checked
                    .iter()
                    .map(|p| format!("  {}", p.display()))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })
}

/// Ensure the binary at `path` has execute permission (no-op on Windows).
#[cfg(unix)]
fn ensure_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("Cannot stat {}: {e}", path.display()))?;
    let mut perms = meta.permissions();
    let mode = perms.mode();
    if mode & 0o111 == 0 {
        perms.set_mode(mode | 0o755);
        std::fs::set_permissions(path, perms)
            .map_err(|e| format!("Cannot chmod {}: {e}", path.display()))?;
        log::info!("Set +x on {}", path.display());
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Apply platform-specific environment variables to a `std::process::Command`
/// so that shared libraries (.dll / .so / .dylib) next to the executable are found.
#[allow(dead_code)]
fn apply_lib_env(cmd: &mut Command, work_dir: &Path) {
    #[cfg(target_os = "linux")]
    {
        // Prepend the working directory to LD_LIBRARY_PATH
        let existing = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        let new_val = if existing.is_empty() {
            work_dir.to_string_lossy().to_string()
        } else {
            format!("{}:{existing}", work_dir.display())
        };
        cmd.env("LD_LIBRARY_PATH", new_val);
    }
    #[cfg(target_os = "macos")]
    {
        let existing = std::env::var("DYLD_LIBRARY_PATH").unwrap_or_default();
        let new_val = if existing.is_empty() {
            work_dir.to_string_lossy().to_string()
        } else {
            format!("{}:{existing}", work_dir.display())
        };
        cmd.env("DYLD_LIBRARY_PATH", new_val);
    }
    #[cfg(target_os = "windows")]
    {
        // Windows finds DLLs via the current directory (set with .current_dir)
        // and PATH. Prepend work_dir to PATH for safety.
        let existing = std::env::var("PATH").unwrap_or_default();
        let new_val = format!("{};{existing}", work_dir.display());
        cmd.env("PATH", new_val);
    }
}

/// Same as `apply_lib_env` but for `tokio::process::Command`.
fn apply_lib_env_tokio(cmd: &mut TokioCommand, work_dir: &Path) {
    #[cfg(target_os = "linux")]
    {
        let existing = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        let new_val = if existing.is_empty() {
            work_dir.to_string_lossy().to_string()
        } else {
            format!("{}:{existing}", work_dir.display())
        };
        cmd.env("LD_LIBRARY_PATH", new_val);
    }
    #[cfg(target_os = "macos")]
    {
        let existing = std::env::var("DYLD_LIBRARY_PATH").unwrap_or_default();
        let new_val = if existing.is_empty() {
            work_dir.to_string_lossy().to_string()
        } else {
            format!("{}:{existing}", work_dir.display())
        };
        cmd.env("DYLD_LIBRARY_PATH", new_val);
    }
    #[cfg(target_os = "windows")]
    {
        let existing = std::env::var("PATH").unwrap_or_default();
        let new_val = format!("{};{existing}", work_dir.display());
        cmd.env("PATH", new_val);
    }
}

#[async_trait]
impl super::ModelBackend for LlamaCliBackend {
    /// For CLI backend, "start" is a no-op since we spawn per request.
    /// We return a dummy handle to indicate the model is "ready".
    async fn start(&self, def: &ModelDef, _models_dir: &Path) -> Result<ModelHandle, String> {
        // Verify the CLI exe exists at startup so we fail fast
        let exe = resolve_cli_exe()?;
        ensure_executable(&exe)?;
        log::info!("LlamaCli backend ready for model: {} (exe: {})", def.id, exe.display());

        // Create a minimal process handle (no actual process yet)
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

        // Resolve exe using the same ancestor-walk strategy as llama_server.rs
        let exe_path = resolve_cli_exe()?;
        ensure_executable(&exe_path)?;

        // Set working directory to the exe's parent for DLL/shared lib resolution
        let work_dir = exe_path
            .parent()
            .ok_or_else(|| "llama-mtmd-cli has no parent directory".to_string())?
            .to_path_buf();

        let mut cmd = TokioCommand::new(&exe_path);
        cmd.current_dir(&work_dir);
        apply_lib_env_tokio(&mut cmd, &work_dir);
        
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

        // Run the command and capture output (non-blocking via tokio)
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let output = cmd
            .output()
            .await
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
    let exe_path = resolve_cli_exe()?;
    ensure_executable(&exe_path)?;

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

    let work_dir = exe_path
        .parent()
        .ok_or_else(|| "llama-mtmd-cli has no parent directory".to_string())?;

    log::info!("Starting streaming vision chat: model={}, image={}", model_file, image_path);

    let mut cmd = TokioCommand::new(&exe_path);
    cmd.current_dir(work_dir);
    apply_lib_env_tokio(&mut cmd, work_dir);

    let mut child = cmd
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

    // Drain stderr in a background task to prevent pipe buffer deadlock
    let mut stderr = child.stderr.take()
        .ok_or("Failed to capture stderr")?;
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        String::from_utf8_lossy(&buf).to_string()
    });

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

    // Log any stderr output
    if let Ok(stderr_output) = stderr_task.await {
        if !stderr_output.is_empty() {
            log::warn!("llama-mtmd-cli stderr: {}", stderr_output.trim());
        }
    }

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
