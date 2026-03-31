import React, { useState, useEffect } from "react";
import { X, Download, Loader2, Trash2, Sparkles, Save, CheckCircle } from "lucide-react";
import type { RegisteredModel, RagModelPreferences } from "../types";
import { Api } from "../api";
import "./ModelsSettingsModal.css";

interface ModelsSettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  models: RegisteredModel[];
  downloads?: Record<string, { progress: number; status: string }>;
  onDownload: (modelId: string) => void;
  onCancelDownload?: (modelId: string) => void;
  onUninstall?: (modelId: string) => void;
  onDownloadMissingOptional?: () => void;
  onConfirm?: (title: string, message: string, confirmLabel?: string) => Promise<boolean>;
  workspaceId?: string;
}

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

const GROUPS: Array<{ id: string; label: string; match: (model: RegisteredModel) => boolean }> = [
  {
    id: "embedding",
    label: "Embedding Models",
    match: (model) => model.tasks.includes("embed"),
  },
  {
    id: "grader",
    label: "Grader Models",
    match: (model) => model.tasks.includes("grade"),
  },
  {
    id: "router",
    label: "Router / Classifier Models",
    match: (model) => model.tasks.includes("classify"),
  },
  {
    id: "other",
    label: "Other Advanced Models",
    match: (model) => model.tasks.some((t) => !CORE_TASKS.has(t) && !OPTIONAL_TASKS.has(t)),
  },
];

const ModelsSettingsModal: React.FC<ModelsSettingsModalProps> = ({
  isOpen,
  onClose,
  models,
  downloads = {},
  onDownload,
  onCancelDownload,
  onUninstall,
  onDownloadMissingOptional,
  onConfirm,
  workspaceId,
}) => {
  const [ragPrefs, setRagPrefs] = useState<RagModelPreferences>({
    embed_model_id: null,
    llm_model_id: null,
  });
  const [ragPrefsSaving, setRagPrefsSaving] = useState(false);
  const [ragPrefsSaved, setRagPrefsSaved] = useState(false);

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

  if (!isOpen) return null;

  const optionalModels = models.filter((model) => model.tasks.some((t) => OPTIONAL_TASKS.has(t)));
  const missingOptional = optionalModels.filter((model) => !model.is_downloaded && model.gdrive_id);

  // Filter models for RAG settings dropdowns
  const embedModels = models.filter(
    (m) => m.tasks.includes("embed") && m.is_downloaded
  );
  const llmModels = models.filter(
    (m) => (m.tasks.includes("chat") || m.tasks.includes("enrich")) && m.is_downloaded
  );

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
            if (groupModels.length === 0) return null;

            return (
              <div key={group.id} className="settings-group">
                <div className="settings-group-title">{group.label}</div>
                <div className="settings-list">
                  {groupModels.map((model) => {
                    const isDownloading = downloads[model.id] !== undefined;
                    const dlState = downloads[model.id];
                    const canDownload = !!model.gdrive_id;

                    return (
                      <div key={model.id} className="settings-item">
                        <div className="settings-item-info">
                          <div className="settings-item-name">{model.name}</div>
                          <div className="settings-item-meta">
                            {model.is_downloaded ? "Installed" : "Not installed"}
                            {!canDownload && " · Not downloadable"}
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
                            <button
                              className="settings-icon-btn"
                              disabled={!canDownload || model.is_downloaded}
                              onClick={() => onDownload(model.id)}
                              title={canDownload ? "Download Model" : "Missing download link"}
                            >
                              <Download size={14} />
                            </button>
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
      </div>
    </div>
  );
};

export default ModelsSettingsModal;
