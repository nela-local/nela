//! Playground node handlers — one function per NodeKind.

use super::types::*;
use crate::commands::rag::RagPipelineState;
use crate::registry::types::{TaskRequest, TaskResponse, TaskType};
use crate::router::TaskRouter;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Emitter, Manager};

/// Mutable execution context passed between nodes.
pub struct RunContext {
    pub output: String,
    pub vars: std::collections::HashMap<String, serde_json::Value>,
}

impl RunContext {
    pub fn new() -> Self {
        Self {
            output: String::new(),
            vars: Default::default(),
        }
    }
}

// ─── LLM helper ──────────────────────────────────────────────────────────────

async fn route_text(
    router: &Arc<TaskRouter>,
    model_id: &str,
    system: &str,
    prompt: &str,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    ctx_size: Option<u32>,
) -> Result<String, String> {
    let mut extra = HashMap::new();
    if !system.is_empty() {
        extra.insert("system_prompt".to_string(), system.to_string());
    }
    if let Some(t) = temperature {
        extra.insert("temperature".to_string(), t.to_string());
    }
    if let Some(mt) = max_tokens {
        extra.insert("max_tokens".to_string(), mt.to_string());
    }
    if let Some(cs) = ctx_size {
        extra.insert("ctx_size".to_string(), cs.to_string());
    }
    let req = TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Chat,
        input: prompt.to_string(),
        model_override: if model_id.is_empty() { None } else { Some(model_id.to_string()) },
        extra,
    };
    match router.route(&req).await? {
        TaskResponse::Text(t) | TaskResponse::ChatWithThinking { content: t, .. } => Ok(t),
        TaskResponse::Error(e) => Err(format!("LLM error: {e}")),
        other => Err(format!("Unexpected LLM response: {:?}", other)),
    }
}

fn resolve_credential(key: &str, app_data_dir: &std::path::PathBuf) -> Result<String, String> {
    let safe_key: String = key
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let path = app_data_dir.join("creds").join(&safe_key);
    std::fs::read_to_string(&path)
        .map_err(|e| format!("Credential not found for key '{key}': {e}"))
        .map(|s| s.trim().to_string())
}

fn render_handlebars(template: &str, ctx: &RunContext) -> Result<String, String> {
    let hb = handlebars::Handlebars::new();
    let mut data = serde_json::Map::new();
    data.insert("output".into(), serde_json::Value::String(ctx.output.clone()));
    for (k, v) in &ctx.vars {
        data.insert(k.clone(), v.clone());
    }
    hb.render_template(template, &serde_json::Value::Object(data))
        .map_err(|e| format!("Handlebars render failed: {e}"))
}

// ─── Node dispatch ─────────────────────────────────────────────────────────────

