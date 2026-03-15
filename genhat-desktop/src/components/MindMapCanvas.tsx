import React, { useEffect, useMemo, useRef, useState } from "react";
import type { MindMapGraph, MindMapNode } from "../types";
import MindMapBackground from "./MindMapBackground";

type Point = { x: number; y: number };

interface MindMapCanvasProps {
  graph: MindMapGraph;
  height?: number;
  zoom?: number;
}

interface VisibleNode {
  id: string;
  label: string;
  depth: number;
  parentId: string | null;
  path: string;
}

const X_STEP = 260;
const Y_STEP = 92;
const NODE_W = 210;
const NODE_H = 42;

function flattenVisible(
  node: MindMapNode,
  collapsed: Set<string>,
  depth: number,
  parentId: string | null,
  path: string,
  out: VisibleNode[]
) {
  out.push({ id: node.id, label: node.label, depth, parentId, path });
  if (collapsed.has(node.id)) return;
  node.children.forEach((child, idx) => {
    flattenVisible(child, collapsed, depth + 1, node.id, `${path}.${idx}`, out);
  });
}

function verticalLayout(nodes: VisibleNode[]): Map<string, Point> {
  const levels = new Map<number, VisibleNode[]>();
  for (const node of nodes) {
    if (!levels.has(node.depth)) levels.set(node.depth, []);
    levels.get(node.depth)!.push(node);
  }

  const entries = Array.from(levels.entries()).sort((a, b) => a[0] - b[0]);
  const pos = new Map<string, Point>();

  for (const [depth, levelNodes] of entries) {
    levelNodes.sort((a, b) => a.path.localeCompare(b.path));
    levelNodes.forEach((node, idx) => {
      pos.set(node.id, {
        x: depth * X_STEP,
        y: idx * Y_STEP,
      });
    });
  }

  return pos;
}

