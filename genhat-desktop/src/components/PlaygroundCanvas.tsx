/**
 * PlaygroundCanvas — the React Flow canvas for the pipeline editor.
 *
 * Renders nodes and edges, supports drag-from-palette to add nodes,
 * and delegates all state mutations to the parent store.
 */

import { useCallback } from "react";
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  BackgroundVariant,
  useReactFlow,
  type NodeMouseHandler,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { PlaygroundNode, PlaygroundEdge, NodeKind } from "../app/playgroundTypes";
import type { NodeChange, EdgeChange, Connection } from "@xyflow/react";
import PlaygroundFlowNode from "./PlaygroundFlowNode";

const nodeTypes = { playgroundNode: PlaygroundFlowNode };

interface PlaygroundCanvasProps {
  nodes: PlaygroundNode[];
  edges: PlaygroundEdge[];
  onNodesChange: (changes: NodeChange<PlaygroundNode>[]) => void;
  onEdgesChange: (changes: EdgeChange<PlaygroundEdge>[]) => void;
  onConnect: (connection: Connection) => void;
  onAddNode: (kind: NodeKind, position: { x: number; y: number }) => void;
  onSelectNode: (nodeId: string | null) => void;
  selectedNodeId: string | null;
}

export default function PlaygroundCanvas({
  nodes,
  edges,
  onNodesChange,
  onEdgesChange,
  onConnect,
  onAddNode,
  onSelectNode,
  selectedNodeId,
}: PlaygroundCanvasProps) {
  const { screenToFlowPosition } = useReactFlow();

  const onDragOver = useCallback((event: React.DragEvent) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
  }, []);

  const onDrop = useCallback(
    (event: React.DragEvent) => {
      event.preventDefault();
      const kind = event.dataTransfer.getData("application/playground-node") as NodeKind;
      if (!kind) return;
      const position = screenToFlowPosition({ x: event.clientX, y: event.clientY });
      onAddNode(kind, position);
    },
    [onAddNode, screenToFlowPosition]
  );

  const onNodeClick: NodeMouseHandler<PlaygroundNode> = useCallback(
    (_event, node) => {
      onSelectNode(node.id);
    },
    [onSelectNode]
  );

  const onPaneClick = useCallback(() => {
    onSelectNode(null);
  }, [onSelectNode]);

  return (
    <div className="w-full h-full">
      <ReactFlow
        nodes={nodes.map(n => ({ ...n, selected: n.id === selectedNodeId }))}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onConnect={onConnect}
        onNodeClick={onNodeClick}
        onPaneClick={onPaneClick}
        onDragOver={onDragOver}
        onDrop={onDrop}
        nodeTypes={nodeTypes}
        fitView
        deleteKeyCode="Delete"
        className="bg-void-900"
      >
        <Background variant={BackgroundVariant.Dots} gap={20} size={1} color="#ffffff18" />
        <Controls className="!bg-void-800 !border-white/10 !text-txt-muted" />
        <MiniMap
          className="!bg-void-800 !border-white/10"
          nodeColor="#6366f1"
          maskColor="#00000060"
        />
      </ReactFlow>
    </div>
  );
}