/// Execute a single node. Returns updated output text.
pub async fn execute_node(
    node: &PlaygroundNode,
    ctx: &mut RunContext,
    router: &Arc<TaskRouter>,
    app_data_dir: &std::path::PathBuf,
    app_handle: &tauri::AppHandle,
) -> Result<String, String> {
    let kind = node.data.kind.as_str();
    let cfg_val = node.data.config.clone();

    match kind {
        "Manual" | "Schedule" => {
            // Trigger nodes — seed ctx.output with the configured prompt if present,
            // otherwise pass through existing context.
            if let Ok(cfg) = serde_json::from_value::<ManualConfig>(cfg_val) {
                if let Some(prompt) = cfg.prompt.filter(|p| !p.trim().is_empty()) {
                    return Ok(prompt);
                }
            }
            Ok(ctx.output.clone())
        }

        "LlmChat" => {
            let cfg: LlmChatConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid LlmChat config: {e}"))?;
            let prompt = if ctx.output.is_empty() {
                "Hello".to_string()
            } else {
                ctx.output.clone()
            };
            let response = route_text(
                    router, &cfg.model_id, &cfg.system_prompt, &prompt,
                    cfg.temperature, cfg.max_tokens, cfg.ctx_size,
                )
                .await
                .map_err(|e| format!("LlmChat failed: {e}"))?;
            Ok(response)
        }

        "Summarize" => {
            let cfg: SummarizeConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid Summarize config: {e}"))?;
            let style = cfg.style.as_deref().unwrap_or("bullet");

            // Built-in style defaults — detailed enough to guide the model but not over-specified.
            let default_system = match style {
                "bullet" => concat!(
                    "You are a precise summarization assistant. ",
                    "Condense the provided content into clear, concise bullet points. ",
                    "Each bullet captures one key idea, fact, or action item. ",
                    "Prioritize concrete information: names, dates, decisions, and next steps. ",
                    "Skip filler phrases, opinions, and redundant detail. ",
                    "Output only the bullet points, one per line, prefixed with '•'.",
                ),
                "tldr" => concat!(
                    "You are a precise summarization assistant. ",
                    "Write a concise TL;DR of the provided content in 2–4 sentences. ",
                    "Capture the most critical information: what happened, what matters, what to do. ",
                    "Be direct and factual. Omit all unnecessary detail and filler.",
                ),
                _ => concat!(
                    "You are a precise summarization assistant. ",
                    "Synthesize the provided content into a coherent paragraph that captures the main ideas and key facts. ",
                    "Write in a neutral, informative tone. ",
                    "Avoid personal commentary, opinions, or filler phrases.",
                ),
            };

            // Use the user-supplied prompt if non-empty, otherwise fall back to the style default.
            let system = cfg.system_prompt
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(default_system);

            let response = route_text(
                    router, &cfg.model_id, system, &ctx.output,
                    Some(0.3), cfg.max_tokens, cfg.ctx_size,
                )
                .await
                .map_err(|e| format!("Summarize failed: {e}"))?;
            Ok(response)
        }

        "Template" => {
            let cfg: TemplateConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid Template config: {e}"))?;
            render_handlebars(&cfg.template, ctx)
        }

        "Condition" => {
            let cfg: ConditionConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid Condition config: {e}"))?;
            let result = evaluate_condition(&cfg.expression, ctx);
            Ok(if result { "true" } else { "false" }.to_string())
        }

        "FileRead" => {
            let cfg: FileReadConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid FileRead config: {e}"))?;
            std::fs::read_to_string(&cfg.path)
                .map_err(|e| format!("FileRead: cannot read '{}': {e}", cfg.path))
        }

        "FileWrite" => {
            let cfg: FileWriteConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid FileWrite config: {e}"))?;
            if cfg.append {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&cfg.path)
                    .map_err(|e| format!("FileWrite: cannot open '{}': {e}", cfg.path))?;
                writeln!(file, "{}", ctx.output)
                    .map_err(|e| format!("FileWrite: write failed '{}': {e}", cfg.path))?;
            } else {
                std::fs::write(&cfg.path, &ctx.output)
                    .map_err(|e| format!("FileWrite: write failed '{}': {e}", cfg.path))?;
            }
            Ok(ctx.output.clone())
        }

        "EmailFetch" => {
            let cfg: EmailFetchConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid EmailFetch config: {e}"))?;
            let password = resolve_credential(&cfg.password_key, app_data_dir)?;
            fetch_emails(&cfg, &password).await
        }

        "Notification" => {
            let cfg: NotificationConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid Notification config: {e}"))?;
            let body = if let Some(tmpl) = &cfg.body_template {
                render_handlebars(tmpl, ctx).unwrap_or_else(|_| ctx.output.clone())
            } else {
                ctx.output.clone()
            };
            // Send an OS-level desktop notification via notify-rust (direct D-Bus call on Linux,
            // native API on macOS/Windows). Works even when the app window is minimised.
            notify_rust::Notification::new()
                .summary(&cfg.title)
                .body(&body)
                .show()
                .map_err(|e| format!("Notification failed: {e}"))?;
            Ok(ctx.output.clone())
        }

        "Script" => {
            let cfg: ScriptConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid Script config: {e}"))?;
            run_script(&cfg, ctx, app_data_dir).await
        }

        "Transcribe" => {
            let cfg: TranscribeConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid Transcribe config: {e}"))?;
            // Use the explicit file_path if configured, otherwise treat ctx.output as the audio path.
            let audio_path = cfg.file_path
                .as_deref()
                .filter(|p| !p.is_empty())
                .unwrap_or(&ctx.output);
            let req = TaskRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                task_type: TaskType::Transcribe,
                input: audio_path.to_string(),
                model_override: if cfg.model_id.is_empty() { None } else { Some(cfg.model_id.clone()) },
                extra: Default::default(),
            };
            match router.route(&req).await? {
                TaskResponse::Transcription { segments } => {
                    Ok(segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" "))
                }
                TaskResponse::Error(e) => Err(format!("Transcribe error: {e}")),
                other => Err(format!("Unexpected Transcribe response: {:?}", other)),
            }
        }

        "Tts" => {
            let cfg: TtsConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid Tts config: {e}"))?;
            let mut extra = HashMap::new();
            if let Some(out_path) = cfg.output_path.as_deref().filter(|p| !p.is_empty()) {
                extra.insert("output_path".to_string(), out_path.to_string());
            }
            let req = TaskRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                task_type: TaskType::Tts,
                input: ctx.output.clone(),
                model_override: if cfg.engine_id.is_empty() { None } else { Some(cfg.engine_id.clone()) },
                extra,
            };
            match router.route(&req).await? {
                TaskResponse::FilePath(path) => Ok(path),
                TaskResponse::Error(e) => Err(format!("TTS error: {e}")),
                other => Err(format!("Unexpected TTS response: {:?}", other)),
            }
        }

        "RagQuery" => {
            let cfg: RagQueryConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid RagQuery config: {e}"))?;
            let rag_state = app_handle
                .try_state::<RagPipelineState>()
                .ok_or_else(|| "RAG pipeline state not available".to_string())?;
            let pipeline = rag_state.active_pipeline()?;
            let top_k = cfg.top_k.unwrap_or(5) as usize;
            // Build the query: render the template if provided, otherwise use ctx.output directly.
            let query = if let Some(tmpl) = &cfg.query_template {
                render_handlebars(tmpl, ctx).unwrap_or_else(|_| ctx.output.clone())
            } else {
                ctx.output.clone()
            };
            let result = pipeline.query(&query, top_k).await?;
            Ok(result.answer)
        }

        "HttpRequest" => {
            let cfg: HttpRequestConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid HttpRequest config: {e}"))?;
            let url = render_handlebars(&cfg.url, ctx).unwrap_or_else(|_| cfg.url.clone());
            let method = cfg.method.to_uppercase();
            let timeout_secs = cfg.timeout_secs.unwrap_or(30);
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .build()
                .map_err(|e| format!("HTTP client build error: {e}"))?;
            let mut req = match method.as_str() {
                "POST" => client.post(&url),
                "PUT" => client.put(&url),
                "PATCH" => client.patch(&url),
                "DELETE" => client.delete(&url),
                "HEAD" => client.head(&url),
                _ => client.get(&url),
            };
            if let Some(headers) = &cfg.headers {
                for (k, v) in headers {
                    req = req.header(k.as_str(), v.as_str());
                }
            }
            if let Some(body_tmpl) = &cfg.body_template {
                let body = render_handlebars(body_tmpl, ctx).unwrap_or_default();
                req = req.body(body);
            }
            let resp = req.send().await.map_err(|e| format!("HTTP request failed: {e}"))?;
            let status = resp.status();
            if !status.is_success() {
                return Err(format!("HTTP {}", status.as_u16()));
            }
            resp.text().await.map_err(|e| format!("HTTP response read failed: {e}"))
        }

        "RssReader" => {
            let cfg: RssReaderConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid RssReader config: {e}"))?;
            let max_items = cfg.max_items.unwrap_or(10) as usize;
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| format!("HTTP client build error: {e}"))?;
            let xml = client
                .get(&cfg.url)
                .header("Accept", "application/rss+xml, application/atom+xml, application/xml, text/xml")
                .send()
                .await
                .map_err(|e| format!("RSS fetch failed: {e}"))?
                .text()
                .await
                .map_err(|e| format!("RSS read failed: {e}"))?;
            parse_feed(&xml, max_items)
        }

        "JsonPath" => {
            let cfg: JsonPathConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid JsonPath config: {e}"))?;
            let parsed: serde_json::Value = serde_json::from_str(&ctx.output)
                .map_err(|e| format!("JsonPath: input is not valid JSON: {e}"))?;
            let result = parsed
                .pointer(&cfg.path)
                .ok_or_else(|| format!("JsonPath: path '{}' not found", cfg.path))?;
            Ok(match result {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
        }

        "SetVariable" => {
            let cfg: SetVariableConfig =
                serde_json::from_value(cfg_val).map_err(|e| format!("Invalid SetVariable config: {e}"))?;
            let value = render_handlebars(&cfg.value_template, ctx)?;
            ctx.vars.insert(cfg.name.clone(), serde_json::Value::String(value));
            Ok(ctx.output.clone())
        }

        other => Err(format!("Unknown node kind: {other}")),
    }
}

