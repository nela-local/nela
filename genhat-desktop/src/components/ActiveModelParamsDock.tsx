import React, { useEffect, useMemo, useState } from "react";
import { CheckCircle, Cpu, Loader2, Save, SlidersHorizontal, X } from "lucide-react";
import { Api, type CompatibilityRating } from "../api";
import { KITTEN_TTS_VOICES } from "../types";

export interface RuntimeParamsTarget {
  key: string;
  identifier: string;
  displayName: string;
  backend?: string;
  modelFile?: string;
  memoryMb?: number;
  params: Record<string, string>;
  isRegistered: boolean;
}

interface ActiveModelParamsDockProps {
  target: RuntimeParamsTarget;
  onApply: (params: Record<string, string>) => Promise<void>;
  onClose?: () => void;
}

type ParamControl = {
  key: string;
  label: string;
  description: string;
  type: "slider" | "select";
  min?: number;
  max?: number;
  step?: number;
  defaultValue: string;
  options?: Array<{ value: string; label: string }>;
  valueFormatter?: (value: number) => string;
  showContextHint?: boolean;
};

const BOOLEAN_OPTIONS = [
  { value: "true", label: "Enabled" },
  { value: "false", label: "Disabled" },
];

const OPTIMAL_CONTEXT_RATINGS = new Set<CompatibilityRating>([
  "efficient",
  "usable",
  "satisfies",
]);

const parseNumber = (raw: string | undefined, fallback: number): number => {
  if (!raw) return fallback;
  const parsed = Number.parseFloat(raw);
  return Number.isFinite(parsed) ? parsed : fallback;
};

const clamp = (value: number, min: number, max: number): number => {
  if (value < min) return min;
  if (value > max) return max;
  return value;
};

const getBackendLabel = (backend: string | undefined): string => {
  switch (backend) {
    case "LlamaServer":
      return "Llama";
    case "KittenTts":
      return "KittenTTS";
    case "Parakeet":
      return "Parakeet";
    default:
      return backend ?? "Unknown";
  }
};

const getParamControls = (backend: string | undefined): ParamControl[] => {
  switch (backend) {
    case "LlamaServer":
      return [
        {
          key: "ctx_size",
          label: "Context Size",
          description: "Maximum prompt window in tokens. Larger values use more RAM.",
          type: "slider",
          min: 512,
          max: 32768,
          step: 512,
          defaultValue: "4096",
          valueFormatter: (v) => `${Math.round(v)} tokens`,
          showContextHint: true,
        },
        {
          key: "max_tokens",
          label: "Max Output Tokens",
          description: "Upper bound on response length per generation.",
          type: "slider",
          min: 64,
          max: 8192,
          step: 64,
          defaultValue: "2048",
          valueFormatter: (v) => `${Math.round(v)}`,
        },
        {
          key: "temp",
          label: "Temperature",
          description: "Higher values increase creativity and randomness.",
          type: "slider",
          min: 0,
          max: 2,
          step: 0.05,
          defaultValue: "0.7",
          valueFormatter: (v) => v.toFixed(2),
        },
        {
          key: "top_p",
          label: "Top P",
          description: "Nucleus sampling cutoff. Lower values make outputs more focused.",
          type: "slider",
          min: 0.05,
          max: 1,
          step: 0.01,
          defaultValue: "0.9",
          valueFormatter: (v) => v.toFixed(2),
        },
        {
          key: "top_k",
          label: "Top K",
          description: "Limits sampling to the K most likely next tokens.",
          type: "slider",
          min: 1,
          max: 200,
          step: 1,
          defaultValue: "40",
          valueFormatter: (v) => `${Math.round(v)}`,
        },
        {
          key: "repeat_penalty",
          label: "Repeat Penalty",
          description: "Discourages repeated phrases in long outputs.",
          type: "slider",
          min: 0.8,
          max: 2,
          step: 0.01,
          defaultValue: "1.1",
          valueFormatter: (v) => v.toFixed(2),
        },
        {
          key: "flash_attn",
          label: "Flash Attention",
          description: "Enables fast attention kernels when supported by the device.",
          type: "select",
          defaultValue: "false",
          options: BOOLEAN_OPTIONS,
        },
        {
          key: "mlock",
          label: "Memory Lock",
          description: "Attempts to keep model pages resident in RAM to reduce paging.",
          type: "select",
          defaultValue: "false",
          options: BOOLEAN_OPTIONS,
        },
      ];

    case "KittenTts":
      return [
        {
          key: "voice",
          label: "Default Voice",
          description: "Default speaker voice used for speech generation.",
          type: "select",
          defaultValue: "Leo",
          options: KITTEN_TTS_VOICES.map((voice) => ({ value: voice, label: voice })),
        },
        {
          key: "speed",
          label: "Default Speed",
          description: "Playback speed multiplier for generated speech.",
          type: "slider",
          min: 0.5,
          max: 2,
          step: 0.1,
          defaultValue: "1.0",
          valueFormatter: (v) => `${v.toFixed(1)}x`,
        },
      ];

    case "Parakeet":
      return [
        {
          key: "max_symbols_per_step",
          label: "Max Symbols / Step",
          description: "Decoder safety cap per frame. Higher can improve recall but is slower.",
          type: "slider",
          min: 1,
          max: 20,
          step: 1,
          defaultValue: "10",
          valueFormatter: (v) => `${Math.round(v)}`,
        },
        {
          key: "preemphasis",
          label: "Preemphasis",
          description: "High-frequency boost before mel features. Keep near 0.97 unless tuning.",
          type: "slider",
          min: 0,
          max: 1,
          step: 0.01,
          defaultValue: "0.97",
          valueFormatter: (v) => v.toFixed(2),
        },
        {
          key: "dither",
          label: "Dither",
          description: "Small input noise for numerical stability in quiet audio.",
          type: "slider",
          min: 0,
          max: 0.001,
          step: 0.00001,
          defaultValue: "0.00001",
          valueFormatter: (v) => v.toFixed(5),
        },
      ];

    default:
      return [];
  }
};

