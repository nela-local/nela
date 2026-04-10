import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

export type TourPlacement = "auto" | "top" | "bottom" | "left" | "right";

export type TourTarget = string | (() => HTMLElement | null);

export type TourBindings = Record<string, unknown>;

export type TourStep = {
  id: string;
  title: string;
  body: React.ReactNode;
  target: TourTarget;
  placement?: TourPlacement;
  isAvailable?: (bindings: TourBindings) => boolean;
  onBeforeStep?: (bindings: TourBindings) => void | Promise<void>;
};

export type TourDefinition = {
  id: string;
  name: string;
  version: number;
  steps: TourStep[];
};

type TourRunSource = "startup" | "help" | "unknown";

type StartTourOptions = {
  source?: TourRunSource;
  onExit?: () => void;
  onComplete?: () => void;
};

type TourStatus = "idle" | "running";

type TourContextValue = {
  tours: TourDefinition[];
  status: TourStatus;
  activeTourId: string | null;
  activeTour: TourDefinition | null;
  stepIndex: number;
  activeStep: TourStep | null;
  startTour: (tourId: string, options?: StartTourOptions) => void;
  next: () => void;
  prev: () => void;
  exit: () => void;
  complete: () => void;
  completedTourIds: Set<string>;
  isTourCompleted: (tourId: string) => boolean;
  resetTourProgress: () => void;
  setBindings: (bindings: TourBindings) => void;
  bindings: TourBindings;
  runSource: TourRunSource;
};

const TOUR_COMPLETED_KEY = "genhat:tours:v1:completed";

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function writeJson(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // ignore
  }
}

function resolveTarget(target: TourTarget): HTMLElement | null {
  if (typeof target === "string") return document.querySelector(target) as HTMLElement | null;
  try {
    return target();
  } catch {
    return null;
  }
}

const TourContext = createContext<TourContextValue | null>(null);

export function TourProvider({
  tours,
  children,
}: {
  tours: TourDefinition[];
  children: React.ReactNode;
}) {
  const [status, setStatus] = useState<TourStatus>("idle");
  const [activeTourId, setActiveTourId] = useState<string | null>(null);
  const [stepIndex, setStepIndex] = useState(0);
  const [bindings, setBindingsState] = useState<TourBindings>({});
  const [runSource, setRunSource] = useState<TourRunSource>("unknown");

  const callbacksRef = useRef<{ onExit?: () => void; onComplete?: () => void } | null>(null);

  const [completedTourIds, setCompletedTourIds] = useState<Set<string>>(() => {
    const ids = readJson<string[]>(TOUR_COMPLETED_KEY, []);
    return new Set(ids);
  });

  const activeTour = useMemo(
    () => (activeTourId ? tours.find((t) => t.id === activeTourId) ?? null : null),
    [activeTourId, tours]
  );

  const activeStep = useMemo(() => {
    if (!activeTour) return null;
    return activeTour.steps[stepIndex] ?? null;
  }, [activeTour, stepIndex]);

  const isStepAvailable = useCallback(
    (tour: TourDefinition, idx: number): boolean => {
      const step = tour.steps[idx];
      if (!step) return false;
      if (step.isAvailable && !step.isAvailable(bindings)) return false;
      return !!resolveTarget(step.target);
    },
    [bindings]
  );

  const findNextAvailableIndex = useCallback(
    (tour: TourDefinition, startIdx: number, direction: 1 | -1): number | null => {
      let i = startIdx;
      while (i >= 0 && i < tour.steps.length) {
        if (isStepAvailable(tour, i)) return i;
        i += direction;
      }
      return null;
    },
    [isStepAvailable]
  );

  const exit = useCallback(() => {
    setStatus("idle");
    setActiveTourId(null);
    setStepIndex(0);
    const cb = callbacksRef.current;
    callbacksRef.current = null;
    try {
      cb?.onExit?.();
    } catch {
      // ignore
    }
  }, []);

  const complete = useCallback(() => {
    if (activeTourId) {
      setCompletedTourIds((prev) => {
        const next = new Set(prev);
        next.add(activeTourId);
        writeJson(TOUR_COMPLETED_KEY, Array.from(next));
        return next;
      });
    }

    setStatus("idle");
    setActiveTourId(null);
    setStepIndex(0);

    const cb = callbacksRef.current;
    callbacksRef.current = null;
    try {
      cb?.onComplete?.();
    } catch {
      // ignore
    }
  }, [activeTourId]);

  const goToIndex = useCallback(
    async (idx: number) => {
      const tour = activeTour;
      if (!tour) return;

      const nextIdx = findNextAvailableIndex(tour, idx, idx >= stepIndex ? 1 : -1);
      if (nextIdx === null) {
        // No more available steps in that direction.
        if (idx > stepIndex) {
          // advancing past end → complete
          complete();
        } else {
          // going back past start → just clamp
          setStepIndex(0);
        }
        return;
      }

      const step = tour.steps[nextIdx];
      if (step?.onBeforeStep) {
        try {
          await step.onBeforeStep(bindings);
        } catch {
          // ignore
        }
      }

      setStepIndex(nextIdx);
    },
    [activeTour, bindings, complete, findNextAvailableIndex, stepIndex]
  );

  const startTour = useCallback(
    (tourId: string, options?: StartTourOptions) => {
      const tour = tours.find((t) => t.id === tourId);
      if (!tour || tour.steps.length === 0) return;

      callbacksRef.current = { onExit: options?.onExit, onComplete: options?.onComplete };
      setRunSource(options?.source ?? "unknown");
      setActiveTourId(tourId);
      setStatus("running");

      // pick first available step
      const firstIdx = findNextAvailableIndex(tour, 0, 1);
      setStepIndex(firstIdx ?? 0);
    },
    [tours, findNextAvailableIndex]
  );

  const next = useCallback(() => {
    if (!activeTour) return;
    void goToIndex(stepIndex + 1);
  }, [activeTour, goToIndex, stepIndex]);

  const prev = useCallback(() => {
    if (!activeTour) return;
    void goToIndex(stepIndex - 1);
  }, [activeTour, goToIndex, stepIndex]);

  const isTourCompleted = useCallback((tourId: string) => completedTourIds.has(tourId), [completedTourIds]);

  const resetTourProgress = useCallback(() => {
    setCompletedTourIds(new Set());
    writeJson(TOUR_COMPLETED_KEY, []);
  }, []);

  const setBindings = useCallback((next: TourBindings) => {
    setBindingsState(next ?? {});
  }, []);

  // Escape to exit
  useEffect(() => {
    if (status !== "running") return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") exit();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [exit, status]);

  const value = useMemo<TourContextValue>(
    () => ({
      tours,
      status,
      activeTourId,
      activeTour,
      stepIndex,
      activeStep,
      startTour,
      next,
      prev,
      exit,
      complete,
      completedTourIds,
      isTourCompleted,
      resetTourProgress,
      setBindings,
      bindings,
      runSource,
    }),
    [
      tours,
      status,
      activeTourId,
      activeTour,
      stepIndex,
      activeStep,
      startTour,
      next,
      prev,
      exit,
      complete,
      completedTourIds,
      isTourCompleted,
      resetTourProgress,
      setBindings,
      bindings,
      runSource,
    ]
  );

  return <TourContext.Provider value={value}>{children}</TourContext.Provider>;
}

export function useTour(): TourContextValue {
  const ctx = useContext(TourContext);
  if (!ctx) throw new Error("useTour must be used within <TourProvider>");
  return ctx;
}

export const TourUtils = {
  resolveTarget,
};
