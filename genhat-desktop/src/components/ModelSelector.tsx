import React, { useState, useRef, useEffect } from "react";
import { Plus, ChevronDown, Check, Music, MessageSquare, Loader2, Trash2 } from "lucide-react";
import type { ModelFile } from "../types";
import InstallModelModal from "./InstallModelModal";
import "./ModelSelector.css";

interface ModelSelectorProps {
  models: ModelFile[];
  selectedModel: string;
  onDownload?: (path: string) => void;
  onCancelDownload?: (path: string) => void;
  onUninstall?: (path: string) => void;
  onConfirm?: (title: string, message: string, confirmLabel?: string) => Promise<boolean>;
  downloads?: Record<string, {progress: number, status: string}>;
  onSelect: (path: string) => void;
  type: "llm" | "audio" | "vision";
  onAdd?: () => void;
}

const ModelSelector: React.FC<ModelSelectorProps> = ({
  models,
  selectedModel,
  onSelect,
  type,
  onDownload,
  onCancelDownload,
  onUninstall,
  onConfirm,
  downloads = {},
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const [isInstallModalOpen, setIsInstallModalOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  // Close dropdown when clicking outside
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        containerRef.current &&
        !containerRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  const currentModelName =
    models.find((m) => m.path === selectedModel)?.name ||
    (type === "audio" && selectedModel === "None" ? "No Audio" : "Select Model");

  const installedModels = models.filter(m => m.is_downloaded || downloads[m.path] !== undefined || !m.gdrive_id);
  const missingModelsCount = models.filter(m => !m.is_downloaded && m.gdrive_id).length;

  return (
    <div className="model-selector-container" ref={containerRef}>
      <button
        className={`model-selector-btn ${isOpen ? "active" : ""}`}
        onClick={() => setIsOpen(!isOpen)}
        title={type === "llm" ? "Switch LLM Model" : type === "vision" ? "Switch Vision Model" : "Switch Audio Model"}
      >
        {type === "llm" ? <MessageSquare size={16} /> : type === "vision" ? <MessageSquare size={16} /> : <Music size={16} />}
        <span className="model-name">{currentModelName}</span>
        <ChevronDown size={14} className="chevron" />
      </button>

      {isOpen && (
        <div className="model-dropdown">
          <div className="dropdown-header">
            <span>{type === "llm" ? "Text Models" : type === "vision" ? "Vision Models" : "Voice Models"}</span>
          </div>
          
          <div className="model-list">
             {type === "audio" && (
                <div
                  className={`model-item ${selectedModel === "None" ? "selected" : ""}`}
                  onClick={() => {
                    onSelect("None");
                    setIsOpen(false);
                  }}
                >
                  <span className="truncate">None (Disable TTS)</span>
                  {selectedModel === "None" && <Check size={14} className="check-icon" />}
                </div>
             )}

            {installedModels.map((model) => {
              const isDownloading = downloads[model.path] !== undefined;
              const dlState = downloads[model.path];
              
              return (
                <div
                  key={model.path}
                  className={`model-item ${selectedModel === model.path ? "selected" : ""}`}
                  onClick={() => {
                    if (isDownloading) return;
                    onSelect(model.path);
                    setIsOpen(false);
                  }}
                  style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}
                >
                  <span className="truncate" style={{ opacity: isDownloading ? 0.6 : 1 }}>
                    {model.name}
                  </span>
                  
                  {isDownloading ? (
                    <div className="download-progress" style={{ display: "flex", alignItems: "center", gap: "6px", fontSize: "0.75rem", color: "var(--accent-color)" }}>
                      <Loader2 size={12} className="animate-spin" />
                      <span>{dlState.progress.toFixed(0)}%</span>
                    </div>
                  ) : (
                    <div className="flex items-center gap-2">
                      {selectedModel === model.path && <Check size={14} className="check-icon" />}
                      {model.is_downloaded && onUninstall && (
                        <button
                          onClick={async (e) => {
                            e.stopPropagation();
                            const ok = onConfirm
                              ? await onConfirm("Delete model", `Delete ${model.name} from this device?`, "Delete")
                              : window.confirm(`Are you sure you want to uninstall ${model.name}?`);
                            if (ok) onUninstall(model.path);
                          }}
                          style={{
                            background: "none", border: "none", cursor: "pointer",
                            padding: "4px", display: "flex", alignItems: "center",
                            color: "var(--text-secondary)", borderRadius: "4px"
                          }}
                          title="Uninstall Model"
                          onMouseEnter={(e) => (e.currentTarget.style.color = "#ef4444")}
                          onMouseLeave={(e) => (e.currentTarget.style.color = "var(--text-secondary)")}
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

          <div className="dropdown-footer">
            <button
              className="add-model-btn"
              onClick={() => {
                setIsInstallModalOpen(true);
                setIsOpen(false);
              }}
            >
              <Plus size={14} />
              <span>Install Model {missingModelsCount > 0 ? `(${missingModelsCount})` : ''}</span>
            </button>
          </div>
        </div>
      )}

      <InstallModelModal 
        isOpen={isInstallModalOpen}
        onClose={() => setIsInstallModalOpen(false)}
        models={models}
        title={`Install ${type === "llm" ? "Text" : type === "vision" ? "Vision" : "Voice"} Model`}
        onDownload={(path) => {
          if (onDownload) onDownload(path);
        }}
        onCancelDownload={onCancelDownload}
        onUninstall={onUninstall}
        onConfirm={onConfirm}
        downloads={downloads}
      />
    </div>
  );
};

export default ModelSelector;
