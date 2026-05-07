/**
 * Playground — agentic pipeline builder types.
 *
 * Mirrors the Rust-side types in src-tauri/src/playground/types.rs.
 * All cross-boundary data is plain JSON-serializable.
 */

// ─── Node kinds ────────────────────────────────────────────────────────────────

export type NodeKind =
  | "Schedule"
  | "Manual"
  | "LlmChat"
  | "Summarize"
  | "Transcribe"
  | "Tts"
  | "RagQuery"
  | "FileRead"
  | "FileWrite"
  | "EmailFetch"
  | "Condition"
  | "Template"
  | "Notification"
  | "Script";

// ─── Node config — one union member per NodeKind ──────────────────────────────

export interface ScheduleConfig {
  cron: string; // e.g. "0 8 * * *"
}

export interface ManualConfig {
  label?: string;
}

export interface LlmChatConfig {
  model_id: string;
  system_prompt: string;
  temperature?: number;
  max_tokens?: number;
  ctx_size?: number; // n_ctx passed to llama-server; up to 262144 (256k)
}

export interface SummarizeConfig {
  model_id: string;
  style?: "bullet" | "paragraph" | "tldr";
  /** Optional override for the built-in style-based system prompt. */
  system_prompt?: string;
  max_tokens?: number;
  ctx_size?: number; // n_ctx passed to llama-server; up to 262144 (256k)
}

export interface TranscribeConfig {
  model_id: string;
}

export interface TtsConfig {
  engine_id: string;
}

export interface RagQueryConfig {
  workspace_id: string;
  top_k?: number;
}

export interface FileReadConfig {
  path: string;
  encoding?: "utf8" | "base64";
}

export interface FileWriteConfig {
  path: string;
  append?: boolean;
}

export interface EmailFetchConfig {
  host: string;
  port: number;
  username: string;
  /** Never stored in plain text. Key references the OS keystore entry. */
  password_key: string;
  mailbox?: string;
  max_messages?: number;
  unseen_only?: boolean;
}

export interface ConditionConfig {
  expression: string; // e.g. "ctx.score > 0.5"
}

export interface TemplateConfig {
  template: string; // Handlebars template string
}

export interface NotificationConfig {
  title: string;
  body_template?: string;
}

export interface ScriptConfig {
  script_path: string;
  interpreter?: string; // "python3" | "node" | "bash" | …
  args?: string[];
  timeout_secs?: number;
}

export type NodeConfig =
  | ScheduleConfig
  | ManualConfig
  | LlmChatConfig
  | SummarizeConfig
  | TranscribeConfig
  | TtsConfig
  | RagQueryConfig
  | FileReadConfig
  | FileWriteConfig
  | EmailFetchConfig
  | ConditionConfig
  | TemplateConfig
  | NotificationConfig
  | ScriptConfig;

// ─── Graph primitives ─────────────────────────────────────────────────────────

export interface PlaygroundNodeData {
  kind: NodeKind;
  label: string;
  config: NodeConfig;
  [key: string]: unknown;
}

/** An xyflow node extended with playground metadata. */
export interface PlaygroundNode {
  id: string;
  type: "playgroundNode"; // custom React Flow node type name
  position: { x: number; y: number };
  data: PlaygroundNodeData;
}

export interface PlaygroundEdge {
  id: string;
  source: string;
  target: string;
  sourceHandle?: string;
  targetHandle?: string;
  label?: string;
}

// ─── Pipeline ─────────────────────────────────────────────────────────────────

export interface Pipeline {
  id: string;
  name: string;
  description?: string;
  auto_resume: boolean;
  nodes: PlaygroundNode[];
  edges: PlaygroundEdge[];
  created_at: number;
  updated_at: number;
}

// ─── Run state ────────────────────────────────────────────────────────────────

export type RunStatus = "idle" | "running" | "success" | "error";

export interface NodeRunState {
  node_id: string;
  status: RunStatus;
  output?: string;
  error?: string;
  started_at?: number;
  finished_at?: number;
}

