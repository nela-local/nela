import { useState, useRef, useEffect, type ElementType } from "react";
import { MessageSquare, Eye, Volume2, Mic, FileText, Share2, ChevronDown } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { Api } from "../api";
import type {
  PodcastRequest,
  PodcastResult,
  PodcastProgress,
  KittenTtsVoice,
  ChatMode,
} from "../types";
import { KITTEN_TTS_VOICES } from "../types";

const MODE_ICON_MAP: Record<ChatMode, ElementType> = {
  text: MessageSquare,
  vision: Eye,
  audio: Volume2,
  rag: FileText,
  podcast: Mic,
  mindmap: Share2,
};

interface PodcastTabProps {
  /** Whether the knowledge base has any ingested documents */
  hasDocuments: boolean;
  modeOptions: { mode: ChatMode; label: string }[];
  currentMode: ChatMode;
  onSelectMode: (mode: ChatMode) => void;
}

const PodcastTab: React.FC<PodcastTabProps> = ({
  hasDocuments,
  modeOptions,
  currentMode,
  onSelectMode,
}) => {
  const [query, setQuery] = useState("");
  const [voiceA, setVoiceA] = useState<KittenTtsVoice>("Leo");
  const [voiceB, setVoiceB] = useState<KittenTtsVoice>("Bella");
  const [speakerA, setSpeakerA] = useState("Alex");
  const [speakerB, setSpeakerB] = useState("Sam");
  const [maxTurns, setMaxTurns] = useState(10);
  const [isGenerating, setIsGenerating] = useState(false);
  const [progress, setProgress] = useState<PodcastProgress | null>(null);
  const [result, setResult] = useState<PodcastResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeLine, setActiveLine] = useState<number>(-1);
  const [isPlaying, setIsPlaying] = useState(false);
  const [showModeMenu, setShowModeMenu] = useState(false);
  const [showVoiceAMenu, setShowVoiceAMenu] = useState(false);
  const [showVoiceBMenu, setShowVoiceBMenu] = useState(false);
  const currentModeLabel = modeOptions.find((option) => option.mode === currentMode)?.label ?? "Mode";
  const CurrentModeIcon = MODE_ICON_MAP[currentMode] ?? MessageSquare;

  const audioRef = useRef<HTMLAudioElement>(null);
  const modeMenuRef = useRef<HTMLDivElement>(null);
  const voiceAMenuRef = useRef<HTMLDivElement>(null);
  const voiceBMenuRef = useRef<HTMLDivElement>(null);

  // Listen for podcast progress events
  useEffect(() => {
    let unlisten: (() => void) | null = null;

    listen<PodcastProgress>("podcast-progress", (event) => {
      setProgress(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (modeMenuRef.current && !modeMenuRef.current.contains(event.target as Node)) {
        setShowModeMenu(false);
      }
      if (voiceAMenuRef.current && !voiceAMenuRef.current.contains(event.target as Node)) {
        setShowVoiceAMenu(false);
      }
      if (voiceBMenuRef.current && !voiceBMenuRef.current.contains(event.target as Node)) {
        setShowVoiceBMenu(false);
      }
    };

    if (showModeMenu || showVoiceAMenu || showVoiceBMenu) {
      document.addEventListener("mousedown", handleClickOutside);
    }

    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [showModeMenu, showVoiceAMenu, showVoiceBMenu]);

  // Track audio playback state
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;

    const onPlay = () => setIsPlaying(true);
    const onPause = () => setIsPlaying(false);
    const onEnded = () => {
      setIsPlaying(false);
      setActiveLine(-1);
    };

    audio.addEventListener("play", onPlay);
    audio.addEventListener("pause", onPause);
    audio.addEventListener("ended", onEnded);

    return () => {
      audio.removeEventListener("play", onPlay);
      audio.removeEventListener("pause", onPause);
      audio.removeEventListener("ended", onEnded);
    };
  }, []);

  const handleGenerate = async () => {
    if (!query.trim() || isGenerating) return;

    setIsGenerating(true);
    setError(null);
    setResult(null);
    setProgress(null);
    setActiveLine(-1);

    try {
      const request: PodcastRequest = {
        query: query.trim(),
        voice_a: voiceA,
        voice_b: voiceB,
        speaker_a_name: speakerA,
        speaker_b_name: speakerB,
        max_turns: maxTurns,
      };

      const podcast = await Api.generatePodcast(request);
      setResult(podcast);
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as Error)?.message || "Unknown error";
      setError(msg);
    } finally {
      setIsGenerating(false);
    }
  };

  const playFullPodcast = () => {
    if (result && audioRef.current) {
      audioRef.current.src = result.combined_audio_data_url;
      audioRef.current.play();
      setActiveLine(0);
    }
  };

  const playSegment = (index: number) => {
    if (result && audioRef.current) {
      const segment = result.segments[index];
      audioRef.current.src = segment.audio_data_url;
      audioRef.current.play();
      setActiveLine(index);
    }
  };

  const stopPlayback = () => {
    if (audioRef.current) {
      audioRef.current.pause();
      audioRef.current.currentTime = 0;
      setIsPlaying(false);
      setActiveLine(-1);
    }
  };

  return (
    <div className="podcast-tab">
      {/* ─── Header ─── */}
      <div className="podcast-header">
        <div className="podcast-header-icon">
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
            <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
            <line x1="12" y1="19" x2="12" y2="23" />
            <line x1="8" y1="23" x2="16" y2="23" />
          </svg>
        </div>
        <div>
          <h2 className="podcast-title">Podcast Studio</h2>
          <p className="podcast-subtitle">
            Generate an interactive two-person podcast from your documents
          </p>
        </div>
      </div>

      {!hasDocuments && (
        <div className="podcast-warning">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="10" /><line x1="12" y1="8" x2="12" y2="12" /><line x1="12" y1="16" x2="12.01" y2="16" />
          </svg>
          <span>No documents ingested. Add documents in Chat mode first, then return here to generate a podcast.</span>
        </div>
      )}

      {/* ─── Configuration ─── */}
      <div className="podcast-config">
        <div className="podcast-config-header">Setup</div>
        <div className="podcast-config-grid">
          {/* Speaker A */}
          <div className="podcast-speaker-config">
            <div className="speaker-badge speaker-a-badge">A</div>
            <div className="speaker-fields">
              <input
                type="text"
                value={speakerA}
                onChange={(e) => setSpeakerA(e.target.value)}
                placeholder="Speaker name"
                className="podcast-input"
                disabled={isGenerating}
              />
              <div className="relative" ref={voiceAMenuRef}>
                <button
                  className="w-full flex items-center justify-between gap-2 h-8.5 px-3.5 rounded-lg bg-void-700 border border-glass-border text-txt cursor-pointer transition-all duration-200 hover:border-neon"
                  onClick={() => {
                    if (isGenerating) return;
                    setShowVoiceAMenu((v) => !v);
                    setShowVoiceBMenu(false);
                  }}
                  disabled={isGenerating}
                >
                  <span className="text-sm">{voiceA}</span>
                  <ChevronDown size={14} className="text-txt-muted" />
                </button>
                {showVoiceAMenu && (
                  <div className="animate-attach-menu absolute top-full left-0 mt-2 w-full rounded-xl bg-void-700/90 backdrop-blur-xl border border-glass-border shadow-[0_8px_32px_rgba(0,0,0,0.5)] p-1 z-50">
                    {KITTEN_TTS_VOICES.map((voice) => {
                      const active = voice === voiceA;
                      return (
                        <button
                          key={voice}
                          className={`w-full text-left py-2 px-3 rounded-lg text-sm transition-all duration-150 ${active ? "bg-neon-subtle text-neon" : "text-txt-secondary hover:bg-glass-hover hover:text-txt"}`}
                          onClick={() => {
                            setVoiceA(voice);
                            setShowVoiceAMenu(false);
                          }}
                        >
                          {voice}
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>
          </div>

          {/* Speaker B */}
          <div className="podcast-speaker-config">
            <div className="speaker-badge speaker-b-badge">B</div>
            <div className="speaker-fields">
              <input
                type="text"
                value={speakerB}
                onChange={(e) => setSpeakerB(e.target.value)}
                placeholder="Speaker name"
                className="podcast-input"
                disabled={isGenerating}
              />
              <div className="relative" ref={voiceBMenuRef}>
                <button
                  className="w-full flex items-center justify-between gap-2 h-8.5 px-3.5 rounded-lg bg-void-700 border border-glass-border text-txt cursor-pointer transition-all duration-200 hover:border-neon"
                  onClick={() => {
                    if (isGenerating) return;
                    setShowVoiceBMenu((v) => !v);
                    setShowVoiceAMenu(false);
                  }}
                  disabled={isGenerating}
                >
                  <span className="text-sm">{voiceB}</span>
                  <ChevronDown size={14} className="text-txt-muted" />
                </button>
                {showVoiceBMenu && (
                  <div className="animate-attach-menu absolute top-full left-0 mt-2 w-full rounded-xl bg-void-700/90 backdrop-blur-xl border border-glass-border shadow-[0_8px_32px_rgba(0,0,0,0.5)] p-1 z-50">
                    {KITTEN_TTS_VOICES.map((voice) => {
                      const active = voice === voiceB;
                      return (
                        <button
                          key={voice}
                          className={`w-full text-left py-2 px-3 rounded-lg text-sm transition-all duration-150 ${active ? "bg-neon-subtle text-neon" : "text-txt-secondary hover:bg-glass-hover hover:text-txt"}`}
                          onClick={() => {
                            setVoiceB(voice);
                            setShowVoiceBMenu(false);
                          }}
                        >
                          {voice}
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>
          </div>

          {/* Turns */}
          <div className="podcast-turns-config">
            <label className="podcast-label">Dialogue Turns</label>
            <input
              type="number"
              min={4}
              max={30}
              value={maxTurns}
              onChange={(e) => setMaxTurns(parseInt(e.target.value) || 10)}
              className="podcast-input podcast-input-small"
              disabled={isGenerating}
            />
          </div>
        </div>

        {/* Query Input */}
        <div className="podcast-query">
          <textarea
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="What topic should the podcast cover? (Based on your ingested documents)"
            rows={3}
            disabled={isGenerating}
            className="podcast-textarea"
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                handleGenerate();
              }
            }}
          />
          <div className="relative" ref={modeMenuRef}>
            <button
              className="glass-btn flex items-center gap-1.5 h-10 px-2.5 rounded-xl bg-glass-bg border border-glass-border text-txt-muted cursor-pointer transition-all duration-200 hover:text-neon hover:border-neon/30 hover:shadow-[0_0_10px_rgba(0,212,255,0.12)]"
              onClick={() => setShowModeMenu((v) => !v)}
              title="Switch mode"
              disabled={isGenerating}
            >
              <CurrentModeIcon size={16} strokeWidth={1.9} />
              <span className="text-[0.74rem] font-medium leading-none">{currentModeLabel}</span>
            </button>

            {showModeMenu && (
              <div className="animate-attach-menu absolute bottom-full right-0 mb-2 w-45 rounded-xl bg-void-700/90 backdrop-blur-xl border border-glass-border shadow-[0_8px_32px_rgba(0,0,0,0.5)] p-1 z-50">
                {modeOptions.map((option) => {
                  const active = option.mode === currentMode;
                  return (
                    <button
                      key={option.mode}
                      className={`w-full flex items-center gap-2 py-2 px-3 rounded-lg text-sm transition-all duration-150 ${active ? "bg-neon-subtle text-neon" : "text-txt-secondary hover:bg-glass-hover hover:text-txt"}`}
                      onClick={() => {
                        onSelectMode(option.mode);
                        setShowModeMenu(false);
                      }}
                    >
                      {(() => {
                        const OptionIcon = MODE_ICON_MAP[option.mode] ?? MessageSquare;
                        return <OptionIcon size={15} strokeWidth={1.9} />;
                      })()}
                      {option.label}
                    </button>
                  );
                })}
              </div>
            )}
          </div>
          <button
            onClick={handleGenerate}
            disabled={isGenerating || !query.trim() || !hasDocuments}
            className="glass-btn podcast-generate-btn"
          >
            {isGenerating ? (
              <>
                <svg className="spin" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M21 12a9 9 0 1 1-6.219-8.56" />
                </svg>
                Generating...
              </>
            ) : (
              <>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
                  <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                </svg>
                Generate Podcast
              </>
            )}
          </button>
        </div>
      </div>

      {/* ─── Progress ─── */}
      {progress && isGenerating && (
        <div className="podcast-progress">
          <div className="podcast-progress-bar">
            <div
              className="podcast-progress-fill"
              style={{ width: `${progress.progress * 100}%` }}
            />
          </div>
          <div className="podcast-progress-detail">
            <span className="podcast-stage-badge">{progress.stage.toUpperCase()}</span>
            <span>{progress.detail}</span>
          </div>
        </div>
      )}

      {/* ─── Error ─── */}
      {error && (
        <div className="podcast-error">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="10" /><line x1="15" y1="9" x2="9" y2="15" /><line x1="9" y1="9" x2="15" y2="15" />
          </svg>
          <span>{error}</span>
        </div>
      )}

      {/* ─── Results ─── */}
      {result && (
        <div className="podcast-result">
          {/* Header with play button */}
          <div className="podcast-result-header">
            <h3 className="podcast-result-title">{result.script.title}</h3>
            <div className="podcast-result-actions">
              {isPlaying ? (
                <button className="glass-btn podcast-stop-btn" onClick={stopPlayback}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                    <rect x="6" y="4" width="4" height="16" /><rect x="14" y="4" width="4" height="16" />
                  </svg>
                  Stop
                </button>
              ) : (
                <button className="glass-btn podcast-play-btn" onClick={playFullPodcast}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                    <polygon points="5 3 19 12 5 21 5 3" />
                  </svg>
                  Play Full Podcast
                </button>
              )}
            </div>
          </div>

          {/* Script Transcript */}
          <div className="podcast-transcript">
            {result.script.lines.map((line, i) => (
              <div
                key={i}
                className={`podcast-line ${activeLine === i ? "active" : ""} ${line.speaker === speakerA ? "speaker-a" : "speaker-b"
                  }`}
                onClick={() => playSegment(i)}
              >
                <span className={`podcast-speaker ${line.speaker === speakerA ? "speaker-a-name" : "speaker-b-name"}`}>
                  {line.speaker}
                </span>
                <span className="podcast-line-text">{line.text}</span>
                <button
                  className="glass-btn podcast-line-play"
                  onClick={(e) => { e.stopPropagation(); playSegment(i); }}
                  title="Play this line"
                >
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
                    <polygon points="5 3 19 12 5 21 5 3" />
                  </svg>
                </button>
              </div>
            ))}
          </div>

          {/* Sources */}
          <details className="podcast-sources">
            <summary>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
                <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
              </svg>
              Sources ({result.script.source_chunks.length} chunks used)
            </summary>
            <div className="podcast-source-list">
              {result.script.source_chunks.map((chunk, i) => (
                <div key={i} className="podcast-source-chunk">
                  <span className="podcast-chunk-num">#{i + 1}</span>
                  <p>{chunk}</p>
                </div>
              ))}
            </div>
          </details>

          {/* Hidden audio element */}
          <audio ref={audioRef} className="podcast-audio-player" controls />
        </div>
      )}
    </div>
  );
};

export default PodcastTab;
