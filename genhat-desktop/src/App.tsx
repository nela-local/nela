import { useState, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  MessageSquare,
  Eye,
  Volume2,
  Plus,
  ImageIcon,
  X,
  FileText,
  FolderOpen,
  Trash2,
  Loader2,
  CheckCircle2,
  ChevronDown,
} from "lucide-react";
import { Api } from "./api";
import type {
  ChatMessage,
  ChatMode,
  ModelFile,
  RegisteredModel,
  IngestionStatus,
  RagResult,
  MediaAsset,
  KittenTtsVoice,
} from "./types";
import { KITTEN_TTS_VOICES } from "./types";
import ChatWindow from "./components/ChatWindow";
import ModelSelector from "./components/ModelSelector";
import PdfViewer from "./components/PdfViewer";
import "./App.css";

/* ── Mode metadata for the sidebar ──────────────────────────────────────── */
const MODE_CONFIG: {
  mode: ChatMode;
  label: string;
  icon: React.ElementType;
  desc: string;
}[] = [
  { mode: "text", label: "Chat", icon: MessageSquare, desc: "Text conversation" },
  { mode: "vision", label: "Vision", icon: Eye, desc: "Image analysis" },
  { mode: "audio", label: "Audio", icon: Volume2, desc: "Text to speech" },
];

