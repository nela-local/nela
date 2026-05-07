/**
 * usePipelineStore — React state management for the Playground pipeline editor.
 *
 * Wraps xyflow node/edge state and exposes CRUD operations for pipelines.
 * Backend persistence goes through Tauri IPC (see api.ts playground commands).
 */

import { useState, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import type { NodeChange, EdgeChange, Connection } from "@xyflow/react";
import { applyNodeChanges, applyEdgeChanges, addEdge } from "@xyflow/react";
import type {
  Pipeline,
  PlaygroundNode,
  PlaygroundEdge,
  PipelineRun,
  NodeKind,
  NodeConfig,
  PlaygroundNodeData,
} from "../app/playgroundTypes";
import { PALETTE } from "../app/playgroundTypes";
import {
  listPipelines,
  loadPipeline,
  savePipeline,
  deletePipeline,
  runPipeline,
  cancelPipelineRun,
} from "../api";

function makeid(): string {
  return crypto.randomUUID();
}

function newPipeline(name = "Untitled Pipeline"): Pipeline {
  return {
    id: makeid(),
    name,
    auto_resume: false,
    nodes: [],
    edges: [],
    created_at: Date.now(),
    updated_at: Date.now(),
  };
}

export interface PipelineStoreActions {
  // pipeline list
  pipelines: Pipeline[];
  activePipelineId: string | null;
  loadPipelines: () => Promise<void>;

  // active pipeline
  activePipeline: Pipeline | null;
  selectPipeline: (id: string) => Promise<void>;
  createPipeline: (name?: string) => Promise<void>;
  renamePipeline: (id: string, name: string) => void;
  toggleAutoResume: (id: string) => void;
  removePipeline: (id: string) => Promise<void>;
  persistPipeline: () => Promise<void>;
  importPipeline: (pipeline: Pipeline) => Promise<void>;

  // graph mutations (xyflow)
  onNodesChange: (changes: NodeChange<PlaygroundNode>[]) => void;
  onEdgesChange: (changes: EdgeChange<PlaygroundEdge>[]) => void;
  onConnect: (connection: Connection) => void;
  addNode: (kind: NodeKind, position: { x: number; y: number }) => void;
  updateNodeConfig: (nodeId: string, patch: Partial<NodeConfig>) => void;
  removeNode: (nodeId: string) => void;

  // run state
  activeRun: PipelineRun | null;
  runActive: boolean;
  startRun: () => Promise<void>;
  cancelRun: () => Promise<void>;
}

export function usePipelineStore(): PipelineStoreActions {
  const [pipelines, setPipelines] = useState<Pipeline[]>([]);
  const [activePipelineId, setActivePipelineId] = useState<string | null>(null);
  const [activePipeline, setActivePipeline] = useState<Pipeline | null>(null);
  const [activeRun, setActiveRun] = useState<PipelineRun | null>(null);
  const [runActive, setRunActive] = useState(false);

  // ── Pipeline list ────────────────────────────────────────────────────────────

  const loadPipelines = useCallback(async () => {
    try {
      const list = await listPipelines();
      setPipelines(list);
    } catch (e) {
      console.error("Failed to list pipelines", e);
    }
  }, []);

  const selectPipeline = useCallback(async (id: string) => {
    try {
      const p = await loadPipeline(id);
      setActivePipeline(p);
      setActivePipelineId(id);
    } catch (e) {
      console.error("Failed to load pipeline", id, e);
    }
  }, []);

  const createPipeline = useCallback(async (name?: string) => {
    const p = newPipeline(name);
    await savePipeline(p);
    setPipelines(prev => [p, ...prev]);
    setActivePipeline(p);
    setActivePipelineId(p.id);
  }, []);

  const renamePipeline = useCallback((id: string, name: string) => {
    setPipelines(prev =>
      prev.map(p => (p.id === id ? { ...p, name, updated_at: Date.now() } : p))
    );
    setActivePipeline(prev => (prev && prev.id === id ? { ...prev, name } : prev));
  }, []);

  const toggleAutoResume = useCallback((id: string) => {
    setActivePipeline(prev => {
      if (!prev || prev.id !== id) return prev;
      return { ...prev, auto_resume: !prev.auto_resume };
    });
    setPipelines(prev =>
      prev.map(p => (p.id === id ? { ...p, auto_resume: !p.auto_resume } : p))
    );
  }, []);

  const removePipeline = useCallback(
    async (id: string) => {
      await deletePipeline(id);
      setPipelines(prev => prev.filter(p => p.id !== id));
      if (activePipelineId === id) {
        setActivePipelineId(null);
        setActivePipeline(null);
      }
    },
    [activePipelineId]
  );

  const persistPipeline = useCallback(async () => {
    if (!activePipeline) return;
    const updated = { ...activePipeline, updated_at: Date.now() };
    await savePipeline(updated);
    setPipelines(prev => prev.map(p => (p.id === updated.id ? updated : p)));
  }, [activePipeline]);

  const importPipeline = useCallback(async (p: Pipeline) => {
    await savePipeline(p);
    setPipelines(prev => [p, ...prev.filter(x => x.id !== p.id)]);
    setActivePipeline(p);
    setActivePipelineId(p.id);
  }, []);

  // ── Graph mutations ──────────────────────────────────────────────────────────

  const onNodesChange = useCallback(
    (changes: NodeChange<PlaygroundNode>[]) => {
      setActivePipeline(prev => {
        if (!prev) return prev;
        return {
          ...prev,
          nodes: applyNodeChanges(changes, prev.nodes),
        };
      });
    },
    []
  );

  const onEdgesChange = useCallback(
    (changes: EdgeChange<PlaygroundEdge>[]) => {
      setActivePipeline(prev => {
        if (!prev) return prev;
        return {
          ...prev,
          edges: applyEdgeChanges(changes, prev.edges) as PlaygroundEdge[],
        };
      });
    },
    []
  );

  const onConnect = useCallback((connection: Connection) => {
    setActivePipeline(prev => {
      if (!prev) return prev;
      const newEdge: PlaygroundEdge = {
        id: makeid(),
        source: connection.source,
        target: connection.target,
        sourceHandle: connection.sourceHandle ?? undefined,
        targetHandle: connection.targetHandle ?? undefined,
      };
      return {
        ...prev,
        edges: addEdge(newEdge, prev.edges) as PlaygroundEdge[],
      };
    });
  }, []);

  const addNode = useCallback(
    (kind: NodeKind, position: { x: number; y: number }) => {
      const entry = PALETTE.find(p => p.kind === kind);
      if (!entry) return;
      const nodeData: PlaygroundNodeData = {
        kind,
        label: entry.label,
        config: { ...entry.defaultConfig },
      };
      const node: PlaygroundNode = {
        id: makeid(),
        type: "playgroundNode",
        position,
        data: nodeData,
      };
      setActivePipeline(prev => {
        if (!prev) return prev;
        return { ...prev, nodes: [...prev.nodes, node] };
      });
    },
    []
  );

  const updateNodeConfig = useCallback(
    (nodeId: string, patch: Partial<NodeConfig>) => {
      setActivePipeline(prev => {
        if (!prev) return prev;
        return {
          ...prev,
          nodes: prev.nodes.map(n =>
            n.id === nodeId
              ? { ...n, data: { ...n.data, config: { ...n.data.config, ...patch } } }
              : n
          ),
        };
      });
    },
    []
  );

  const removeNode = useCallback((nodeId: string) => {
    setActivePipeline(prev => {
      if (!prev) return prev;
      return {
        ...prev,
        nodes: prev.nodes.filter(n => n.id !== nodeId),
        edges: prev.edges.filter(
          e => e.source !== nodeId && e.target !== nodeId
        ),
      };
    });
  }, []);

  // ── Run ──────────────────────────────────────────────────────────────────────

  const startRun = useCallback(async () => {
    if (!activePipeline) return;
    setRunActive(true);
    setActiveRun(null);

    // Set up event listeners BEFORE triggering the run so no event is missed.
    const unlistenUpdate = await listen<PipelineRun>("playground-run-update", (event) => {
      setActiveRun(event.payload);
    });

    const unlistenComplete = await listen<PipelineRun>("playground-run-complete", (event) => {
      setActiveRun(event.payload);
      setRunActive(false);
      unlistenUpdate();
      unlistenComplete();
    });

    try {
      // Persist current state first so backend runs the latest version
      const updated = { ...activePipeline, updated_at: Date.now() };
      await savePipeline(updated);
      setPipelines(prev => prev.map(p => (p.id === updated.id ? updated : p)));
      // runPipeline now returns a run_id immediately; the pipeline runs in the background
      await runPipeline(activePipeline.id);
    } catch (e) {
      console.error("Pipeline run failed", e);
      setRunActive(false);
      unlistenUpdate();
      unlistenComplete();
    }
  }, [activePipeline]);

  const cancelRun = useCallback(async () => {
    if (!activeRun) return;
    try {
      await cancelPipelineRun(activeRun.run_id);
    } catch (e) {
      console.error("Cancel failed", e);
    }
    setRunActive(false);
  }, [activeRun]);

  return {
    pipelines,
    activePipelineId,
    loadPipelines,
    activePipeline,
    selectPipeline,
    createPipeline,
    renamePipeline,
    toggleAutoResume,
    removePipeline,
    persistPipeline,
    importPipeline,
    onNodesChange,
    onEdgesChange,
    onConnect,
    addNode,
    updateNodeConfig,
    removeNode,
    activeRun,
    runActive,
    startRun,
    cancelRun,
  };
}
