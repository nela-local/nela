import React, { useEffect } from "react";
import { Plus, FileUp } from "lucide-react";
import "./StartupModal.css";

interface StartupModalProps {
  onNewProject: () => void;
  onImportProject: () => void;
  busy?: boolean;
}

const StartupModal: React.FC<StartupModalProps> = ({
  onNewProject,
  onImportProject,
  busy = false,
}) => {
  useEffect(() => {
    // Prevent background scroll
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = "";
    };
  }, []);

  return (
    <div className="startup-modal-overlay">
      <div className="startup-modal-container">
        {/* Header */}
        <div className="startup-header">
          <div className="startup-logo">
            <img src="/logo-dark.png" alt="Nela" className="startup-logo-img" />
          </div>
          <h1 className="startup-title">Nela</h1>
          <p className="startup-subtitle">The Local Intelligence Engine</p>
        </div>

        {/* Description */}
        <p className="startup-description">
          Start a new intelligent workspace or import an existing one
        </p>

        {/* Action Buttons */}
        <div className="startup-actions">
          <button
            className="startup-action-btn startup-btn-primary"
            onClick={onNewProject}
            disabled={busy}
            title="Create a new workspace"
          >
            <div className="startup-btn-icon">
              <Plus size={24} />
            </div>
            <div className="startup-btn-content">
              <h3>New Project</h3>
              <p>Create a fresh workspace</p>
            </div>
          </button>

          <button
            className="startup-action-btn startup-btn-secondary"
            onClick={onImportProject}
            disabled={busy}
            title="Import a saved .nela file"
          >
            <div className="startup-btn-icon">
              <FileUp size={24} />
            </div>
            <div className="startup-btn-content">
              <h3>Import Project</h3>
              <p>Load a saved .nela workspace</p>
            </div>
          </button>
        </div>

        {/* Footer Info */}
        <p className="startup-footer">
          All processing happens locally. No data leaves your machine.
        </p>
      </div>
    </div>
  );
};

export default StartupModal;
