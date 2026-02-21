//! Whisper.cpp backend — spawns whisper-cli per-request for audio transcription.
//!
//! Uses whisper.cpp CLI to transcribe audio files into text segments.
//! The binary is expected at `bin/<os-folder>/whisper-cli` alongside the
//! other llama.cpp tools. If not found, falls back to system PATH.
//!
//! Supports Windows, macOS, and Linux. On Unix platforms, shared libraries
//! are located via LD_LIBRARY_PATH / DYLD_LIBRARY_PATH, and the executable
//! is ensured to have +x permissions before spawning.

use crate::registry::types::{
    ModelDef, ModelHandle, ProcessHandle, TaskRequest, TaskResponse, TranscriptSegment,
};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;
use tokio::process::Command as TokioCommand;

#[derive(Debug)]
pub struct WhisperCppBackend;

/// Platform-specific constants.
const OS_FOLDER: &str = if cfg!(target_os = "windows") {
    "llama-win"
} else if cfg!(target_os = "macos") {
    "llama-mac"
} else {
    "llama-lin"
};

const EXE_NAME: &str = if cfg!(target_os = "windows") {
    "whisper-cli.exe"
} else {
    "whisper-cli"
};

impl WhisperCppBackend {
    pub fn new() -> Self {
        Self
    }
}

