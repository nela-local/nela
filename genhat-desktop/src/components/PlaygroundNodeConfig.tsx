/**
 * PlaygroundNodeConfig — right-side config drawer for a selected node.
 *
 * Renders a form appropriate for each NodeKind.
 * All field changes propagate back through onUpdateConfig.
 */

import { useState, useEffect } from "react";
import { X, Trash2, AlertTriangle, ChevronDown } from "lucide-react";
import { Api } from "../api";
import type { RegisteredModel } from "../types";
import type {
  PlaygroundNode,
  NodeConfig,
  ManualConfig,
  LlmChatConfig,
  SummarizeConfig,
  ScheduleConfig,
  FileReadConfig,
  FileWriteConfig,
  EmailFetchConfig,
  ConditionConfig,
  TemplateConfig,
  ScriptConfig,
  TtsConfig,
  TranscribeConfig,
  RagQueryConfig,
  NotificationConfig,
  HttpRequestConfig,
  RssReaderConfig,
  JsonPathConfig,
  SetVariableConfig,
} from "../app/playgroundTypes";

interface Props {
  node: PlaygroundNode;
  onUpdateConfig: (nodeId: string, patch: Partial<NodeConfig>) => void;
  onClose: () => void;
  onDelete?: () => void;
}

// ─── Reusable field components ────────────────────────────────────────────────

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1">
      <label className="text-xs font-medium text-txt-primary">{label}</label>
      {hint && <p className="text-[10px] text-txt-muted leading-snug">{hint}</p>}
      {children}
    </div>
  );
}

const inputCls =
  "w-full bg-white/5 border border-white/15 rounded-lg px-2.5 py-1.5 text-xs text-txt-primary placeholder:text-txt-muted focus:outline-none focus:border-white/30 transition-colors";

// ─── Model picker ─────────────────────────────────────────────────────────────

/**
 * Dropdown that lists every registered model supporting a given task.
 * Falls back to a plain text input if the model list is empty or still loading.
 */
function ModelPicker({
  value,
  onChange,
  taskFilter = "chat",
  placeholder = "Select a model",
}: {
  value: string;
  onChange: (id: string) => void;
  taskFilter?: string;
  placeholder?: string;
}) {
  const [models, setModels] = useState<RegisteredModel[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    Api.listRegisteredModels()
      .then(list => {
        const filtered = list.filter(
          m => m.is_downloaded && m.tasks.some(t => t.toLowerCase() === taskFilter.toLowerCase())
        );
        setModels(filtered);
      })
      .catch(() => setModels([]))
      .finally(() => setLoading(false));
  }, [taskFilter]);

  if (loading) {
    return (
      <div className={inputCls + " text-txt-muted animate-pulse"}>
        Loading models…
      </div>
    );
  }

  if (models.length === 0) {
    // Fallback: plain text so the user can still type an id manually
    return (
      <input
        className={inputCls}
        value={value}
        onChange={e => onChange(e.target.value)}
        placeholder={placeholder}
      />
    );
  }

  return (
    <div className="relative">
      <select
        className={inputCls + " appearance-none pr-7 cursor-pointer"}
        value={value}
        onChange={e => onChange(e.target.value)}
      >
        {value === "" && (
          <option value="" disabled>
            {placeholder}
          </option>
        )}
        {models.map(m => (
          <option key={m.id} value={m.id}>
            {m.name}
          </option>
        ))}
      </select>
      <ChevronDown
        size={12}
        className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-txt-muted"
      />
    </div>
  );
}

const textareaCls =
  "w-full bg-white/5 border border-white/15 rounded-lg px-2.5 py-1.5 text-xs text-txt-primary placeholder:text-txt-muted focus:outline-none focus:border-white/30 transition-colors resize-none min-h-[80px] font-mono";

// ─── Per-kind config forms ─────────────────────────────────────────────────────

