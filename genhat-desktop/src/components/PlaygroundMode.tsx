/**
 * PlaygroundMode — root component for the Playground tab.
 *
 * Layout:
 *   [ PipelineList (left sidebar) ] [ NodePalette ] [ Canvas (center) ] [ NodeConfig (right, optional) ]
 *                                        [ RunPanel (bottom) ]
 */

import { useEffect, useState, useCallback } from "react";
import { Plus, Trash2, RefreshCw, Clock, Download, Upload, Sparkles, X as XIcon, ToggleLeft, ToggleRight, ArrowLeft } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { ReactFlowProvider } from "@xyflow/react";
import { usePipelineStore } from "../hooks/usePipelineStore";
import PlaygroundCanvas from "./PlaygroundCanvas";
import PlaygroundNodePalette from "./PlaygroundNodePalette";
import PlaygroundNodeConfig from "./PlaygroundNodeConfig";
import PlaygroundRunPanel from "./PlaygroundRunPanel";
import type { NodeKind, Pipeline } from "../app/playgroundTypes";

// ─── Pipeline list sidebar ────────────────────────────────────────────────────

interface PipelineListProps {
  pipelines: ReturnType<typeof usePipelineStore>["pipelines"];
  activePipelineId: string | null;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onDelete: (id: string) => void;
  onExport: (pipeline: Pipeline) => void;
  onImport: () => void;
  onLoadTemplate: (path: string) => void;
  onToggleAutoResume: (id: string) => void;
  onRefresh: () => void;
}

function PipelineList({
  pipelines,
  activePipelineId,
  onSelect,
  onCreate,
  onDelete,
  onExport,
  onImport,
  onLoadTemplate,
  onToggleAutoResume,
  onRefresh,
}: PipelineListProps) {
  return (
    <aside className="w-52 shrink-0 flex flex-col border-r border-white/10 bg-void-950">
      {/* header */}
      <div className="flex items-center gap-1 px-3 py-2.5 border-b border-white/10">
        <span className="text-xs font-semibold text-txt-primary flex-1">Pipelines</span>
        <button
          onClick={onRefresh}
          className="text-txt-muted hover:text-txt-primary transition-colors p-1 rounded"
          title="Refresh"
        >
          <RefreshCw size={12} />
        </button>
        <button
          onClick={onImport}
          className="text-txt-muted hover:text-txt-primary transition-colors p-1 rounded"
          title="Import pipeline from JSON file"
        >
          <Upload size={13} />
        </button>
        <button
          onClick={onCreate}
          className="text-txt-muted hover:text-txt-primary transition-colors p-1 rounded"
          title="New pipeline"
        >
          <Plus size={14} />
        </button>
      </div>

      {/* list */}
      <div className="flex-1 overflow-y-auto py-1">
        {pipelines.length === 0 && (
          <p className="px-3 py-4 text-[10px] text-txt-muted text-center leading-relaxed">
            No pipelines yet. Click + to create one.
          </p>
        )}
        {pipelines.map(p => (
          <div
            key={p.id}
            onClick={() => onSelect(p.id)}
            className={`
              group flex items-center gap-2 px-3 py-2 cursor-pointer
              hover:bg-white/5 transition-colors
              ${activePipelineId === p.id ? "bg-white/8" : ""}
            `}
          >
            <div className="flex-1 min-w-0">
              <p className="text-xs text-txt-primary truncate font-medium">{p.name}</p>
              <p className="text-[10px] text-txt-muted">
                {new Date(p.updated_at).toLocaleDateString()}
              </p>
            </div>

            {/* auto-resume toggle */}
            <button
              onClick={e => { e.stopPropagation(); onToggleAutoResume(p.id); }}
              title={p.auto_resume ? "Scheduled — click to disable" : "Not scheduled"}
              className="shrink-0 text-txt-muted hover:text-indigo-400 transition-colors"
            >
              {p.auto_resume ? (
                <Clock size={12} className="text-indigo-400" />
              ) : (
                <Clock size={12} className="opacity-30" />
              )}
            </button>

            {/* export */}
            <button
              onClick={e => { e.stopPropagation(); onExport(p); }}
              className="shrink-0 text-txt-muted hover:text-sky-400 transition-colors opacity-0 group-hover:opacity-100"
              title="Export as JSON"
            >
              <Download size={12} />
            </button>

            {/* delete */}
            <button
              onClick={e => { e.stopPropagation(); onDelete(p.id); }}
              className="shrink-0 text-txt-muted hover:text-rose-400 transition-colors opacity-0 group-hover:opacity-100"
              title="Delete pipeline"
            >
              <Trash2 size={12} />
            </button>
          </div>
        ))}
      </div>

      {/* starter templates */}
      <div className="border-t border-white/10 shrink-0">
        <div className="px-3 pt-2 pb-1 flex items-center gap-1">
          <Sparkles size={9} className="text-txt-muted" />
          <span className="text-[9px] font-semibold text-txt-muted uppercase tracking-wider">
            Starter templates
          </span>
        </div>
        <button
          onClick={() => onLoadTemplate("/templates/email-summarizer.pipeline.json")}
          className="w-full flex items-start gap-2 px-3 py-2 text-left hover:bg-white/5 transition-colors group/tpl"
          title="Add Email Summarizer pipeline from starter template"
        >
          <div className="min-w-0">
            <p className="text-xs text-txt-primary font-medium group-hover/tpl:text-indigo-300 transition-colors">
              Email Summarizer
            </p>
            <p className="text-[10px] text-txt-muted leading-snug">
              Fetch &amp; summarize unread emails daily
            </p>
          </div>
        </button>
        <button
          onClick={() => onLoadTemplate("/templates/morning-briefing.pipeline.json")}
          className="w-full flex items-start gap-2 px-3 py-2 text-left hover:bg-white/5 transition-colors group/tpl"
          title="Add Morning Briefing pipeline from starter template"
        >
          <div className="min-w-0">
            <p className="text-xs text-txt-primary font-medium group-hover/tpl:text-indigo-300 transition-colors">
              Morning Briefing
            </p>
            <p className="text-[10px] text-txt-muted leading-snug">
              Schedule → RSS → Summarize → Notify
            </p>
          </div>
        </button>
        <button
          onClick={() => onLoadTemplate("/templates/voice-memo.pipeline.json")}
          className="w-full flex items-start gap-2 px-3 py-2 text-left hover:bg-white/5 transition-colors group/tpl"
          title="Add Voice Memo pipeline from starter template"
        >
          <div className="min-w-0">
            <p className="text-xs text-txt-primary font-medium group-hover/tpl:text-indigo-300 transition-colors">
              Voice Memo Note
            </p>
            <p className="text-[10px] text-txt-muted leading-snug">
              Transcribe → LLM → FileWrite
            </p>
          </div>
        </button>
        <button
          onClick={() => onLoadTemplate("/templates/code-review.pipeline.json")}
          className="w-full flex items-start gap-2 px-3 py-2 text-left hover:bg-white/5 transition-colors group/tpl"
          title="Add Code Review pipeline from starter template"
        >
          <div className="min-w-0">
            <p className="text-xs text-txt-primary font-medium group-hover/tpl:text-indigo-300 transition-colors">
              Code Review
            </p>
            <p className="text-[10px] text-txt-muted leading-snug">
              FileRead → LLM → FileWrite → Notify
            </p>
          </div>
        </button>
      </div>
    </aside>
  );
}

