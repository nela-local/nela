import React from "react";
import { Info, AlertTriangle, XCircle, CheckCircle2, X } from "lucide-react";
import "./AppModal.css";

export type AppModalKind = "info" | "warning" | "error" | "confirm";

interface AppModalProps {
  isOpen: boolean;
  kind: AppModalKind;
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  showCancel?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

const AppModal: React.FC<AppModalProps> = ({
  isOpen,
  kind,
  title,
  message,
  confirmLabel = "OK",
  cancelLabel = "Cancel",
  showCancel = false,
  onConfirm,
  onCancel,
}) => {
  if (!isOpen) return null;

  const icon = (() => {
    switch (kind) {
      case "error":
        return <XCircle size={20} />;
      case "warning":
        return <AlertTriangle size={20} />;
      case "confirm":
        return <CheckCircle2 size={20} />;
      default:
        return <Info size={20} />;
    }
  })();

  return (
    <div className="app-modal-overlay" onClick={onCancel}>
      <div className="app-modal" onClick={(e) => e.stopPropagation()}>
        <div className={`app-modal-header ${kind}`}>
          <div className="app-modal-title">
            {icon}
            <span>{title}</span>
          </div>
          <button className="app-modal-close" onClick={onCancel}>
            <X size={16} />
          </button>
        </div>
        <div className="app-modal-body">
          <p>{message}</p>
        </div>
        <div className="app-modal-actions">
          {showCancel && (
            <button className="app-modal-btn ghost" onClick={onCancel}>
              {cancelLabel}
            </button>
          )}
          <button className={`app-modal-btn ${kind}`} onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
};

export default AppModal;
