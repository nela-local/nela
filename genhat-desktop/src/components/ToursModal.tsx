import { X } from "lucide-react";
import { useTour } from "../hooks/useTour";
import "./ToursModal.css";

export default function ToursModal({ isOpen, onClose }: { isOpen: boolean; onClose: () => void }) {
  const { tours, isTourCompleted, resetTourProgress, startTour } = useTour();

  if (!isOpen) return null;

  return (
    <div className="tours-modal-overlay" onClick={onClose}>
      <div className="tours-modal" onClick={(e) => e.stopPropagation()}>
        <div className="tours-modal-header">
          <div className="tours-modal-title">Help · Tours</div>
          <button className="tours-modal-close" onClick={onClose} aria-label="Close">
            <X size={16} />
          </button>
        </div>

        <div className="tours-modal-body">
          <p className="tours-modal-subtitle">
            Pick a tour. You can run them anytime.
          </p>

          <div className="tours-list">
            {tours.map((t) => {
              const done = isTourCompleted(t.id);
              return (
                <div key={t.id} className="tours-item">
                  <div className="tours-item-main">
                    <div className="tours-item-name">{t.name}</div>
                    <div className="tours-item-meta">
                      <span>{t.steps.length} steps</span>
                      {done && <span className="tours-item-done">Completed</span>}
                    </div>
                  </div>
                  <div className="tours-item-actions">
                    <button
                      className="tours-btn primary"
                      onClick={() => {
                        onClose();
                        startTour(t.id, { source: "help" });
                      }}
                    >
                      Start
                    </button>
                  </div>
                </div>
              );
            })}
          </div>

          <div className="tours-footer">
            <button className="tours-btn ghost" onClick={resetTourProgress}>
              Reset tour progress
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
