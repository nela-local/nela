import React, { useState, useEffect, useMemo } from "react";
import { X, Download, Loader2, Trash2, Sparkles, Save, CheckCircle, SlidersHorizontal, Cpu } from "lucide-react";
import type { RegisteredModel, RagModelPreferences } from "../types";
import { KITTEN_TTS_VOICES } from "../types";
import { Api, type CompatibilityRating } from "../api";
import InstallModelModal from "./InstallModelModal";
import "./ModelsSettingsModal.css";

interface ModelsSettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  models: RegisteredModel[];
  modelCatalog?: RegisteredModel[];
  downloads?: Record<string, { progress: number; status: string }>;
  onDownload: (modelId: string) => void;
  onCancelDownload?: (modelId: string) => void;
  onUninstall?: (modelId: string) => void;
  onDownloadMissingOptional?: () => void;
  onConfirm?: (title: string, message: string, confirmLabel?: string) => Promise<boolean>;
  workspaceId?: string;
  onModelsUpdated?: () => void;
}

type ParamControl = {
  key: string;
  label: string;
  description: string;
  type: "slider" | "select";
  min?: number;
  max?: number;
  step?: number;
  defaultValue: string;
  options?: Array<{ value: string; label: string }>;
  valueFormatter?: (value: number) => string;
  showContextHint?: boolean;
};

const OPTIONAL_TASKS = new Set(["embed", "grade", "classify"]);
const CORE_TASKS = new Set([
  "chat",
  "summarize",
  "mindmap",
  "enrich",
  "hyde",
  "podcast_script",
  "vision_chat",
  "tts",
  "transcribe",
  "stt",
]);

type AdvancedCategory = "embedding" | "grader" | "classifier";

const GROUPS: Array<{ id: string; label: string; description: string; category?: AdvancedCategory; match: (model: RegisteredModel) => boolean }> = [
  {
    id: "embedding",
    label: "Embedding Models",
    description: "Embedding models convert text into vectors so the app can find semantically similar content during retrieval.",
    category: "embedding",
    match: (model) => model.tasks.includes("embed"),
  },
  {
    id: "grader",
    label: "Grader Models",
    description: "Grader models score and rerank retrieved chunks so the most relevant context is used in answers.",
    category: "grader",
    match: (model) => model.tasks.includes("grade"),
  },
  {
    id: "router",
    label: "Router / Classifier Models",
    description: "Classifier and router models categorize inputs and help route tasks to the most appropriate pipeline.",
    category: "classifier",
    match: (model) => model.tasks.includes("classify"),
  },
  {
    id: "other",
    label: "Other Advanced Models",
    description: "Specialized helper models used for advanced tasks beyond core chat, vision, and audio workflows.",
    match: (model) => model.tasks.some((t) => !CORE_TASKS.has(t) && !OPTIONAL_TASKS.has(t)),
  },
];

const BOOLEAN_OPTIONS = [
  { value: "true", label: "Enabled" },
  { value: "false", label: "Disabled" },
];

const OPTIMAL_CONTEXT_RATINGS = new Set<CompatibilityRating>([
  "efficient",
  "usable",
  "satisfies",
]);

const parseNumber = (raw: string | undefined, fallback: number): number => {
  if (!raw) return fallback;
  const value = Number.parseFloat(raw);
  return Number.isFinite(value) ? value : fallback;
};

const clamp = (value: number, min: number, max: number): number => {
  if (value < min) return min;
  if (value > max) return max;
  return value;
};

const getBackendLabel = (backend: string | undefined): string => {
  switch (backend) {
    case "LlamaServer":
      return "Llama";
    case "KittenTts":
      return "KittenTTS";
    case "Parakeet":
      return "Parakeet";
    case "OnnxClassifier":
      return "ONNX Classifier";
    case "CrossEncoder":
      return "Cross Encoder";
    default:
      return backend ?? "Unknown";
  }
};