function ScheduleForm({
  config,
  onChange,
}: {
  config: ScheduleConfig;
  onChange: (patch: Partial<ScheduleConfig>) => void;
}) {
  return (
    <Field label="Cron Expression" hint="e.g. '0 8 * * *' for every day at 8 AM">
      <input
        className={inputCls}
        value={config.cron}
        onChange={e => onChange({ cron: e.target.value })}
        placeholder="0 8 * * *"
      />
    </Field>
  );
}

function LlmChatForm({
  config,
  onChange,
}: {
  config: LlmChatConfig;
  onChange: (patch: Partial<LlmChatConfig>) => void;
}) {
  return (
    <>
      <Field label="Model" hint="Only downloaded models that support the chat task are listed.">
        <ModelPicker
          value={config.model_id}
          onChange={id => onChange({ model_id: id })}
          taskFilter="chat"
          placeholder="Select a model"
        />
      </Field>
      <Field label="System Prompt" hint="Guides how the model responds. The pipeline input is sent as the user message.">
        <textarea
          className={textareaCls}
          value={config.system_prompt}
          onChange={e => onChange({ system_prompt: e.target.value })}
          placeholder="You are a helpful AI assistant working inside an automated pipeline. You receive text from the previous step as user input. Process it thoughtfully and respond with clear, accurate output."
          rows={4}
        />
      </Field>
      <Field label="Temperature (0–2)">
        <input
          type="number"
          className={inputCls}
          value={config.temperature ?? 0.7}
          min={0}
          max={2}
          step={0.1}
          onChange={e => onChange({ temperature: parseFloat(e.target.value) })}
        />
      </Field>
      <Field label="Max Output Tokens">
        <input
          type="number"
          className={inputCls}
          value={config.max_tokens ?? ""}
          min={1}
          max={262144}
          step={1}
          placeholder="2048"
          onChange={e => onChange({ max_tokens: e.target.value ? parseInt(e.target.value) : undefined })}
        />
      </Field>
      <Field label="Context Size (tokens)">
        <input
          type="number"
          className={inputCls}
          value={config.ctx_size ?? ""}
          min={512}
          max={262144}
          step={512}
          placeholder="8192 — increase if input is long (max 262144)"
          onChange={e => onChange({ ctx_size: e.target.value ? parseInt(e.target.value) : undefined })}
        />
      </Field>
    </>
  );
}

// ─── Summarize style → default system prompt mapping ────────────────────────

const SUMMARIZE_DEFAULTS: Record<string, string> = {
  bullet:
    "You are a precise summarization assistant. Condense the provided content into clear, concise bullet points. Each bullet captures one key idea, fact, or action item. Prioritize concrete information: names, dates, decisions, and next steps. Skip filler phrases, opinions, and redundant detail. Output only the bullet points, one per line, prefixed with '•'.",
  paragraph:
    "You are a precise summarization assistant. Synthesize the provided content into a coherent paragraph that captures the main ideas and key facts. Write in a neutral, informative tone. Avoid personal commentary, opinions, or filler phrases.",
  tldr:
    "You are a precise summarization assistant. Write a concise TL;DR of the provided content in 2–4 sentences. Capture the most critical information: what happened, what matters, what to do. Be direct and factual. Omit all unnecessary detail and filler.",
};