const MindMapCanvas: React.FC<MindMapCanvasProps> = ({ graph, height = 620, zoom = 1 }) => {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [containerSize, setContainerSize] = useState({ width: 900, height });
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [manualOffsets, setManualOffsets] = useState<Record<string, Point>>({});
  const [dragging, setDragging] = useState<{ id: string; startX: number; startY: number; base: Point } | null>(null);
  const [panning, setPanning] = useState<{ startX: number; startY: number; base: Point } | null>(null);
  const [panOffset, setPanOffset] = useState<Point>({ x: 0, y: 0 });

  useEffect(() => {
    setCollapsed(new Set());
    setManualOffsets({});
    setPanOffset({ x: 0, y: 0 });
    setPanning(null);
  }, [graph]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height: h } = entry.contentRect;
        setContainerSize({ width: Math.max(300, width), height: Math.max(280, h) });
      }
    });

    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const visibleNodes = useMemo(() => {
    const out: VisibleNode[] = [];
    flattenVisible(graph.root, collapsed, 0, null, "0", out);
    return out;
  }, [graph.root, collapsed]);

  const basePositions = useMemo(() => verticalLayout(visibleNodes), [visibleNodes]);

  const finalPositions = useMemo(() => {
    const merged = new Map<string, Point>();
    for (const node of visibleNodes) {
      const base = basePositions.get(node.id)!;
      const offset = manualOffsets[node.id] ?? { x: 0, y: 0 };
      merged.set(node.id, { x: base.x + offset.x, y: base.y + offset.y });
    }
    return merged;
  }, [visibleNodes, basePositions, manualOffsets]);

  const bbox = useMemo(() => {
    if (visibleNodes.length === 0) return { minX: 0, maxX: 0, minY: 0, maxY: 0 };
    let minX = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;
    let minY = Number.POSITIVE_INFINITY;
    let maxY = Number.NEGATIVE_INFINITY;

    for (const node of visibleNodes) {
      const p = finalPositions.get(node.id)!;
      minX = Math.min(minX, p.x);
      maxX = Math.max(maxX, p.x + NODE_W);
      minY = Math.min(minY, p.y);
      maxY = Math.max(maxY, p.y + NODE_H);
    }

    return { minX, maxX, minY, maxY };
  }, [visibleNodes, finalPositions]);

  const centerOffset = useMemo(() => {
    const graphWidth = bbox.maxX - bbox.minX;
    const graphHeight = bbox.maxY - bbox.minY;
    return {
      x: containerSize.width / 2 - (bbox.minX + graphWidth / 2),
      y: containerSize.height / 2 - (bbox.minY + graphHeight / 2),
    };
  }, [bbox, containerSize.height, containerSize.width]);

  const mapOffset = useMemo(
    () => ({
      x: centerOffset.x + panOffset.x,
      y: centerOffset.y + panOffset.y,
    }),
    [centerOffset.x, centerOffset.y, panOffset.x, panOffset.y]
  );

  const toggleNode = (id: string) => {
    setPanOffset({ x: 0, y: 0 });
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  useEffect(() => {
    if (!dragging) return;

    const onMove = (e: PointerEvent) => {
      const dx = e.clientX - dragging.startX;
      const dy = e.clientY - dragging.startY;
      setManualOffsets((prev) => ({
        ...prev,
        [dragging.id]: {
          x: dragging.base.x + dx / zoom,
          y: dragging.base.y + dy / zoom,
        },
      }));
    };

    const onUp = () => setDragging(null);

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
  }, [dragging, zoom]);

  useEffect(() => {
    if (!panning) return;

    const onMove = (e: PointerEvent) => {
      const dx = e.clientX - panning.startX;
      const dy = e.clientY - panning.startY;
      setPanOffset({
        x: panning.base.x + dx,
        y: panning.base.y + dy,
      });
    };

    const onUp = () => setPanning(null);

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
  }, [panning]);

  const onCanvasPointerDown = (event: React.PointerEvent<SVGSVGElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    const target = event.target as Element | null;
    if (target?.closest("[data-node-rect='true']")) return;

    setPanning({
      startX: event.clientX,
      startY: event.clientY,
      base: panOffset,
    });
  };

  return (
    <div className="mindmap-canvas-root h-full w-full rounded-2xl border border-glass-border bg-void-900/70 backdrop-blur-md overflow-hidden">
      <div className="flex items-center justify-between px-3.5 py-2.5 border-b border-glass-border bg-void-800/75">
        <div className="text-sm font-semibold text-txt truncate" title={graph.title}>{graph.title}</div>
        <div className="flex items-center gap-2 text-[0.74rem] text-txt-muted">
          <span>{graph.generatedFrom === "documents" ? "Document-grounded" : "Model knowledge"}</span>
          <button
            className="px-2 py-1 rounded-md border border-glass-border hover:border-neon/40 hover:text-neon"
            onClick={() => {
              setCollapsed(new Set());
              setPanOffset({ x: 0, y: 0 });
            }}
            title="Expand all"
          >
            Expand all
          </button>
          <button
            className="px-2 py-1 rounded-md border border-glass-border hover:border-neon/40 hover:text-neon"
            onClick={() => {
              setCollapsed(new Set(visibleNodes.filter((n) => n.depth > 0).map((n) => n.id)));
              setPanOffset({ x: 0, y: 0 });
            }}
            title="Collapse all children"
          >
            Collapse all
          </button>
        </div>
      </div>

      <div ref={containerRef} className="relative w-full" style={{ height: height - 47 }}>
        <div className="absolute inset-0 z-0 pointer-events-none rounded-b-2xl overflow-hidden">
          <MindMapBackground width={Math.max(300, containerSize.width)} height={Math.max(220, containerSize.height - 47)} />
        </div>

        <svg
          width="100%"
          height="100%"
          role="img"
          aria-label="Mindmap graph"
          className="relative z-10 select-none"
          onPointerDown={onCanvasPointerDown}
          style={{ cursor: panning ? "grabbing" : "grab", touchAction: "none" }}
        >
          <g transform={`translate(${mapOffset.x}, ${mapOffset.y})`}>
            <g transform={`scale(${zoom})`}>
            {visibleNodes
              .filter((node) => node.parentId)
              .map((node) => {
                const child = finalPositions.get(node.id)!;
                const parent = finalPositions.get(node.parentId!)!;
                const startX = parent.x + NODE_W;
                const startY = parent.y + NODE_H / 2;
                const endX = child.x;
                const endY = child.y + NODE_H / 2;
                const c1x = startX + 70;
                const c2x = endX - 70;

                return (
                  <path
                    key={`${node.parentId}-${node.id}`}
                    d={`M ${startX} ${startY} C ${c1x} ${startY}, ${c2x} ${endY}, ${endX} ${endY}`}
                    fill="none"
                    stroke="rgba(168,189,255,0.45)"
                    strokeWidth={1.5}
                  />
                );
              })}

            {visibleNodes.map((node) => {
              const point = finalPositions.get(node.id)!;
              const hasChildren = graphIndex(node.id, graph.root)?.children.length ? true : false;
              const isCollapsed = collapsed.has(node.id);
              const isRoot = node.depth === 0;

              return (
                <g key={node.id} transform={`translate(${point.x}, ${point.y})`}>
                  <rect
                    data-node-rect="true"
                    width={NODE_W}
                    height={NODE_H}
                    rx={10}
                    fill={isRoot ? "rgb(168,178,255)" : "rgb(28,42,58)"}
                    stroke={isRoot ? "rgb(155,172,255)" : "rgb(112,197,255)"}
                    strokeWidth={1.4}
                    onPointerDown={(e) => {
                      if (e.button !== 0) return;
                      e.stopPropagation();
                      const base = manualOffsets[node.id] ?? { x: 0, y: 0 };
                      setDragging({ id: node.id, startX: e.clientX, startY: e.clientY, base });
                    }}
                    style={{ cursor: "grab" }}
                  />
                  <text
                    x={12}
                    y={24}
                    fill="rgba(224,245,255,0.96)"
                    fontSize="13"
                    fontWeight={isRoot ? 600 : 500}
                    style={{ pointerEvents: "none", userSelect: "none" }}
                  >
                    {trim(node.label, 34)}
                  </text>

                  {hasChildren && (
                    <g
                      transform={`translate(${NODE_W - 16}, 9)`}
                      onClick={() => toggleNode(node.id)}
                      style={{ cursor: "pointer" }}
                    >
                      <circle r={7} cx={0} cy={0} fill="rgba(8,14,22,0.78)" stroke="rgba(96,185,255,0.65)" />
                      <line x1={-3.4} y1={0} x2={3.4} y2={0} stroke="rgba(218,243,255,0.95)" strokeWidth={1.4} />
                      {isCollapsed && (
                        <line x1={0} y1={-3.4} x2={0} y2={3.4} stroke="rgba(218,243,255,0.95)" strokeWidth={1.4} />
                      )}
                    </g>
                  )}
                </g>
              );
            })}
            </g>
          </g>
        </svg>
      </div>
    </div>
  );
};

function trim(text: string, max: number): string {
  if (text.length <= max) return text;
  return `${text.slice(0, max - 1)}…`;
}

function graphIndex(id: string, root: MindMapNode): MindMapNode | null {
  if (root.id === id) return root;
  for (const child of root.children) {
    const found = graphIndex(id, child);
    if (found) return found;
  }
  return null;
}

export default MindMapCanvas;
