// src/components/ModelSelector.tsx
import React, { useState, useRef, useEffect } from "react";
import { Plus, ChevronDown, Check, Music, MessageSquare } from "lucide-react";
import type { ModelFile } from "../types";
import "./ModelSelector.css";

// Helper to open file dialog and copy file (mocked for now as we need Rust backend command)
// For now, we will just simulate the UI flow or ask user to drop file.
// Real implementation requires a Rust command `add_model(path)` or similar.

interface ModelSelectorProps {
  models: ModelFile[];
  selectedModel: string;
  onSelect: (path: string) => void;
  type: "llm" | "audio";
  onAdd: () => void;
}

const ModelSelector: React.FC<ModelSelectorProps> = ({
  models,
  selectedModel,
  onSelect,
  type,
  onAdd,
}) => {
  const [isOpen, setIsOpen] = useState(false);
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

  return (
    <div className="model-selector-container" ref={containerRef}>
      <button
        className={`model-selector-btn ${isOpen ? "active" : ""}`}
        onClick={() => setIsOpen(!isOpen)}
        title={type === "llm" ? "Switch LLM Model" : "Switch Audio Model"}
      >
        {type === "llm" ? <MessageSquare size={16} /> : <Music size={16} />}
        <span className="model-name">{currentModelName}</span>
        <ChevronDown size={14} className="chevron" />
      </button>

      {isOpen && (
        <div className="model-dropdown">
          <div className="dropdown-header">
            <span>{type === "llm" ? "Text Models" : "Voice Models"}</span>
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

            {models.map((model) => (
              <div
                key={model.path}
                className={`model-item ${selectedModel === model.path ? "selected" : ""}`}
                onClick={() => {
                  onSelect(model.path);
                  setIsOpen(false);
                }}
              >
                <span className="truncate">{model.name}</span>
                {selectedModel === model.path && <Check size={14} className="check-icon" />}
              </div>
            ))}
          </div>

          <div className="dropdown-footer">
            <button
              className="add-model-btn"
              onClick={() => {
                onAdd();
                setIsOpen(false);
              }}
            >
              <Plus size={14} />
              <span>Add New Model</span>
            </button>
          </div>
        </div>
      )}
    </div>
  );
};

export default ModelSelector;
