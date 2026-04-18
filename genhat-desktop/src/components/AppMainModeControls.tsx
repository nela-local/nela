import { Loader2, Scissors, SlidersHorizontal } from "lucide-react";
import type {
  ChatContextUsage,
  ChatMode,
  ModelFile,
  RegisteredModel,
} from "../types";
import type { DownloadStateMap } from "../app/types";
import type { RuntimeParamsTarget } from "./ActiveModelParamsDock";
import ModelSelector from "./ModelSelector";

interface AppMainModeControlsProps {
  chatMode: ChatMode;
  models: ModelFile[];
  selectedModel: string;
  onModelChange: (path: string) => void;
  onAddModel: () => void;
  onDownloadModel: (modelId: string) => void;
  onCancelDownload: (modelId: string) => void;
  onUninstallModel: (modelId: string) => void;
  onConfirmAction: (
    title: string,
    message: string,
    confirmLabel?: string,
    cancelLabel?: string
  ) => Promise<boolean>;
  downloads: DownloadStateMap;
  ttsEngines: RegisteredModel[];
  selectedTtsEngine: string;
  onSelectTtsEngine: (engineId: string) => void;
  visionModels: RegisteredModel[];
  selectedVisionModel: string;
  onSelectVisionModel: (modelId: string) => void;
  onAddVisionModel: () => void;
  activeRuntimeParamTarget: RuntimeParamsTarget | null;
  paramsDockOpen: boolean;
  onToggleParamsDock: () => void;
  contextUsage: ChatContextUsage | null;
  onCompactContext: () => void;
  canCompactContext: boolean;
  isCompactingContext: boolean;
}

export default function AppMainModeControls({
  chatMode,
  models,
  selectedModel,
  onModelChange,
  onAddModel,
  onDownloadModel,
  onCancelDownload,
  onUninstallModel,
  onConfirmAction,
  downloads,
  ttsEngines,
  selectedTtsEngine,
  onSelectTtsEngine,
  visionModels,
  selectedVisionModel,
  onSelectVisionModel,
  onAddVisionModel,
  activeRuntimeParamTarget,
  paramsDockOpen,
  onToggleParamsDock,
  contextUsage,
  onCompactContext,
  canCompactContext,
  isCompactingContext,
}: AppMainModeControlsProps) {
  return (
    <div className="flex items-center gap-3">
      {(chatMode === "text" || chatMode === "mindmap") && (
        <ModelSelector
          models={models}
          selectedModel={selectedModel}
          onSelect={onModelChange}
          type="llm"
          onAdd={onAddModel}
          onDownload={onDownloadModel}
          onCancelDownload={onCancelDownload}
          onUninstall={onUninstallModel}
          onConfirm={onConfirmAction}
          downloads={downloads}
        />
      )}

      {chatMode === "audio" && ttsEngines.length > 0 && (
        <div className="flex items-center gap-2.5">
          <ModelSelector
            models={ttsEngines.map((m) => ({
              name: m.name,
              path: m.id,
              is_downloaded: m.is_downloaded,
              gdrive_id: m.gdrive_id,
            }))}
            selectedModel={selectedTtsEngine}
            onSelect={onSelectTtsEngine}
            type="audio"
            onDownload={onDownloadModel}
            onCancelDownload={onCancelDownload}
            onUninstall={onUninstallModel}
            onConfirm={onConfirmAction}
            downloads={downloads}
          />

          {selectedTtsEngine === "kitten-tts"}
        </div>
      )}

      {chatMode === "vision" && visionModels.length > 0 && (
        <ModelSelector
          models={visionModels.map((m) => ({
            name: m.name,
            path: m.id,
            is_downloaded: m.is_downloaded,
            gdrive_id: m.gdrive_id,
          }))}
          selectedModel={selectedVisionModel}
          onSelect={onSelectVisionModel}
          type="vision"
          onAdd={onAddVisionModel}
          onDownload={onDownloadModel}
          onCancelDownload={onCancelDownload}
          onUninstall={onUninstallModel}
          onConfirm={onConfirmAction}
          downloads={downloads}
        />
      )}

      {activeRuntimeParamTarget && (
        <button
          className={`glass-btn inline-flex items-center gap-1.5 py-1.5 px-3 text-[0.78rem] font-medium rounded-lg cursor-pointer transition-all duration-200 border backdrop-blur-md ${paramsDockOpen ? "bg-neon-subtle text-neon border-neon/30 shadow-[0_0_12px_rgba(0,212,255,0.12)]" : "bg-glass-bg text-txt-secondary border-glass-border hover:border-neon hover:text-neon hover:shadow-[0_0_12px_rgba(0,212,255,0.08)]"}`}
          onClick={onToggleParamsDock}
          title="Toggle runtime parameter panel"
        >
          <SlidersHorizontal size={14} />
          {paramsDockOpen ? "Hide Params" : "Show Params"}
        </button>
      )}

      {(chatMode === "text" || chatMode === "mindmap") && (
        <button
          className="glass-btn inline-flex items-center gap-1.5 py-1.5 px-3 text-[0.78rem] font-medium rounded-lg cursor-pointer transition-all duration-200 border border-glass-border bg-glass-bg text-txt-secondary hover:border-neon hover:text-neon disabled:opacity-50 disabled:cursor-not-allowed"
          onClick={onCompactContext}
          disabled={!canCompactContext || isCompactingContext}
          title={
            contextUsage
              ? `Compact conversation context (projected usage ${contextUsage.projectedPercent.toFixed(1)}%)`
              : "Compact conversation context"
          }
        >
          {isCompactingContext ? <Loader2 size={14} className="animate-spin" /> : <Scissors size={14} />}
          {isCompactingContext ? "Compacting..." : "Compact Context"}
        </button>
      )}
    </div>
  );
}