function SummarizeForm({
  config,
  onChange,
}: {
  config: SummarizeConfig;
  onChange: (patch: Partial<SummarizeConfig>) => void;
}) {
  return (
    <>
      <Field label="Model" hint="Only downloaded models that support the chat task are listed.">
        <ModelPicker
          value={config.model_id}
          onChange={id => onChange({ model_id: id })}
          taskFilter="chat"
          placeholder="Select a model"
        />
      </Field>
      <Field label="Style">
        <select
          className={inputCls}
          value={config.style ?? "bullet"}
          onChange={e => onChange({ style: e.target.value as SummarizeConfig["style"] })}
        >
          <option value="bullet">Bullet points</option>
          <option value="paragraph">Paragraph</option>
          <option value="tldr">TL;DR</option>
        </select>
      </Field>
      <Field
        label="System Prompt"
        hint="Leave empty to use the built-in default for the selected style."
      >
        <textarea
          className={textareaCls}
          value={config.system_prompt ?? ""}
          onChange={e => onChange({ system_prompt: e.target.value || undefined })}
          placeholder={SUMMARIZE_DEFAULTS[config.style ?? "bullet"] ?? SUMMARIZE_DEFAULTS.bullet}
          rows={4}
        />
      </Field>
      <Field label="Max Output Tokens">
        <input
          type="number"
          className={inputCls}
          value={config.max_tokens ?? ""}
          min={1}
          max={262144}
          step={1}
          placeholder="2048"
          onChange={e => onChange({ max_tokens: e.target.value ? parseInt(e.target.value) : undefined })}
        />
      </Field>
      <Field label="Context Size (tokens)">
        <input
          type="number"
          className={inputCls}
          value={config.ctx_size ?? ""}
          min={512}
          max={262144}
          step={512}
          placeholder="32768 — increase if input is long (max 262144)"
          onChange={e => onChange({ ctx_size: e.target.value ? parseInt(e.target.value) : undefined })}
        />
      </Field>
    </>
  );
}

function FileReadForm({
  config,
  onChange,
}: {
  config: FileReadConfig;
  onChange: (patch: Partial<FileReadConfig>) => void;
}) {
  return (
    <Field label="File Path">
      <input
        className={inputCls}
        value={config.path}
        onChange={e => onChange({ path: e.target.value })}
        placeholder="/home/user/notes.txt"
      />
    </Field>
  );
}

function FileWriteForm({
  config,
  onChange,
}: {
  config: FileWriteConfig;
  onChange: (patch: Partial<FileWriteConfig>) => void;
}) {
  return (
    <>
      <Field label="File Path">
        <input
          className={inputCls}
          value={config.path}
          onChange={e => onChange({ path: e.target.value })}
          placeholder="/home/user/output.txt"
        />
      </Field>
      <label className="flex items-center gap-2 text-xs text-txt-primary cursor-pointer select-none">
        <input
          type="checkbox"
          checked={config.append ?? false}
          onChange={e => onChange({ append: e.target.checked })}
          className="accent-indigo-500"
        />
        Append instead of overwrite
      </label>
    </>
  );
}

