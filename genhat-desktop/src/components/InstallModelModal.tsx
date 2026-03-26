import React from "react";
import { createPortal } from "react-dom";
import { X, Download, Loader2, HardDrive, Trash2 } from "lucide-react";
import type { ModelFile } from "../types";
import "./InstallModelModal.css";

interface InstallModelModalProps {
  isOpen: boolean;
  onClose: () => void;
  models: ModelFile[];
  title: string;
  onDownload: (path: string) => void;
  onCancelDownload?: (path: string) => void;
  onUninstall?: (path: string) => void;
  onConfirm?: (title: string, message: string, confirmLabel?: string) => Promise<boolean>;
  downloads?: Record<string, {progress: number, status: string}>;
}

const InstallModelModal: React.FC<InstallModelModalProps> = ({
  isOpen, onClose, models, title, onDownload, onCancelDownload, onUninstall, onConfirm, downloads = {}
}) => {
  if (!isOpen) return null;

  const missingModels = models.filter(m => !m.is_downloaded && m.gdrive_id);
  const installedModels = models.filter(m => m.is_downloaded);
  
  const modal = (
    <div className="install-modal-overlay" onClick={onClose}>
      <div className="install-modal-content" onClick={e => e.stopPropagation()}>
        <div className="install-modal-header">
          <div className="flex items-center gap-2">
            <HardDrive size={18} className="text-neon" />
            <span className="font-semibold">{title}</span>
          </div>
          <button className="close-btn" onClick={onClose}><X size={16} /></button>
        </div>
        <div className="install-modal-body">
          {missingModels.length === 0 && installedModels.length === 0 ? (
            <div className="text-center text-txt-secondary py-8 text-sm">
              No downloadable models found.
            </div>
          ) : (
            <div className="install-list">
              {missingModels.length > 0 && (
                <div className="install-section-title">Available to Install</div>
              )}
              {missingModels.map(model => {
                const isDownloading = downloads[model.path] !== undefined;
                const dlState = downloads[model.path];
                
                return (
                  <div key={model.path} className="install-item">
                    <div className="install-item-info">
                      <span className="install-item-name">{model.name}</span>
                      <span className="install-item-path">{model.path}</span>
                    </div>
                    {isDownloading ? (
                      <div className="flex items-center gap-2">
                        <div className="install-progress">
                          <Loader2 size={14} className="animate-spin text-neon" />
                          <span className="text-neon text-sm">{dlState.progress.toFixed(0)}%</span>
                        </div>
                        {onCancelDownload && (
                          <button 
                            className="cancel-btn" 
                            onClick={() => onCancelDownload(model.path)}
                            title="Cancel Download"
                          >
                            <X size={14} />
                          </button>
                        )}
                      </div>
                    ) : (
                      <button className="install-btn" onClick={() => onDownload(model.path)}>
                        <Download size={14} />
                        <span>Install</span>
                      </button>
                    )}
                  </div>
                );
              })}

              {installedModels.length > 0 && (
                <div className="install-section-title">Installed</div>
              )}
              {installedModels.map(model => (
                <div key={model.path} className="install-item">
                  <div className="install-item-info">
                    <span className="install-item-name">{model.name}</span>
                    <span className="install-item-path">{model.path}</span>
                  </div>
                  {onUninstall && (
                    <button
                      className="install-btn uninstall-btn"
                      onClick={async () => {
                        const ok = onConfirm
                          ? await onConfirm("Delete model", `Delete ${model.name} from this device?`, "Delete")
                          : window.confirm(`Are you sure you want to uninstall ${model.name}?`);
                        if (ok) onUninstall(model.path);
                      }}
                    >
                      <Trash2 size={14} />
                      <span>Delete</span>
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );

  return createPortal(modal, document.body);
};

export default InstallModelModal;
