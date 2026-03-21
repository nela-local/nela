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
        <div className="startup-hero">
          <div className="startup-top-row">
            <div className="startup-brand">
              <img src="/logo-dark.png" alt="Nela" className="startup-logo-img" />
              <div className="startup-brand-copy">
                <h1 className="startup-title">Nela</h1>
                <p className="startup-subtitle">The Local Intelligence Engine</p>
              </div>
            </div>

            <div className="startup-actions">
              <button
                className="startup-action startup-btn-primary"
                onClick={onNewProject}
                disabled={busy}
                title="Create a new workspace"
              >
                <Plus size={16} />
                <span>New Project</span>
              </button>

              <button
                className="startup-action startup-btn-secondary"
                onClick={onImportProject}
                disabled={busy}
                title="Import a saved .nela file"
              >
                <FileUp size={16} />
                <span>Import Project</span>
              </button>
            </div>
          </div>

          <div className="startup-content-left">
            <p className="startup-description">
              Start a new intelligent workspace or import an existing one.
            </p>
          </div>

          <svg
            className="startup-waves"
            viewBox="0 0 1200 260"
            preserveAspectRatio="none"
            aria-hidden="true"
          >
            <defs>
              <linearGradient id="nelaWaveStrokeA" x1="0%" y1="0%" x2="100%" y2="0%">
                <stop offset="0%" stopColor="rgba(102, 229, 255, 0.2)" />
                <stop offset="40%" stopColor="rgba(0, 212, 255, 0.55)" />
                <stop offset="100%" stopColor="rgba(145, 236, 255, 0.18)" />
              </linearGradient>
              <linearGradient id="nelaWaveStrokeB" x1="0%" y1="0%" x2="100%" y2="0%">
                <stop offset="0%" stopColor="rgba(145, 236, 255, 0.08)" />
                <stop offset="55%" stopColor="rgba(0, 212, 255, 0.4)" />
                <stop offset="100%" stopColor="rgba(145, 236, 255, 0.06)" />
              </linearGradient>
            </defs>

            <g className="startup-wave-layer startup-wave-layer-1">
              <path
                d="M0 146 C80 136, 140 184, 220 172 C300 158, 360 98, 440 110 C520 122, 590 190, 670 178 C750 166, 820 110, 900 122 C980 134, 1040 186, 1120 172 C1160 166, 1180 156, 1200 148"
                fill="none"
                stroke="url(#nelaWaveStrokeA)"
                strokeWidth="3"
                strokeLinecap="round"
              />
            </g>

            <g className="startup-wave-layer startup-wave-layer-2">
              <path
                d="M0 176 C70 152, 130 196, 205 180 C275 166, 338 132, 410 144 C482 156, 550 204, 625 190 C702 176, 764 126, 842 140 C920 154, 990 205, 1062 192 C1128 180, 1174 162, 1200 154"
                fill="none"
                stroke="url(#nelaWaveStrokeB)"
                strokeWidth="2.2"
                strokeLinecap="round"
              />
            </g>

            <g className="startup-wave-layer startup-wave-layer-3">
              <path
                d="M0 198 C92 182, 160 222, 250 206 C340 190, 420 156, 512 168 C604 180, 678 226, 770 212 C860 198, 940 164, 1030 176 C1104 186, 1162 204, 1200 198"
                fill="none"
                stroke="rgba(145, 236, 255, 0.22)"
                strokeWidth="1.4"
                strokeLinecap="round"
              />
            </g>
          </svg>
        </div>
      </div>
    </div>
  );
};

export default StartupModal;
