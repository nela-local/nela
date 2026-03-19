import React, { useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, Check, Plus, Trash2, Pencil } from "lucide-react";
import type { WorkspaceRecord } from "../types";
import "./WorkspaceSelector.css";

interface WorkspaceSelectorProps {
  workspaces: WorkspaceRecord[];
  activeWorkspaceId: string | null;
  onSelectWorkspace: (workspaceId: string) => void;
  onCreateWorkspace: () => void;
  onDeleteWorkspace: (workspaceId: string) => void;
  onRenameWorkspace: (workspaceId: string, newName: string) => Promise<void> | void;
  busy?: boolean;
}

const WorkspaceSelector: React.FC<WorkspaceSelectorProps> = ({
  workspaces,
  activeWorkspaceId,
  onSelectWorkspace,
  onCreateWorkspace,
  onDeleteWorkspace,
  onRenameWorkspace,
  busy = false,
}) => {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const active = useMemo(
    () => workspaces.find((w) => w.id === activeWorkspaceId) ?? null,
    [workspaces, activeWorkspaceId]
  );

  useEffect(() => {
    if (!open) return;
    const handleMouseDown = (e: MouseEvent) => {
      if (!containerRef.current) return;
      if (!containerRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", handleMouseDown);
    return () => document.removeEventListener("mousedown", handleMouseDown);
  }, [open]);

  return (
    <div className="workspace-selector-container" ref={containerRef}>
      <button
        className={`workspace-selector-btn ${open ? "active" : ""}`}
        onClick={() => !busy && setOpen((v) => !v)}
        disabled={busy}
        title={active ? active.name : "Select a workspace"}
        type="button"
      >
        <span className="workspace-name">{active ? active.name : "Workspaces"}</span>
        <ChevronDown size={14} className="chevron" />
      </button>

      {open && (
        <div className="workspace-dropdown animate-dropdown">
          <div className="dropdown-header">Workspaces</div>

          <div className="workspace-list">
            {workspaces.map((ws) => {
              const selected = ws.id === activeWorkspaceId;
              return (
                <div
                  key={ws.id}
                  className={`workspace-item ${selected ? "selected" : ""}`}
                  onClick={() => {
                    onSelectWorkspace(ws.id);
                    setOpen(false);
                  }}
                  title={ws.name}
                  role="button"
                  tabIndex={0}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      onSelectWorkspace(ws.id);
                      setOpen(false);
                    }
                  }}
                >
                  <span className="truncate">{ws.name}</span>

                  <div className="workspace-item-actions" onClick={(e) => e.stopPropagation()}>
                    {selected && <Check size={14} className="check-icon" />}

                    <button
                      type="button"
                      className="workspace-action-btn"
                      onClick={async () => {
                        const next = window.prompt("Rename workspace", ws.name);
                        if (next === null) return;
                        const trimmed = next.trim();
                        if (!trimmed) return;
                        await onRenameWorkspace(ws.id, trimmed);
                      }}
                      disabled={busy}
                      title="Rename workspace"
                    >
                      <Pencil size={14} />
                    </button>

                    <button
                      type="button"
                      className="workspace-action-btn danger"
                      onClick={() => {
                        setOpen(false);
                        onDeleteWorkspace(ws.id);
                      }}
                      disabled={busy}
                      title="Delete workspace"
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>

          <div className="dropdown-footer">
            <button
              className="add-workspace-btn"
              onClick={() => {
                onCreateWorkspace();
                setOpen(false);
              }}
              disabled={busy}
              type="button"
            >
              <Plus size={14} />
              <span>Create workspace</span>
            </button>
          </div>
        </div>
      )}
    </div>
  );
};

export default WorkspaceSelector;

