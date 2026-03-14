import { useEffect, useRef, useState, useCallback } from "react";

/* ─── Types ─────────────────────────────────────────────────────────────────── */

interface AudioPlayerProps {
  /** data:audio/wav;base64,… URL or blob URL pointing to a WAV file */
  src: string;
  /** If true, start playing immediately on mount */
  autoPlay?: boolean;
  barCount?: number; // Optional: override number of bars in waveform
}

/* ─── Constants ─────────────────────────────────────────────────────────────── */

// Default bar count, can be overridden by prop
const DEFAULT_BAR_COUNT = 48;
const BAR_WIDTH = 3;
const BAR_GAP = 1.5;
const BAR_MIN_H = 2;
const BAR_MAX_H = 32;
const BAR_RADIUS = 1.5;
const CANVAS_H = 40;
const SMOOTHING = 0.6;
/** How fast bars rise toward target */
const LERP_UP = 0.2;
/** How fast bars fall back (slower for a natural tail) */
const LERP_DOWN = 0.08;

/* ─── Component ─────────────────────────────────────────────────────────────── */

import React from "react";

const AudioPlayer: React.FC<AudioPlayerProps> = function AudioPlayer({ src, autoPlay = false, barCount }) {
  const BAR_COUNT = barCount ?? DEFAULT_BAR_COUNT;
  const CANVAS_W = (BAR_WIDTH + BAR_GAP) * BAR_COUNT;
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animRef = useRef(0);

  // Web Audio nodes — persisted across renders, never trigger re-render
  const ctxRef = useRef<AudioContext | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const sourceRef = useRef<MediaElementAudioSourceNode | null>(null);
  const connectedRef = useRef(false);

  /** Per-bar smoothed values (0..1) for silky interpolation */
  const smoothedRef = useRef<Float32Array>(new Float32Array(BAR_COUNT));
  /** Random mapping of bar index to frequency bin index */
  const barMapRef = useRef<number[]>([]);

  // ...existing code...


  const [playing, setPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);



  // Shuffle mapping on each playback
  useEffect(() => {
    if (playing) {
      // Generate a random, smooth mapping (neighboring bars get neighboring bins, but start is random)
      const makeRandomBarMap = () => {
        const bins = Array.from({ length: BAR_COUNT }, (_, i) => i);
        // Pick a random start offset
        const offset = Math.floor(Math.random() * BAR_COUNT);
        // Optionally reverse
        const reverse = Math.random() > 0.5;
        const arr = bins.slice(offset).concat(bins.slice(0, offset));
        return reverse ? arr.reverse() : arr;
      };
      barMapRef.current = makeRandomBarMap();
    }
  }, [playing, BAR_COUNT]);

  /* ── Connect Web Audio analyser (once per <audio> element) ─────────────── */
  const ensureAnalyser = useCallback(() => {
    if (connectedRef.current) return;
    const audio = audioRef.current;
    if (!audio) return;

    const actx = ctxRef.current ?? new AudioContext();
    ctxRef.current = actx;

    const analyser = actx.createAnalyser();
    analyser.fftSize = 256;
    analyser.smoothingTimeConstant = SMOOTHING;
    analyser.minDecibels = -90;
    analyser.maxDecibels = -10;
    analyserRef.current = analyser;

    const source = actx.createMediaElementSource(audio);
    source.connect(analyser);
    analyser.connect(actx.destination);
    sourceRef.current = source;
    connectedRef.current = true;
  }, []);

  /* ── Canvas draw loop ──────────────────────────────────────────────────── */
  const drawBars = useCallback(() => {
    const canvas = canvasRef.current;
    const analyser = analyserRef.current;
    if (!canvas || !analyser) return;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const w = CANVAS_W;
    const h = CANVAS_H;

    // Ensure canvas buffer matches logical size × dpr (set once)
    if (canvas.width !== Math.round(w * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }

    const freqData = new Uint8Array(analyser.frequencyBinCount);
    analyser.getByteFrequencyData(freqData);

    // Also grab time-domain data for extra responsiveness
    const timeData = new Uint8Array(analyser.frequencyBinCount);
    analyser.getByteTimeDomainData(timeData);

    ctx.clearRect(0, 0, w, h);

  const smoothed = smoothedRef.current;

    // Random smooth mapping: bar i gets bin barMapRef.current[i]
    const barMap = barMapRef.current.length === BAR_COUNT
      ? barMapRef.current
      : Array.from({ length: BAR_COUNT }, (_, i) => i);

    // 1. Compute raw target values for each bar
    const rawTargets = new Float32Array(BAR_COUNT);
    for (let i = 0; i < BAR_COUNT; i++) {
      const freqIdx = Math.floor(barMap[i] / BAR_COUNT * (freqData.length / 2 - 1));
      let fSum = 0, count = 0;
      for (let j = -1; j <= 1; j++) {
        const idx = freqIdx + j;
        if (idx >= 0 && idx < freqData.length / 2) {
          fSum += freqData[idx];
          count++;
        }
      }
      const fAvg = (fSum / Math.max(1, count)) / 255;
      let tMax = 0;
      for (let j = -1; j <= 1; j++) {
        const idx = freqIdx + j;
        if (idx >= 0 && idx < timeData.length / 2) {
          const dev = Math.abs((timeData[idx] || 128) - 128) / 128;
          if (dev > tMax) tMax = dev;
        }
      }
      rawTargets[i] = Math.min(Math.max(fAvg, tMax) * 2.5, 1);
    }

    // 2. Apply a moving average smoothing filter to the targets
    const smoothTargets = new Float32Array(BAR_COUNT);
    for (let i = 0; i < BAR_COUNT; i++) {
      let sum = 0, wsum = 0;
      for (let k = -2; k <= 2; k++) {
        const idx = i + k;
        if (idx >= 0 && idx < BAR_COUNT) {
          // Triangular weights: 1,2,3,2,1
          const weight = 3 - Math.abs(k);
          sum += rawTargets[idx] * weight;
          wsum += weight;
        }
      }
      smoothTargets[i] = sum / wsum;
    }

    // 3. Lerp and render
    for (let i = 0; i < BAR_COUNT; i++) {
      const target = smoothTargets[i];
      const prev = smoothed[i];
      smoothed[i] = target > prev
        ? prev + (target - prev) * LERP_UP
        : prev + (target - prev) * LERP_DOWN;

      const val = smoothed[i];
      const barH = BAR_MIN_H + val * (BAR_MAX_H - BAR_MIN_H);
      const x = i * (BAR_WIDTH + BAR_GAP);
      const y = h - barH;

      const grad = ctx.createLinearGradient(0, y, 0, h);
      grad.addColorStop(0, `rgba(0, 220, 255, ${0.5 + val * 0.5})`);
      grad.addColorStop(1, `rgba(0, 140, 200, ${0.15 + val * 0.3})`);
      ctx.fillStyle = grad;
      ctx.beginPath();
      ctx.roundRect(x, y, BAR_WIDTH, barH, [BAR_RADIUS, BAR_RADIUS, 0, 0]);
      ctx.fill();
    }

    animRef.current = requestAnimationFrame(drawBars);
  }, [BAR_COUNT, CANVAS_W]);
  /** Idle bar heights — randomised once, referenced in drawIdleBars */
  const idleHeightsRef = useRef<number[]>(
    Array.from({ length: BAR_COUNT }, (_, i) => {
      // Gentle arch shape: taller in the middle, shorter at the edges
      const t = i / (BAR_COUNT - 1); // 0..1
      const arch = Math.sin(t * Math.PI); // 0 → 1 → 0
      return BAR_MIN_H + arch * 8 + (Math.random() - 0.5) * 3;
    })
  );
  /* ── Idle bars (when paused/stopped) ───────────────────────────────────── */
  const drawIdleBars = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const w = CANVAS_W;
    const h = CANVAS_H;

    if (canvas.width !== Math.round(w * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }

    ctx.clearRect(0, 0, w, h);

    for (let i = 0; i < BAR_COUNT; i++) {
      const x = i * (BAR_WIDTH + BAR_GAP);
      const barH = idleHeightsRef.current[i];
      const y = h - barH;
      ctx.fillStyle = "rgba(0, 212, 255, 0.18)";
      ctx.beginPath();
      ctx.roundRect(x, y, BAR_WIDTH, barH, [BAR_RADIUS, BAR_RADIUS, 0, 0]);
      ctx.fill();
    }
  }, [BAR_COUNT, CANVAS_W]);

  /* ── Play / Pause ──────────────────────────────────────────────────────── */
  const togglePlay = useCallback(async () => {
    const audio = audioRef.current;
    if (!audio) return;

    ensureAnalyser();

    // Resume AudioContext if it was suspended (browser autoplay policy)
    if (ctxRef.current?.state === "suspended") {
      await ctxRef.current.resume();
    }

    if (audio.paused) {
      await audio.play();
      setPlaying(true);
      animRef.current = requestAnimationFrame(drawBars);
    } else {
      audio.pause();
      setPlaying(false);
      cancelAnimationFrame(animRef.current);
      drawIdleBars();
    }
  }, [ensureAnalyser, drawBars, drawIdleBars]);

  /* ── Time formatting ───────────────────────────────────────────────────── */
  function fmt(t: number) {
    if (!isFinite(t) || t < 0) return "0:00";
    const m = Math.floor(t / 60);
    const s = Math.floor(t % 60);
    return `${m}:${s.toString().padStart(2, "0")}`;
  }

  /* ── Lifecycle ─────────────────────────────────────────────────────────── */
  useEffect(() => {
    const audio = new Audio();
    audio.crossOrigin = "anonymous";
    audio.preload = "auto";
    audio.src = src;
    audioRef.current = audio;

    const onMeta = () => setDuration(audio.duration);
    const onTime = () => setCurrentTime(audio.currentTime);
    const onEnded = () => {
      setPlaying(false);
      cancelAnimationFrame(animRef.current);
      drawIdleBars();
    };

    audio.addEventListener("loadedmetadata", onMeta);
    audio.addEventListener("timeupdate", onTime);
    audio.addEventListener("ended", onEnded);

    // Draw idle bars immediately
    drawIdleBars();

    if (autoPlay) {
      // Small delay to let audio decode before playing
      const t = setTimeout(() => togglePlay(), 120);
      return () => {
        clearTimeout(t);
        cleanup();
      };
    }

    function cleanup() {
      cancelAnimationFrame(animRef.current);
      audio.removeEventListener("loadedmetadata", onMeta);
      audio.removeEventListener("timeupdate", onTime);
      audio.removeEventListener("ended", onEnded);
      audio.pause();
      audio.src = "";
      // Disconnect Web Audio nodes
      if (sourceRef.current) {
        try { sourceRef.current.disconnect(); } catch { /* already disconnected */ }
      }
      if (analyserRef.current) {
        try { analyserRef.current.disconnect(); } catch { /* already disconnected */ }
      }
      if (ctxRef.current && ctxRef.current.state !== "closed") {
        ctxRef.current.close();
      }
      connectedRef.current = false;
      ctxRef.current = null;
      analyserRef.current = null;
      sourceRef.current = null;
    }

    return cleanup;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [src]);

  /* ── Seek via progress bar click ───────────────────────────────────────── */
  const handleSeek = (e: React.MouseEvent<HTMLDivElement>) => {
    const audio = audioRef.current;
    if (!audio || !duration) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const ratio = Math.min(Math.max((e.clientX - rect.left) / rect.width, 0), 1);
    audio.currentTime = ratio * duration;
  };

  const progress = duration > 0 ? (currentTime / duration) * 100 : 0;

  /* ── Render ────────────────────────────────────────────────────────────── */
  return (
    <div className="flex flex-col gap-2 px-3.5 py-2.5 rounded-xl bg-white/[0.03] border border-white/[0.07] select-none max-w-md">
      {/* Top row: Play button + Waveform */}
      <div className="flex items-center gap-3">
        {/* Play / Pause button */}
        <button
          onClick={togglePlay}
          className="shrink-0 w-12 h-12 flex items-center justify-center rounded-full
            bg-neon/15 text-neon border border-neon/25
            hover:bg-neon/25 hover:border-neon/40
            transition-all duration-200 cursor-pointer"
          aria-label={playing ? "Pause" : "Play"}
        >
          {playing ? (
            /* Pause icon */
            <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor">
              <rect x="6" y="4" width="4" height="16" rx="1" />
              <rect x="14" y="4" width="4" height="16" rx="1" />
            </svg>
          ) : (
            /* Play icon */
            <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor">
              <path d="M8 5.14v13.72a1 1 0 001.5.86l11.04-6.86a1 1 0 000-1.72L9.5 4.28A1 1 0 008 5.14z" />
            </svg>
          )}
        </button>

        {/* Waveform bars */}
        <canvas
          ref={canvasRef}
          style={{ width: CANVAS_W, height: CANVAS_H, marginLeft: 10 }}
          className="shrink-0"
        />
      </div>

      {/* Time display */}
      <span className="text-[0.65rem] text-txt-muted tabular-nums leading-none whitespace-nowrap">
        {fmt(currentTime)}{" / "}{fmt(duration)}
      </span>

      {/* Seek bar (full width) */}
      <div
        className="h-1.5 rounded-full bg-white/[0.06] cursor-pointer overflow-hidden w-full"
        onClick={handleSeek}
      >
        <div
          className="h-full rounded-full bg-neon/50 transition-[width] duration-150"
          style={{ width: `${progress}%` }}
        />
      </div>
    </div>
  );
};

export default React.memo(AudioPlayer);
