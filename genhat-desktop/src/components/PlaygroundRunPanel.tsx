/**
 * PlaygroundRunPanel — bottom panel showing run status and log output.
 */

import { CheckCircle2, XCircle, Loader2, ChevronDown, ChevronUp } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { PipelineRun, RunStatus } from "../app/playgroundTypes";

const STATUS_ICON: Record<RunStatus, React.ReactNode> = {
  idle: null,
  running: <Loader2 size={13} className="animate-spin text-indigo-400" />,
  success: <CheckCircle2 size={13} className="text-emerald-400" />,
  error: <XCircle size={13} className="text-rose-400" />,
};

const STATUS_LABEL: Record<RunStatus, string> = {
  idle: "Ready",
  running: "Running…",
  success: "Completed",
  error: "Failed",
};

interface Props {
  run: PipelineRun | null;
  running: boolean;
  onRun: () => void;
  onCancel: () => void;
  onSave: () => void;
  nodeLabels?: Record<string, string>;
}

export default function PlaygroundRunPanel({ run, running, onRun, onCancel, onSave, nodeLabels }: Props) {
  const [expanded, setExpanded] = useState(false);
  const logRef = useRef<HTMLDivElement>(null);
  const status: RunStatus = running ? "running" : (run?.status ?? "idle");

  // Auto-expand log panel when a run starts or finishes
  useEffect(() => {
    if (running || run) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setExpanded(true);
    }
  }, [running, run]);

  // Auto-scroll log to bottom on new log entries
  useEffect(() => {
    if (expanded && logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [run?.log, expanded]);

  const hasNodes = run && Object.values(run.node_states).length > 0;

  return (
    <div
      className="
        border-t border-white/10 bg-void-950/80 backdrop-blur-sm
        flex flex-col transition-all
      "
      style={{ minHeight: 44 }}
    >
      {/* toolbar row */}
      <div className="flex items-center gap-2 px-4 py-2">
        <button
          disabled={running}
          onClick={onRun}
          className="
            flex items-center gap-1.5 text-xs font-medium px-3 py-1.5 rounded-lg
            bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 disabled:cursor-not-allowed
            text-white transition-colors
          "
        >
          {running ? (
            <Loader2 size={12} className="animate-spin" />
          ) : null}
          {running ? "Running" : "Run"}
        </button>

        {running && (
          <button
            onClick={onCancel}
            className="
              text-xs px-3 py-1.5 rounded-lg border border-rose-500/40 text-rose-400
              hover:bg-rose-500/10 transition-colors
            "
          >
            Cancel
          </button>
        )}

        <button
          onClick={onSave}
          className="
            text-xs px-3 py-1.5 rounded-lg border border-white/15 text-txt-muted
            hover:bg-white/5 hover:text-txt-primary transition-colors
          "
        >
          Save
        </button>

        {/* status indicator */}
        <div
          className={`flex items-center gap-1.5 ml-2 text-xs ${
            status === "success"
              ? "text-emerald-400"
              : status === "error"
              ? "text-rose-400"
              : status === "running"
              ? "text-indigo-400"
              : "text-txt-muted"
          }`}
        >
          {STATUS_ICON[status]}
          <span>{STATUS_LABEL[status]}</span>
          {run && (
            <span className="text-txt-muted ml-1">
              ({new Date(run.started_at).toLocaleTimeString()})
            </span>
          )}
        </div>

        {/* expand/collapse log toggle */}
        {(running || run) && (
          <button
            onClick={() => setExpanded(e => !e)}
            className="ml-auto flex items-center gap-1 text-[10px] text-txt-muted hover:text-txt-primary transition-colors"
          >
            Logs
            {expanded ? <ChevronDown size={12} /> : <ChevronUp size={12} />}
          </button>
        )}
      </div>

      {/* per-node status chips — always show when there's a run */}
      {hasNodes && (
        <div className="flex flex-wrap gap-1.5 px-4 pb-2">
          {Object.values(run!.node_states).map(ns => (
            <span
              key={ns.node_id}
              className={`
                flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] border
                ${
                  ns.status === "success"
                    ? "bg-emerald-500/10 border-emerald-500/30 text-emerald-400"
                    : ns.status === "error"
                    ? "bg-rose-500/10 border-rose-500/30 text-rose-400"
                    : ns.status === "running"
                    ? "bg-indigo-500/10 border-indigo-500/30 text-indigo-400"
                    : "bg-white/5 border-white/10 text-txt-muted"
                }
              `}
              title={ns.node_id}
            >
              {STATUS_ICON[ns.status]}
              {nodeLabels?.[ns.node_id] ?? ns.node_id.slice(0, 8)}
            </span>
          ))}
        </div>
      )}

      {/* log output — collapsible, auto-scrolls to bottom */}
      {expanded && (
        <div
          ref={logRef}
          className="
            px-4 pb-3 font-mono text-[10px] text-txt-muted overflow-y-auto max-h-48
            whitespace-pre-wrap leading-relaxed border-t border-white/5
          "
        >
          {running && !run ? (
            <span className="text-indigo-400 animate-pulse">Starting pipeline…</span>
          ) : run && run.log.length > 0 ? (
            run.log.join("\n")
          ) : (
            <span className="text-txt-muted/50 italic">No log output yet.</span>
          )}
        </div>
      )}
    </div>
  );
}