export interface PipelineRun {
  pipeline_id: string;
  run_id: string;
  started_at: number;
  status: RunStatus;
  node_states: Record<string, NodeRunState>;
  log: string[];
}

// ─── Palette metadata (used to populate the drag-drop node list) ──────────────

export interface PaletteEntry {
  kind: NodeKind;
  label: string;
  description: string;
  category: "trigger" | "ai" | "io" | "logic" | "script";
  defaultConfig: NodeConfig;
}

export const PALETTE: PaletteEntry[] = [
  // triggers
  {
    kind: "Schedule",
    label: "Schedule",
    description: "Runs the pipeline on a cron schedule",
    category: "trigger",
    defaultConfig: { cron: "0 8 * * *" } satisfies ScheduleConfig,
  },
  {
    kind: "Manual",
    label: "Manual Run",
    description: "Triggered manually by the user",
    category: "trigger",
    defaultConfig: { label: "Start" } satisfies ManualConfig,
  },
  // AI
  {
    kind: "LlmChat",
    label: "LLM Chat",
    description: "Send a prompt to an LLM and get a response",
    category: "ai",
    defaultConfig: { model_id: "", system_prompt: "" } satisfies LlmChatConfig,
  },
  {
    kind: "Summarize",
    label: "Summarize",
    description: "Summarize text using a local LLM",
    category: "ai",
    defaultConfig: { model_id: "", style: "bullet" } satisfies SummarizeConfig,
  },
  {
    kind: "Transcribe",
    label: "Transcribe",
    description: "Speech-to-text via Parakeet ASR",
    category: "ai",
    defaultConfig: { model_id: "parakeet-tdt" } satisfies TranscribeConfig,
  },
  {
    kind: "Tts",
    label: "Text to Speech",
    description: "Convert text to audio using KittenTTS",
    category: "ai",
    defaultConfig: { engine_id: "kitten-tts" } satisfies TtsConfig,
  },
  {
    kind: "RagQuery",
    label: "RAG Query",
    description: "Retrieve context from an ingested knowledge base",
    category: "ai",
    defaultConfig: { workspace_id: "", top_k: 5 } satisfies RagQueryConfig,
  },
  // I/O
  {
    kind: "FileRead",
    label: "Read File",
    description: "Read a file from disk",
    category: "io",
    defaultConfig: { path: "" } satisfies FileReadConfig,
  },
  {
    kind: "FileWrite",
    label: "Write File",
    description: "Write text to a file on disk",
    category: "io",
    defaultConfig: { path: "", append: false } satisfies FileWriteConfig,
  },
  {
    kind: "EmailFetch",
    label: "Fetch Email",
    description: "Fetch emails via IMAP (on-device, credentials in OS keystore)",
    category: "io",
    defaultConfig: {
      host: "",
      port: 993,
      username: "",
      password_key: "",
      mailbox: "INBOX",
      max_messages: 20,
      unseen_only: true,
    } satisfies EmailFetchConfig,
  },
  {
    kind: "Notification",
    label: "Notification",
    description: "Show a system notification",
    category: "io",
    defaultConfig: { title: "NELA", body_template: "{{output}}" } satisfies NotificationConfig,
  },
  // logic
  {
    kind: "Condition",
    label: "Condition",
    description: "Branch based on a simple expression",
    category: "logic",
    defaultConfig: { expression: "" } satisfies ConditionConfig,
  },
  {
    kind: "Template",
    label: "Template",
    description: "Render a Handlebars template against pipeline context",
    category: "logic",
    defaultConfig: { template: "{{output}}" } satisfies TemplateConfig,
  },
  // script
  {
    kind: "Script",
    label: "Custom Script",
    description: "Run your own script. JSON context piped via stdin; JSON output read from stdout.",
    category: "script",
    defaultConfig: {
      script_path: "",
      interpreter: "",
      args: [],
      timeout_secs: 30,
    } satisfies ScriptConfig,
  },
];

