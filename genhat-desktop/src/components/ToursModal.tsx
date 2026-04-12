import { useState } from "react";
import { BookOpen, Map, X } from "lucide-react";
import { useTour } from "../hooks/useTour";
import MarkdownRenderer from "./MarkdownRenderer";
import helpGuideMarkdown from "../content/help-guide.md?raw";
import "./ToursModal.css";

export default function ToursModal({ isOpen, onClose }: { isOpen: boolean; onClose: () => void }) {
  const { tours, isTourCompleted, resetTourProgress, startTour, bindings } = useTour();
  const [activeTab, setActiveTab] = useState<"tours" | "guide">("tours");

  const handleClose = () => {
    setActiveTab("tours");
    onClose();
  };

  const waitForTourTarget = (selector: string, timeoutMs = 1800): Promise<void> => {
    return new Promise((resolve) => {
      const startedAt = Date.now();

      const check = () => {
        if (document.querySelector(selector)) {
          resolve();
          return;
        }
        if (Date.now() - startedAt >= timeoutMs) {
          resolve();
          return;
        }
        window.setTimeout(check, 40);
      };

      check();
    });
  };

  const launchTour = (tourId: string) => {
    handleClose();

    if (tourId === "podcast") {
      const switchMode = bindings.switchMode;
      if (typeof switchMode === "function") {
        (switchMode as (mode: string) => void)("podcast");
      }

      void waitForTourTarget('[data-tour="podcast-header"]').then(() => {
        startTour(tourId, { source: "help" });
      });
      return;
    }

    window.setTimeout(() => {
      startTour(tourId, { source: "help" });
    }, 30);
  };

  if (!isOpen) return null;

  return (
    <div className="tours-modal-overlay" onClick={handleClose}>
      <div className="tours-modal" onClick={(e) => e.stopPropagation()}>
        <div className="tours-modal-header">
          <div className="tours-modal-title">Help Center</div>
          <button className="tours-modal-close" onClick={handleClose} aria-label="Close">
            <X size={16} />
          </button>
        </div>

        <div className="tours-modal-body">
          <div className="tours-modal-tabs" role="tablist" aria-label="Help views">
            <button
              className={`tours-tab ${activeTab === "tours" ? "active" : ""}`}
              onClick={() => setActiveTab("tours")}
              role="tab"
              aria-selected={activeTab === "tours"}
            >
              <Map size={14} />
              <span>Tours</span>
            </button>
            <button
              className={`tours-tab ${activeTab === "guide" ? "active" : ""}`}
              onClick={() => setActiveTab("guide")}
              role="tab"
              aria-selected={activeTab === "guide"}
            >
              <BookOpen size={14} />
              <span>Help Guide</span>
            </button>
          </div>

          {activeTab === "tours" && (
            <>
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
                          onClick={() => launchTour(t.id)}
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
            </>
          )}

          {activeTab === "guide" && (
            <div className="help-guide-panel">
              <MarkdownRenderer content={helpGuideMarkdown} />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
