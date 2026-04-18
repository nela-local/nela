import {
  ChevronLeft,
  Minus,
  Loader2,
  Trash2,
} from "lucide-react";
import type {
  DownloadStateMap,
  StartupModelToastState,
} from "../app/types";
import {
  formatDownloadSpeedLabel,
  formatModelSizeLabel,
  formatTotalSizeLabel,
} from "../app/modelUtils";

interface StartupModelToastProps {
  toast: StartupModelToastState;
  minimized: boolean;
  downloads: DownloadStateMap;
  startupCancelledIds: string[];
  startupCancellingIds: string[];
  startupOverallSpeedBps: number;
  startupSelectedTotalMb: number;
  onToggleMinimized: (next: boolean) => void;
  onDecline: () => void;
  onAccept: () => void;
  onToggleModelSelection: (modelId: string) => void;
  onCancelAllDownloads: () => void;
  onCancelSingleDownload: (modelId: string) => void;
}

export default function StartupModelToast({
  toast,
  minimized,
  downloads,
  startupCancelledIds,
  startupCancellingIds,
  startupOverallSpeedBps,
  startupSelectedTotalMb,
  onToggleMinimized,
  onDecline,
  onAccept,
  onToggleModelSelection,
  onCancelAllDownloads,
  onCancelSingleDownload,
}: StartupModelToastProps) {
  if (!toast.open) return null;

  if (minimized) {
    return (
      <button
        onClick={() => onToggleMinimized(false)}
        className="fixed bottom-10 right-0 z-[90] w-12 h-12 rounded-l-full bg-void-800 border-y border-l border-neon/60 shadow-[0_4px_16px_rgba(0,212,255,0.25)] flex items-center justify-center text-neon hover:bg-void-700 transition-all group"
        title="Expand Download Status"
      >
        <ChevronLeft className="w-6 h-6 shrink-0 ml-1 group-hover:-translate-x-0.5 transition-transform" />
      </button>
    );
  }

  return (
    <div className="fixed bottom-4 right-4 z-[90] w-[360px] max-w-[92vw] rounded-xl border border-neon/60 bg-void-800/95 shadow-[0_12px_36px_rgba(0,0,0,0.45)] backdrop-blur-md">
      <div className="px-4 py-3 text-sm text-txt">
        <div className="mb-1 flex items-center justify-between gap-2">
          <div className="font-medium">
            {toast.phase === "prompt"
              ? "Model(s) absent"
              : toast.phase === "downloading"
                ? "Downloading models"
                : "Model setup"}
          </div>
          <button
            type="button"
            className="p-1 rounded text-txt-muted hover:text-txt hover:bg-void-700/50"
            onClick={() => onToggleMinimized(true)}
            title="Minimize"
          >
            <Minus size={14} />
          </button>
        </div>

        {toast.phase === "downloading" && (
          <div className="mb-1 text-[11px] text-neon">
            Overall speed: {formatDownloadSpeedLabel(startupOverallSpeedBps)}
          </div>
        )}

        <div className="flex items-center justify-between gap-2">
          <div className="text-txt-muted leading-relaxed">{toast.message}</div>
          {toast.phase === "downloading" && (
            <div className="flex items-center gap-1">
              <button
                type="button"
                className="px-2 py-1 rounded border border-glass-border text-[11px] text-txt-muted hover:text-txt hover:border-neon"
                onClick={onCancelAllDownloads}
                title="Cancel startup downloads"
              >
                Cancel
              </button>
            </div>
          )}
        </div>

        {toast.phase === "prompt" && toast.missingNames.length > 0 && (
          <div className="mt-2 text-xs leading-relaxed">
            <div className="text-txt-muted">The following models are not present:</div>
            <ul className="mt-2 space-y-1">
              {toast.missingIds.map((modelId, idx) => {
                const name = toast.missingNames[idx] ?? modelId;
                const sizeLabel = formatModelSizeLabel(toast.missingSizesMb[idx]);
                const checked = toast.selectedIds.includes(modelId);
                return (
                  <li key={modelId}>
                    <label className="flex items-center gap-2 cursor-pointer rounded px-1 py-0.5 hover:bg-void-700/40">
                      <input
                        type="checkbox"
                        className="accent-neon"
                        checked={checked}
                        onChange={() => onToggleModelSelection(modelId)}
                      />
                      <span className="text-neon font-medium truncate" title={name}>{name}</span>
                      <span className="ml-auto text-[11px] text-txt-muted">{sizeLabel}</span>
                    </label>
                  </li>
                );
              })}
            </ul>
            <div className="mt-2 text-[11px] text-txt-muted">
              Total selected size:{" "}
              <span className="text-neon font-medium">{formatTotalSizeLabel(startupSelectedTotalMb)}</span>
            </div>
          </div>
        )}

        {toast.phase === "downloading" && (
          <div className="mt-2 space-y-2">
            <div className="flex items-center gap-2 text-neon text-xs">
              <Loader2 size={13} className="animate-spin" />
              <span>
                Progress: {toast.completed}/{toast.total}
              </span>
            </div>

            {toast.selectedIds.map((modelId) => {
              const dl = downloads[modelId];
              const isDone = toast.doneIds.includes(modelId);
              const isFailed = toast.failedIds.includes(modelId);
              const isCancelled = startupCancelledIds.includes(modelId);
              const isCancelling = startupCancellingIds.includes(modelId);
              const pct = isDone
                ? 100
                : typeof dl?.progress === "number"
                  ? Math.max(0, Math.min(100, dl.progress))
                  : 0;
              const idx = toast.missingIds.indexOf(modelId);
              const name = idx >= 0 ? toast.missingNames[idx] ?? modelId : modelId;

              return (
                <div key={modelId} className="space-y-1">
                  <div className="flex items-center justify-between text-[11px] text-txt-muted gap-2">
                    <span className="truncate max-w-[220px]" title={name}>{name}</span>
                    <div className="flex items-center gap-2 shrink-0">
                      <span>
                        {isDone
                          ? "Done"
                          : isCancelled
                            ? "Cancelled"
                            : isFailed
                              ? "Failed"
                              : isCancelling
                                ? "Cancelling..."
                                : dl
                                  ? `${pct.toFixed(0)}%`
                                  : "Queued"}
                      </span>
                      {!isDone && !isFailed && !isCancelled && (
                        <button
                          type="button"
                          className="px-1.5 py-0.5 rounded border border-glass-border hover:border-neon hover:text-txt disabled:opacity-60"
                          title="Cancel and delete partial download"
                          aria-label={`Cancel download for ${name}`}
                          onClick={() => onCancelSingleDownload(modelId)}
                          disabled={isCancelling}
                        >
                          <span className="inline-flex items-center gap-1">
                            <Trash2 size={10} />
                            {isCancelling ? "..." : "Delete"}
                          </span>
                        </button>
                      )}
                    </div>
                  </div>
                  <div className="h-1.5 w-full rounded bg-void-700/80 overflow-hidden">
                    <div
                      className={`h-full transition-all duration-300 ${
                        isFailed
                          ? (isCancelled ? "bg-yellow-500" : "bg-red-500")
                          : "bg-neon"
                      }`}
                      style={{ width: `${pct}%` }}
                    />
                  </div>
                </div>
              );
            })}
          </div>
        )}

        {toast.phase === "prompt" && (
          <div className="mt-3 flex items-center justify-end gap-2">
            <button
              className="px-3 py-1.5 rounded-md border border-glass-border text-txt-muted text-xs hover:text-txt"
              onClick={onDecline}
            >
              No
            </button>
            <button
              className="px-3 py-1.5 rounded-md bg-neon text-void-900 text-xs font-semibold hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed"
              onClick={onAccept}
              disabled={toast.selectedIds.length === 0}
            >
              Yes, download ({toast.selectedIds.length})
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
