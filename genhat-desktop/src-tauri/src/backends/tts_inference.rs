//! TTS Inference backend: runs the PyInstaller-bundled `tts-inference` binary
//! to synthesize speech from text.
//!
//! Like `LlamaCli`, this backend spawns a new child process per request —
//! no persistent server is maintained. The binary takes CLI arguments for
//! the model file, text, companion model files, and output path.
//!
//! The `model_path` extra param (sent from the frontend) points to the S3Gen
//! GGUF file. Sibling T3 and VE files are resolved automatically from
//! the same directory. An `--encoder_dir` argument is also passed, pointing
//! to the directory containing `tokenizer.json` and `conds.pt`.

use crate::backends::ModelBackend;
use crate::registry::types::{
    ModelDef, ModelHandle, ProcessHandle, TaskRequest, TaskResponse,
};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;
use tokio::process::Command as TokioCommand;

#[derive(Debug)]
pub struct TtsInferenceBackend;

/// Platform-specific constants for locating the tts-inference binary.
const TTS_OS_FOLDER: &str = if cfg!(target_os = "windows") {
    "tts-win"
} else if cfg!(target_os = "macos") {
    "tts-mac"
} else {
    "tts-lin"
};

const TTS_EXE_NAME: &str = if cfg!(target_os = "windows") {
    "tts-inference.exe"
} else {
    "tts-inference"
};

impl TtsInferenceBackend {
    pub fn new() -> Self {
        Self
    }
}

/// Resolve the tts-inference executable path.
/// Walks up from `current_exe()` checking known subdirectories.
fn resolve_tts_exe() -> Result<PathBuf, String> {
    let exe_path =
        std::env::current_exe().map_err(|e| format!("Cannot get current_exe: {e}"))?;
    let mut checked = Vec::new();

    exe_path
        .ancestors()
        .find_map(|dir| {
            for sub in &["src-tauri/bin", "bin", "resources/bin"] {
                let candidate = dir
                    .join(sub)
                    .join(TTS_OS_FOLDER)
                    .join("tts-inference")
                    .join(TTS_EXE_NAME);
                checked.push(candidate.clone());
                if candidate.exists() {
                    return Some(candidate);
                }
            }
            None
        })
        .ok_or_else(|| {
            format!(
                "tts-inference not found. Checked:\n{}",
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
    let meta =
        std::fs::metadata(path).map_err(|e| format!("Cannot stat {}: {e}", path.display()))?;
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

/// Find a sibling GGUF file in the same directory by prefix.
/// E.g., given `s3gen-bf16.gguf`, find `t3_cfg*.gguf` next to it.
fn find_sibling_gguf(model_dir: &Path, prefix: &str) -> Result<PathBuf, String> {
    if let Ok(entries) = std::fs::read_dir(model_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(prefix)
                && name_str.ends_with(".gguf")
            {
                return Ok(entry.path());
            }
        }
    }
    Err(format!(
        "No '{}*.gguf' file found in {}",
        prefix,
        model_dir.display()
    ))
}

#[async_trait]
impl ModelBackend for TtsInferenceBackend {
    /// For TTS CLI backend, "start" verifies the binary exists.
    /// Like `LlamaCli`, no persistent server is maintained.
    async fn start(&self, def: &ModelDef, _models_dir: &Path) -> Result<ModelHandle, String> {
        let exe = resolve_tts_exe()?;
        ensure_executable(&exe)?;
        log::info!(
            "TtsInference backend ready for model: {} (exe: {})",
            def.id,
            exe.display()
        );

        // Create a minimal dummy handle to indicate readiness
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

    /// TTS backend is always "healthy" since there's no persistent process.
    async fn is_healthy(&self, _handle: &ModelHandle) -> bool {
        true
    }

    /// Execute a TTS request by spawning the tts-inference binary.
    ///
    /// Expected `extra` params:
    /// - `model_path`: absolute path to the S3Gen GGUF model file (from frontend)
    ///
    /// The backend resolves sibling T3 and VE model files and the encoder
    /// directory from the same parent directory as the S3Gen model.
    async fn execute(
        &self,
        _handle: &ModelHandle,
        request: &TaskRequest,
        _models_dir: &Path,
    ) -> Result<TaskResponse, String> {
        let exe_path = resolve_tts_exe()?;
        ensure_executable(&exe_path)?;

        // The frontend passes model_path (absolute path to s3gen GGUF)
        let s3gen_path_str = request
            .extra
            .get("model_path")
            .ok_or("TTS request missing 'model_path' in extra params")?;
        let s3gen_path = PathBuf::from(s3gen_path_str);

        if !s3gen_path.exists() {
            return Err(format!(
                "S3Gen model file not found: {}",
                s3gen_path.display()
            ));
        }

        // Resolve the model directory (parent of s3gen file)
        let model_dir = s3gen_path
            .parent()
            .ok_or("S3Gen model path has no parent directory")?;

        // Find sibling T3 and VE GGUF files
        let t3_path = find_sibling_gguf(model_dir, "t3_")?;
        let ve_path = find_sibling_gguf(model_dir, "ve_")?;

        // Encoder directory — same as model directory (contains tokenizer.json + conds.pt)
        let encoder_dir = model_dir;

        // Output file
        let output_path =
            std::env::temp_dir().join(format!("genhat-tts-{}.wav", uuid::Uuid::new_v4()));

        log::info!(
            "TTS request: s3gen={}, t3={}, ve={}, encoder_dir={}, text_len={}, output={}",
            s3gen_path.display(),
            t3_path.display(),
            ve_path.display(),
            encoder_dir.display(),
            request.input.len(),
            output_path.display(),
        );

        let mut cmd = TokioCommand::new(&exe_path);

        // Set working directory to the binary's parent
        if let Some(bin_dir) = exe_path.parent() {
            cmd.current_dir(bin_dir);
        }

        cmd.arg("--model_gguf")
            .arg(&s3gen_path)
            .arg("--text")
            .arg(&request.input)
            .arg("--clip_gguf")
            .arg(&t3_path)
            .arg("--vae_gguf")
            .arg(&ve_path)
            .arg("--encoder_dir")
            .arg(encoder_dir)
            .arg("--output")
            .arg(&output_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd
            .output()
            .await
            .map_err(|e| format!("Failed to run tts-inference: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            log::error!("tts-inference failed. stderr: {}", stderr);
            log::error!("tts-inference stdout: {}", stdout);
            return Err(format!(
                "tts-inference failed (exit {}): {}",
                output.status, stderr
            ));
        }

        // Verify output file exists
        if !output_path.exists() {
            return Err("tts-inference completed but output .wav file was not created".into());
        }

        log::info!(
            "TTS completed: output={} ({})",
            output_path.display(),
            humanize_size(std::fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0)),
        );

        Ok(TaskResponse::FilePath(
            output_path.to_string_lossy().to_string(),
        ))
    }

    /// No persistent process to stop.
    async fn stop(&self, _handle: &ModelHandle) -> Result<(), String> {
        Ok(())
    }
}

/// Quick human-readable file size.
fn humanize_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