/// Resolve the whisper-cli executable path.
/// Walks up from `current_exe()` checking known subdirectories, mirroring
/// the strategy used by `llama_server.rs` / `llama_cli.rs`.
fn resolve_whisper_exe() -> Result<PathBuf, String> {
    let exe_path =
        std::env::current_exe().map_err(|e| format!("Cannot get current_exe: {e}"))?;
    let mut checked = Vec::new();

    // Check alongside other llama.cpp binaries
    if let Some(found) = exe_path.ancestors().find_map(|dir| {
        for sub in &["src-tauri/bin", "bin", "resources/bin"] {
            let candidate = dir.join(sub).join(OS_FOLDER).join(EXE_NAME);
            checked.push(candidate.clone());
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }) {
        return Ok(found);
    }

    // Fallback: check system PATH
    #[cfg(unix)]
    {
        if let Ok(output) = Command::new("which").arg("whisper-cli").output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Ok(PathBuf::from(path));
                }
            }
        }
    }
    #[cfg(windows)]
    {
        if let Ok(output) = Command::new("where").arg("whisper-cli").output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if !path.is_empty() {
                    return Ok(PathBuf::from(path));
                }
            }
        }
    }

    Err(format!(
        "whisper-cli not found. Place it in bin/{OS_FOLDER}/ or install whisper.cpp.\nChecked:\n{}",
        checked
            .iter()
            .map(|p| format!("  {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    ))
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

/// Apply platform-specific environment variables to a `tokio::process::Command`
/// so that shared libraries next to the executable are found at runtime.
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
impl super::ModelBackend for WhisperCppBackend {
    /// Validate that the whisper binary and model file exist.
    /// Returns a dummy ProcessHandle since whisper runs per-request.
    async fn start(&self, def: &ModelDef, models_dir: &Path) -> Result<ModelHandle, String> {
        // Validate model file
        let model_path = models_dir.join(&def.model_file);
        if !model_path.exists() {
            return Err(format!("Whisper model not found: {}", model_path.display()));
        }

        // Validate whisper binary exists
        let exe = resolve_whisper_exe()?;
        ensure_executable(&exe)?;
        log::info!(
            "WhisperCpp backend ready for model: {} (exe: {})",
            def.id,
            exe.display()
        );

        // Create a minimal dummy process handle (no persistent process)
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
            work_dir: model_path, // Store the model path for later use in execute()
        }))
    }

    /// Whisper is always "healthy" since there's no persistent process.
    async fn is_healthy(&self, _handle: &ModelHandle) -> bool {
        resolve_whisper_exe().is_ok()
    }

    /// Transcribe an audio file by spawning whisper-cli.
    /// `request.input` must be the absolute path to the audio file.
    async fn execute(
        &self,
        handle: &ModelHandle,
        request: &TaskRequest,
        _models_dir: &Path,
    ) -> Result<TaskResponse, String> {
        let model_path = match handle {
            ModelHandle::Process(ph) => ph.work_dir.clone(),
            _ => return Err("WhisperCpp requires a ProcessHandle".into()),
        };

        let exe_path = resolve_whisper_exe()?;
        ensure_executable(&exe_path)?;

        let audio_path = PathBuf::from(&request.input);
        if !audio_path.exists() {
            return Err(format!("Audio file not found: {}", audio_path.display()));
        }

        let work_dir = exe_path
            .parent()
            .ok_or_else(|| "whisper-cli has no parent directory".to_string())?
            .to_path_buf();

        // Create temp dir for JSON output
        let temp_dir =
            tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {e}"))?;
        let output_stem = temp_dir.path().join("output");

        // Build command:
        //   whisper-cli -m model.gguf -f audio.wav -oj -of output --no-prints
        let mut cmd = TokioCommand::new(&exe_path);
        cmd.current_dir(&work_dir);
        apply_lib_env_tokio(&mut cmd, &work_dir);

        cmd.args([
            "-m",
            &model_path.to_string_lossy(),
            "-f",
            &audio_path.to_string_lossy(),
            "-oj",          // output JSON
            "-of",
            &output_stem.to_string_lossy(),
            "--no-prints",  // suppress progress output
        ]);

        // Add optional language override from extra params
        if let Some(lang) = request.extra.get("language") {
            cmd.args(["-l", lang]);
        }

        // Add optional thread count
        if let Some(threads) = request.extra.get("threads") {
            cmd.args(["-t", threads]);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        log::info!(
            "Running whisper-cli for request {}: model={}, audio={}",
            request.request_id,
            model_path.display(),
            audio_path.display()
        );

        let output = cmd
            .output()
            .await
            .map_err(|e| format!("Failed to run whisper-cli: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            log::error!("whisper-cli failed. stderr: {}", stderr);
            log::error!("whisper-cli stdout: {}", stdout);
            return Err(format!(
                "whisper-cli failed (exit {}): {}",
                output.status, stderr
            ));
        }

        // Parse the JSON output file written by whisper-cli
        let json_path = output_stem.with_extension("json");
        let segments = if json_path.exists() {
            parse_whisper_json(&json_path)?
        } else {
            // Fallback: parse plain stdout
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                Vec::new()
            } else {
                vec![TranscriptSegment {
                    text,
                    start_ms: 0,
                    end_ms: 0,
                }]
            }
        };

        log::info!(
            "whisper-cli completed for request {}: {} segments",
            request.request_id,
            segments.len()
        );

        Ok(TaskResponse::Transcription { segments })
    }

    /// Nothing persistent to stop — whisper runs per-request.
    async fn stop(&self, _handle: &ModelHandle) -> Result<(), String> {
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Whisper JSON parsing
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse the JSON output file produced by `whisper-cli -oj`.
fn parse_whisper_json(path: &Path) -> Result<Vec<TranscriptSegment>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read whisper output: {e}"))?;

    let json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse whisper JSON: {e}"))?;

    let mut segments = Vec::new();

    // whisper.cpp JSON format: { "transcription": [ { "timestamps": { "from": "...", "to": "..." }, "text": "..." }, ... ] }
    if let Some(transcription) = json.get("transcription").and_then(|t| t.as_array()) {
        for seg in transcription {
            let text = seg
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .trim()
                .to_string();

            if text.is_empty() {
                continue;
            }

            let start_ms = parse_timestamp(
                seg.get("timestamps")
                    .and_then(|t| t.get("from"))
                    .and_then(|f| f.as_str())
                    .unwrap_or("00:00:00.000"),
            );
            let end_ms = parse_timestamp(
                seg.get("timestamps")
                    .and_then(|t| t.get("to"))
                    .and_then(|f| f.as_str())
                    .unwrap_or("00:00:00.000"),
            );

            segments.push(TranscriptSegment {
                text,
                start_ms,
                end_ms,
            });
        }
    }

    // Fallback: check for a top-level "text" field
    if segments.is_empty() {
        if let Some(full_text) = json.get("text").and_then(|t| t.as_str()) {
            let trimmed = full_text.trim().to_string();
            if !trimmed.is_empty() {
                segments.push(TranscriptSegment {
                    text: trimmed,
                    start_ms: 0,
                    end_ms: 0,
                });
            }
        }
    }

    Ok(segments)
}

/// Parse whisper.cpp timestamp format "HH:MM:SS.mmm" → milliseconds.
fn parse_timestamp(ts: &str) -> u64 {
    let parts: Vec<&str> = ts.split(':').collect();
    if parts.len() == 3 {
        let hours: u64 = parts[0].parse().unwrap_or(0);
        let minutes: u64 = parts[1].parse().unwrap_or(0);
        let sec_parts: Vec<&str> = parts[2].split('.').collect();
        let seconds: u64 = sec_parts[0].parse().unwrap_or(0);
        let millis: u64 = if sec_parts.len() > 1 {
            sec_parts[1].parse().unwrap_or(0)
        } else {
            0
        };
        hours * 3600_000 + minutes * 60_000 + seconds * 1000 + millis
    } else {
        0
    }
}
