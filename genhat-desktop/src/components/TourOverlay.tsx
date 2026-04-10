import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTour, TourUtils, type TourPlacement } from "../hooks/useTour";
import "./TourOverlay.css";

type Rect = { left: number; top: number; width: number; height: number; right: number; bottom: number };

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

function pickPlacement(rect: Rect, preferred: TourPlacement): Exclude<TourPlacement, "auto"> {
  if (preferred !== "auto") return preferred;
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  const spaceBottom = vh - rect.bottom;
  const spaceTop = rect.top;
  const spaceRight = vw - rect.right;
  const spaceLeft = rect.left;

  if (spaceBottom > 180) return "bottom";
  if (spaceTop > 180) return "top";
  if (spaceRight > 240) return "right";
  if (spaceLeft > 240) return "left";
  return spaceBottom >= spaceTop ? "bottom" : "top";
}

function computeTooltipPosition(rect: Rect, placement: Exclude<TourPlacement, "auto">) {
  const gap = 12;
  switch (placement) {
    case "top":
      return { left: rect.left + rect.width / 2, top: rect.top - gap, transform: "translate(-50%, -100%)" };
    case "bottom":
      return { left: rect.left + rect.width / 2, top: rect.bottom + gap, transform: "translate(-50%, 0)" };
    case "left":
      return { left: rect.left - gap, top: rect.top + rect.height / 2, transform: "translate(-100%, -50%)" };
    case "right":
      return { left: rect.right + gap, top: rect.top + rect.height / 2, transform: "translate(0, -50%)" };
  }
}

export default function TourOverlay() {
  const { status, activeTour, activeStep, stepIndex, next, prev, exit, complete } = useTour();
  const tooltipRef = useRef<HTMLDivElement>(null);
  const [targetRect, setTargetRect] = useState<Rect | null>(null);
  const [tick, setTick] = useState(0);

  const targetEl = useMemo(() => {
    if (!activeStep) return null;
    return TourUtils.resolveTarget(activeStep.target);
  }, [activeStep, tick]);

  useLayoutEffect(() => {
    if (status !== "running" || !activeStep) return;

    const update = () => {
      const el = TourUtils.resolveTarget(activeStep.target);
      if (!el) {
        setTargetRect(null);
        return;
      }
      const r = el.getBoundingClientRect();
      setTargetRect({ left: r.left, top: r.top, width: r.width, height: r.height, right: r.right, bottom: r.bottom });
    };

    update();

    const onResize = () => update();
    const onScroll = () => update();

    window.addEventListener("resize", onResize);
    window.addEventListener("scroll", onScroll, true);
    const id = window.setInterval(() => setTick((v) => v + 1), 500);

    return () => {
      window.removeEventListener("resize", onResize);
      window.removeEventListener("scroll", onScroll, true);
      window.clearInterval(id);
    };
  }, [activeStep, status]);

  useEffect(() => {
    if (status !== "running") return;
    // Focus the tooltip for keyboard usage.
    const t = window.setTimeout(() => tooltipRef.current?.focus(), 0);
    return () => window.clearTimeout(t);
  }, [status, stepIndex]);

  if (status !== "running" || !activeTour || !activeStep) return null;

  const placement = pickPlacement(targetRect ?? { left: 0, top: 0, width: 0, height: 0, right: 0, bottom: 0 }, activeStep.placement ?? "auto");

  const tooltipPos = targetRect
    ? computeTooltipPosition(targetRect, placement)
    : { left: window.innerWidth / 2, top: window.innerHeight / 2, transform: "translate(-50%, -50%)" };

  const progressLabel = `${clamp(stepIndex + 1, 1, activeTour.steps.length)}/${activeTour.steps.length}`;

  const pad = 6;
  const spot = targetRect
    ? {
        left: Math.max(0, targetRect.left - pad),
        top: Math.max(0, targetRect.top - pad),
        width: Math.max(0, targetRect.width + pad * 2),
        height: Math.max(0, targetRect.height + pad * 2),
      }
    : null;
  const spotRight = spot ? spot.left + spot.width : 0;
  const spotBottom = spot ? spot.top + spot.height : 0;

  return createPortal(
    <div className="tour-root" role="dialog" aria-modal="true">
      {spot ? (
        <>
          <div className="tour-dim-pane" onClick={exit} style={{ left: 0, top: 0, right: 0, height: `${spot.top}px` }} />
          <div
            className="tour-dim-pane"
            onClick={exit}
            style={{ left: 0, top: `${spot.top}px`, width: `${spot.left}px`, height: `${spot.height}px` }}
          />
          <div
            className="tour-dim-pane"
            onClick={exit}
            style={{ left: `${spotRight}px`, top: `${spot.top}px`, right: 0, height: `${spot.height}px` }}
          />
          <div className="tour-dim-pane" onClick={exit} style={{ left: 0, top: `${spotBottom}px`, right: 0, bottom: 0 }} />
        </>
      ) : (
        <div className="tour-dim" onClick={exit} />
      )}

      {spot && (
        <div
          className="tour-highlight"
          style={{
            left: `${spot.left}px`,
            top: `${spot.top}px`,
            width: `${spot.width}px`,
            height: `${spot.height}px`,
          }}
        />
      )}

      <div
        className="tour-tooltip"
        style={{ left: `${tooltipPos.left}px`, top: `${tooltipPos.top}px`, transform: tooltipPos.transform }}
        tabIndex={-1}
        ref={tooltipRef}
      >
        <div className="tour-tooltip-header">
          <div className="tour-tooltip-title">{activeStep.title}</div>
          <div className="tour-tooltip-meta">
            <span className="tour-tooltip-progress">{progressLabel}</span>
            <button className="tour-icon-btn" onClick={exit} aria-label="Close tour" title="Close">
              ×
            </button>
          </div>
        </div>

        <div className="tour-tooltip-body">{activeStep.body}</div>

        <div className="tour-tooltip-actions">
          <button className="tour-btn ghost" onClick={exit}>
            Skip
          </button>
          <div className="tour-spacer" />
          <button className="tour-btn ghost" onClick={prev} disabled={stepIndex <= 0}>
            Back
          </button>
          <button className="tour-btn primary" onClick={stepIndex >= activeTour.steps.length - 1 ? complete : next}>
            {stepIndex >= activeTour.steps.length - 1 ? "Done" : "Next"}
          </button>
        </div>

        {!targetEl && (
          <div className="tour-tooltip-note">
            This step’s UI isn’t visible right now. You can press Next to continue.
          </div>
        )}
      </div>
    </div>,
    document.body
  );
}
