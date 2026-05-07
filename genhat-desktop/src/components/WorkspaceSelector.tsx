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
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const renameInputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const commitRename = async () => {
    if (!renamingId) return;
    const trimmed = renameValue.trim();
    if (trimmed) {
      await onRenameWorkspace(renamingId, trimmed);
    }
    setRenamingId(null);
    setRenameValue("");
  };

  const cancelRename = () => {
    setRenamingId(null);
    setRenameValue("");
  };

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
    <div className="workspace-selector-container" ref={containerRef} data-tour="workspace-selector">
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
              const isRenaming = renamingId === ws.id;
              return (
                <div
                  key={ws.id}
                  className={`workspace-item ${selected ? "selected" : ""}`}
                  onClick={() => {
                    if (isRenaming) return;
                    onSelectWorkspace(ws.id);
                    setOpen(false);
                  }}
                  title={isRenaming ? undefined : ws.name}
                  role="button"
                  tabIndex={0}
                  onKeyDown={(e) => {
                    if (isRenaming) return;
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      onSelectWorkspace(ws.id);
                      setOpen(false);
                    }
                  }}
                >
                  {isRenaming ? (
                    <input
                      ref={renameInputRef}
                      className="workspace-rename-input"
                      value={renameValue}
                      autoFocus
                      onClick={(e) => e.stopPropagation()}
                      onChange={(e) => setRenameValue(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          e.preventDefault();
                          void commitRename();
                        } else if (e.key === "Escape") {
                          e.preventDefault();
                          cancelRename();
                        }
                      }}
                      onBlur={() => void commitRename()}
                    />
                  ) : (
                    <span className="truncate">{ws.name}</span>
                  )}

                  <div className="workspace-item-actions" onClick={(e) => e.stopPropagation()}>
                    {selected && !isRenaming && <Check size={14} className="check-icon" />}

                    <button
                      type="button"
                      className="workspace-action-btn"
                      onClick={(e) => {
                        e.stopPropagation();
                        setRenamingId(ws.id);
                        setRenameValue(ws.name);
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

