import React, { useEffect, useMemo, useState } from "react";
import { ArrowRight, Plus, FileUp, Sparkles } from "lucide-react";
import "./StartupModal.css";

interface StartupModalProps {
  onContinueWorkspace: () => void;
  canContinueWorkspace: boolean;
  continueWorkspaceName?: string | null;
  onNewProject: () => void;
  onImportProject: () => void;
  onStartTour?: () => void;
  busy?: boolean;
}

interface WaveConfig {
  baseY: number;
  amplitude: number;
  wavelength: number;
  phase: number;
  speed: number;
}

const WAVE_WIDTH = 1200;
const SAMPLE_STEP = 32;

const WAVE_CONFIGS: WaveConfig[] = [
  { baseY: 156, amplitude: 28, wavelength: 400, phase: 0.1, speed: 2.5 },
  { baseY: 176, amplitude: 15, wavelength: 300, phase: 1.6, speed: 2.05 },
  { baseY: 198, amplitude: 12, wavelength: 380, phase: 1.0, speed: 0.95 },
];

const generateSinePath = (config: WaveConfig, timeInSeconds: number): string => {
  const twoPi = Math.PI * 2;
  const initialY = config.baseY + config.amplitude * Math.sin((0 / config.wavelength) * twoPi + config.phase - timeInSeconds * config.speed);
  let pathData = `M0 ${initialY.toFixed(2)}`;

  for (let xCoord = SAMPLE_STEP; xCoord <= WAVE_WIDTH; xCoord += SAMPLE_STEP) {
    const yCoord =
      config.baseY +
      config.amplitude * Math.sin((xCoord / config.wavelength) * twoPi + config.phase - timeInSeconds * config.speed);
    pathData += ` L${xCoord} ${yCoord.toFixed(2)}`;
  }

  if (WAVE_WIDTH % SAMPLE_STEP !== 0) {
    const edgeY =
      config.baseY +
      config.amplitude * Math.sin((WAVE_WIDTH / config.wavelength) * twoPi + config.phase - timeInSeconds * config.speed);
    pathData += ` L${WAVE_WIDTH} ${edgeY.toFixed(2)}`;
  }

  return pathData;
};

const StartupModal: React.FC<StartupModalProps> = ({
  onContinueWorkspace,
  canContinueWorkspace,
  continueWorkspaceName = null,
  onNewProject,
  onImportProject,
  onStartTour,
  busy = false,
}) => {
  const [elapsedTime, setElapsedTime] = useState(0);

  useEffect(() => {
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = "";
    };
  }, []);

  useEffect(() => {
    let animationFrameId = 0;
    const startTimestamp = performance.now();

    const tick = (timestamp: number) => {
      setElapsedTime((timestamp - startTimestamp) / 1000);
      animationFrameId = window.requestAnimationFrame(tick);
    };

    animationFrameId = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(animationFrameId);
  }, []);

  const wavePaths = useMemo(
    () => WAVE_CONFIGS.map((config) => generateSinePath(config, elapsedTime * 1.5)),
    [elapsedTime],
  );

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
                onClick={onContinueWorkspace}
                disabled={busy || !canContinueWorkspace}
                title={
                  canContinueWorkspace && continueWorkspaceName
                    ? `Continue ${continueWorkspaceName}`
                    : "Continue with your existing workspace"
                }
              >
                <ArrowRight size={16} />
                <span>
                  {canContinueWorkspace
                    ? `Continue ${continueWorkspaceName ?? "Workspace"}`
                    : "No Existing Workspace"}
                </span>
              </button>

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

              {onStartTour && (
                <button
                  className="startup-action startup-btn-secondary"
                  onClick={onStartTour}
                  disabled={busy}
                  title="Take a quick tour of the app"
                  data-tour="startup-start-tour"
                >
                  <Sparkles size={16} />
                  <span>Start Tour</span>
                </button>
              )}
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
                d={wavePaths[0]}
                fill="none"
                stroke="url(#nelaWaveStrokeA)"
                strokeWidth="3"
                strokeLinecap="round"
              />
            </g>

            <g className="startup-wave-layer startup-wave-layer-2">
              <path
                d={wavePaths[1]}
                fill="none"
                stroke="url(#nelaWaveStrokeB)"
                strokeWidth="2.2"
                strokeLinecap="round"
              />
            </g>

            <g className="startup-wave-layer startup-wave-layer-3">
              <path
                d={wavePaths[2]}
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