// ─── Condition evaluator ──────────────────────────────────────────────────────

fn evaluate_condition(expression: &str, ctx: &RunContext) -> bool {
    // Supported forms (case-insensitive):
    //   contains:<substring>
    //   not_empty
    //   starts_with:<prefix>
    //   ends_with:<suffix>
    if expression.eq_ignore_ascii_case("not_empty") {
        return !ctx.output.trim().is_empty();
    }
    if let Some(sub) = expression.strip_prefix("contains:") {
        return ctx.output.to_lowercase().contains(&sub.to_lowercase());
    }
    if let Some(prefix) = expression.strip_prefix("starts_with:") {
        return ctx.output.starts_with(prefix);
    }
    if let Some(suffix) = expression.strip_prefix("ends_with:") {
        return ctx.output.ends_with(suffix);
    }
    // Fallback — always true so pipeline continues
    log::warn!("Condition expression not understood: '{expression}'; defaulting to true");
    true
}

// ─── Email fetch ──────────────────────────────────────────────────────────────

async fn fetch_emails(cfg: &EmailFetchConfig, password: &str) -> Result<String, String> {
    let host = cfg.host.clone();
    let port = cfg.port;
    let username = cfg.username.clone();
    let password = password.to_string();
    let mailbox = cfg.mailbox.clone();
    let max_messages = cfg.max_messages;
    let unseen_only = cfg.unseen_only;

    tokio::task::spawn_blocking(move || -> Result<String, String> {
        let tls = native_tls::TlsConnector::builder()
            .build()
            .map_err(|e| format!("Failed to build TLS connector: {e}"))?;
        let addr = format!("{host}:{port}");
        let client = imap::connect(&addr, &host, &tls)
            .map_err(|e| format!("IMAP connect to '{addr}' failed: {e}"))?;

        let mut session = client
            .login(&username, &password)
            .map_err(|(e, _)| format!("IMAP login failed: {e}"))?;

        session
            .select(&mailbox)
            .map_err(|e| format!("IMAP SELECT '{mailbox}' failed: {e}"))?;

        let criteria = if unseen_only { "UNSEEN" } else { "ALL" };
        let uid_set = session.search(criteria)
            .map_err(|e| format!("IMAP SEARCH failed: {e}"))?;

        // Sort descending so highest sequence numbers (newest messages) come first.
        let mut uid_vec: Vec<u32> = uid_set.into_iter().collect();
        uid_vec.sort_unstable_by(|a, b| b.cmp(a));

        let count = (max_messages as usize).min(uid_vec.len());
        let seq_set: Vec<String> = uid_vec
            .into_iter()
            .take(count)
            .map(|n| n.to_string())
            .collect();

        let mut messages = Vec::new();
        for seq in &seq_set {
            if let Ok(fetched) = session.fetch(seq, "RFC822.TEXT") {
                for msg in fetched.iter() {
                    if let Some(body) = msg.text() {
                        if let Ok(text) = std::str::from_utf8(body) {
                            messages.push(text.chars().take(2000).collect::<String>());
                        }
                    }
                }
            }
        }

        session.logout().ok();
        Ok(messages.join("\n\n---\n\n"))
    })
    .await
    .map_err(|e| format!("Email fetch task panicked: {e}"))?
}

