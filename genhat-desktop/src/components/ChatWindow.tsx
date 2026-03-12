import React, { useState, useEffect, useRef, memo } from "react";
import MarkdownRenderer from "./MarkdownRenderer";
import AudioPlayer from "./AudioPlayer";
import { Api } from "../api";
import type { ChatMessage, MediaAsset, IngestionStatus } from "../types";

/** Copy button for a full assistant response */
const CopyMsgButton: React.FC<{ text: string }> = ({ text }) => {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <button className="p-1.5 glass border border-glass-border text-txt-muted cursor-pointer rounded-lg transition-all duration-200 hover:text-neon hover:border-neon/30 hover:shadow-[0_0_8px_rgba(0,212,255,0.1)]" onClick={handleCopy} title="Copy response">
      {copied ? (
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none">
          <path d="M20 6L9 17L4 12" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      ) : (
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none">
          <rect x="9" y="2" width="6" height="4" rx="1" stroke="currentColor" strokeWidth="2" />
          <path d="M9 3H6a2 2 0 00-2 2v14a2 2 0 002 2h12a2 2 0 002-2V5a2 2 0 00-2-2h-3" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
        </svg>
      )}
    </button>
  );
};

interface ChatWindowProps {
  messages: ChatMessage[];
  streamingContent: string;
  isLoading: boolean;
  onSend: (text: string) => void;
  onCancel?: () => void;
  cancelled?: boolean;
  audioSrc?: string;
  placeholder?: string;
  mediaAssets?: Record<number, MediaAsset[]>;
  chatMode?: string;
  ttsGenerating?: boolean;
  ttsElapsedTime?: number;
  ttsGenerationTime?: number | null;
  generalGenerating?: boolean;
  generalElapsedTime?: number;
  generalGenerationTime?: number | null;
  ragDocs?: IngestionStatus[];
  ragIngesting?: boolean;
  enrichmentStatus?: string | null;
  onIngestFile?: () => void;
  onIngestDir?: () => void;
  onToggleDocPanel?: () => void;
  showRagControls?: boolean;
  docPanelOpen?: boolean;
}

