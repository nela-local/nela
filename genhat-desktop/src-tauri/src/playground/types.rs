//! Playground pipeline types — mirrors TypeScript `playgroundTypes.ts`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Node kinds ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum NodeConfig {
    Schedule(ScheduleConfig),
    Manual(ManualConfig),
    LlmChat(LlmChatConfig),
    Summarize(SummarizeConfig),
    Transcribe(TranscribeConfig),
    Tts(TtsConfig),
    RagQuery(RagQueryConfig),
    FileRead(FileReadConfig),
    FileWrite(FileWriteConfig),
    EmailFetch(EmailFetchConfig),
    Condition(ConditionConfig),
    Template(TemplateConfig),
    Notification(NotificationConfig),
    Script(ScriptConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    pub cron: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualConfig {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmChatConfig {
    pub model_id: String,
    pub system_prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummarizeConfig {
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>, // "bullet" | "paragraph" | "tldr"
    /// Optional user-provided system prompt; overrides the built-in style default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeConfig {
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsConfig {
    pub engine_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagQueryConfig {
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadConfig {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWriteConfig {
    pub path: String,
    #[serde(default)]
    pub append: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailFetchConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password_key: String, // keystore reference — NOT the actual password
    #[serde(default = "default_mailbox")]
    pub mailbox: String,
    #[serde(default = "default_max_messages")]
    pub max_messages: u32,
    #[serde(default = "default_true")]
    pub unseen_only: bool,
}

fn default_mailbox() -> String {
    "INBOX".to_string()
}
fn default_max_messages() -> u32 {
    20
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionConfig {
    pub expression: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateConfig {
    pub template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptConfig {
    pub script_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    30
}

// ─── Graph elements ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaygroundNodeData {
    pub label: String,
    pub kind: String,
    pub config: serde_json::Value, // raw JSON — re-typed in executor
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaygroundNode {
    pub id: String,
    pub r#type: String, // always "playgroundNode"
    pub position: NodePosition,
    pub data: PlaygroundNodeData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaygroundEdge {
    pub id: String,
    pub source: String,
    pub target: String,
}

// ─── Pipeline ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub auto_resume: bool,
    #[serde(default)]
    pub nodes: Vec<PlaygroundNode>,
    #[serde(default)]
    pub edges: Vec<PlaygroundEdge>,
    /// Unix epoch milliseconds — matches TypeScript `number` timestamps.
    pub created_at: i64,
    pub updated_at: i64,
}

// ─── Run state ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Idle,
    Running,
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRunState {
    pub node_id: String,
    pub status: RunStatus,
    pub output: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: String,
    pub pipeline_id: String,
    pub status: RunStatus,
    pub node_states: HashMap<String, NodeRunState>,
    pub log: Vec<String>,
}