const getParamControls = (model: RegisteredModel | null): ParamControl[] => {
  if (!model) return [];

  switch (model.backend) {
    case "LlamaServer":
      return [
        {
          key: "ctx_size",
          label: "Context Size",
          description: "Maximum prompt window in tokens. Larger values use more RAM.",
          type: "slider",
          min: 512,
          max: 32768,
          step: 512,
          defaultValue: "4096",
          valueFormatter: (v) => `${Math.round(v)} tokens`,
          showContextHint: true,
        },
        {
          key: "max_tokens",
          label: "Max Output Tokens",
          description: "Upper bound on response length per generation.",
          type: "slider",
          min: 64,
          max: 8192,
          step: 64,
          defaultValue: "2048",
          valueFormatter: (v) => `${Math.round(v)}`,
        },
        {
          key: "temp",
          label: "Temperature",
          description: "Higher values increase creativity and randomness.",
          type: "slider",
          min: 0,
          max: 2,
          step: 0.05,
          defaultValue: "0.7",
          valueFormatter: (v) => v.toFixed(2),
        },
        {
          key: "top_p",
          label: "Top P",
          description: "Nucleus sampling cutoff. Lower values make outputs more focused.",
          type: "slider",
          min: 0.05,
          max: 1,
          step: 0.01,
          defaultValue: "0.9",
          valueFormatter: (v) => v.toFixed(2),
        },
        {
          key: "top_k",
          label: "Top K",
          description: "Limits sampling to the K most likely next tokens.",
          type: "slider",
          min: 1,
          max: 200,
          step: 1,
          defaultValue: "40",
          valueFormatter: (v) => `${Math.round(v)}`,
        },
        {
          key: "repeat_penalty",
          label: "Repeat Penalty",
          description: "Discourages repeated phrases in long outputs.",
          type: "slider",
          min: 0.8,
          max: 2,
          step: 0.01,
          defaultValue: "1.1",
          valueFormatter: (v) => v.toFixed(2),
        },
        {
          key: "flash_attn",
          label: "Flash Attention",
          description: "Enables fast attention kernels when supported by the device.",
          type: "select",
          defaultValue: "false",
          options: BOOLEAN_OPTIONS,
        },
        {
          key: "mlock",
          label: "Memory Lock",
          description: "Attempts to keep model pages resident in RAM to reduce paging.",
          type: "select",
          defaultValue: "false",
          options: BOOLEAN_OPTIONS,
        },
      ];

    case "KittenTts":
      return [
        {
          key: "voice",
          label: "Default Voice",
          description: "Default speaker voice used for speech generation.",
          type: "select",
          defaultValue: "Leo",
          options: KITTEN_TTS_VOICES.map((voice) => ({ value: voice, label: voice })),
        },
        {
          key: "speed",
          label: "Default Speed",
          description: "Playback speed multiplier for generated speech.",
          type: "slider",
          min: 0.5,
          max: 2,
          step: 0.1,
          defaultValue: "1.0",
          valueFormatter: (v) => `${v.toFixed(1)}x`,
        },
      ];

    case "Parakeet":
      return [
        {
          key: "max_symbols_per_step",
          label: "Max Symbols / Step",
          description: "Decoder safety cap per frame. Higher can improve recall but is slower.",
          type: "slider",
          min: 1,
          max: 20,
          step: 1,
          defaultValue: "10",
          valueFormatter: (v) => `${Math.round(v)}`,
        },
        {
          key: "preemphasis",
          label: "Preemphasis",
          description: "High-frequency boost before mel features. Keep near 0.97 unless tuning.",
          type: "slider",
          min: 0,
          max: 1,
          step: 0.01,
          defaultValue: "0.97",
          valueFormatter: (v) => v.toFixed(2),
        },
        {
          key: "dither",
          label: "Dither",
          description: "Small input noise for numerical stability in quiet audio.",
          type: "slider",
          min: 0,
          max: 0.001,
          step: 0.00001,
          defaultValue: "0.00001",
          valueFormatter: (v) => v.toFixed(5),
        },
      ];

    default:
      return [];
  }
};

