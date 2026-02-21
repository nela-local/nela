//! llama-server backend: spawns llama-server as a child process, communicates
//! via its OpenAI-compatible HTTP API on a dynamically assigned port.
//!
//! Supports multiple concurrent instances on different ports.

use crate::backends::ModelBackend;
use crate::registry::types::{
    ModelDef, ModelHandle, ModelHandle::Process, ProcessHandle, TaskRequest, TaskResponse,
};
use async_trait::async_trait;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

#[derive(Debug)]
pub struct LlamaServerBackend;

impl LlamaServerBackend {
    pub fn new() -> Self {
        Self
    }
}

/// Resolve the llama-server executable path.
/// Walks up from `current_exe()` looking in known locations.
fn resolve_llama_exe() -> Result<PathBuf, String> {
    let os_folder = if cfg!(windows) {
        "llama-win"
    } else if cfg!(target_os = "macos") {
        "llama-mac"
    } else {
        "llama-lin"
    };

    let exe_names: Vec<&str> = if cfg!(windows) {
        vec!["llama-server.exe"]
    } else if cfg!(target_os = "macos") {
        vec![
            "llama-server",
            "llama-server-macos",
            "llama-server-macos-arm64",
        ]
    } else {
        vec!["llama-server"]
    };

    let exe_path = std::env::current_exe().map_err(|e| format!("Cannot get current_exe: {e}"))?;
    let mut checked = Vec::new();

    exe_path
        .ancestors()
        .find_map(|dir| {
            for &exe_name in &exe_names {
                for sub in &["src-tauri/bin", "bin", "resources/bin"] {
                    let candidate = dir.join(sub).join(os_folder).join(exe_name);
                    checked.push(candidate.clone());
                    if candidate.exists() {
                        return Some(candidate);
                    }
                }
            }
            None
        })
        .ok_or_else(|| {
            format!(
                "llama-server not found. Checked:\n{}",
                checked
                    .iter()
                    .map(|p| format!("  {}", p.display()))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })
}

/// Spawn a llama-server child process with the given model and port.
fn spawn_llama(model_path: &Path, port: u16, def: &ModelDef) -> Result<Child, String> {
    let exe = resolve_llama_exe()?;
    let work_dir = exe
        .parent()
        .ok_or_else(|| "llama-server exe has no parent dir".to_string())?;

    // Read params from model config with defaults
    let ctx_size = def.param_or("ctx_size", "4096");
    let max_tokens = def.param_or("max_tokens", "256");
    let temp = def.param_or("temp", "0.7");
    let top_p = def.param_or("top_p", "0.9");
    let top_k = def.param_or("top_k", "40");
    let repeat_penalty = def.param_or("repeat_penalty", "1.1");

    let port_str = port.to_string();
    let model_str = model_path.to_string_lossy();

    // Set up logging
    let log_path = std::env::temp_dir().join(format!("genhat-llama-server-{port}.log"));
    if let Ok(mut log_file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = writeln!(log_file, "--- llama-server start (port {port}) ---");
        let _ = writeln!(log_file, "exe: {}", exe.display());
        let _ = writeln!(log_file, "model: {model_str}");
    }

    let mut args = vec![
        "-m".to_string(),
        model_str.to_string(),
        "--ctx-size".to_string(),
        ctx_size.clone(),
        "--port".to_string(),
        port_str.clone(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "-n".to_string(),
        max_tokens.clone(),
        "--temp".to_string(),
        temp,
        "--top-p".to_string(),
        top_p,
        "--top-k".to_string(),
        top_k,
        "--repeat-penalty".to_string(),
        repeat_penalty,
    ];

    // Enable embedding mode if configured (for embedding models)
    if def.param_or("embedding", "false") == "true" {
        args.push("--embedding".to_string());
        // Add batch size for embedding throughput
        let batch_size = def.param_or("batch_size", "512");
        args.push("--batch-size".to_string());
        args.push(batch_size);
    }

    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let mut child = Command::new(&exe)
        .args(&args_refs)
        .current_dir(work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn llama-server: {e}"))?;

    // Redirect stdout/stderr to log file in background threads
    let pid = child.id();
    if let Some(stdout) = child.stdout.take() {
        let lp = log_path.clone();
        std::thread::spawn(move || {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&lp) {
                for line in BufReader::new(stdout).lines().flatten() {
                    let _ = writeln!(f, "[stdout][pid={pid}] {line}");
                }
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let lp = log_path.clone();
        std::thread::spawn(move || {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&lp) {
                for line in BufReader::new(stderr).lines().flatten() {
                    let _ = writeln!(f, "[stderr][pid={pid}] {line}");
                }
            }
        });
    }

    log::info!("llama-server spawned: pid={pid}, port={port}, model={model_str}");
    Ok(child)
}

/// Wait for llama-server to become ready by polling its /health endpoint.
async fn wait_for_ready(port: u16, timeout_secs: u64) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{port}/health");
    let client = reqwest::Client::new();
    let deadline = Instant::now() + std::time::Duration::from_secs(timeout_secs);

    loop {
        if Instant::now() > deadline {
            return Err(format!(
                "llama-server on port {port} did not become ready within {timeout_secs}s"
            ));
        }
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                log::info!("llama-server on port {port} is ready");
                return Ok(());
            }
            _ => {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
}

#[async_trait]
impl ModelBackend for LlamaServerBackend {
    async fn start(&self, def: &ModelDef, models_dir: &Path) -> Result<ModelHandle, String> {
        let model_path = models_dir.join(&def.model_file);
        if !model_path.exists() {
            return Err(format!("Model file not found: {}", model_path.display()));
        }

        // Pick a port — use configured port if nonzero, otherwise find a free one
        let configured_port: u16 = def.param_or("port", "0").parse().unwrap_or(0);
        let port = if configured_port == 0 {
            portpicker::pick_unused_port().ok_or("No free port available")?
        } else {
            configured_port
        };

        let child = spawn_llama(&model_path, port, def)?;
        let pid = child.id();
        let exe = resolve_llama_exe()?;
        let work_dir = exe.parent().unwrap().to_path_buf();

        let handle = ModelHandle::Process(ProcessHandle {
            child,
            pid,
            port: Some(port),
            started_at: Instant::now(),
            work_dir,
        });

        // Wait for the server to be ready (up to 60s for large models)
        wait_for_ready(port, 60).await?;

        Ok(handle)
    }

    async fn is_healthy(&self, handle: &ModelHandle) -> bool {
        match handle {
            Process(ph) => {
                if let Some(port) = ph.port {
                    let url = format!("http://127.0.0.1:{port}/health");
                    reqwest::get(&url)
                        .await
                        .map(|r| r.status().is_success())
                        .unwrap_or(false)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    async fn execute(
        &self,
        handle: &ModelHandle,
        request: &TaskRequest,
        _models_dir: &Path,
    ) -> Result<TaskResponse, String> {
        let port = match handle {
            Process(ph) => ph.port.ok_or("llama-server has no port assigned")?,
            _ => return Err("LlamaServerBackend requires a ProcessHandle".into()),
        };

        // ── Embedding requests go to /v1/embeddings ──
        if request.task_type == crate::registry::types::TaskType::Embed {
            return self.execute_embedding(port, request).await;
        }

        // ── Classification requests get a short completion and parse the label ──
        if request.task_type == crate::registry::types::TaskType::Classify {
            return self.execute_classification(port, request).await;
        }

        let url = format!("http://127.0.0.1:{port}/v1/chat/completions");

        // Build the chat messages. The task type determines the system prompt.
        let system_prompt = match &request.task_type {
            crate::registry::types::TaskType::Summarize => {
                "You are a helpful assistant that creates concise summaries."
            }
            crate::registry::types::TaskType::Mindmap => {
                "You are a helpful assistant that generates structured mindmaps in markdown format."
            }
            crate::registry::types::TaskType::Enrich => {
                "You are a helpful assistant. Generate a brief contextual description (50-100 tokens) for the following text chunk to improve its searchability."
            }
            crate::registry::types::TaskType::Grade => {
                "You are a relevance grading assistant. Rate the relevance of the provided context to the query on a scale of 1-5. Respond with only the number."
            }
            crate::registry::types::TaskType::Hyde => {
                "You are a helpful assistant. Generate a hypothetical answer to the following question that could appear in a document."
            }
            crate::registry::types::TaskType::VisionChat => {
                "You are a helpful vision assistant that can describe and analyze images."
            }
            _ => "You are a helpful assistant.",
        };

        // Build the user message - handle vision requests with images
        let user_message = if request.task_type == crate::registry::types::TaskType::VisionChat {
            // Check for base64 image in extra params
            if let (Some(image_base64), Some(image_mime)) = (
                request.extra.get("image_base64"),
                request.extra.get("image_mime"),
            ) {
                // Multimodal message format (OpenAI-compatible)
                serde_json::json!({
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": &request.input
                        },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{};base64,{}", image_mime, image_base64)
                            }
                        }
                    ]
                })
            } else {
                // No image, just text
                serde_json::json!({
                    "role": "user",
                    "content": &request.input
                })
            }
        } else {
            // Standard text message
            serde_json::json!({
                "role": "user",
                "content": &request.input
            })
        };

        let body = serde_json::json!({
            "model": "local",
            "messages": [
                { "role": "system", "content": system_prompt },
                user_message
            ],
            "stream": false
        });

        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request to llama-server failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("llama-server returned {status}: {text}"));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse llama-server response: {e}"))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(TaskResponse::Text(content))
    }

    async fn stop(&self, handle: &ModelHandle) -> Result<(), String> {
        match handle {
            Process(ph) => {
                log::info!("Stopping llama-server pid={}", ph.pid);
                // We need mutable access to call kill; use platform-specific kill by PID
                #[cfg(windows)]
                {
                    let _ = Command::new("taskkill")
                        .args(["/F", "/PID", &ph.pid.to_string()])
                        .output();
                }
                #[cfg(unix)]
                {
                    unsafe {
                        libc::kill(ph.pid as i32, libc::SIGTERM);
                    }
                }
                Ok(())
            }
            _ => Err("LlamaServerBackend requires a ProcessHandle".into()),
        }
    }
}

// ── Embedding helper (outside the trait impl to keep it clean) ──
impl LlamaServerBackend {
    /// Classify a query using a DistilBERT-based router model.
    /// The model outputs a classification label as short text.
    async fn execute_classification(
        &self,
        port: u16,
        request: &TaskRequest,
    ) -> Result<TaskResponse, String> {
        let url = format!("http://127.0.0.1:{port}/v1/chat/completions");

        let body = serde_json::json!({
            "model": "local",
            "messages": [
                {
                    "role": "system",
                    "content": "You are a query classifier. Classify the user's query into exactly one of these categories: no_retrieval, simple_rag, multi_doc, summarization. Respond with only the category name."
                },
                {
                    "role": "user",
                    "content": &request.input
                }
            ],
            "stream": false,
            "temperature": 0.0,
            "max_tokens": 16
        });

        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Classification request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Classification endpoint returned {status}: {text}"));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse classification response: {e}"))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("simple_rag")
            .trim()
            .to_lowercase();

        // Parse the label and map to a confidence score
        let (label, confidence) = match content.as_str() {
            l if l.contains("no_retrieval") => ("no_retrieval".to_string(), 0.9),
            l if l.contains("simple_rag") => ("simple_rag".to_string(), 0.9),
            l if l.contains("multi_doc") => ("multi_doc".to_string(), 0.9),
            l if l.contains("summarization") => ("summarization".to_string(), 0.9),
            _ => ("simple_rag".to_string(), 0.5), // Default fallback
        };

        Ok(TaskResponse::Classification {
            label,
            confidence: confidence as f32,
        })
    }

    /// Call the /v1/embeddings endpoint on a llama-server running in --embedding mode.
    async fn execute_embedding(
        &self,
        port: u16,
        request: &TaskRequest,
    ) -> Result<TaskResponse, String> {
        let url = format!("http://127.0.0.1:{port}/v1/embeddings");

        // Input is a JSON array of strings (from embed_request)
        let texts: Vec<String> = serde_json::from_str(&request.input)
            .unwrap_or_else(|_| vec![request.input.clone()]);

        let body = serde_json::json!({
            "input": texts
        });

        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Embedding request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Embedding endpoint returned {status}: {text}"));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse embedding response: {e}"))?;

        // Parse the OpenAI-compatible response:
        // { "data": [ { "embedding": [f32...], "index": 0 }, ... ] }
        let data = json["data"]
            .as_array()
            .ok_or("Embedding response missing 'data' array")?;

        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(data.len());
        for item in data {
            let embedding = item["embedding"]
                .as_array()
                .ok_or("Embedding item missing 'embedding' array")?;
            let vec: Vec<f32> = embedding
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            embeddings.push(vec);
        }

        Ok(TaskResponse::Embeddings(embeddings))
    }
}