const ActiveModelParamsDock: React.FC<ActiveModelParamsDockProps> = ({ target, onApply, onClose }) => {
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [contextHint, setContextHint] = useState<{ rating: CompatibilityRating; reason: string } | null>(null);
  const [contextHintLoading, setContextHintLoading] = useState(false);

  const controls = useMemo(() => getParamControls(target.backend), [target.backend]);

  useEffect(() => {
    setDraft({ ...(target.params ?? {}) });
    setSaved(false);
    setError(null);
  }, [target.key, target.params]);

  useEffect(() => {
    if (target.backend !== "LlamaServer") {
      setContextHint(null);
      setContextHintLoading(false);
      return;
    }

    const rawContext = draft.ctx_size ?? target.params?.ctx_size ?? "4096";
    const contextLength = Number.parseInt(rawContext, 10);
    if (!Number.isFinite(contextLength) || contextLength <= 0) {
      setContextHint(null);
      setContextHintLoading(false);
      return;
    }

    let cancelled = false;
    const timer = window.setTimeout(async () => {
      setContextHintLoading(true);
      try {
        const approxSizeMb = Math.max(target.memoryMb ?? 1400, 256);
        const compatibility = await Api.checkCompatibility(
          approxSizeMb,
          target.memoryMb,
          undefined,
          target.modelFile,
          contextLength
        );
        if (!cancelled) {
          setContextHint({ rating: compatibility.rating, reason: compatibility.reason });
        }
      } catch (err) {
        if (!cancelled) {
          console.warn("Failed to check context hint", err);
          setContextHint(null);
        }
      } finally {
        if (!cancelled) {
          setContextHintLoading(false);
        }
      }
    }, 220);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [target.backend, target.memoryMb, target.modelFile, target.params?.ctx_size, draft.ctx_size]);

  const setValue = (key: string, value: string) => {
    setDraft((prev) => ({ ...prev, [key]: value }));
    setSaved(false);
    setError(null);
  };

  const getValue = (control: ParamControl): string => {
    return draft[control.key] ?? target.params?.[control.key] ?? control.defaultValue;
  };

  const handleApply = async () => {
    setSaving(true);
    setError(null);
    try {
      const merged = {
        ...(target.params ?? {}),
        ...draft,
      };
      const cleaned = Object.fromEntries(
        Object.entries(merged).filter(([, value]) => value !== "")
      );

      await onApply(cleaned);
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1800);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  const contextOptimal = contextHint ? OPTIMAL_CONTEXT_RATINGS.has(contextHint.rating) : null;

  return (
    <div className="h-full w-full bg-void-800 flex flex-col border-l border-glass-border">
      <div className="flex items-center justify-between py-3.5 px-4 border-b border-glass-border shrink-0">
        <div className="flex items-center gap-2 text-[0.85rem] font-semibold text-txt">
          <SlidersHorizontal size={15} />
          Model Parameters
        </div>
        {onClose && (
          <button
            className="glass-btn bg-transparent! border border-transparent! text-txt-muted! cursor-pointer p-1.5! rounded-lg! flex items-center justify-center transition-all duration-200 hover:text-txt! hover:border-glass-border! hover:bg-void-700!"
            onClick={onClose}
            title="Close parameter panel"
          >
            <X size={15} />
          </button>
        )}
      </div>

      <div className="runtime-params-scroll flex-1 overflow-y-auto p-3 flex flex-col gap-3">
        <div className="rounded-xl border border-glass-border bg-void-700/60 px-3 py-2">
          <div className="text-[0.78rem] font-semibold text-txt truncate" title={target.displayName}>
            {target.displayName}
          </div>
          <div className="text-[0.68rem] text-txt-muted mt-1">
            {getBackendLabel(target.backend)} backend {target.isRegistered ? "(registered)" : "(detected from file)"}
          </div>
        </div>

        {controls.length === 0 && (
          <div className="text-[0.76rem] text-txt-muted border border-dashed border-glass-border rounded-lg px-3 py-2">
            This model backend has no runtime parameters exposed yet.
          </div>
        )}

        {controls.map((control) => {
          const rawValue = getValue(control);

          if (control.type === "select") {
            const options = control.options ?? [];
            const hasCurrent = options.some((option) => option.value === rawValue);
            return (
              <div key={control.key} className="rounded-lg border border-glass-border bg-void-700/55 p-2.5 flex flex-col gap-1.5">
                <label htmlFor={`runtime-param-${target.key}-${control.key}`} className="text-[0.74rem] font-semibold text-txt">
                  {control.label}
                </label>
                <select
                  id={`runtime-param-${target.key}-${control.key}`}
                  className="bg-void-800 border border-glass-border rounded-md px-2.5 py-1.5 text-[0.76rem] text-txt"
                  value={rawValue}
                  onChange={(e) => setValue(control.key, e.target.value)}
                >
                  {!hasCurrent && <option value={rawValue}>{rawValue}</option>}
                  {options.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
                <span className="text-[0.67rem] text-txt-muted">{control.description}</span>
              </div>
            );
          }

          const min = control.min ?? 0;
          const max = control.max ?? 1;
          const step = control.step ?? 0.1;
          const numericValue = clamp(parseNumber(rawValue, parseNumber(control.defaultValue, min)), min, max);
          const displayValue = control.valueFormatter ? control.valueFormatter(numericValue) : String(numericValue);

          return (
            <div key={control.key} className="rounded-lg border border-glass-border bg-void-700/55 p-2.5 flex flex-col gap-1.5">
              <div className="flex items-center justify-between gap-2">
                <label htmlFor={`runtime-param-${target.key}-${control.key}`} className="text-[0.74rem] font-semibold text-txt">
                  {control.label}
                </label>
                <span className="text-[0.7rem] text-[#8ae8ff]">{displayValue}</span>
              </div>
              <input
                id={`runtime-param-${target.key}-${control.key}`}
                type="range"
                min={min}
                max={max}
                step={step}
                value={numericValue}
                onChange={(e) => setValue(control.key, e.target.value)}
                className="w-full h-1 accent-neon cursor-pointer"
              />
              <span className="text-[0.67rem] text-txt-muted">{control.description}</span>

              {control.showContextHint && (
                <div
                  className={`mt-1 rounded-md border px-2 py-1.5 text-[0.67rem] flex items-start gap-1.5 ${
                    contextOptimal === null
                      ? "border-glass-border bg-white/5 text-txt-muted"
                      : contextOptimal
                        ? "border-green-400/40 bg-green-500/10 text-green-300"
                        : "border-amber-300/35 bg-amber-400/10 text-amber-200"
                  }`}
                >
                  <Cpu size={12} className="mt-[1px] shrink-0" />
                  <span>
                    {contextHintLoading && "Checking device fit for this context size..."}
                    {!contextHintLoading && contextHint && (
                      contextOptimal
                        ? `Optimal for this device: ${contextHint.reason}`
                        : `Sub-optimal for this device: ${contextHint.reason}`
                    )}
                    {!contextHintLoading && !contextHint && "Device-fit hint unavailable for current value."}
                  </span>
                </div>
              )}
            </div>
          );
        })}

        {!target.isRegistered && (
          <div className="text-[0.67rem] text-[#8db5c9] bg-neon/10 border border-neon/20 rounded-md px-2.5 py-2">
            This model was detected from a local file path. Applying parameters will auto-bind it when possible.
          </div>
        )}

        {error && (
          <div className="text-[0.67rem] text-[#ffb9b9] bg-red-500/10 border border-red-400/25 rounded-md px-2.5 py-2">
            Failed to apply: {error}
          </div>
        )}

        {controls.length > 0 && (
          <div className="pt-1">
            <button
              className="w-full bg-gradient-to-r from-cyan-400 to-sky-300 text-void-900 font-semibold text-[0.76rem] rounded-md px-3 py-2 inline-flex items-center justify-center gap-1.5 disabled:opacity-50 disabled:cursor-not-allowed"
              onClick={() => void handleApply()}
              disabled={saving}
            >
              {saving ? (
                <Loader2 size={13} className="animate-spin" />
              ) : saved ? (
                <CheckCircle size={13} />
              ) : (
                <Save size={13} />
              )}
              <span>{saved ? "Applied" : "Apply Parameters"}</span>
            </button>
          </div>
        )}
      </div>
    </div>
  );
};

export default ActiveModelParamsDock;