const ModelsSettingsModal: React.FC<ModelsSettingsModalProps> = ({
  isOpen,
  onClose,
  models,
  modelCatalog = [],
  downloads = {},
  onDownload,
  onCancelDownload,
  onUninstall,
  onDownloadMissingOptional,
  onConfirm,
  workspaceId,
  onModelsUpdated,
}) => {
  const [ragPrefs, setRagPrefs] = useState<RagModelPreferences>({
    embed_model_id: null,
    llm_model_id: null,
  });
  const [ragPrefsSaving, setRagPrefsSaving] = useState(false);
  const [ragPrefsSaved, setRagPrefsSaved] = useState(false);
  const [selectedParamModelId, setSelectedParamModelId] = useState("");
  const [openGroupHelpId, setOpenGroupHelpId] = useState<string | null>(null);
  const [paramDraft, setParamDraft] = useState<Record<string, string>>({});
  const [paramSaving, setParamSaving] = useState(false);
  const [paramSaved, setParamSaved] = useState(false);
  const [paramError, setParamError] = useState<string | null>(null);
  const [contextHint, setContextHint] = useState<{ rating: CompatibilityRating; reason: string } | null>(null);
  const [contextHintLoading, setContextHintLoading] = useState(false);
  const [activePickerGroupId, setActivePickerGroupId] = useState<string | null>(null);

  const selectedParamModel = useMemo(
    () => models.find((m) => m.id === selectedParamModelId) ?? null,
    [models, selectedParamModelId]
  );

  const paramControls = useMemo(
    () => getParamControls(selectedParamModel),
    [selectedParamModel]
  );

  // Load RAG preferences when modal opens
  useEffect(() => {
    if (isOpen && workspaceId) {
      Api.getRagModelPreferences(workspaceId)
        .then(setRagPrefs)
        .catch((e) => console.error("Failed to load RAG preferences:", e));
    }
  }, [isOpen, workspaceId]);

  // Reset saved indicator when preferences change
  useEffect(() => {
    setRagPrefsSaved(false);
  }, [ragPrefs]);

  useEffect(() => {
    if (!isOpen) {
      setOpenGroupHelpId(null);
      return;
    }

    const handleMouseDown = (event: MouseEvent) => {
      const candidate = event.target;
      if (!(candidate instanceof HTMLElement)) return;
      if (candidate.closest(".settings-group-help-anchor")) return;
      setOpenGroupHelpId(null);
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpenGroupHelpId(null);
      }
    };

    document.addEventListener("mousedown", handleMouseDown);
    document.addEventListener("keydown", handleKeyDown);

    return () => {
      document.removeEventListener("mousedown", handleMouseDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    if (!selectedParamModelId || !models.some((m) => m.id === selectedParamModelId)) {
      setSelectedParamModelId(models[0]?.id ?? "");
    }
  }, [isOpen, models, selectedParamModelId]);

  useEffect(() => {
    if (!selectedParamModel) return;
    setParamDraft({ ...(selectedParamModel.params ?? {}) });
    setParamSaved(false);
    setParamError(null);
  }, [selectedParamModel?.id]);

  useEffect(() => {
    if (!isOpen || selectedParamModel?.backend !== "LlamaServer") {
      setContextHint(null);
      setContextHintLoading(false);
      return;
    }

    const rawContext = paramDraft.ctx_size ?? selectedParamModel.params?.ctx_size ?? "4096";
    const contextLength = Number.parseInt(rawContext, 10);
    if (!Number.isFinite(contextLength) || contextLength <= 0) {
      setContextHint(null);
      setContextHintLoading(false);
      return;
    }

    let cancelled = false;
    const timer = window.setTimeout(async () => {
      setContextHintLoading(true);
      try {
        const approxSizeMb = Math.max(selectedParamModel.memory_mb, 256);
        const compatibility = await Api.checkCompatibility(
          approxSizeMb,
          selectedParamModel.memory_mb,
          undefined,
          selectedParamModel.model_file,
          contextLength
        );
        if (!cancelled) {
          setContextHint({ rating: compatibility.rating, reason: compatibility.reason });
        }
      } catch (error) {
        if (!cancelled) {
          console.warn("Failed to compute context compatibility hint:", error);
          setContextHint(null);
        }
      } finally {
        if (!cancelled) {
          setContextHintLoading(false);
        }
      }
    }, 220);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [isOpen, selectedParamModel, paramDraft.ctx_size]);

  const handleSaveRagPrefs = async () => {
    if (!workspaceId) return;
    setRagPrefsSaving(true);
    try {
      await Api.saveRagModelPreferences(workspaceId, ragPrefs);
      setRagPrefsSaved(true);
      setTimeout(() => setRagPrefsSaved(false), 2000);
    } catch (e) {
      console.error("Failed to save RAG preferences:", e);
    } finally {
      setRagPrefsSaving(false);
    }
  };

  const activePickerGroup = useMemo(
    () => GROUPS.find((group) => group.id === activePickerGroupId) ?? null,
    [activePickerGroupId]
  );

  const activePickerModels = useMemo(() => {
    if (!activePickerGroup) return [];
    const source = modelCatalog.length > 0 ? modelCatalog : models;
    return source
      .filter(activePickerGroup.match)
      .map((model) => ({
        name: model.name,
        path: model.id,
        is_downloaded: model.is_downloaded,
        gdrive_id: model.gdrive_id ?? null,
      }));
  }, [activePickerGroup, modelCatalog, models]);

  const getControlValue = (control: ParamControl): string => {
    return paramDraft[control.key] ?? selectedParamModel?.params?.[control.key] ?? control.defaultValue;
  };

  const setControlValue = (key: string, value: string) => {
    setParamDraft((prev) => ({ ...prev, [key]: value }));
    setParamSaved(false);
    setParamError(null);
  };

  const handleSaveParams = async () => {
    if (!selectedParamModel) return;

    setParamSaving(true);
    setParamError(null);
    try {
      const merged = {
        ...(selectedParamModel.params ?? {}),
        ...paramDraft,
      };
      const cleaned = Object.fromEntries(
        Object.entries(merged).filter(([, value]) => value !== "")
      );

      const updated = await Api.updateModelParams(selectedParamModel.id, cleaned);
      setParamDraft({ ...(updated.params ?? {}) });
      setParamSaved(true);
      onModelsUpdated?.();
      window.setTimeout(() => setParamSaved(false), 2000);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setParamError(message);
    } finally {
      setParamSaving(false);
    }
  };

  if (!isOpen) return null;

  const optionalModels = models.filter((model) => model.tasks.some((t) => OPTIONAL_TASKS.has(t)));
  const missingOptional = optionalModels.filter((model) => !model.is_downloaded && model.gdrive_id);
  const showRuntimeParamsInAdvanced = false;

  // Filter models for RAG settings dropdowns
  const embedModels = models.filter(
    (m) => m.tasks.includes("embed") && m.is_downloaded
  );
  const llmModels = models.filter(
    (m) => (m.tasks.includes("chat") || m.tasks.includes("enrich")) && m.is_downloaded
  );

  const contextOptimal = contextHint ? OPTIMAL_CONTEXT_RATINGS.has(contextHint.rating) : null;

  return (
    <div className="settings-modal-overlay" onClick={onClose}>
      <div className="settings-modal" onClick={(e) => e.stopPropagation()}>
        <div className="settings-modal-header">
          <div className="settings-title">
            <Sparkles size={18} />
            <span>Advanced Models</span>
          </div>
          <button className="settings-close" onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        <div className="settings-modal-body">
          <div className="settings-layout">
            <div className="settings-main">
              <div className="settings-summary">
                <div>
                  Optional models improve retrieval, grading, and routing quality.
                </div>
                <button
                  className="settings-primary"
                  onClick={onDownloadMissingOptional}
                  disabled={missingOptional.length === 0}
                >
                  Download Missing ({missingOptional.length})
                </button>
              </div>

              {/* RAG Pipeline Settings Section */}
              {workspaceId && (
                <div className="settings-group">
                  <div className="settings-group-title">RAG Pipeline Settings</div>
                  <div className="settings-rag-prefs">
                    <div className="settings-rag-field">
                      <label htmlFor="embed-model-select">Embedding Model</label>
                      <select
                        id="embed-model-select"
                        className="settings-select"
                        value={ragPrefs.embed_model_id ?? ""}
                        onChange={(e) =>
                          setRagPrefs((prev) => ({
                            ...prev,
                            embed_model_id: e.target.value || null,
                          }))
                        }
                      >
                        <option value="">Auto (default)</option>
                        {embedModels.map((m) => (
                          <option key={m.id} value={m.id}>
                            {m.name}
                          </option>
                        ))}
                      </select>
                      <span className="settings-field-hint">
                        Model used for generating vector embeddings
                      </span>
                    </div>

                    <div className="settings-rag-field">
                      <label htmlFor="llm-model-select">LLM Model</label>
                      <select
                        id="llm-model-select"
                        className="settings-select"
                        value={ragPrefs.llm_model_id ?? ""}
                        onChange={(e) =>
                          setRagPrefs((prev) => ({
                            ...prev,
                            llm_model_id: e.target.value || null,
                          }))
                        }
                      >
                        <option value="">Auto (default)</option>
                        {llmModels.map((m) => (
                          <option key={m.id} value={m.id}>
                            {m.name}
                          </option>
                        ))}
                      </select>
                      <span className="settings-field-hint">
                        Model used for enrichment and chat tasks
                      </span>
                    </div>

                    <div className="settings-rag-actions">
                      <button
                        className="settings-primary"
                        onClick={handleSaveRagPrefs}
                        disabled={ragPrefsSaving}
                      >
                        {ragPrefsSaving ? (
                          <Loader2 size={14} className="animate-spin" />
                        ) : ragPrefsSaved ? (
                          <CheckCircle size={14} />
                        ) : (
                          <Save size={14} />
                        )}
                        <span>{ragPrefsSaved ? "Saved" : "Save Preferences"}</span>
                      </button>
                    </div>
                  </div>
                </div>
              )}

              {GROUPS.map((group) => {
                const groupModels = models.filter(group.match);
                const shownModels = groupModels.filter((m) => m.is_downloaded || downloads[m.id] !== undefined);
                const isCategoryGroup = !!group.category;
                if (!isCategoryGroup && groupModels.length === 0) return null;

                return (
                  <div key={group.id} className="settings-group">
                    <div className="settings-group-title-row">
                      <div className="settings-group-title">{group.label}</div>
                      <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                        <div className="settings-group-help-anchor">
                        <button
                          type="button"
                          className="settings-group-help-btn"
                          aria-label={`Explain ${group.label}`}
                          aria-expanded={openGroupHelpId === group.id}
                          onClick={(event) => {
                            event.stopPropagation();
                            setOpenGroupHelpId((prev) => (prev === group.id ? null : group.id));
                          }}
                        >
                          ?
                        </button>
                        {openGroupHelpId === group.id && (
                          <div className="settings-group-help-popover" role="tooltip">
                            <p>{group.description}</p>
                          </div>
                        )}
                        </div>
                        {group.category && (
                          <button
                            className="settings-icon-btn"
                            onClick={() => setActivePickerGroupId(group.id)}
                            title={`Choose ${group.label}`}
                          >
                            <Download size={14} />
                          </button>
                        )}
                      </div>
                    </div>
                    <div className="settings-list">
                      {shownModels.length === 0 && (
                        <div className="settings-empty">No installed models found in this category.</div>
                      )}
                      {shownModels.map((model) => {
                        const isDownloading = downloads[model.id] !== undefined;
                        const dlState = downloads[model.id];

                        return (
                          <div key={model.id} className="settings-item">
                            <div className="settings-item-info">
                              <div className="settings-item-name">{model.name}</div>
                              <div className="settings-item-meta">
                                {model.is_downloaded ? "Installed" : "Not installed"}
                              </div>
                            </div>
                            {isDownloading ? (
                              <div className="settings-actions">
                                <div className="settings-progress">
                                  <Loader2 size={14} className="animate-spin" />
                                  <span>{dlState.progress.toFixed(0)}%</span>
                                </div>
                                {onCancelDownload && (
                                  <button
                                    className="settings-icon-btn"
                                    onClick={() => onCancelDownload(model.id)}
                                    title="Cancel Download"
                                  >
                                    <X size={14} />
                                  </button>
                                )}
                              </div>
                            ) : (
                              <div className="settings-actions">
                                {model.is_downloaded && onUninstall && (
                                  <button
                                    className="settings-icon-btn danger"
                                    onClick={async () => {
                                      const ok = onConfirm
                                        ? await onConfirm("Delete model", `Delete ${model.name} from this device?`, "Delete")
                                        : window.confirm(`Uninstall ${model.name}?`);
                                      if (ok) onUninstall(model.id);
                                    }}
                                    title="Uninstall Model"
                                  >
                                    <Trash2 size={14} />
                                  </button>
                                )}
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  </div>
                );
              })}
            </div>

            {showRuntimeParamsInAdvanced && (
            <aside className="settings-params-sidebar">
              <div className="settings-params-header">
                <div className="settings-title-inline">
                  <SlidersHorizontal size={15} />
                  <span>Model Parameters</span>
                </div>
                <span className="settings-field-hint">Configure any registered model</span>
              </div>

              <div className="settings-model-picker">
                {models.map((model) => (
                  <button
                    key={model.id}
                    className={`settings-model-pill ${selectedParamModelId === model.id ? "active" : ""}`}
                    onClick={() => setSelectedParamModelId(model.id)}
                    type="button"
                  >
                    <span className="settings-model-pill-name">{model.name}</span>
                    <span className="settings-model-pill-meta">
                      {getBackendLabel(model.backend)} · {model.is_downloaded ? "Installed" : "Missing"}
                    </span>
                  </button>
                ))}
              </div>

              <div className="settings-params-panel">
                {!selectedParamModel && (
                  <div className="settings-empty">
                    Select a model to edit parameters.
                  </div>
                )}

                {selectedParamModel && (
                  <>
                    <div className="settings-params-model-header">
                      <div className="settings-item-name">{selectedParamModel.name}</div>
                      <div className="settings-item-meta">
                        {getBackendLabel(selectedParamModel.backend)} backend
                      </div>
                    </div>

                    {paramControls.length === 0 && (
                      <div className="settings-empty">
                        This backend does not expose configurable runtime parameters yet.
                      </div>
                    )}

                    {paramControls.map((control) => {
                      const rawValue = getControlValue(control);

                      if (control.type === "select") {
                        const options = control.options ?? [];
                        const hasCurrentValue = options.some((option) => option.value === rawValue);
                        return (
                          <div className="settings-param-card" key={control.key}>
                            <div className="settings-param-head">
                              <label htmlFor={`model-param-${control.key}`}>{control.label}</label>
                            </div>
                            <select
                              id={`model-param-${control.key}`}
                              className="settings-select"
                              value={rawValue}
                              onChange={(e) => setControlValue(control.key, e.target.value)}
                            >
                              {!hasCurrentValue && (
                                <option value={rawValue}>{rawValue}</option>
                              )}
                              {options.map((option) => (
                                <option key={option.value} value={option.value}>
                                  {option.label}
                                </option>
                              ))}
                            </select>
                            <span className="settings-field-hint">{control.description}</span>
                          </div>
                        );
                      }

                      const min = control.min ?? 0;
                      const max = control.max ?? 1;
                      const step = control.step ?? 0.1;
                      const numericValue = clamp(parseNumber(rawValue, parseNumber(control.defaultValue, min)), min, max);
                      const displayValue = control.valueFormatter
                        ? control.valueFormatter(numericValue)
                        : String(numericValue);

                      return (
                        <div className="settings-param-card" key={control.key}>
                          <div className="settings-param-head">
                            <label htmlFor={`model-param-${control.key}`}>{control.label}</label>
                            <span>{displayValue}</span>
                          </div>
                          <input
                            id={`model-param-${control.key}`}
                            type="range"
                            min={min}
                            max={max}
                            step={step}
                            value={numericValue}
                            onChange={(e) => setControlValue(control.key, e.target.value)}
                            className="settings-slider"
                          />
                          <span className="settings-field-hint">{control.description}</span>

                          {control.showContextHint && (
                            <div
                              className={`settings-context-hint ${
                                contextOptimal === null
                                  ? ""
                                  : contextOptimal
                                    ? "optimal"
                                    : "suboptimal"
                              }`}
                            >
                              <Cpu size={14} />
                              <span>
                                {contextHintLoading && "Checking device fit for this context size..."}
                                {!contextHintLoading && contextHint && (
                                  contextOptimal
                                    ? `Optimal for this device: ${contextHint.reason}`
                                    : `Sub-optimal for this device: ${contextHint.reason}`
                                )}
                                {!contextHintLoading && !contextHint && "Device-fit hint unavailable for current value."}
                              </span>
                            </div>
                          )}
                        </div>
                      );
                    })}

                    {selectedParamModel.backend === "LlamaServer" && (
                      <div className="settings-note">
                        Startup parameters are applied immediately by restarting the model if it is currently loaded.
                      </div>
                    )}

                    {selectedParamModel.backend === "Parakeet" && (
                      <div className="settings-note">
                        ASR parameter updates are applied on model restart and affect future transcriptions.
                      </div>
                    )}

                    {paramError && <div className="settings-error">Failed to save: {paramError}</div>}

                    <div className="settings-param-actions">
                      <button
                        className="settings-primary"
                        onClick={handleSaveParams}
                        disabled={paramSaving || !selectedParamModel.is_downloaded}
                      >
                        {paramSaving ? (
                          <Loader2 size={14} className="animate-spin" />
                        ) : paramSaved ? (
                          <CheckCircle size={14} />
                        ) : (
                          <Save size={14} />
                        )}
                        <span>{paramSaved ? "Saved" : "Save Parameters"}</span>
                      </button>
                    </div>
                  </>
                )}
              </div>
            </aside>
            )}
          </div>
        </div>
      </div>
      <InstallModelModal
        isOpen={activePickerGroup !== null}
        onClose={() => setActivePickerGroupId(null)}
        models={activePickerModels}
        title={activePickerGroup ? `${activePickerGroup.label} Downloads` : "Model Downloads"}
        onDownload={onDownload}
        onCancelDownload={onCancelDownload}
        onUninstall={onUninstall}
        onConfirm={onConfirm}
        downloads={downloads}
      />
    </div>
  );
};

export default ModelsSettingsModal;