function EmailFetchForm({
  config,
  onChange,
}: {
  config: EmailFetchConfig;
  onChange: (patch: Partial<EmailFetchConfig>) => void;
}) {
  const [showPasswordInput, setShowPasswordInput] = useState(false);
  const [rawPassword, setRawPassword] = useState("");

  return (
    <>
      <Field label="IMAP Host">
        <input
          className={inputCls}
          value={config.host}
          onChange={e => onChange({ host: e.target.value })}
          placeholder="imap.gmail.com"
        />
      </Field>
      <Field label="Port">
        <input
          type="number"
          className={inputCls}
          value={config.port}
          onChange={e => onChange({ port: parseInt(e.target.value, 10) })}
        />
      </Field>
      <Field label="Username">
        <input
          className={inputCls}
          value={config.username}
          onChange={e => onChange({ username: e.target.value })}
          placeholder="you@example.com"
        />
      </Field>
      <Field
        label="Password"
        hint="Stored in OS keystore — never saved in the pipeline JSON."
      >
        {config.password_key ? (
          <div className="flex items-center gap-2">
            <span className="text-xs text-emerald-400 flex-1 truncate font-mono">
              {config.password_key}
            </span>
            <button
              className="text-[10px] text-txt-muted hover:text-txt-primary underline"
              onClick={() => setShowPasswordInput(true)}
            >
              Update
            </button>
          </div>
        ) : (
          <button
            className="w-full text-xs text-indigo-400 hover:text-indigo-300 border border-indigo-500/30 rounded-lg py-1.5 transition-colors"
            onClick={() => setShowPasswordInput(true)}
          >
            Set password
          </button>
        )}
        {showPasswordInput && (
          <div className="flex gap-2 mt-1">
            <input
              type="password"
              className={inputCls + " flex-1"}
              value={rawPassword}
              onChange={e => setRawPassword(e.target.value)}
              placeholder="Enter password"
              autoFocus
            />
            <button
              className="text-xs bg-indigo-600 hover:bg-indigo-500 text-white px-2 rounded-lg transition-colors"
              onClick={() => {
                if (!rawPassword) return;
                // The key format is deterministic — the backend stores the actual secret
                const key = `nela/pipeline/imap/${config.username || "user"}`;
                // Fire-and-forget: store via Tauri IPC (import from api.ts)
                import("../api").then(({ storeCredential }) => {
                  storeCredential(key, rawPassword).catch(console.error);
                });
                onChange({ password_key: key });
                setRawPassword("");
                setShowPasswordInput(false);
              }}
            >
              Save
            </button>
          </div>
        )}
      </Field>
      <Field label="Mailbox">
        <input
          className={inputCls}
          value={config.mailbox ?? "INBOX"}
          onChange={e => onChange({ mailbox: e.target.value })}
        />
      </Field>
      <Field label="Max Messages">
        <input
          type="number"
          className={inputCls}
          value={config.max_messages ?? 20}
          onChange={e => onChange({ max_messages: parseInt(e.target.value, 10) })}
        />
      </Field>
      <label className="flex items-center gap-2 text-xs text-txt-primary cursor-pointer select-none">
        <input
          type="checkbox"
          checked={config.unseen_only ?? true}
          onChange={e => onChange({ unseen_only: e.target.checked })}
          className="accent-indigo-500"
        />
        Unseen messages only
      </label>
    </>
  );
}

function ConditionForm({
  config,
  onChange,
}: {
  config: ConditionConfig;
  onChange: (patch: Partial<ConditionConfig>) => void;
}) {
  return (
    <Field
      label="Expression"
      hint="Use 'ctx' to access the run context. e.g. ctx.output.length > 0"
    >
      <input
        className={inputCls + " font-mono"}
        value={config.expression}
        onChange={e => onChange({ expression: e.target.value })}
        placeholder="ctx.output.length > 0"
      />
    </Field>
  );
}

function TemplateForm({
  config,
  onChange,
}: {
  config: TemplateConfig;
  onChange: (patch: Partial<TemplateConfig>) => void;
}) {
  return (
    <Field
      label="Handlebars Template"
      hint="Use {{output}}, {{date}}, etc. Context keys come from preceding nodes."
    >
      <textarea
        className={textareaCls}
        value={config.template}
        onChange={e => onChange({ template: e.target.value })}
        placeholder="{{output}}"
      />
    </Field>
  );
}

function NotificationForm({
  config,
  onChange,
}: {
  config: NotificationConfig;
  onChange: (patch: Partial<NotificationConfig>) => void;
}) {
  return (
    <>
      <Field label="Title">
        <input
          className={inputCls}
          value={config.title}
          onChange={e => onChange({ title: e.target.value })}
          placeholder="NELA"
        />
      </Field>
      <Field label="Body Template" hint="Handlebars — use {{output}} for pipeline output.">
        <textarea
          className={textareaCls}
          value={config.body_template ?? ""}
          onChange={e => onChange({ body_template: e.target.value })}
          placeholder="{{output}}"
        />
      </Field>
    </>
  );
}