/** Inline gallery for extracted images/tables attached to an assistant message. */
const MediaGallery: React.FC<{ assets: MediaAsset[] }> = ({ assets }) => {
  const [expanded, setExpanded] = useState<number | null>(null);
  const [dataUrls, setDataUrls] = useState<Record<number, string>>({});

  useEffect(() => {
    if (!assets || assets.length === 0) return;
    let cancelled = false;

    const loadAll = async () => {
      const entries: [number, string][] = [];
      for (const asset of assets) {
        try {
          const dataUrl = await Api.readImageBase64(asset.file_path);
          if (!cancelled) entries.push([asset.id, dataUrl]);
        } catch (e) {
          console.warn(`Failed to load media ${asset.id}:`, e);
        }
      }
      if (!cancelled) {
        setDataUrls(Object.fromEntries(entries));
      }
    };

    loadAll();
    return () => { cancelled = true; };
  }, [assets]);

  if (!assets || assets.length === 0) return null;

  return (
    <div className="mt-2.5 pt-2.5 border-t border-glass-border">
      <div className="text-[0.75rem] text-txt-muted mb-2 font-medium">
        📎 {assets.length} related {assets.length === 1 ? "figure" : "figures"}
      </div>
      <div className="flex flex-wrap gap-2">
        {assets.map((asset) => (
          <div
            key={asset.id}
            className={`media-thumb relative rounded-lg overflow-hidden cursor-pointer transition-all duration-200 border border-glass-border hover:border-neon hover:shadow-md ${expanded === asset.id ? "max-w-full flex-[1_1_100%]" : "max-w-[200px]"}`}
            onClick={() => setExpanded(expanded === asset.id ? null : asset.id)}
          >
            {dataUrls[asset.id] ? (
              <img
                src={dataUrls[asset.id]}
                alt={asset.caption || `${asset.asset_type} from document`}
                loading="lazy"
                className={`block w-full h-auto ${expanded === asset.id ? "" : "max-h-[160px] object-cover"}`}
              />
            ) : (
              <div className="flex items-center justify-center w-[160px] h-[120px] text-txt-muted text-[0.75rem] bg-void-800">Loading…</div>
            )}
            <span className="absolute top-1 right-1 text-[0.7rem] bg-black/60 rounded px-1 py-0.5 leading-none">
              {asset.asset_type === "table" ? "📊" : "🖼️"}
            </span>
            {expanded === asset.id && asset.caption && (
              <div className="p-1.5 px-2 text-[0.72rem] text-txt-muted bg-void-800 leading-snug max-h-[100px] overflow-y-auto">{asset.caption}</div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
};

const ChatWindow: React.FC<ChatWindowProps> = memo(({
  messages,
  streamingContent,
  isLoading,
  onSend,
  onCancel,
  cancelled = false,
  audioSrc,
  placeholder = "Message NELA...",
  mediaAssets = {},
  chatMode = "text",
  ttsGenerating = false,
  ttsElapsedTime = 0,
  ttsGenerationTime = null,
  generalGenerating = false,
  generalElapsedTime = 0,
  generalGenerationTime = null,
  ragDocs = [],
  ragIngesting = false,
  enrichmentStatus = null,
  onIngestFile,
  onIngestDir,
  onToggleDocPanel,
  showRagControls = false,
  docPanelOpen = false,
}) => {
  const [inputObj, setInputObj] = useState("");
  const [showAttachMenu, setShowAttachMenu] = useState(false);
  const attachMenuRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);

  /** Tracks the number of messages that have already been rendered and animated.
   *  Only messages at index >= this value get the entrance animation.
   *  We use state (not a ref) so ESLint doesn't flag .current reads during render. */
  const [prevMsgCount, setPrevMsgCount] = useState(0);
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- intentional: track previous render's msg count
    setPrevMsgCount(messages.length);
  }, [messages.length]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingContent]);

  // Close attach menu on outside click
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (attachMenuRef.current && !attachMenuRef.current.contains(e.target as Node)) {
        setShowAttachMenu(false);
      }
    };
    if (showAttachMenu) {
      document.addEventListener("mousedown", handleClickOutside);
    }
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [showAttachMenu]);

  const handleSend = () => {
    if (!inputObj.trim() || isLoading) return;
    onSend(inputObj);
    setInputObj("");
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (!isLoading) handleSend();
    }
  };

  const hasMessages = messages.length > 0 || isLoading;

  // ─── Centered Welcome State (Claude/Copilot style) ───
  if (!hasMessages) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center relative px-6">
        {/* Animated orb */}
        <div className="welcome-orb" />

        {/* Brand & Greeting */}
        <div className="relative z-10 flex flex-col items-center mb-8">
          <img
            src="/logo-dark.png"
            alt="NELA"
            className="w-14 h-14 rounded-2xl object-contain shadow-[0_4px_30px_rgba(0,212,255,0.3)] mb-4"
            draggable={false}
          />
          <h2 className="text-2xl font-bold text-txt m-0 mb-1">What can I help with?</h2>
          <p className="text-sm text-txt-muted m-0">Ask anything — chat, analyze images, or explore your documents</p>
        </div>

        {/* Centered Input */}
        <div className="relative z-10 w-full max-w-2xl">
          {/* RAG doc indicators */}
          {showRagControls && ragDocs.length > 0 && (
            <div className="flex items-center gap-2 mb-2 justify-center">
              <button
                className={`glass-btn inline-flex items-center gap-1.5 py-1 px-3 text-[0.78rem] font-medium rounded-full cursor-pointer transition-all duration-200 border backdrop-blur-md ${docPanelOpen ? "bg-neon-subtle text-neon border-neon/30 shadow-[0_0_12px_rgba(0,212,255,0.12)]" : "bg-glass-bg text-txt-secondary border-glass-border hover:border-neon hover:text-neon hover:shadow-[0_0_12px_rgba(0,212,255,0.08)]"}`}
                onClick={onToggleDocPanel}
                title="Toggle knowledge base panel"
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
                  <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
                </svg>
                {ragDocs.length} file{ragDocs.length !== 1 ? "s" : ""} loaded
              </button>
              {ragIngesting && (
                <span className="inline-flex items-center gap-1 text-[0.72rem] text-warning">
                  <svg className="spin" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 12a9 9 0 1 1-6.219-8.56" /></svg>
                  Ingesting...
                </span>
              )}
              {enrichmentStatus && (
                <span className="inline-flex items-center gap-1 text-[0.72rem] text-success">
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M20 6L9 17l-5-5" /></svg>
                  {enrichmentStatus}
                </span>
              )}
            </div>
          )}

          <div className="input-wrapper glass-strong flex items-end gap-2 rounded-2xl px-3 py-2 transition-all duration-200 shadow-[0_4px_24px_rgba(0,0,0,0.3)] focus-within:border-neon focus-within:shadow-[0_0_24px_rgba(0,212,255,0.15),0_4px_24px_rgba(0,0,0,0.3)]">
            {showRagControls && (
              <div className="relative" ref={attachMenuRef}>
                <button
                  className="glass-btn flex items-center justify-center w-9 h-9 bg-glass-bg border border-glass-border text-txt-muted cursor-pointer rounded-lg transition-all duration-200 backdrop-blur-sm hover:text-neon hover:border-neon/30 hover:shadow-[0_0_8px_rgba(0,212,255,0.1)] disabled:opacity-40"
                  onClick={() => setShowAttachMenu(!showAttachMenu)}
                  title="Add documents to knowledge base"
                  disabled={isLoading}
                >
                  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
                    style={{ transform: showAttachMenu ? "rotate(45deg)" : "none", transition: "transform 0.2s" }}>
                    <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
                  </svg>
                </button>

                {showAttachMenu && (
                  <div className="animate-attach-menu absolute bottom-full left-0 mb-2 w-[220px] rounded-xl bg-void-700/80 backdrop-blur-xl border border-glass-border shadow-[0_8px_32px_rgba(0,0,0,0.5)] p-1 z-50">
                    <button className="w-full flex items-center gap-2.5 py-2 px-3 rounded-lg text-sm text-txt-secondary bg-transparent border-none cursor-pointer transition-all duration-150 hover:bg-glass-hover hover:text-txt"
                      onClick={() => { onIngestFile?.(); setShowAttachMenu(false); }}>
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                        <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z" /><polyline points="14 2 14 8 20 8" />
                      </svg>
                      <div className="flex flex-col items-start">
                        <span className="font-medium">Add Files</span>
                        <span className="text-[0.68rem] text-txt-muted">PDF, DOCX, TXT, code...</span>
                      </div>
                    </button>
                    <button className="w-full flex items-center gap-2.5 py-2 px-3 rounded-lg text-sm text-txt-secondary bg-transparent border-none cursor-pointer transition-all duration-150 hover:bg-glass-hover hover:text-txt"
                      onClick={() => { onIngestDir?.(); setShowAttachMenu(false); }}>
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                        <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" />
                      </svg>
                      <div className="flex flex-col items-start">
                        <span className="font-medium">Add Folder</span>
                        <span className="text-[0.68rem] text-txt-muted">Ingest entire directory</span>
                      </div>
                    </button>
                  </div>
                )}
              </div>
            )}

            <textarea
              value={inputObj}
              onChange={(e) => setInputObj(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={placeholder}
              rows={1}
              className="flex-1 bg-transparent border-none outline-none text-txt text-[0.92rem] py-2 px-1 min-h-[40px] max-h-[200px] resize-none leading-relaxed font-inherit placeholder:text-txt-muted"
            />
            <button className="send-btn flex items-center justify-center w-10 h-10 rounded-xl bg-neon text-void-900 border border-neon/50 cursor-pointer transition-all duration-200 shadow-[0_0_16px_rgba(0,212,255,0.2)] hover:bg-neon-hover hover:shadow-[0_0_24px_rgba(0,212,255,0.35)] disabled:opacity-30 disabled:cursor-not-allowed disabled:shadow-none shrink-0" onClick={handleSend} disabled={!inputObj.trim()}>
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                <path d="M5 12H19M19 12L12 5M19 12L12 19" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </button>
          </div>
        </div>
      </div>
    );
  }

  // ─── Normal Chat State ───
  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div className="messages-area flex-1 overflow-y-auto px-6 py-4">
        {messages.map((msg, idx) => {
          const isNew = idx >= prevMsgCount;
          return (
          <div key={idx} className={`${isNew ? "animate-msg-fade" : ""} flex gap-3 mb-6 max-w-3xl mx-auto ${msg.role === "user" ? "flex-row-reverse" : ""}`}>
            {msg.role === "user" ? (
              <>
                <div className="flex flex-col items-end flex-1 min-w-0">
                  <div className="py-3 px-4 rounded-2xl rounded-tr-sm text-[0.9rem] leading-relaxed text-txt max-w-[85%] bg-[rgba(0,212,255,0.04)] backdrop-blur-lg border border-[rgba(0,212,255,0.1)] shadow-[0_4px_20px_rgba(0,0,0,0.25),inset_0_1px_0_rgba(255,255,255,0.04)]">
                    {msg.content}
                  </div>
                </div>
                <div className="w-8 h-8 rounded-xl bg-neon-subtle text-neon flex items-center justify-center shrink-0 border border-neon/15 shadow-[0_2px_8px_rgba(0,212,255,0.1)]">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                    <path d="M12 12c2.761 0 5-2.239 5-5s-2.239-5-5-5-5 2.239-5 5 2.239 5 5 5z" fill="currentColor" />
                    <path d="M4 20c0-3.3137 2.6863-6 6-6h4c3.3137 0 6 2.6863 6 6v1H4v-1z" fill="currentColor" />
                  </svg>
                </div>
              </>
            ) : (
              <>
                <img
                  src="/logo-dark.png"
                  alt="NELA"
                  className="w-8 h-8 rounded-xl object-contain shrink-0 shadow-[0_2px_12px_rgba(0,212,255,0.25)]"
                  draggable={false}
                />
                <div className="flex-1 min-w-0">
                  <div className="text-[0.9rem] leading-relaxed text-txt glass rounded-2xl rounded-tl-sm py-3 px-4 shadow-[0_4px_20px_rgba(0,0,0,0.2),inset_0_1px_0_rgba(255,255,255,0.03)]">
                    <MarkdownRenderer content={msg.content} />
                    {mediaAssets[idx] && <MediaGallery assets={mediaAssets[idx]} />}
                    <div className="flex items-center gap-1 mt-2 pt-1.5">
                      <CopyMsgButton text={msg.content} />
                      {msg.generateTime !== undefined && (
                        <span className="text-[0.7rem] text-txt-muted ml-1" title={msg.firstTokenTime !== undefined ? `Generated in ${msg.generateTime}s\nFirst token in ${msg.firstTokenTime}s` : `Generated in ${msg.generateTime}s`}>
                          Generated in {msg.generateTime}s {msg.firstTokenTime !== undefined && `• First token in ${msg.firstTokenTime}s`}
                        </span>
                      )}
                    </div>
                  </div>
                </div>
              </>
            )}
          </div>
          );
        })}

        {isLoading && (
          <div className="animate-msg-fade flex gap-3 mb-5 max-w-3xl mx-auto">
            <img
              src="/logo-dark.png"
              alt="NELA"
              className="w-8 h-8 rounded-xl object-contain shrink-0 shadow-[0_2px_10px_rgba(0,212,255,0.2)]"
              draggable={false}
            />
            <div className="flex-1 min-w-0 text-[0.9rem] leading-relaxed text-txt glass rounded-2xl rounded-tl-sm py-3 px-4">
              {streamingContent ? (
                <MarkdownRenderer content={streamingContent} />
              ) : (
                <div className="typing-dots flex gap-1.5 py-2">
                  <span></span><span></span><span></span>
                </div>
              )}
            </div>
          </div>
        )}

        {/* Response Time Timer - Audio Mode */}
        {chatMode === "audio" && ttsGenerating && (
          <div className="flex items-center gap-2 py-1.5 px-3 rounded-full bg-neon-subtle border border-neon/20 max-w-3xl mx-auto text-sm text-txt-secondary">
            <div className="tts-timer-pulse" />
            <span>Generating speech... <span className="text-neon font-semibold tabular-nums">{ttsElapsedTime.toFixed(1)}s</span></span>
          </div>
        )}

        {/* Response Time Timer - Chat/Vision/RAG Modes */}
        {chatMode !== "audio" && generalGenerating && (
          <div className="flex items-center gap-2 py-1.5 px-3 rounded-full bg-neon-subtle border border-neon/20 max-w-3xl mx-auto text-sm text-txt-secondary">
            <div className="tts-timer-pulse" />
            <span>
              {chatMode === "vision" && "Analyzing image... "}
              {chatMode === "rag" && "Processing query... "}
              {chatMode === "text" && "Generating response... "}
              <span className="text-neon font-semibold tabular-nums">{generalElapsedTime.toFixed(1)}s</span>
            </span>
          </div>
        )}

        {/* Audio Player */}
        {audioSrc && (
          <div className="max-w-3xl mx-auto mt-3">
            {ttsGenerationTime !== null && (
              <div className="text-[0.72rem] text-txt-muted mb-1">Generated in {ttsGenerationTime.toFixed(1)}s</div>
            )}
            <AudioPlayer src={audioSrc} autoPlay />
          </div>
        )}

        {/* Response time completion */}
        {chatMode !== "audio" && generalGenerationTime !== null && !generalGenerating && (
          <div className="flex items-center gap-1.5 py-1 px-3 rounded-full max-w-3xl mx-auto text-[0.72rem] text-success">
            <span>✓</span>
            <span>
              {chatMode === "vision" && `Analyzed in ${generalGenerationTime.toFixed(1)}s`}
              {chatMode === "rag" && `Processed in ${generalGenerationTime.toFixed(1)}s`}
              {chatMode === "text" && `Generated in ${generalGenerationTime.toFixed(1)}s`}
            </span>
          </div>
        )}

        {/* Cancelled notice */}
        {cancelled && (
          <div className="text-center py-1.5 text-[0.78rem] text-txt-muted">⏹ Response stopped</div>
        )}

        <div ref={endRef} />
      </div>

      {/* ── Input Area ── */}
      <div className="px-6 py-3 shrink-0 border-t border-glass-border bg-void-900">
        {/* RAG doc indicators */}
        {showRagControls && ragDocs.length > 0 && (
          <div className="flex items-center gap-2 mb-2 max-w-3xl mx-auto">
            <button
              className={`glass-btn inline-flex items-center gap-1.5 py-1 px-3 text-[0.78rem] font-medium rounded-full cursor-pointer transition-all duration-200 border backdrop-blur-md ${docPanelOpen ? "bg-neon-subtle text-neon border-neon/30 shadow-[0_0_12px_rgba(0,212,255,0.12)]" : "bg-glass-bg text-txt-secondary border-glass-border hover:border-neon hover:text-neon hover:shadow-[0_0_12px_rgba(0,212,255,0.08)]"}`}
              onClick={onToggleDocPanel}
              title="Toggle knowledge base panel"
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
                <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
              </svg>
              {ragDocs.length} file{ragDocs.length !== 1 ? "s" : ""} loaded
            </button>
            {ragIngesting && (
              <span className="inline-flex items-center gap-1 text-[0.72rem] text-warning">
                <svg className="spin" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 12a9 9 0 1 1-6.219-8.56" /></svg>
                Ingesting...
              </span>
            )}
            {enrichmentStatus && (
              <span className="inline-flex items-center gap-1 text-[0.72rem] text-success">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M20 6L9 17l-5-5" /></svg>
                {enrichmentStatus}
              </span>
            )}
          </div>
        )}

        <div className="input-wrapper glass-strong flex items-end gap-2 rounded-2xl px-3 py-2 max-w-3xl mx-auto transition-all duration-200 shadow-[0_4px_24px_rgba(0,0,0,0.3)] focus-within:border-neon focus-within:shadow-[0_0_24px_rgba(0,212,255,0.15),0_4px_24px_rgba(0,0,0,0.3)]">
          {showRagControls && (
            <div className="relative" ref={attachMenuRef}>
              <button
                className="glass-btn flex items-center justify-center w-9 h-9 bg-glass-bg border border-glass-border text-txt-muted cursor-pointer rounded-lg transition-all duration-200 backdrop-blur-sm hover:text-neon hover:border-neon/30 hover:shadow-[0_0_8px_rgba(0,212,255,0.1)] disabled:opacity-40"
                onClick={() => setShowAttachMenu(!showAttachMenu)}
                title="Add documents to knowledge base"
                disabled={isLoading}
              >
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
                  style={{ transform: showAttachMenu ? "rotate(45deg)" : "none", transition: "transform 0.2s" }}>
                  <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
                </svg>
              </button>

              {showAttachMenu && (
                <div className="animate-attach-menu absolute bottom-full left-0 mb-2 w-[220px] rounded-xl bg-void-700/80 backdrop-blur-xl border border-glass-border shadow-[0_8px_32px_rgba(0,0,0,0.5)] p-1 z-50">
                  <button className="w-full flex items-center gap-2.5 py-2 px-3 rounded-lg text-sm text-txt-secondary bg-transparent border-none cursor-pointer transition-all duration-150 hover:bg-glass-hover hover:text-txt"
                    onClick={() => { onIngestFile?.(); setShowAttachMenu(false); }}>
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z" /><polyline points="14 2 14 8 20 8" />
                    </svg>
                    <div className="flex flex-col items-start">
                      <span className="font-medium">Add Files</span>
                      <span className="text-[0.68rem] text-txt-muted">PDF, DOCX, TXT, code...</span>
                    </div>
                  </button>
                  <button className="w-full flex items-center gap-2.5 py-2 px-3 rounded-lg text-sm text-txt-secondary bg-transparent border-none cursor-pointer transition-all duration-150 hover:bg-glass-hover hover:text-txt"
                    onClick={() => { onIngestDir?.(); setShowAttachMenu(false); }}>
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" />
                    </svg>
                    <div className="flex flex-col items-start">
                      <span className="font-medium">Add Folder</span>
                      <span className="text-[0.68rem] text-txt-muted">Ingest entire directory</span>
                    </div>
                  </button>
                </div>
              )}
            </div>
          )}

          <textarea
            value={inputObj}
            onChange={(e) => setInputObj(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={placeholder}
            rows={1}
            className="flex-1 bg-transparent border-none outline-none text-txt text-[0.92rem] py-2 px-1 min-h-[40px] max-h-[200px] resize-none leading-relaxed font-inherit placeholder:text-txt-muted"
          />
          {isLoading ? (
            <button className="flex items-center justify-center w-10 h-10 rounded-xl bg-danger/80 backdrop-blur-sm text-white border border-danger/30 cursor-pointer transition-all duration-200 shadow-[0_0_12px_rgba(248,113,113,0.2)] hover:bg-danger hover:shadow-[0_0_20px_rgba(248,113,113,0.3)] shrink-0" onClick={onCancel} title="Stop generation">
              <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor"><rect x="4" y="4" width="16" height="16" rx="2" /></svg>
            </button>
          ) : (
            <button className="send-btn flex items-center justify-center w-10 h-10 rounded-xl bg-neon text-void-900 border border-neon/50 cursor-pointer transition-all duration-200 shadow-[0_0_16px_rgba(0,212,255,0.2)] hover:bg-neon-hover hover:shadow-[0_0_24px_rgba(0,212,255,0.35)] disabled:opacity-30 disabled:cursor-not-allowed disabled:shadow-none shrink-0" onClick={handleSend} disabled={!inputObj.trim()}>
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                <path d="M5 12H19M19 12L12 5M19 12L12 19" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </button>
          )}
        </div>
      </div>
    </div>
  );
});

ChatWindow.displayName = "ChatWindow";

export default ChatWindow;
