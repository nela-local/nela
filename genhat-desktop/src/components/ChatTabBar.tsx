import React, { useRef, useState, useCallback, useEffect, memo } from "react";
import { Plus, X, Loader2, MessageSquare, GripVertical } from "lucide-react";
import type { ChatSession } from "../types";

interface ChatTabBarProps {
  sessions: ChatSession[];
  activeSessionId: string;
  onSelectSession: (id: string) => void;
  onNewSession: () => void;
  onCloseSession: (id: string) => void;
  onReorderSessions: (sessions: ChatSession[]) => void;
}

/**
 * Horizontal, scrollable tab bar for managing multiple chat sessions.
 *
 * Features:
 * - Horizontal scrolling (mouse wheel, scroll arrows, drag-scroll)
 * - Drag-to-reorder tabs (pointer events — works reliably in Tauri WebView)
 * - Close button per tab (won't close the last tab; creates a new empty one)
 * - Loading indicator on tabs with active inference
 * - "New Chat" button pinned at the end
 */
const ChatTabBar: React.FC<ChatTabBarProps> = memo(({
  sessions,
  activeSessionId,
  onSelectSession,
  onNewSession,
  onCloseSession,
  onReorderSessions,
}) => {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [showLeftArrow, setShowLeftArrow] = useState(false);
  const [showRightArrow, setShowRightArrow] = useState(false);

  // ── Pointer-based drag state ───────────────────────────────────────────
  const dragState = useRef<{
    id: string;
    startX: number;
    el: HTMLElement;
    clone: HTMLElement | null;
    offsetX: number;
  } | null>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);
  const [dropSide, setDropSide] = useState<"left" | "right" | null>(null);

  // ── Scroll overflow detection ──────────────────────────────────────────
  const checkOverflow = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    setShowLeftArrow(el.scrollLeft > 4);
    setShowRightArrow(el.scrollLeft + el.clientWidth < el.scrollWidth - 4);
  }, []);

  useEffect(() => {
    checkOverflow();
    const el = scrollRef.current;
    if (!el) return;
    el.addEventListener("scroll", checkOverflow, { passive: true });
    const ro = new ResizeObserver(checkOverflow);
    ro.observe(el);
    return () => {
      el.removeEventListener("scroll", checkOverflow);
      ro.disconnect();
    };
  }, [checkOverflow, sessions.length]);

  // Scroll active tab into view on selection
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const activeTab = el.querySelector(`[data-session-id="${activeSessionId}"]`) as HTMLElement | null;
    if (activeTab) {
      activeTab.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "nearest" });
    }
  }, [activeSessionId]);

  // ── Wheel scroll (horizontal) ──────────────────────────────────────────
  const handleWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    const el = scrollRef.current;
    if (el) {
      el.scrollLeft += e.deltaY !== 0 ? e.deltaY : e.deltaX;
    }
  };

  // ── Scroll by arrows ──────────────────────────────────────────────────
  const scrollBy = (delta: number) => {
    scrollRef.current?.scrollBy({ left: delta, behavior: "smooth" });
  };

  // ── Pointer-based drag & drop reorder ──────────────────────────────────
  const DRAG_THRESHOLD = 5; // px — movement needed before drag activates

  const handleGripPointerDown = (e: React.PointerEvent, sessionId: string) => {
    e.preventDefault();
    e.stopPropagation();
    const target = (e.currentTarget as HTMLElement).closest("[data-session-id]") as HTMLElement;
    if (!target) return;

    // Capture pointer on the grip element
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);

    dragState.current = {
      id: sessionId,
      startX: e.clientX,
      el: target,
      clone: null,
      offsetX: e.clientX - target.getBoundingClientRect().left,
    };
  };

  useEffect(() => {
    const handlePointerMove = (e: PointerEvent) => {
      const ds = dragState.current;
      if (!ds) return;

      const dx = Math.abs(e.clientX - ds.startX);

      // Create the floating clone once past threshold
      if (!ds.clone && dx >= DRAG_THRESHOLD) {
        setDragId(ds.id);
        const rect = ds.el.getBoundingClientRect();
        const clone = ds.el.cloneNode(true) as HTMLElement;
        clone.style.position = "fixed";
        clone.style.top = `${rect.top}px`;
        clone.style.left = `${e.clientX - ds.offsetX}px`;
        clone.style.width = `${rect.width}px`;
        clone.style.height = `${rect.height}px`;
        clone.style.pointerEvents = "none";
        clone.style.zIndex = "9999";
        clone.style.opacity = "0.85";
        clone.style.boxShadow = "0 4px 20px rgba(0,212,255,0.25)";
        clone.style.borderRadius = "8px";
        clone.style.transition = "none";
        clone.classList.add("dragging-clone");
        document.body.appendChild(clone);
        ds.clone = clone;
      }

      // Move the clone
      if (ds.clone) {
        ds.clone.style.left = `${e.clientX - ds.offsetX}px`;

        // Find which tab we're over
        const scrollEl = scrollRef.current;
        if (scrollEl) {
          const tabs = scrollEl.querySelectorAll<HTMLElement>("[data-session-id]");
          let foundTarget: string | null = null;
          let foundSide: "left" | "right" | null = null;
          for (const tab of tabs) {
            const tabId = tab.getAttribute("data-session-id");
            if (tabId === ds.id) continue;
            const tr = tab.getBoundingClientRect();
            if (e.clientX >= tr.left && e.clientX <= tr.right) {
              foundTarget = tabId;
              foundSide = e.clientX < tr.left + tr.width / 2 ? "left" : "right";
              break;
            }
          }
          setDropTargetId(foundTarget);
          setDropSide(foundSide);
        }
      }
    };

    const handlePointerUp = () => {
      const ds = dragState.current;
      if (!ds) return;

      // Perform reorder if we have a valid drop target
      if (ds.clone && dropTargetId && dropTargetId !== ds.id) {
        const fromIdx = sessions.findIndex((s) => s.id === ds.id);
        const toIdx = sessions.findIndex((s) => s.id === dropTargetId);
        if (fromIdx !== -1 && toIdx !== -1) {
          const reordered = [...sessions];
          const [moved] = reordered.splice(fromIdx, 1);
          // Insert based on which side of the target we're on
          const insertIdx = dropSide === "right" ? toIdx : (toIdx > fromIdx ? toIdx - 1 : toIdx);
          reordered.splice(insertIdx < 0 ? 0 : insertIdx, 0, moved);
          onReorderSessions(reordered);
        }
      }

      // Cleanup
      if (ds.clone) {
        ds.clone.remove();
      }
      dragState.current = null;
      setDragId(null);
      setDropTargetId(null);
      setDropSide(null);
    };

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }, [sessions, dropTargetId, dropSide, onReorderSessions]);

  // ── Middle-click to close ──────────────────────────────────────────────
  const handleMouseDown = (e: React.MouseEvent, sessionId: string) => {
    if (e.button === 1) {
      // Middle click
      e.preventDefault();
      onCloseSession(sessionId);
    }
  };

  return (
    <div className="chat-tab-bar flex items-center h-[38px] bg-void-800 border-b border-glass-border shrink-0 relative select-none">
      {/* Left scroll arrow */}
      {showLeftArrow && (
        <button
          className="tab-scroll-arrow left-0 z-10 flex items-center justify-center w-6 h-full bg-void-800/90 border-r border-glass-border text-txt-muted cursor-pointer transition-colors duration-150 hover:text-neon shrink-0"
          onClick={() => scrollBy(-160)}
          title="Scroll left"
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="15 18 9 12 15 6" />
          </svg>
        </button>
      )}

      {/* Scrollable tab container */}
      <div
        ref={scrollRef}
        className="chat-tab-scroll flex items-center flex-1 overflow-x-auto overflow-y-hidden min-w-0"
        onWheel={handleWheel}
      >
        {sessions.map((session) => {
          const isActive = session.id === activeSessionId;
          const isDragging = session.id === dragId;
          const isDropTarget = session.id === dropTargetId && dragId !== session.id;

          return (
            <div
              key={session.id}
              data-session-id={session.id}
              onMouseDown={(e) => handleMouseDown(e, session.id)}
              onClick={() => { if (!dragId) onSelectSession(session.id); }}
              className={`chat-tab group flex items-center gap-1 h-[34px] px-1 pr-2 mx-px cursor-pointer transition-all duration-150 rounded-t-lg shrink-0 max-w-[220px] min-w-[110px] border border-transparent relative
                ${isActive
                  ? "bg-void-900 border-glass-border border-b-transparent text-txt shadow-[0_-2px_8px_rgba(0,212,255,0.06)]"
                  : "text-txt-muted hover:bg-void-700/60 hover:text-txt-secondary"
                }
                ${isDragging ? "opacity-30" : ""}
              `}
              title={session.title}
            >
              {/* Drop indicator — left edge */}
              {isDropTarget && dropSide === "left" && (
                <div className="absolute left-0 top-1 bottom-1 w-[2px] bg-neon rounded-full shadow-[0_0_6px_rgba(0,212,255,0.5)]" />
              )}
              {/* Drop indicator — right edge */}
              {isDropTarget && dropSide === "right" && (
                <div className="absolute right-0 top-1 bottom-1 w-[2px] bg-neon rounded-full shadow-[0_0_6px_rgba(0,212,255,0.5)]" />
              )}

              {/* Drag grip handle */}
              <div
                className={`drag-grip flex items-center justify-center w-[18px] h-[24px] rounded cursor-grab shrink-0 transition-colors duration-150
                  ${isActive
                    ? "text-txt-muted/60 hover:text-neon"
                    : "text-transparent group-hover:text-txt-muted/40 hover:!text-neon"
                  }
                  ${isDragging ? "cursor-grabbing" : ""}
                `}
                onPointerDown={(e) => handleGripPointerDown(e, session.id)}
                title="Drag to reorder"
              >
                <GripVertical size={12} strokeWidth={2} />
              </div>

              {/* Tab icon */}
              {session.loading ? (
                <Loader2 size={13} className="spin shrink-0 text-neon" />
              ) : (
                <MessageSquare size={13} className={`shrink-0 ${isActive ? "text-neon" : ""}`} />
              )}

              {/* Tab title */}
              <span className="text-[0.74rem] font-medium truncate leading-none flex-1 min-w-0">
                {session.title}
              </span>

              {/* Close button */}
              <button
                className={`chat-tab-close flex items-center justify-center w-[18px] h-[18px] rounded transition-all duration-150 shrink-0
                  ${isActive
                    ? "text-txt-muted hover:text-danger hover:bg-danger/10"
                    : "text-transparent group-hover:text-txt-muted hover:!text-danger hover:!bg-danger/10"
                  }
                `}
                onClick={(e) => {
                  e.stopPropagation();
                  onCloseSession(session.id);
                }}
                title="Close chat"
              >
                <X size={12} strokeWidth={2.5} />
              </button>

              {/* Active tab accent */}
              {isActive && (
                <div className="absolute bottom-0 left-2 right-2 h-[2px] bg-neon rounded-full shadow-[0_0_6px_rgba(0,212,255,0.4)]" />
              )}
            </div>
          );
        })}

        {/* New Tab button (inside scrollable area, pinned after last tab) */}
        <button
          className="chat-tab-new flex items-center justify-center w-[30px] h-[30px] mx-1 rounded-lg text-txt-muted border border-dashed border-transparent cursor-pointer transition-all duration-150 shrink-0 hover:border-glass-border hover:text-neon hover:bg-neon-subtle"
          onClick={onNewSession}
          title="New chat (Ctrl+T)"
        >
          <Plus size={15} strokeWidth={2} />
        </button>
      </div>

      {/* Right scroll arrow */}
      {showRightArrow && (
        <button
          className="tab-scroll-arrow right-0 z-10 flex items-center justify-center w-6 h-full bg-void-800/90 border-l border-glass-border text-txt-muted cursor-pointer transition-colors duration-150 hover:text-neon shrink-0"
          onClick={() => scrollBy(160)}
          title="Scroll right"
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="9 18 15 12 9 6" />
          </svg>
        </button>
      )}
    </div>
  );
});

ChatTabBar.displayName = "ChatTabBar";

export default ChatTabBar;