function ScriptForm({
  config,
  onChange,
}: {
  config: ScriptConfig;
  onChange: (patch: Partial<ScriptConfig>) => void;
}) {
  return (
    <>
      <div className="flex items-start gap-2 p-2.5 rounded-lg bg-amber-500/10 border border-amber-500/30 text-amber-300">
        <AlertTriangle size={14} className="mt-0.5 shrink-0" />
        <p className="text-[10px] leading-snug">
          Custom scripts run as your user account on this machine. Only use scripts you
          trust. The pipeline context is piped to stdin as JSON; read JSON from stdout.
        </p>
      </div>
      <Field label="Script Path">
        <input
          className={inputCls + " font-mono"}
          value={config.script_path}
          onChange={e => onChange({ script_path: e.target.value })}
          placeholder="/home/user/my_script.py"
        />
      </Field>
      <Field label="Interpreter" hint="Leave blank to detect from shebang.">
        <input
          className={inputCls + " font-mono"}
          value={config.interpreter ?? ""}
          onChange={e => onChange({ interpreter: e.target.value })}
          placeholder="python3"
        />
      </Field>
      <Field label="Timeout (seconds)">
        <input
          type="number"
          className={inputCls}
          value={config.timeout_secs ?? 30}
          onChange={e => onChange({ timeout_secs: parseInt(e.target.value, 10) })}
        />
      </Field>
    </>
  );
}

function TtsForm({
  config,
  onChange,
}: {
  config: TtsConfig;
  onChange: (patch: Partial<TtsConfig>) => void;
}) {
  return (
    <>
      <Field label="TTS Engine" hint="Only downloaded models that support the text-to-speech task are listed.">
        <ModelPicker
          value={config.engine_id}
          onChange={id => onChange({ engine_id: id })}
          taskFilter="tts"
          placeholder="Select a TTS engine"
        />
      </Field>
      <Field
        label="Output File Path"
        hint="Optional. Where to save the generated audio. Leave empty to use a temporary file."
      >
        <input
          className={inputCls}
          value={config.output_path ?? ""}
          onChange={e => onChange({ output_path: e.target.value || undefined })}
          placeholder="/home/user/output.wav"
        />
      </Field>
    </>
  );
}

function TranscribeForm({
  config,
  onChange,
}: {
  config: TranscribeConfig;
  onChange: (patch: Partial<TranscribeConfig>) => void;
}) {
  return (
    <>
      <Field label="ASR Model" hint="Only downloaded models that support the transcription task are listed.">
        <ModelPicker
          value={config.model_id}
          onChange={id => onChange({ model_id: id })}
          taskFilter="transcribe"
          placeholder="Select an ASR model"
        />
      </Field>
      <Field
        label="Audio File Path"
        hint="Optional. Provide an audio file path directly. If left empty, the output of the previous node is used as the file path."
      >
        <input
          className={inputCls}
          value={config.file_path ?? ""}
          onChange={e => onChange({ file_path: e.target.value || undefined })}
          placeholder="/home/user/recording.wav"
        />
      </Field>
    </>
  );
}

function RagQueryForm({
  config,
  onChange,
}: {
  config: RagQueryConfig;
  onChange: (patch: Partial<RagQueryConfig>) => void;
}) {
  return (
    <>
      <div className="flex items-start gap-2 p-2.5 rounded-lg bg-indigo-500/10 border border-indigo-500/30 text-indigo-300">
        <p className="text-[10px] leading-snug">
          Queries the <strong>currently active Knowledge Base</strong> workspace. Open the KB
          sidebar and activate a workspace before running this pipeline.
        </p>
      </div>
      <Field label="Top K Results" hint="Number of relevant chunks to retrieve.">
        <input
          type="number"
          className={inputCls}
          value={config.top_k ?? 5}
          min={1}
          max={50}
          onChange={e => onChange({ top_k: parseInt(e.target.value, 10) })}
        />
      </Field>
      <Field
        label="Query Template"
        hint="Optional. Handlebars template to build the query string. Leave empty to use the previous node's output as the query."
      >
        <textarea
          className={textareaCls}
          value={config.query_template ?? ""}
          onChange={e => onChange({ query_template: e.target.value || undefined })}
          placeholder="Summarize key facts about: {{output}}"
          rows={3}
        />
      </Field>
    </>
  );
}