function App() {
  // ── Model state ────────────────────────────────────────────────────────────
  const [models, setModels] = useState<ModelFile[]>([]);
  const [selectedModel, setSelectedModel] = useState("");

  // ── TTS engine state (registered models with TTS task) ─────────────────
  const [ttsEngines, setTtsEngines] = useState<RegisteredModel[]>([]);
  const [selectedTtsEngine, setSelectedTtsEngine] = useState("");
  const [ttsVoice, setTtsVoice] = useState<KittenTtsVoice>("Leo");
  const [ttsSpeed, setTtsSpeed] = useState(1.0);
  const [ttsGenerating, setTtsGenerating] = useState(false);
  const [ttsElapsedTime, setTtsElapsedTime] = useState(0);
  const [ttsGenerationTime, setTtsGenerationTime] = useState<number | null>(null);
  const ttsIntervalRef = useRef<NodeJS.Timeout | null>(null);

  const [visionModels, setVisionModels] = useState<RegisteredModel[]>([]);
  const [selectedVisionModel, setSelectedVisionModel] = useState("");

  // ── Response time tracking for all modes ────────────────────────────────────
  const [generalElapsedTime, setGeneralElapsedTime] = useState(0);
  const [generalGenerationTime, setGeneralGenerationTime] = useState<number | null>(null);
  const [generalGenerating, setGeneralGenerating] = useState(false);
  const generalIntervalRef = useRef<NodeJS.Timeout | null>(null);

  // ── Chat state ─────────────────────────────────────────────────────────────
  const [chatMode, setChatMode] = useState<ChatMode>("text");
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [streamingContent, setStreamingContent] = useState("");
  const [loading, setLoading] = useState(false);
  const [audioOutput, setAudioOutput] = useState("");
  const [cancelled, setCancelled] = useState(false);
  const abortControllerRef = useRef<AbortController | null>(null);

  // ── Vision state ───────────────────────────────────────────────────────────
  const [imagePath, setImagePath] = useState<string | null>(null);
  const [imagePreview, setImagePreview] = useState<string | null>(null);
  const visionUnlistenRef = useRef<(() => void) | null>(null);

  // ── RAG state ──────────────────────────────────────────────────────────────
  const [ragDocs, setRagDocs] = useState<IngestionStatus[]>([]);
  const [ragResult, setRagResult] = useState<RagResult | null>(null);
  const [ragIngesting, setRagIngesting] = useState(false);
  const [enrichmentStatus, setEnrichmentStatus] = useState<string | null>(null);
  /** Media assets (images/tables) keyed by message index in the messages array. */
  const [mediaAssets, setMediaAssets] = useState<Record<number, MediaAsset[]>>({});

  // ── Right sidebar (Knowledge Base) ─────────────────────────────────────────
  const [docPanelOpen, setDocPanelOpen] = useState(false);

  // ── PDF Viewer state ───────────────────────────────────────────────────────
  const [pdfViewerData, setPdfViewerData] = useState<{
    data: string;
    title: string;
  } | null>(null);
  const [pdfLoading, setPdfLoading] = useState(false);

  // ── Lifecycle ──────────────────────────────────────────────────────────────

  useEffect(() => {
    refreshModels();
    loadRagDocs(); // Always load RAG docs on startup
    return () => {
      visionUnlistenRef.current?.();
      visionUnlistenRef.current = null;
    };
  }, []);

  // Reload RAG docs periodically when in text mode
  useEffect(() => {
    if (chatMode === "text") loadRagDocs();
  }, [chatMode]);

  // Listen for background enrichment progress events
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listen<{ enriched_this_round: number; status: string }>(
      "rag:enrichment_progress",
      (event) => {
        if (event.payload.status === "in_progress") {
          setEnrichmentStatus(
            `Enriched ${event.payload.enriched_this_round} chunks`
          );
          loadRagDocs();
          setTimeout(() => setEnrichmentStatus(null), 5000);
        }
      }
    ).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [chatMode]);

  // Clean up TTS and general timers on unmount
  useEffect(() => {
    return () => {
      if (ttsIntervalRef.current) clearInterval(ttsIntervalRef.current);
      if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
    };
  }, []);

  // ── Model helpers ──────────────────────────────────────────────────────────

  const refreshModels = () => {
    Api.listModels()
      .then((list) => {
        setModels(list);
        if (list.length > 0 && !selectedModel) {
          setSelectedModel(list[0].path);
        }
      })
      .catch(console.error);

    Api.listRegisteredModels()
      .then((list) => {
        const vision = list.filter((m) => m.tasks.includes("vision_chat"));
        setVisionModels(vision);
        if (vision.length > 0) setSelectedVisionModel(vision[0].id);

        // TTS engines from the registry
        const tts = list.filter((m) => m.tasks.includes("tts"));
        setTtsEngines(tts);
        if (tts.length > 0 && !selectedTtsEngine) {
          setSelectedTtsEngine(tts[0].id);
        }
      })
      .catch(console.error);
  };

  const handleModelChange = async (path: string) => {
    try {
      setSelectedModel(path);
      await Api.switchModel(path);
      setMessages([]);
    } catch (err) {
      console.error(err);
      alert("Failed to switch model");
    }
  };

  const handleAddModel = () => {
    alert(
      "To add a model, place the .gguf file into the 'models' folder of the application and restart/refresh."
    );
  };

  // ── Vision helpers ─────────────────────────────────────────────────────────

  const selectImage = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: "Images",
            extensions: ["jpg", "jpeg", "png", "webp", "gif", "bmp"],
          },
        ],
      });
      if (selected && typeof selected === "string") {
        setImagePath(selected);
        const dataUrl = await Api.readImageBase64(selected);
        setImagePreview(dataUrl);
      }
    } catch (err) {
      console.error("Failed to select image:", err);
    }
  };

  const clearImage = () => {
    setImagePath(null);
    setImagePreview(null);
  };

  // ── RAG helpers ────────────────────────────────────────────────────────────

  const loadRagDocs = async () => {
    try {
      const docs = await Api.listRagDocuments();
      setRagDocs(docs);
    } catch (e) {
      console.error("Failed to load RAG docs:", e);
    }
  };

  const ingestFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: "Documents",
            extensions: [
              "pdf", "docx", "pptx", "txt", "md", "rs", "py", "js", "ts",
              "java", "c", "cpp", "go", "toml", "yaml", "json", "xml", "csv",
              "mp3", "wav", "m4a", "ogg", "flac",
            ],
          },
        ],
      });
      if (selected && typeof selected === "string") {
        setRagIngesting(true);
        await Api.ingestDocument(selected);
        await loadRagDocs();
        setRagIngesting(false);
      }
    } catch (e) {
      console.error(e);
      setRagIngesting(false);
      alert(`Ingest failed: ${e}`);
    }
  };

  const ingestDir = async () => {
    try {
      const selected = await open({ directory: true });
      if (selected && typeof selected === "string") {
        setRagIngesting(true);
        await Api.ingestFolder(selected);
        await loadRagDocs();
        setRagIngesting(false);
      }
    } catch (e) {
      console.error(e);
      setRagIngesting(false);
      alert(`Folder ingest failed: ${e}`);
    }
  };

  const deleteRagDoc = async (docId: number) => {
    try {
      await Api.deleteRagDocument(docId);
      await loadRagDocs();
    } catch (e) {
      console.error(e);
      alert(`Delete failed: ${e}`);
    }
  };

  // ── PDF Viewer handlers ───────────────────────────────────────────────────

  const openPdfViewer = async (doc: IngestionStatus) => {
    const ext = doc.file_path.split(".").pop()?.toLowerCase();
    if (ext !== "pdf") return; // only PDFs are viewable

    try {
      setPdfLoading(true);
      const data = await Api.readFileBase64(doc.file_path);
      setPdfViewerData({ data, title: doc.title });
    } catch (e) {
      console.error("Failed to load PDF:", e);
      alert(`Failed to open PDF: ${e}`);
    } finally {
      setPdfLoading(false);
    }
  };

  const closePdfViewer = () => {
    setPdfViewerData(null);
  };

  // ── Cancel handler ────────────────────────────────────────────────────────

  const handleCancel = () => {
    // Abort the SSE fetch (text / RAG modes)
    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    // Stop vision event listener
    visionUnlistenRef.current?.();
    visionUnlistenRef.current = null;
    // Commit whatever partial response was received
    setMessages((prev) =>
      streamingContent ? [...prev, { role: "assistant", content: streamingContent }] : prev
    );
    setStreamingContent("");
    setLoading(false);
    setCancelled(true);
  };

  // ── Main send handler ─────────────────────────────────────────────────────

  const handleSend = async (text: string) => {
    if (loading) return; // Prevent concurrent requests
    const newMsg: ChatMessage = { role: "user", content: text };
    setMessages((prev) => [...prev, newMsg]);
    setLoading(true);
    setStreamingContent("");
    setAudioOutput("");
    setCancelled(false);
    // Create a fresh AbortController for this request
    const ctrl = new AbortController();
    abortControllerRef.current = ctrl;

    try {
      // ── RAG-enhanced Chat (auto-activates when documents are ingested) ──
      if (chatMode === "text" && ragDocs.length > 0) {
        try {
          // Start timer
          setGeneralGenerating(true);
          setGeneralElapsedTime(0);
          setGeneralGenerationTime(null);
          const ragStartTime = Date.now();

          // Set up interval to update elapsed time
          if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
          generalIntervalRef.current = setInterval(() => {
            const elapsed = Math.floor((Date.now() - ragStartTime) / 100) / 10;
            setGeneralElapsedTime(elapsed);
          }, 100);

          // Phase 1: Retrieval — sources come back immediately
          const setup = await Api.queryRagStream(text);
          setRagResult({ answer: "", sources: setup.sources });

          // If no relevant docs found, fall through to normal text chat
          if (!setup.prompt || setup.sources.length === 0) {
            // Fall through to normal text chat below
          } else {
            // Phase 2: Stream the answer from llama-server SSE
            let fullAnswer = "";
            await Api.streamChat(
              [
                { role: "system", content: "You are a helpful assistant." },
                { role: "user", content: setup.prompt },
              ],
              (chunk) => {
                fullAnswer += chunk;
                setStreamingContent((prev) => prev + chunk);
              },
              () => {
                setRagResult((prev) =>
                  prev ? { ...prev, answer: fullAnswer } : null
                );
                setMessages((prev) => {
                  const updated = [
                    ...prev,
                    { role: "assistant" as const, content: fullAnswer },
                  ];
                  const assistantIdx = updated.length - 1;
                  Api.retrieveMediaForResponse(fullAnswer)
                    .then((assets) => {
                      if (assets.length > 0) {
                        setMediaAssets((prev) => ({
                          ...prev,
                          [assistantIdx]: assets,
                        }));
                      }
                    })
                    .catch((e) =>
                      console.warn("Media retrieval failed:", e)
                    );
                  return updated;
                });
                setStreamingContent("");
                setLoading(false);
              },
              (err) => {
                console.error("RAG stream error:", err);
                setMessages((prev) => [
                  ...prev,
                  { role: "assistant", content: `RAG query error: ${err}` },
                ]);
                setLoading(false);
              },
              setup.llama_port,
              ctrl.signal
            );
            return;
          }
        } catch (e) {
          // Stop timer on error
          if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
          setGeneralGenerating(false);
          console.error("RAG attempt failed, falling back to normal chat:", e);
          // Fall through to normal text chat
        }
      }

      // ── Audio Mode ──────────────────────────────────────────────────────
      if (chatMode === "audio" && selectedTtsEngine) {
        try {
          // Start timer
          setTtsGenerating(true);
          setTtsElapsedTime(0);
          setTtsGenerationTime(null);
          const startTime = Date.now();

          // Set up interval to update elapsed time
          if (ttsIntervalRef.current) clearInterval(ttsIntervalRef.current);
          ttsIntervalRef.current = setInterval(() => {
            const elapsed = Math.floor((Date.now() - startTime) / 100) / 10; // One decimal place, in seconds
            setTtsElapsedTime(elapsed);
          }, 100);

          const audioUrl = await Api.generateSpeech(
            text,
            {
              voice: ttsVoice,
              speed: ttsSpeed,
            }
          );

          // Stop timer and calculate total time
          if (ttsIntervalRef.current) clearInterval(ttsIntervalRef.current);
          const totalTime = Math.floor((Date.now() - startTime) / 100) / 10; // One decimal place, in seconds
          setTtsGenerating(false);
          setTtsElapsedTime(totalTime);
          setTtsGenerationTime(totalTime);

          setAudioOutput(audioUrl);
          setMessages((prev) => [
            ...prev,
            {
              role: "assistant",
              content: `🔊 Audio generated (${ttsVoice}, ${ttsSpeed}x speed).`,
            },
          ]);
        } catch (e) {
          console.error(e);
          // Stop timer on error
          if (ttsIntervalRef.current) clearInterval(ttsIntervalRef.current);
          setTtsGenerating(false);
          setMessages((prev) => [
            ...prev,
            { role: "assistant", content: `Error generating audio: ${e}` },
          ]);
        }
        setLoading(false);
        return;
      }

      // ── Vision Mode (streaming via Tauri events) ────────────────────────
      if (chatMode === "vision") {
        if (!imagePath) {
          setMessages((prev) => [
            ...prev,
            { role: "assistant", content: "Please select an image first." },
          ]);
          setLoading(false);
          return;
        }

        try {
          // Start timer
          setGeneralGenerating(true);
          setGeneralElapsedTime(0);
          setGeneralGenerationTime(null);
          const startTime = Date.now();

          // Set up interval to update elapsed time
          if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
          generalIntervalRef.current = setInterval(() => {
            const elapsed = Math.floor((Date.now() - startTime) / 100) / 10;
            setGeneralElapsedTime(elapsed);
          }, 100);

          // Clean up any previous listener
          visionUnlistenRef.current?.();
          visionUnlistenRef.current = null;

          let visionResponse = "";

          const unlisten = await listen<{ chunk: string; done: boolean }>(
            "vision-stream",
            (event) => {
              if (event.payload.done) {
                // Stop timer
                if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
                const totalTime = Math.floor((Date.now() - startTime) / 100) / 10;
                setGeneralGenerating(false);
                setGeneralElapsedTime(totalTime);
                setGeneralGenerationTime(totalTime);

                setLoading(false);
                if (visionResponse) {
                  setMessages((prev) => [
                    ...prev,
                    { role: "assistant", content: visionResponse },
                  ]);
                  setStreamingContent("");
                }
                visionUnlistenRef.current?.();
                visionUnlistenRef.current = null;
              } else if (event.payload.chunk) {
                visionResponse += event.payload.chunk;
                setStreamingContent((prev) => prev + event.payload.chunk);
              }
            }
          );
          visionUnlistenRef.current = unlisten;

          // Start the streaming vision chat
          await Api.visionChatStream(
            imagePath,
            text || "What's in this image?",
            selectedVisionModel || undefined
          );
        } catch (e) {
          console.error(e);
          // Stop timer on error
          if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
          setGeneralGenerating(false);
          setMessages((prev) => [
            ...prev,
            { role: "assistant", content: `Vision error: ${e}` },
          ]);
          setLoading(false);
          visionUnlistenRef.current?.();
          visionUnlistenRef.current = null;
        }
        return;
      }

      // ── Text Chat Mode (streaming via SSE) ─────────────────────────────
      // Start timer
      setGeneralGenerating(true);
      setGeneralElapsedTime(0);
      setGeneralGenerationTime(null);
      const chatStartTime = Date.now();

      // Set up interval to update elapsed time
      if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
      generalIntervalRef.current = setInterval(() => {
        const elapsed = Math.floor((Date.now() - chatStartTime) / 100) / 10;
        setGeneralElapsedTime(elapsed);
      }, 100);

      let fullResponse = "";
      Api.streamChat(
        [...messages, newMsg],
        (chunk) => {
          setStreamingContent((prev) => prev + chunk);
          fullResponse += chunk;
        },
        () => {
          // Stop timer
          if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
          const totalTime = Math.floor((Date.now() - chatStartTime) / 100) / 10;
          setGeneralGenerating(false);
          setGeneralElapsedTime(totalTime);
          setGeneralGenerationTime(totalTime);

          setLoading(false);
          if (fullResponse) {
            setMessages((prev) => [
              ...prev,
              { role: "assistant", content: fullResponse },
            ]);
            setStreamingContent("");
          }
        },
        (err) => {
          // Stop timer on error
          if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
          setGeneralGenerating(false);
          console.error("Stream error", err);
          setMessages((prev) => [
            ...prev,
            { role: "assistant", content: `Error: ${err}` },
          ]);
          setLoading(false);
        },
        undefined,
        ctrl.signal
      );
    } catch (err) {
      // Stop timer on error
      if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
      setGeneralGenerating(false);
      console.error(err);
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: "An unexpected error occurred." },
      ]);
      setLoading(false);
    }
  };

  // ── Derived ────────────────────────────────────────────────────────────────

  const getPlaceholder = (): string => {
    switch (chatMode) {
      case "vision":
        return "Ask about the image (e.g., 'What's in this image?')";
      case "audio":
        return "Type text to generate speech...";
      default:
        return ragDocs.length > 0
          ? "Ask about your documents or chat freely..."
          : "Message NELA...";
    }
  };

  // ── Render ─────────────────────────────────────────────────────────────────

  const currentModeConfig = MODE_CONFIG.find((m) => m.mode === chatMode)!;

  return (
    <div className="app-container">
      {/* ══════════ LEFT NAV SIDEBAR ══════════ */}
      <nav className="nav-sidebar">
        {/* Brand */}
        <div className="nav-brand">
          <div className="nav-brand-icon">G</div>
        </div>

        {/* Mode Buttons */}
        <div className="nav-modes">
          {MODE_CONFIG.map(({ mode, label, icon: Icon, desc }) => (
            <button
              key={mode}
              className={`nav-mode-btn ${chatMode === mode ? "active" : ""}`}
              onClick={() => setChatMode(mode)}
              title={desc}
            >
              <Icon size={20} strokeWidth={1.8} />
              <span className="nav-mode-label">{label}</span>
            </button>
          ))}
        </div>

        {/* Spacer */}
        <div className="nav-spacer" />

        {/* New Chat */}
        <button
          className="nav-new-chat"
          onClick={() => {
            setMessages([]);
            setStreamingContent("");
            setAudioOutput("");
            setRagResult(null);
          }}
          title="New conversation"
        >
          <Plus size={18} strokeWidth={2} />
          <span className="nav-mode-label">New Chat</span>
        </button>
      </nav>

      {/* ══════════ MAIN CONTENT ══════════ */}
      <main className="main-content">
        {/* ── Top Bar ── */}
        <header className="top-bar">
          <div className="top-bar-left">
            <currentModeConfig.icon size={18} strokeWidth={1.8} className="top-bar-icon" />
            <h1 className="top-bar-title">{currentModeConfig.label}</h1>
            <span className="top-bar-desc">{currentModeConfig.desc}</span>
          </div>

          <div className="top-bar-right">
            {chatMode === "text" && (
              <ModelSelector
                models={models}
                selectedModel={selectedModel}
                onSelect={handleModelChange}
                type="llm"
                onAdd={handleAddModel}
              />
            )}
            {chatMode === "audio" && ttsEngines.length > 0 && (
              <div className="tts-controls">
                {/* TTS engine selector */}
                <div className="tts-engine-select">
                  <select
                    value={selectedTtsEngine}
                    onChange={(e) => setSelectedTtsEngine(e.target.value)}
                    className="model-select"
                    disabled={loading}
                  >
                    {ttsEngines.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.name}
                      </option>
                    ))}
                  </select>
                  <ChevronDown size={14} className="select-chevron" />
                </div>

                {/* KittenTTS voice + speed controls */}
                {selectedTtsEngine === "kitten-tts" && (
                  <>
                    <div className="tts-voice-select">
                      <select
                        value={ttsVoice}
                        onChange={(e) => setTtsVoice(e.target.value as KittenTtsVoice)}
                        className="model-select"
                        disabled={loading}
                      >
                        {KITTEN_TTS_VOICES.map((v) => (
                          <option key={v} value={v}>
                            {v}
                          </option>
                        ))}
                      </select>
                      <ChevronDown size={14} className="select-chevron" />
                    </div>

                    <div className="tts-speed-control">
                      <label className="tts-speed-label" title="Speaking speed">
                        {ttsSpeed.toFixed(1)}x
                      </label>
                      <input
                        type="range"
                        min="0.5"
                        max="2.0"
                        step="0.1"
                        value={ttsSpeed}
                        onChange={(e) => setTtsSpeed(parseFloat(e.target.value))}
                        className="tts-speed-slider"
                        disabled={loading}
                      />
                    </div>
                  </>
                )}
              </div>
            )}
            {chatMode === "vision" && visionModels.length > 0 && (
              <div className="vision-model-select">
                <select
                  value={selectedVisionModel}
                  onChange={(e) => setSelectedVisionModel(e.target.value)}
                  className="model-select"
                  disabled={loading}
                >
                  {visionModels.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                    </option>
                  ))}
                </select>
                <ChevronDown size={14} className="select-chevron" />
              </div>
            )}
          </div>
        </header>

        {/* ── Vision Panel ── */}
        {chatMode === "vision" && (
          <div className="context-panel">
            <div className="context-panel-header">
              <ImageIcon size={16} strokeWidth={1.8} />
              <span>Image Input</span>
            </div>
            <div className="context-panel-body">
              <div className="vision-controls">
                <button
                  onClick={selectImage}
                  disabled={loading}
                  className="action-btn"
                >
                  <ImageIcon size={14} />
                  Select Image
                </button>
                {imagePath && (
                  <button
                    onClick={clearImage}
                    disabled={loading}
                    className="action-btn danger"
                  >
                    <X size={14} />
                    Clear
                  </button>
                )}
                {imagePath && (
                  <span className="file-path-badge">
                    {imagePath.split(/[/\\]/).pop()}
                  </span>
                )}
              </div>
              {imagePreview && (
                <img
                  src={imagePreview}
                  alt="Selected"
                  className="vision-preview"
                />
              )}
            </div>
          </div>
        )}

        {/* ── RAG Panel ── now in right sidebar */}

        {/* ── Chat Area ── */}
        <ChatWindow
          messages={messages}
          streamingContent={streamingContent}
          isLoading={loading}
          onSend={handleSend}
          onCancel={handleCancel}
          cancelled={cancelled}
          audioSrc={audioOutput}
          placeholder={getPlaceholder()}
          mediaAssets={mediaAssets}
          ragDocs={ragDocs}
          ragIngesting={ragIngesting}
          enrichmentStatus={enrichmentStatus}
          onIngestFile={ingestFile}
          onIngestDir={ingestDir}
          onToggleDocPanel={() => setDocPanelOpen((v) => !v)}
          showRagControls={chatMode === "text"}
          docPanelOpen={docPanelOpen}
        />

        {/* ── PDF Viewer Overlay ── */}
        {pdfLoading && (
          <div className="pdf-loading-overlay">
            <div className="pdf-spinner" />
            <span>Loading PDF...</span>
          </div>
        )}
        {pdfViewerData && (
          <PdfViewer
            pdfData={pdfViewerData.data}
            title={pdfViewerData.title}
            onClose={closePdfViewer}
          />
        )}
      </main>

      {/* ══════════ RIGHT SIDEBAR — Knowledge Base ══════════ */}
      <aside className={`kb-sidebar ${docPanelOpen ? "open" : ""}`}>
        <div className="kb-sidebar-inner">
          {/* Header */}
          <div className="kb-sidebar-header">
            <div className="kb-sidebar-title">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
                <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
              </svg>
              Knowledge Base
            </div>
            <button
              className="kb-sidebar-close"
              onClick={() => setDocPanelOpen(false)}
              title="Close panel"
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>

          {/* Actions */}
          <div className="kb-sidebar-actions">
            <button onClick={ingestFile} disabled={ragIngesting} className="action-btn">
              <FileText size={14} />
              Add File
            </button>
            <button onClick={ingestDir} disabled={ragIngesting} className="action-btn">
              <FolderOpen size={14} />
              Add Folder
            </button>
          </div>

          {/* Status indicators */}
          {(ragIngesting || enrichmentStatus) && (
            <div className="kb-sidebar-status">
              {ragIngesting && (
                <span className="status-badge warning">
                  <Loader2 size={12} className="spin" />
                  Ingesting...
                </span>
              )}
              {enrichmentStatus && (
                <span className="status-badge success">
                  <CheckCircle2 size={12} />
                  {enrichmentStatus}
                </span>
              )}
            </div>
          )}

          {/* Document List */}
          <div className="kb-sidebar-docs">
            {ragDocs.length === 0 ? (
              <p className="rag-empty">
                No documents ingested yet. Use the buttons above to add files.
              </p>
            ) : (
              <div className="rag-doc-list">
                {ragDocs.map((doc) => {
                  const isPdf = doc.file_path?.toLowerCase().endsWith(".pdf");
                  return (
                  <div
                    key={doc.doc_id}
                    className={`rag-doc-item${isPdf ? " clickable" : ""}`}
                    onClick={() => isPdf && openPdfViewer(doc)}
                    title={isPdf ? "Click to view PDF" : doc.title}
                  >
                    <FileText size={14} className="doc-icon" />
                    <span className="doc-title">{doc.title}</span>
                    <span className="doc-meta">{doc.total_chunks} chunks</span>
                    <span
                      className={`doc-phase ${doc.phase.includes("phase2_complete") ? "complete" : ""}`}
                    >
                      {doc.phase.replace(/_/g, " ")}
                    </span>
                    <button
                      onClick={(e) => { e.stopPropagation(); deleteRagDoc(doc.doc_id); }}
                      className="doc-delete"
                      title="Remove document"
                    >
                      <Trash2 size={12} />
                    </button>
                  </div>
                  );
                })}
              </div>
            )}
          </div>

          {/* RAG Source Citations */}
          {ragResult && ragResult.sources.length > 0 && (
            <div className="kb-sidebar-sources">
              <div className="rag-sources-header">
                <FileText size={14} />
                <strong>Sources ({ragResult.sources.length})</strong>
              </div>
              {ragResult.sources.map((src, i) => (
                <details key={src.chunk_id} className="source-item">
                  <summary>
                    [Source {i + 1}] {src.doc_title} (score: {src.score.toFixed(4)})
                  </summary>
                  <pre className="source-text">{src.text}</pre>
                </details>
              ))}
            </div>
          )}
        </div>
      </aside>
    </div>
  );
}

export default App;
