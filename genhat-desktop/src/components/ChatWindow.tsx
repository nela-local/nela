import React, { useState, useEffect, useRef } from "react";
import MarkdownRenderer from "./MarkdownRenderer";
import { Api } from "../api";
import type { MediaAsset, IngestionStatus } from "../types";

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
    <button className="msg-copy-btn" onClick={handleCopy} title="Copy response">
      {copied ? (
        /* Check icon */
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none">
          <path d="M20 6L9 17L4 12" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"/>
        </svg>
      ) : (
        /* Clipboard icon */
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none">
          <rect x="9" y="2" width="6" height="4" rx="1" stroke="currentColor" strokeWidth="2"/>
          <path d="M9 3H6a2 2 0 00-2 2v14a2 2 0 002 2h12a2 2 0 002-2V5a2 2 0 00-2-2h-3" stroke="currentColor" strokeWidth="2" strokeLinecap="round"/>
        </svg>
      )}
    </button>
  );
};

interface ChatWindowProps {
  messages: { role: string; content: string }[];
  streamingContent: string;
  isLoading: boolean;
  onSend: (text: string) => void;
  onCancel?: () => void;
  cancelled?: boolean;
  audioSrc?: string;
  placeholder?: string;
  /** Media assets (images/tables) keyed by message index. */
  mediaAssets?: Record<number, MediaAsset[]>;
  /** Chat mode for picking which timer to display */
  chatMode?: string;
  /** TTS generation state */
  ttsGenerating?: boolean;
  ttsElapsedTime?: number;
  ttsGenerationTime?: number | null;
  /** General response time tracking for chat, vision, RAG */
  generalGenerating?: boolean;
  generalElapsedTime?: number;
  generalGenerationTime?: number | null;
  /** RAG integration props */
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
  // Load images as base64 data URLs via the backend (avoids asset-protocol issues)
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
    <div className="media-gallery">
      <div className="media-gallery-label">
        📎 {assets.length} related {assets.length === 1 ? "figure" : "figures"}
      </div>
      <div className="media-gallery-grid">
        {assets.map((asset) => (
          <div
            key={asset.id}
            className={`media-thumb ${expanded === asset.id ? "expanded" : ""}`}
            onClick={() => setExpanded(expanded === asset.id ? null : asset.id)}
          >
            {dataUrls[asset.id] ? (
              <img
                src={dataUrls[asset.id]}
                alt={asset.caption || `${asset.asset_type} from document`}
                loading="lazy"
              />
            ) : (
              <div className="media-loading">Loading…</div>
            )}
            <span className="media-badge">
              {asset.asset_type === "table" ? "📊" : "🖼️"}
            </span>
            {expanded === asset.id && asset.caption && (
              <div className="media-caption">{asset.caption}</div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
};

const ChatWindow: React.FC<ChatWindowProps> = ({
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

  return (
    <div className="chat-container">
      <div className="messages-area">
        {messages.map((msg, idx) => (
          <div key={idx} className={`message ${msg.role}`}>
            {msg.role === "user" ? (
              <>
                <div className="content">{msg.content}</div>
                <div className="avatar">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                    <path d="M12 12c2.761 0 5-2.239 5-5s-2.239-5-5-5-5 2.239-5 5 2.239 5 5 5z" fill="currentColor" />
                    <path d="M4 20c0-3.3137 2.6863-6 6-6h4c3.3137 0 6 2.6863 6 6v1H4v-1z" fill="currentColor" />
                  </svg>
                </div>
              </>
            ) : (
              <>
                <div className="avatar">AI</div>
                <div className="content">
                  <div className="assistant-body">
                    <MarkdownRenderer content={msg.content} />
                    {mediaAssets[idx] && (
                      <MediaGallery assets={mediaAssets[idx]} />
                    )}
                    <div className="msg-actions">
                      <CopyMsgButton text={msg.content} />
                    </div>
                  </div>
                </div>
              </>
            )}
          </div>
        ))}

        {isLoading && (
          <div className="message assistant loading">
            <div className="avatar">AI</div>
            <div className="content">
              {streamingContent ? (
                <MarkdownRenderer content={streamingContent} />
              ) : (
                <span className="typing-indicator">...</span>
              )}
            </div>
          </div>
        )}
        
        {/* Response Time Timer - Audio Mode */}
        {chatMode === "audio" && ttsGenerating && (
          <div className="tts-timer">
            <div className="tts-timer-pulse" />
            <span className="tts-timer-text">
              Generating speech... <span className="tts-timer-value">{ttsElapsedTime.toFixed(1)}s</span>
            </span>
          </div>
        )}

        {/* Response Time Timer - Chat/Vision/RAG Modes */}
        {chatMode !== "audio" && generalGenerating && (
          <div className="tts-timer">
            <div className="tts-timer-pulse" />
            <span className="tts-timer-text">
              {chatMode === "vision" && "Analyzing image... "}
              {chatMode === "rag" && "Processing query... "}
              {chatMode === "text" && "Generating response... "}
              <span className="tts-timer-value">{generalElapsedTime.toFixed(1)}s</span>
            </span>
          </div>
        )}

        {/* Audio Player if generated */}
        {audioSrc && (
          <div className="audio-player">
            {ttsGenerationTime !== null && (
              <div className="tts-completion-time">
                Generated in {ttsGenerationTime.toFixed(1)}s
              </div>
            )}
            <audio controls src={audioSrc} autoPlay />
          </div>
        )}

        {/* Response time completion display for non-audio modes */}
        {chatMode !== "audio" && generalGenerationTime !== null && !generalGenerating && (
          <div className="response-completion-badge">
            <span className="response-time-indicator">✓</span>
            <span className="response-time-text">
              {chatMode === "vision" && `Analyzed in ${generalGenerationTime.toFixed(1)}s`}
              {chatMode === "rag" && `Processed in ${generalGenerationTime.toFixed(1)}s`}
              {chatMode === "text" && `Generated in ${generalGenerationTime.toFixed(1)}s`}
            </span>
          </div>
        )}

        {/* Cancelled notice */}
        {cancelled && (
          <div className="cancelled-notice">
            ⏹ Response stopped
          </div>
        )}

        <div ref={endRef} />
      </div>

      <div className="input-area">
        {/* RAG doc indicators — inline pill toggles the right sidebar */}
        {showRagControls && ragDocs.length > 0 && (
          <div className="rag-inline-bar">
            <button
              className={`rag-docs-pill ${docPanelOpen ? "active" : ""}`}
              onClick={onToggleDocPanel}
              title="Toggle knowledge base panel"
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
                <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
              </svg>
              {ragDocs.length} doc{ragDocs.length !== 1 ? "s" : ""} loaded
            </button>
            {ragIngesting && (
              <span className="rag-inline-status ingesting">
                <svg className="spin" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M21 12a9 9 0 1 1-6.219-8.56" />
                </svg>
                Ingesting...
              </span>
            )}
            {enrichmentStatus && (
              <span className="rag-inline-status enriched">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M20 6L9 17l-5-5" />
                </svg>
                {enrichmentStatus}
              </span>
            )}
          </div>
        )}

        <div className="input-wrapper">
          {/* + Attach button for RAG uploads */}
          {showRagControls && (
            <div className="attach-container" ref={attachMenuRef}>
              <button
                className="attach-btn"
                onClick={() => setShowAttachMenu(!showAttachMenu)}
                title="Add documents to knowledge base"
                disabled={isLoading}
              >
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
                  style={{ transform: showAttachMenu ? "rotate(45deg)" : "none", transition: "transform 0.2s" }}
                >
                  <line x1="12" y1="5" x2="12" y2="19" />
                  <line x1="5" y1="12" x2="19" y2="12" />
                </svg>
              </button>

              {showAttachMenu && (
                <div className="attach-menu">
                  <button
                    className="attach-menu-item"
                    onClick={() => { onIngestFile?.(); setShowAttachMenu(false); }}
                  >
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z" />
                      <polyline points="14 2 14 8 20 8" />
                    </svg>
                    <span>Add File</span>
                    <span className="attach-menu-hint">PDF, DOCX, TXT, code...</span>
                  </button>
                  <button
                    className="attach-menu-item"
                    onClick={() => { onIngestDir?.(); setShowAttachMenu(false); }}
                  >
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" />
                    </svg>
                    <span>Add Folder</span>
                    <span className="attach-menu-hint">Ingest entire directory</span>
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
          />
          {isLoading ? (
            <button className="stop-btn" onClick={onCancel} title="Stop generation">
              <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
                <rect x="4" y="4" width="16" height="16" rx="2" />
              </svg>
            </button>
          ) : (
            <button className="send-btn" onClick={handleSend} disabled={!inputObj.trim()}>
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                <path d="M5 12H19M19 12L12 5M19 12L12 19" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/>
              </svg>
            </button>
          )}
        </div>
      </div>
    </div>
  );
};

export default ChatWindow;
