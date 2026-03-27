import React from "react";
import type { ModelFile } from "../types";

interface SidebarProps {
  models: ModelFile[];
  selectedModel: string;
  onModelSelect: (path: string) => void;

  audioModels: ModelFile[];
  selectedAudio: string;
  onAudioSelect: (path: string) => void;
}

const Sidebar: React.FC<SidebarProps> = ({
  models,
  selectedModel,
  onModelSelect,
  audioModels,
  selectedAudio,
  onAudioSelect,
}) => {
  return (
    <div className="sidebar">
      <h2>GenHat</h2>
      
      <div className="model-section">
        <h3>LLM Models</h3>
        <ul>
          {models.map((m) => (
            <li
              key={m.path}
              className={selectedModel === m.path ? "active" : ""}
              onClick={() => onModelSelect(m.path)}
            >
              {m.name || m.path}
            </li>
          ))}
          {models.length === 0 && <li>No models found</li>}
        </ul>
      </div>

      <div className="model-section">
        <h3>Audio Models</h3>
        <ul>
          {audioModels.map((m) => (
            <li
              key={m.path}
              className={selectedAudio === m.path ? "active" : ""}
              onClick={() => onAudioSelect(m.path)}
            >
              {m.name || m.path}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
};

export default Sidebar;