// ─── Script runner ─────────────────────────────────────────────────────────────

async fn run_script(
    cfg: &ScriptConfig,
    ctx: &RunContext,
    _app_data_dir: &std::path::PathBuf,
) -> Result<String, String> {
    // Resolve interpreter: explicit config > shebang > default to script extension
    let script_path = std::path::Path::new(&cfg.script_path);

    let interpreter = if let Some(interp) = &cfg.interpreter {
        if !interp.is_empty() {
            Some(interp.clone())
        } else {
            None
        }
    } else {
        // Read shebang line
        std::fs::read_to_string(script_path)
            .ok()
            .and_then(|s| {
                let first = s.lines().next()?;
                if first.starts_with("#!") {
                    Some(first[2..].trim().to_string())
                } else {
                    None
                }
            })
    };

    let stdin_json = serde_json::json!({
        "output": ctx.output,
        "vars": ctx.vars,
    })
    .to_string();

    let mut cmd = if let Some(interp) = interpreter {
        let mut c = tokio::process::Command::new(&interp);
        c.arg(&cfg.script_path);
        c
    } else {
        tokio::process::Command::new(&cfg.script_path)
    };

    use tokio::io::AsyncWriteExt;
    use std::process::Stdio;

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn script '{}': {e}", cfg.script_path))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(stdin_json.as_bytes()).await.ok();
    }

    let timeout = std::time::Duration::from_secs(cfg.timeout_secs);
    let result = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| format!("Script timed out after {}s", cfg.timeout_secs))?
        .map_err(|e| format!("Script process error: {e}"))?;

    if !result.stderr.is_empty() {
        log::info!(
            "[Script stderr] {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    if !result.status.success() {
        return Err(format!(
            "Script exited with {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    // Try to parse as JSON with an "output" key; otherwise return raw stdout
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
        if let Some(out) = v.get("output").and_then(|o| o.as_str()) {
            return Ok(out.to_string());
        }
    }
    Ok(stdout)
}

// ─── RSS / Atom feed parser ────────────────────────────────────────────────────

fn parse_feed(xml: &str, max_items: usize) -> Result<String, String> {
    let mut items: Vec<String> = Vec::new();
    let mut search_from = 0;

    while items.len() < max_items {
        // Find the next <item> or <entry> opening tag (RSS vs Atom)
        let item_pos = xml[search_from..].find("<item").map(|p| (search_from + p, "item"));
        let entry_pos = xml[search_from..].find("<entry").map(|p| (search_from + p, "entry"));

        let (abs_start, tag_name) = match (item_pos, entry_pos) {
            (Some(a), Some(b)) => if a.0 <= b.0 { a } else { b },
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };

        let close_tag = format!("</{}>", tag_name);
        let Some(rel_end) = xml[abs_start..].find(&close_tag) else { break };
        let abs_end = abs_start + rel_end + close_tag.len();
        let block = &xml[abs_start..abs_end];

        let title = extract_xml_text(block, "title").unwrap_or_default();
        let link = extract_xml_text(block, "link")
            .filter(|s| s.starts_with("http"))
            .or_else(|| extract_xml_attr(block, "link", "href"))
            .unwrap_or_default();

        if !title.is_empty() {
            items.push(if link.is_empty() {
                format!("• {title}")
            } else {
                format!("• {title}\n  {link}")
            });
        }

        search_from = abs_end;
    }

    if items.is_empty() {
        Ok("(no items found in feed)".to_string())
    } else {
        Ok(items.join("\n\n"))
    }
}

fn extract_xml_text(xml: &str, tag: &str) -> Option<String> {
    let open_prefix = format!("<{}", tag);
    let close_tag = format!("</{}>", tag);
    let start = xml.find(&open_prefix)?;
    let gt = xml[start..].find('>')?;
    let content_start = start + gt + 1;
    let end = xml[content_start..].find(&close_tag)?;
    let content = xml[content_start..content_start + end].trim();
    // Strip CDATA wrapper
    let content = content
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
        .unwrap_or(content)
        .trim();
    if content.is_empty() { None } else { Some(content.to_string()) }
}

fn extract_xml_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let open_prefix = format!("<{}", tag);
    let start = xml.find(&open_prefix)?;
    let gt = xml[start..].find('>')?;
    let tag_content = &xml[start..start + gt];
    let attr_prefix = format!("{}=\"", attr);
    let attr_start = tag_content.find(&attr_prefix)?;
    let value_start = attr_start + attr_prefix.len();
    let value_end = tag_content[value_start..].find('"')?;
    Some(tag_content[value_start..value_start + value_end].to_string())
}