function HttpRequestForm({
  config,
  onChange,
}: {
  config: HttpRequestConfig;
  onChange: (patch: Partial<HttpRequestConfig>) => void;
}) {
  return (
    <>
      <Field label="URL">
        <input
          className={inputCls}
          value={config.url}
          onChange={e => onChange({ url: e.target.value })}
          placeholder="https://api.example.com/data"
        />
      </Field>
      <Field label="Method">
        <select
          className={inputCls}
          value={config.method}
          onChange={e => onChange({ method: e.target.value as HttpRequestConfig["method"] })}
        >
          {(["GET", "POST", "PUT", "PATCH", "DELETE"] as const).map(m => (
            <option key={m} value={m}>{m}</option>
          ))}
        </select>
      </Field>
      <Field label="Timeout (seconds)">
        <input
          type="number"
          className={inputCls}
          value={config.timeout_secs ?? 30}
          onChange={e => onChange({ timeout_secs: parseInt(e.target.value, 10) })}
        />
      </Field>
      <Field
        label="Body Template"
        hint="Handlebars template. Only used for POST/PUT/PATCH requests."
      >
        <textarea
          className={textareaCls}
          value={config.body_template ?? ""}
          onChange={e => onChange({ body_template: e.target.value || undefined })}
          placeholder='{"input": "{{output}}"}'
        />
      </Field>
    </>
  );
}

function RssReaderForm({
  config,
  onChange,
}: {
  config: RssReaderConfig;
  onChange: (patch: Partial<RssReaderConfig>) => void;
}) {
  return (
    <>
      <Field label="Feed URL">
        <input
          className={inputCls}
          value={config.url}
          onChange={e => onChange({ url: e.target.value })}
          placeholder="https://example.com/feed.rss"
        />
      </Field>
      <Field label="Max Items">
        <input
          type="number"
          className={inputCls}
          value={config.max_items ?? 10}
          min={1}
          max={100}
          onChange={e => onChange({ max_items: parseInt(e.target.value, 10) })}
        />
      </Field>
    </>
  );
}

function JsonPathForm({
  config,
  onChange,
}: {
  config: JsonPathConfig;
  onChange: (patch: Partial<JsonPathConfig>) => void;
}) {
  return (
    <Field
      label="JSON Pointer"
      hint="RFC 6901 syntax. e.g. /items/0/title extracts the first item's title field."
    >
      <input
        className={inputCls + " font-mono"}
        value={config.path}
        onChange={e => onChange({ path: e.target.value })}
        placeholder="/items/0/title"
      />
    </Field>
  );
}

function SetVariableForm({
  config,
  onChange,
}: {
  config: SetVariableConfig;
  onChange: (patch: Partial<SetVariableConfig>) => void;
}) {
  return (
    <>
      <Field label="Variable Name" hint="Stored in ctx.vars for subsequent nodes.">
        <input
          className={inputCls + " font-mono"}
          value={config.name}
          onChange={e => onChange({ name: e.target.value })}
          placeholder="my_var"
        />
      </Field>
      <Field label="Value Template" hint="Handlebars — {{output}}, {{date}}, etc.">
        <textarea
          className={textareaCls}
          value={config.value_template}
          onChange={e => onChange({ value_template: e.target.value })}
          placeholder="{{output}}"
        />
      </Field>
    </>
  );
}

// ─── Main drawer ─────────────────────────────────────────────────────────────

