import React, { useEffect, useMemo, useState } from "react";
import { Maximize2, Minimize, Minimize2, X } from "lucide-react";
import type { MindMapGraph } from "../types";
import MindMapCanvas from "./MindMapCanvas";
import MindMapBackground from "./MindMapBackground";

interface MindMapOverlayProps {
  graph: MindMapGraph | null;
  isGenerating?: boolean;
  query?: string;
  onClose: () => void;
}

type WindowMode = "normal" | "maximized" | "minimized";

interface WindowRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

const NORMAL_WIDTH = 1180;
const NORMAL_HEIGHT = 760;
const EDGE_GAP = 24;
const HEADER_H = 56;

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

const MindMapOverlay: React.FC<MindMapOverlayProps> = ({
  graph,
  isGenerating = false,
  query,
  onClose,
}) => {
  const [viewport, setViewport] = useState({
    width: window.innerWidth,
    height: window.innerHeight,
  });
  const [mode, setMode] = useState<WindowMode>("normal");
  const [normalRect, setNormalRect] = useState<WindowRect>({
    x: Math.max(EDGE_GAP, (window.innerWidth - NORMAL_WIDTH) / 2),
    y: Math.max(EDGE_GAP, (window.innerHeight - NORMAL_HEIGHT) / 2),
    width: Math.min(NORMAL_WIDTH, window.innerWidth - EDGE_GAP * 2),
    height: Math.min(NORMAL_HEIGHT, window.innerHeight - EDGE_GAP * 2),
  });
  const [dragging, setDragging] = useState<{
    startX: number;
    startY: number;
    originX: number;
    originY: number;
  } | null>(null);
  const [zoom, setZoom] = useState(1);

  useEffect(() => {
    if (isGenerating) return;
    setZoom(1);
  }, [graph?.id, isGenerating]);

  useEffect(() => {
    const onResize = () => {
      const nextViewport = { width: window.innerWidth, height: window.innerHeight };
      setViewport(nextViewport);
      setNormalRect((prev) => {
        const maxX = Math.max(EDGE_GAP, nextViewport.width - prev.width - EDGE_GAP);
        const maxY = Math.max(EDGE_GAP, nextViewport.height - prev.height - EDGE_GAP);
        return {
          ...prev,
          x: clamp(prev.x, EDGE_GAP, maxX),
          y: clamp(prev.y, EDGE_GAP, maxY),
        };
      });
    };

    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  useEffect(() => {
    if (!dragging || mode !== "normal") return;

    const onMove = (event: PointerEvent) => {
      const dx = event.clientX - dragging.startX;
      const dy = event.clientY - dragging.startY;
      setNormalRect((prev) => {
        const nextX = clamp(
          dragging.originX + dx,
          EDGE_GAP,
          Math.max(EDGE_GAP, viewport.width - prev.width - EDGE_GAP)
        );
        const nextY = clamp(
          dragging.originY + dy,
          EDGE_GAP,
          Math.max(EDGE_GAP, viewport.height - prev.height - EDGE_GAP)
        );
        return { ...prev, x: nextX, y: nextY };
      });
    };

    const onUp = () => setDragging(null);

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
  }, [dragging, mode, viewport.height, viewport.width]);

  const resolvedRect = useMemo<WindowRect>(() => {
    if (mode === "maximized") {
      return {
        x: EDGE_GAP,
        y: EDGE_GAP,
        width: Math.max(340, viewport.width - EDGE_GAP * 2),
        height: Math.max(280, viewport.height - EDGE_GAP * 2),
      };
    }

    if (mode === "minimized") {
      const width = 440;
      const height = HEADER_H;
      return {
        x: Math.max(EDGE_GAP, viewport.width - width - EDGE_GAP),
        y: Math.max(EDGE_GAP, viewport.height - height - EDGE_GAP),
        width,
        height,
      };
    }

    return normalRect;
  }, [mode, normalRect, viewport.height, viewport.width]);

  const handleHeaderPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (mode !== "normal") return;
    const target = event.target as Element | null;
    if (target?.closest("[data-overlay-control='true']")) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    setDragging({
      startX: event.clientX,
      startY: event.clientY,
      originX: normalRect.x,
      originY: normalRect.y,
    });
  };

  const toggleMinimize = () => {
    setMode((prev) => (prev === "minimized" ? "normal" : "minimized"));
  };

  const toggleMaximize = () => {
    setMode((prev) => (prev === "maximized" ? "normal" : "maximized"));
  };

  const zoomOut = () => setZoom((prev) => Math.max(0.5, +(prev - 0.1).toFixed(2)));
  const zoomIn = () => setZoom((prev) => Math.min(2.5, +(prev + 0.1).toFixed(2)));

  const canvasHeight = Math.max(420, resolvedRect.height - HEADER_H - 24);

  return (
    <div className="fixed inset-0 z-[72] bg-black/70 backdrop-blur-sm">
      <div
        className="absolute bg-void-800 border border-glass-border rounded-2xl overflow-hidden flex flex-col shadow-[0_12px_48px_rgba(0,0,0,0.55)]"
        style={{
          left: resolvedRect.x,
          top: resolvedRect.y,
          width: resolvedRect.width,
          height: resolvedRect.height,
        }}
      >
        <div
          className="h-14 shrink-0 border-b border-glass-border flex items-center justify-between px-3.5 cursor-grab select-none"
          onPointerDown={handleHeaderPointerDown}
        >
          <div className="min-w-0">
            <div className="text-[0.86rem] text-txt font-semibold truncate" title={graph?.title ?? "Generating mindmap"}>
              {isGenerating ? "Generating mindmap" : graph?.title ?? "Mindmap"}
            </div>
            <div className="text-[0.72rem] text-txt-muted truncate" title={query || graph?.query || ""}>
              {query || graph?.query || ""}
            </div>
          </div>

          <div className="h-full flex items-center gap-1.5">
            <button
              data-overlay-control="true"
              className="glass-btn inline-flex items-center justify-center w-8 h-8 rounded-lg text-txt-secondary hover:text-txt text-[0.98rem] font-semibold leading-none"
              onClick={zoomOut}
              title="Zoom out"
            >
              −
            </button>
            <span className="text-[0.72rem] text-txt-muted min-w-[48px] text-center select-none">
              {Math.round(zoom * 100)}%
            </span>
            <button
              data-overlay-control="true"
              className="glass-btn inline-flex items-center justify-center w-8 h-8 rounded-lg text-txt-secondary hover:text-txt text-[0.98rem] font-semibold leading-none"
              onClick={zoomIn}
              title="Zoom in"
            >
              +
            </button>
            <button
              data-overlay-control="true"
              className="glass-btn inline-flex items-center justify-center w-8 h-8 rounded-lg text-txt-secondary hover:text-txt"
              onClick={toggleMinimize}
              title={mode === "minimized" ? "Restore" : "Minimize window"}
            >
              <Minimize size={14} />
            </button>
            <button
              data-overlay-control="true"
              className="glass-btn inline-flex items-center justify-center w-8 h-8 rounded-lg text-txt-secondary hover:text-txt"
              onClick={toggleMaximize}
              title={mode === "maximized" ? "Restore window" : "Maximize window"}
            >
              {mode === "maximized" ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
            </button>
            <button
              data-overlay-control="true"
              className="glass-btn inline-flex items-center justify-center w-8 h-8 rounded-lg text-txt-secondary hover:text-danger"
              onClick={onClose}
              title="Close mindmap"
            >
              <X size={14} />
            </button>
          </div>
        </div>

        {mode !== "minimized" && (
          <div className="relative flex-1 min-h-0 p-3">
            <div className="relative h-full">
              {isGenerating ? (
                <div className="relative h-full w-full rounded-2xl border border-glass-border overflow-hidden bg-void-900/55 backdrop-blur-md flex flex-col items-center justify-center text-txt-secondary">
                  <div className="absolute inset-0 pointer-events-none">
                    <MindMapBackground width={Math.max(300, resolvedRect.width - 24)} height={Math.max(220, resolvedRect.height - HEADER_H - 24)} />
                  </div>
                  <div className="relative z-10 flex flex-col items-center justify-center">
                    <div className="typing-dots flex gap-1.5 py-2 mb-2">
                      <span></span><span></span><span></span>
                    </div>
                    <div className="text-sm">Building your mindmap…</div>
                  </div>
                </div>
              ) : graph ? (
                <MindMapCanvas graph={graph} height={canvasHeight} zoom={zoom} />
              ) : (
                <div className="h-full w-full rounded-2xl border border-glass-border bg-void-900/70 backdrop-blur-md flex items-center justify-center text-txt-muted text-sm">
                  No mindmap selected.
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
};

export default MindMapOverlay;
