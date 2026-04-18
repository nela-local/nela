import type { ElementType, ReactNode } from "react";
import type { ChatContextUsage, WorkspaceRecord } from "../types";
import WorkspaceSelector from "./WorkspaceSelector";

interface AppMainTopBarProps {
  currentModeConfig: {
    icon: ElementType;
    label: string;
    desc: string;
  };
  workspaces: WorkspaceRecord[];
  activeWorkspace: WorkspaceRecord | null;
  onSelectWorkspace: (workspaceId: string) => void;
  onCreateWorkspace: () => void;
  onDeleteWorkspace: (workspaceId: string) => void;
  onRenameWorkspace: (workspaceId: string, name: string) => void;
  workspaceBusy: boolean;
  modelLoadingStatus: {
    loading: boolean;
    modelId: string;
    message: string;
  };
  contextUsage: ChatContextUsage | null;
  modeControls: ReactNode;
}

export default function AppMainTopBar({
  currentModeConfig,
  workspaces,
  activeWorkspace,
  onSelectWorkspace,
  onCreateWorkspace,
  onDeleteWorkspace,
  onRenameWorkspace,
  workspaceBusy,
  modelLoadingStatus,
  contextUsage,
  modeControls,
}: AppMainTopBarProps) {
  const CurrentModeIcon = currentModeConfig.icon;

  return (
    <header className="min-h-14 py-2 flex items-center justify-between px-6 border-b border-glass-border bg-void-800/80 backdrop-blur-xl shrink-0 z-20">
      <div className="flex flex-col items-start gap-1.5">
        <div className="flex items-center gap-2.5">
          <CurrentModeIcon size={18} strokeWidth={1.8} className="text-neon" />
          <h1 className="text-[0.95rem] font-semibold m-0 text-txt">{currentModeConfig.label}</h1>
          <span className="text-[0.78rem] text-txt-muted pl-2.5 border-l border-glass-border">
            {currentModeConfig.desc}
          </span>
        </div>

        <WorkspaceSelector
          workspaces={workspaces}
          activeWorkspaceId={activeWorkspace?.id ?? null}
          onSelectWorkspace={onSelectWorkspace}
          onCreateWorkspace={onCreateWorkspace}
          onDeleteWorkspace={onDeleteWorkspace}
          onRenameWorkspace={onRenameWorkspace}
          busy={workspaceBusy}
        />

        {modelLoadingStatus.loading && (
          <div className="flex items-center gap-2 px-3 py-1.5 bg-amber-900/30 border border-amber-500/40 rounded-lg text-amber-300 text-xs">
            <div className="w-3 h-3 border-2 border-amber-400 border-t-transparent rounded-full animate-spin" />
            <span>{modelLoadingStatus.message || "Loading model..."}</span>
          </div>
        )}

        {contextUsage && (
          <div className="w-full max-w-140 mt-1 px-3 py-2 rounded-lg border border-glass-border bg-void-700/60">
            <div className="flex items-center justify-between text-[0.68rem] text-txt-muted">
              <span>Context window tracks how much conversation history is sent to the model each turn.</span>
              <span>{contextUsage.contextWindowTokens.toLocaleString()} tokens</span>
            </div>

            <div className="mt-1.5 h-1.5 rounded-full bg-void-900 overflow-hidden">
              <div
                className={`h-full transition-all duration-200 ${
                  contextUsage.projectedPercent >= contextUsage.thresholdPercent
                    ? "bg-amber-400"
                    : "bg-neon"
                }`}
                style={{ width: `${Math.min(100, contextUsage.projectedPercent)}%` }}
              />
            </div>

            <div className="mt-1.5 flex items-center justify-between text-[0.68rem] text-txt-secondary">
              <span>
                Used {contextUsage.usedTokens.toLocaleString()} ({contextUsage.usedPercent.toFixed(1)}%)
              </span>
              <span>
                Remaining {contextUsage.remainingTokens.toLocaleString()}
              </span>
              <span>
                Auto-compact at {contextUsage.thresholdPercent.toFixed(0)}%
              </span>
            </div>
          </div>
        )}
      </div>

      {modeControls}
    </header>
  );
}