export default function PlaygroundNodeConfig({ node, onUpdateConfig, onClose, onDelete }: Props) {
  const { kind, config } = node.data;

  function patch(p: Partial<NodeConfig>) {
    onUpdateConfig(node.id, p);
  }

  return (
    <aside
      className="
        w-72 shrink-0 flex flex-col border-l border-white/10 bg-void-950
        overflow-y-auto
      "
    >
      {/* header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-white/10">
        <span className="text-xs font-semibold text-txt-primary">{node.data.label}</span>
        <div className="flex items-center gap-2">
          {onDelete && (
            <button
              onClick={onDelete}
              title="Delete node"
              className="text-rose-400 hover:text-rose-300 transition-colors"
            >
              <Trash2 size={14} />
            </button>
          )}
          <button
            onClick={onClose}
            className="text-txt-muted hover:text-txt-primary transition-colors"
          >
            <X size={14} />
          </button>
        </div>
      </div>

      {/* label editor */}
      <div className="px-4 pt-3">
        <Field label="Node Label">
          <input
            className={inputCls}
            value={node.data.label}
            onChange={e =>
              onUpdateConfig(node.id, { ...(config as object), _label: e.target.value } as Partial<NodeConfig>)
            }
            placeholder="Label"
          />
        </Field>
      </div>

      {/* kind-specific fields */}
      <div className="flex flex-col gap-4 px-4 py-3">
        {kind === "Schedule" && (
          <ScheduleForm config={config as ScheduleConfig} onChange={patch} />
        )}
        {kind === "LlmChat" && (
          <LlmChatForm config={config as LlmChatConfig} onChange={patch} />
        )}
        {kind === "Summarize" && (
          <SummarizeForm config={config as SummarizeConfig} onChange={patch} />
        )}
        {kind === "FileRead" && (
          <FileReadForm config={config as FileReadConfig} onChange={patch} />
        )}
        {kind === "FileWrite" && (
          <FileWriteForm config={config as FileWriteConfig} onChange={patch} />
        )}
        {kind === "EmailFetch" && (
          <EmailFetchForm config={config as EmailFetchConfig} onChange={patch} />
        )}
        {kind === "Condition" && (
          <ConditionForm config={config as ConditionConfig} onChange={patch} />
        )}
        {kind === "Template" && (
          <TemplateForm config={config as TemplateConfig} onChange={patch} />
        )}
        {kind === "Notification" && (
          <NotificationForm config={config as NotificationConfig} onChange={patch} />
        )}
        {kind === "Script" && (
          <ScriptForm config={config as ScriptConfig} onChange={patch} />
        )}
        {kind === "Tts" && (
          <TtsForm config={config as TtsConfig} onChange={patch} />
        )}
        {kind === "Transcribe" && (
          <TranscribeForm config={config as TranscribeConfig} onChange={patch} />
        )}
        {kind === "RagQuery" && (
          <RagQueryForm config={config as RagQueryConfig} onChange={patch} />
        )}
        {kind === "HttpRequest" && (
          <HttpRequestForm config={config as HttpRequestConfig} onChange={patch} />
        )}
        {kind === "RssReader" && (
          <RssReaderForm config={config as RssReaderConfig} onChange={patch} />
        )}
        {kind === "JsonPath" && (
          <JsonPathForm config={config as JsonPathConfig} onChange={patch} />
        )}
        {kind === "SetVariable" && (
          <SetVariableForm config={config as SetVariableConfig} onChange={patch} />
        )}
        {kind === "Manual" && (
          <>
            <p className="text-xs text-txt-muted">
              This node triggers the pipeline when you press Run.
            </p>
            <Field
              label="Seed Input"
              hint="Optional. Text injected into the pipeline context when run manually. Leave empty to start with a blank context."
            >
              <textarea
                className={textareaCls}
                value={(config as ManualConfig).prompt ?? ""}
                onChange={e =>
                  patch({ prompt: e.target.value || undefined } as Partial<ManualConfig>)
                }
                placeholder="Enter an initial prompt or text to pass to the first node…"
                rows={3}
              />
            </Field>
          </>
        )}
      </div>
    </aside>
  );
}
