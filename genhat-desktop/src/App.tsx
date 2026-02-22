import { useState, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  MessageSquare,
  Eye,
  Volume2,
  BookOpen,
  Plus,
  ImageIcon,
  FileText,
  FolderOpen,
  X,
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
} from "./types";
import ChatWindow from "./components/ChatWindow";
import ModelSelector from "./components/ModelSelector";
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
  { mode: "rag", label: "RAG", icon: BookOpen, desc: "Document Q&A" },
];

function App() {
  // ── Model state ────────────────────────────────────────────────────────────
  const [models, setModels] = useState<ModelFile[]>([]);
  const [selectedModel, setSelectedModel] = useState("");

  const [audioModels, setAudioModels] = useState<ModelFile[]>([]);
  const [selectedAudioModel, setSelectedAudioModel] = useState("None");

  const [visionModels, setVisionModels] = useState<RegisteredModel[]>([]);
  const [selectedVisionModel, setSelectedVisionModel] = useState("");

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

  // ── Lifecycle ──────────────────────────────────────────────────────────────

  useEffect(() => {
    refreshModels();
    return () => {
      visionUnlistenRef.current?.();
      visionUnlistenRef.current = null;
    };
  }, []);

  // Load RAG docs when switching to RAG mode
  useEffect(() => {
    if (chatMode === "rag") loadRagDocs();
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
          if (chatMode === "rag") loadRagDocs();
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

    Api.listAudioModels()
      .then((list) => setAudioModels(list))
      .catch(console.error);

    Api.listRegisteredModels()
      .then((list) => {
        const vision = list.filter((m) => m.tasks.includes("vision_chat"));
        setVisionModels(vision);
        if (vision.length > 0) setSelectedVisionModel(vision[0].id);
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
      // ── RAG Mode (streaming) ────────────────────────────────────────────
      if (chatMode === "rag") {
        try {
          // Phase 1: Retrieval — sources come back immediately
          const setup = await Api.queryRagStream(text);
          setRagResult({ answer: "", sources: setup.sources });

          // Handle empty results
          if (!setup.prompt) {
            const msg =
              setup.sources.length === 0
                ? "No relevant documents found. Please ingest some documents first."
                : "";
            setMessages((prev) => [
              ...prev,
              { role: "assistant", content: msg },
            ]);
            setLoading(false);
            return;
          }

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
              setMessages((prev) => [
                ...prev,
                { role: "assistant", content: fullAnswer },
              ]);
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
        } catch (e) {
          console.error(e);
          setMessages((prev) => [
            ...prev,
            { role: "assistant", content: `RAG query error: ${e}` },
          ]);
          setLoading(false);
        }
        return;
      }

      // ── Audio Mode ──────────────────────────────────────────────────────
      if (chatMode === "audio" && selectedAudioModel !== "None") {
        try {
          const audioUrl = await Api.generateSpeech(selectedAudioModel, text);
          setAudioOutput(audioUrl);
          setMessages((prev) => [
            ...prev,
            { role: "assistant", content: "🔊 Audio generated successfully." },
          ]);
        } catch (e) {
          console.error(e);
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
          // Clean up any previous listener
          visionUnlistenRef.current?.();
          visionUnlistenRef.current = null;

          let visionResponse = "";

          const unlisten = await listen<{ chunk: string; done: boolean }>(
            "vision-stream",
            (event) => {
              if (event.payload.done) {
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
      let fullResponse = "";
      Api.streamChat(
        [...messages, newMsg],
        (chunk) => {
          setStreamingContent((prev) => prev + chunk);
          fullResponse += chunk;
        },
        () => {
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
      case "rag":
        return "Ask a question about your documents...";
      default:
        return "Message NELA...";
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
            {chatMode === "audio" && (
              <ModelSelector
                models={audioModels}
                selectedModel={selectedAudioModel}
                onSelect={setSelectedAudioModel}
                type="audio"
                onAdd={handleAddModel}
              />
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

        {/* ── RAG Panel ── */}
        {chatMode === "rag" && (
          <div className="context-panel">
            <div className="context-panel-header">
              <BookOpen size={16} strokeWidth={1.8} />
              <span>Knowledge Base</span>
              <div className="context-panel-actions">
                <button
                  onClick={ingestFile}
                  disabled={ragIngesting}
                  className="action-btn"
                >
                  <FileText size={14} />
                  Add File
                </button>
                <button
                  onClick={ingestDir}
                  disabled={ragIngesting}
                  className="action-btn"
                >
                  <FolderOpen size={14} />
                  Add Folder
                </button>
              </div>
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
            <div className="context-panel-body">
              {ragDocs.length === 0 ? (
                <p className="rag-empty">
                  No documents ingested yet. Add files to build your knowledge base.
                </p>
              ) : (
                <div className="rag-doc-list">
                  {ragDocs.map((doc) => (
                    <div key={doc.doc_id} className="rag-doc-item">
                      <FileText size={14} className="doc-icon" />
                      <span className="doc-title">{doc.title}</span>
                      <span className="doc-meta">
                        {doc.total_chunks} chunks
                      </span>
                      <span className="doc-meta">
                        {doc.enriched_chunks}/{doc.total_chunks} enriched
                      </span>
                      <span
                        className={`doc-phase ${
                          doc.phase.includes("phase2_complete") ? "complete" : ""
                        }`}
                      >
                        {doc.phase.replace(/_/g, " ")}
                      </span>
                      <button
                        onClick={() => deleteRagDoc(doc.doc_id)}
                        className="doc-delete"
                        title="Remove document"
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                  ))}
                </div>
              )}
              {/* RAG Source Citations */}
              {ragResult && ragResult.sources.length > 0 && (
                <div className="rag-sources">
                  <div className="rag-sources-header">
                    <FileText size={14} />
                    <strong>Sources ({ragResult.sources.length})</strong>
                  </div>
                  {ragResult.sources.map((src, i) => (
                    <details key={src.chunk_id} className="source-item">
                      <summary>
                        [Source {i + 1}] {src.doc_title} (score:{" "}
                        {src.score.toFixed(4)})
                      </summary>
                      <pre className="source-text">{src.text}</pre>
                    </details>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}

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
        />
      </main>
    </div>
  );
}

export default App;