// ─── Root ─────────────────────────────────────────────────────────────────────

export default function PlaygroundMode({ onNavigateBack }: { onNavigateBack?: () => void }) {
  const store = usePipelineStore();
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [notification, setNotification] = useState<{ title: string; body: string } | null>(null);

  // Listen for backend-emitted notifications from pipeline Notification nodes
  useEffect(() => {
    const unlistenPromise = listen<{ title: string; body: string }>("playground-notification", e => {
      setNotification(e.payload);
      const timer = setTimeout(() => setNotification(null), 6000);
      return () => clearTimeout(timer);
    });
    return () => {
      unlistenPromise.then(fn => fn());
    };
  }, []);

  // On mount, load the pipeline list
  useEffect(() => {
    store.loadPipelines();
  }, [store]);

  const selectedNode = store.activePipeline?.nodes.find(n => n.id === selectedNodeId) ?? null;

  const handleAddNode = useCallback(
    (kind: NodeKind, position: { x: number; y: number }) => {
      store.addNode(kind, position);
    },
    [store]
  );

  const handleAutoResumeToggle = useCallback(
    (id: string) => {
      store.toggleAutoResume(id);
    },
    [store]
  );

  const handleExportPipeline = useCallback((pipeline: Pipeline) => {
    const json = JSON.stringify(pipeline, null, 2);
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${pipeline.name.replace(/[^a-z0-9]+/gi, "-").toLowerCase()}.pipeline.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }, []);

  const handleImportPipeline = useCallback(() => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      try {
        const text = await file.text();
        const raw = JSON.parse(text);
        const imported: Pipeline = {
          ...raw,
          id: crypto.randomUUID(),
          name: (raw.name as string) ?? "Imported Pipeline",
          nodes: raw.nodes ?? [],
          edges: raw.edges ?? [],
          created_at: Date.now(),
          updated_at: Date.now(),
        };
        await store.importPipeline(imported);
      } catch {
        console.error("Failed to import pipeline — invalid JSON");
      }
    };
    input.click();
  }, [store]);

  const handleLoadTemplate = useCallback(async (templatePath: string) => {
    try {
      const res = await fetch(templatePath);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const raw = await res.json();
      const p: Pipeline = {
        ...raw,
        id: crypto.randomUUID(),
        created_at: Date.now(),
        updated_at: Date.now(),
      };
      await store.importPipeline(p);
    } catch (e) {
      console.error("Failed to load starter template", e);
    }
  }, [store]);

  return (
    <div className="flex flex-col h-full w-full bg-void-900 overflow-hidden">
      {/* breadcrumb back bar */}
      <div className="flex items-center gap-2 px-3 py-1.5 border-b border-white/10 bg-void-950/60 shrink-0">
        <button
          onClick={onNavigateBack}
          disabled={!onNavigateBack}
          className="flex items-center gap-1.5 text-[11px] text-txt-muted hover:text-neon disabled:opacity-30 disabled:cursor-default transition-colors"
          title="Back to Chat"
        >
          <ArrowLeft size={12} />
          Back to Chat
        </button>
        <span className="text-[11px] text-txt-muted opacity-40">/</span>
        <span className="text-[11px] text-txt font-medium">Playground</span>
      </div>
      {/* main row */}
      <div className="flex flex-1 overflow-hidden">
        {/* pipeline list sidebar */}
        <PipelineList
          pipelines={store.pipelines}
          activePipelineId={store.activePipelineId}
          onSelect={store.selectPipeline}
          onCreate={() => store.createPipeline()}
          onDelete={store.removePipeline}
          onExport={handleExportPipeline}
          onImport={handleImportPipeline}
          onLoadTemplate={handleLoadTemplate}
          onToggleAutoResume={handleAutoResumeToggle}
          onRefresh={store.loadPipelines}
        />

        {/* node palette */}
        <PlaygroundNodePalette />

        {/* canvas — needs ReactFlowProvider */}
        <div className="flex-1 relative overflow-hidden">
          {store.activePipeline ? (
            <ReactFlowProvider>
              <PlaygroundCanvas
                nodes={store.activePipeline.nodes}
                edges={store.activePipeline.edges}
                onNodesChange={store.onNodesChange}
                onEdgesChange={store.onEdgesChange}
                onConnect={store.onConnect}
                onAddNode={handleAddNode}
                onSelectNode={setSelectedNodeId}
                selectedNodeId={selectedNodeId}
              />
            </ReactFlowProvider>
          ) : (
            <div className="flex h-full items-center justify-center text-txt-muted text-sm">
              Select or create a pipeline to get started.
            </div>
          )}
        </div>

        {/* node config drawer */}
        {selectedNode && (
          <PlaygroundNodeConfig
            node={selectedNode}
            onUpdateConfig={store.updateNodeConfig}
            onClose={() => setSelectedNodeId(null)}
            onDelete={() => {
              store.removeNode(selectedNodeId!);
              setSelectedNodeId(null);
            }}
          />
        )}
      </div>

      {/* run panel */}
      {store.activePipeline && (
        <PlaygroundRunPanel
          run={store.activeRun}
          running={store.runActive}
          onRun={store.startRun}
          onCancel={store.cancelRun}
          onSave={store.persistPipeline}
          nodeLabels={Object.fromEntries(
            store.activePipeline.nodes.map(n => [n.id, n.data.label])
          )}
        />
      )}

      {/* auto-resume header for active pipeline */}
      {store.activePipeline && (
        <div className="absolute top-2 right-4 flex items-center gap-2 z-10">
          <button
            onClick={() => store.toggleAutoResume(store.activePipeline!.id)}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-full border border-white/15 bg-void-800/80 backdrop-blur text-xs text-txt-muted hover:text-txt-primary transition-colors"
            title="Toggle scheduled auto-run on app start"
          >
            {store.activePipeline.auto_resume ? (
              <ToggleRight size={14} className="text-indigo-400" />
            ) : (
              <ToggleLeft size={14} />
            )}
            {store.activePipeline.auto_resume ? "Scheduled" : "Manual only"}
          </button>
        </div>
      )}

      {/* Notification toast from pipeline Notification nodes */}
      {notification && (
        <div className="absolute bottom-6 right-6 z-50 max-w-xs bg-void-800 border border-white/15 rounded-xl shadow-xl px-4 py-3 flex items-start gap-3 animate-fade-in">
          <div className="flex-1 min-w-0">
            <p className="text-xs font-semibold text-txt-primary">{notification.title}</p>
            <p className="text-xs text-txt-muted mt-0.5 leading-snug">{notification.body}</p>
          </div>
          <button
            onClick={() => setNotification(null)}
            className="text-txt-muted hover:text-txt-primary transition-colors mt-0.5"
          >
            <XIcon size={12} />
          </button>
        </div>
      )}
    </div>
  );
}
