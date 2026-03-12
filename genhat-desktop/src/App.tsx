import { useState, useEffect, useRef, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  MessageSquare,
  Eye,
  Volume2,
  Mic,
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
  ChatSession,
  ModelFile,
  RegisteredModel,
  IngestionStatus,
  KittenTtsVoice,
} from "./types";
import { KITTEN_TTS_VOICES } from "./types";
import ChatWindow from "./components/ChatWindow";
import ChatTabBar from "./components/ChatTabBar";
import ModelSelector from "./components/ModelSelector";
import PdfViewer from "./components/PdfViewer";
import DocumentViewer from "./components/DocumentViewer";
import PodcastTab from "./components/PodcastTab";
import "./App.css";

/** Extensions the DocumentViewer can render (non-PDF). */
const VIEWABLE_EXTS = new Set([
  "docx", "pptx", "xlsx", "xls", "ods",
  "txt", "md", "csv", "tsv", "json", "xml", "html", "htm",
  "rs", "py", "js", "ts", "jsx", "tsx", "java", "c", "cpp",
  "h", "go", "rb", "sh", "css", "scss", "sql", "log", "ini", "cfg",
  "toml", "yaml", "yml",
  "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg",
  "mp3", "wav", "ogg", "m4a", "flac",
]);

/* ── Mode metadata for the sidebar ──────────────────────────────────────── */

/** Turn raw page_info metadata (e.g. "page:3", "slide:2") into a readable label. */
function formatPageLabel(meta?: string): string {
  if (!meta) return "";
  if (meta.startsWith("page:")) return `Page ${meta.split(":")[1]}`;
  if (meta.startsWith("slide:")) return `Slide ${meta.split(":")[1]}`;
  if (meta.startsWith("paragraph:")) return `Paragraph ${meta.split(":")[1]}`;
  return meta;
}

const MODE_CONFIG: {
  mode: ChatMode;
  label: string;
  icon: React.ElementType;
  desc: string;
}[] = [
    { mode: "text", label: "Chat", icon: MessageSquare, desc: "Text conversation" },
    { mode: "vision", label: "Vision", icon: Eye, desc: "Image analysis" },
    { mode: "audio", label: "Audio", icon: Volume2, desc: "Text to speech" },
    { mode: "podcast", label: "Podcast", icon: Mic, desc: "AI podcast generation" },
  ];

// ── Session helpers (pure functions, no hooks) ──────────────────────────────

/** Create a fresh, empty ChatSession with a unique ID. */
function createEmptySession(): ChatSession {
  return {
    id: crypto.randomUUID(),
    title: "New Chat",
    messages: [],
    streamingContent: "",
    loading: false,
    audioOutput: "",
    cancelled: false,
    ragResult: null,
    mediaAssets: {},
    createdAt: Date.now(),
  };
}

/** Derive a short title from the first user message in a session. */
function deriveTitleFromMessage(text: string): string {
  const trimmed = text.trim().replace(/\n+/g, " ");
  return trimmed.length > 32 ? trimmed.slice(0, 32) + "…" : trimmed || "New Chat";
}

function App() {
  // ── Model state ────────────────────────────────────────────────────────────
  const [models, setModels] = useState<ModelFile[]>([]);
  const [selectedModel, setSelectedModel] = useState("");

  // ── TTS engine state (registered models with TTS task) ─────────────────
  const [ttsEngines, setTtsEngines] = useState<RegisteredModel[]>([]);
  const [selectedTtsEngine, setSelectedTtsEngine] = useState("");
  const [ttsVoice, setTtsVoice] = useState<KittenTtsVoice>("Leo");
  const [ttsSpeed, setTtsSpeed] = useState(1.0);
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const [_ttsGenerating, setTtsGenerating] = useState(false);
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const [_ttsElapsedTime, setTtsElapsedTime] = useState(0);
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const [_ttsGenerationTime, setTtsGenerationTime] = useState<number | null>(null);
  const ttsIntervalRef = useRef<NodeJS.Timeout | null>(null);

  const [visionModels, setVisionModels] = useState<RegisteredModel[]>([]);
  const [selectedVisionModel, setSelectedVisionModel] = useState("");

  // ── Response time tracking for all modes ────────────────────────────────────
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const [_generalElapsedTime, setGeneralElapsedTime] = useState(0);
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const [_generalGenerationTime, setGeneralGenerationTime] = useState<number | null>(null);
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const [_generalGenerating, setGeneralGenerating] = useState(false);
  const generalIntervalRef = useRef<NodeJS.Timeout | null>(null);

  // ── Multi-session chat state ───────────────────────────────────────────────
  const [chatMode, setChatMode] = useState<ChatMode>("text");
  const [sessions, setSessions] = useState<ChatSession[]>(() => [createEmptySession()]);
  const [activeSessionId, setActiveSessionId] = useState<string>(() => sessions[0]?.id ?? "");
  /** AbortControllers keyed by session ID — persists across renders. */
  const abortControllersRef = useRef<Map<string, AbortController>>(new Map());

  // ── Vision state ───────────────────────────────────────────────────────────
  const [imagePath, setImagePath] = useState<string | null>(null);
  const [imagePreview, setImagePreview] = useState<string | null>(null);
  const visionUnlistenRef = useRef<(() => void) | null>(null);

  // ── RAG state ──────────────────────────────────────────────────────────────
  const [ragDocs, setRagDocs] = useState<IngestionStatus[]>([]);
  const [ragIngesting, setRagIngesting] = useState(false);
  const [enrichmentStatus, setEnrichmentStatus] = useState<string | null>(null);

  // ── Right sidebar (Knowledge Base) ─────────────────────────────────────────
  const [docPanelOpen, setDocPanelOpen] = useState(false);

  // ── Session accessor helpers ───────────────────────────────────────────────

  /** Get the currently active session object (read-only snapshot). */
  const activeSession: ChatSession = sessions.find((s) => s.id === activeSessionId) ?? sessions[0];

  /** Immutably update a specific session by ID. */
  const updateSession = useCallback(
    (sessionId: string, patch: Partial<ChatSession> | ((prev: ChatSession) => Partial<ChatSession>)) => {
      setSessions((prev) =>
        prev.map((s) => {
          if (s.id !== sessionId) return s;
          const changes = typeof patch === "function" ? patch(s) : patch;
          return { ...s, ...changes };
        })
      );
    },
    []
  );

  /** Update only the active session (convenience wrapper). */
  const updateActiveSession = useCallback(
    (patch: Partial<ChatSession> | ((prev: ChatSession) => Partial<ChatSession>)) => {
      updateSession(activeSessionId, patch);
    },
    [activeSessionId, updateSession]
  );

  /** Create a new session, add it to the list, and activate it. */
  const addNewSession = useCallback(() => {
    const newSession = createEmptySession();
    setSessions((prev) => [...prev, newSession]);
    setActiveSessionId(newSession.id);
  }, []);

  /** Close a session by ID. If it's the last one, create a fresh session. */
  const closeSession = useCallback(
    (sessionId: string) => {
      // Abort any in-flight request for this session
      abortControllersRef.current.get(sessionId)?.abort();
      abortControllersRef.current.delete(sessionId);

      setSessions((prev) => {
        const remaining = prev.filter((s) => s.id !== sessionId);
        if (remaining.length === 0) {
          // Last tab closed — create a fresh one
          const fresh = createEmptySession();
          setActiveSessionId(fresh.id);
          return [fresh];
        }
        // If the closed tab was active, activate the nearest neighbor
        if (sessionId === activeSessionId) {
          const closedIdx = prev.findIndex((s) => s.id === sessionId);
          const nextIdx = Math.min(closedIdx, remaining.length - 1);
          setActiveSessionId(remaining[nextIdx].id);
        }
        return remaining;
      });
    },
    [activeSessionId]
  );

  /** Reorder sessions (called from drag-and-drop in the tab bar). */
  const reorderSessions = useCallback((reordered: ChatSession[]) => {
    setSessions(reordered);
  }, []);

  // ── PDF Viewer state ───────────────────────────────────────────────────────
  const [pdfViewerData, setPdfViewerData] = useState<{
    data: string;
    title: string;
  } | null>(null);
  const [pdfLoading, setPdfLoading] = useState(false);

  // ── Document Viewer state (non-PDF formats) ────────────────────────────
  const [docViewerFile, setDocViewerFile] = useState<{
    filePath: string;
    title: string;
  } | null>(null);

  // ── Keyboard shortcut: Ctrl+T to open a new tab ──────────────────────────
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "t") {
        e.preventDefault();
        addNewSession();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [addNewSession]);

  // ── Lifecycle ──────────────────────────────────────────────────────────────

  useEffect(() => {
    refreshModels();
    loadRagDocs(); // Always load RAG docs on startup
    return () => {
      visionUnlistenRef.current?.();
      visionUnlistenRef.current = null;
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
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
    Api.listRegisteredModels()
      .then((list) => {
        // Vision models
        const vision = list.filter((m) => m.tasks.includes("vision_chat"));
        setVisionModels(vision);
        if (vision.length > 0) setSelectedVisionModel(vision[0].id);

        // Text models
        const chatModels = list
          .filter((m) => m.tasks.includes("chat"))
          .sort((a, b) => b.priority - a.priority)
          .map((m) => ({ name: m.name, path: m.id }));
        
        setModels(chatModels);
        if (chatModels.length > 0 && !selectedModel) {
          setSelectedModel(chatModels[0].path);
          // No need to call switchModel here — the backend already defaults
          // to the highest-priority chat model from models.toml on startup.
        }

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
      // Clear messages in the active session only when switching models
      updateActiveSession({ messages: [], streamingContent: "", ragResult: null, mediaAssets: {} });
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
              "pdf", "docx", "pptx", "xlsx", "xls", "ods",
              "txt", "md", "csv", "tsv", "json", "xml", "html", "htm",
              "rs", "py", "js", "ts", "jsx", "tsx", "java", "c", "cpp",
              "h", "go", "rb", "sh", "toml", "yaml", "yml", "css",
              "scss", "sql", "log", "ini", "cfg",
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

  // ── Viewer handlers ────────────────────────────────────────────────────────

  /** Open the right viewer for any supported document format. */
  const openDocViewer = async (doc: IngestionStatus) => {
    const ext = doc.file_path.split(".").pop()?.toLowerCase() || "";

    if (ext === "pdf") {
      // PDF uses the dedicated PdfViewer
      try {
        setPdfLoading(true);
        const data = await Api.readFileBase64(doc.file_path);
        setPdfViewerData({ data, title: doc.title });
        setDocViewerFile(null); // Clear any other open viewer
      } catch (e) {
        console.error("Failed to load PDF:", e);
        alert(`Failed to open PDF: ${e}`);
      } finally {
        setPdfLoading(false);
      }
    } else if (VIEWABLE_EXTS.has(ext)) {
      // Everything else uses the universal DocumentViewer
      setDocViewerFile({ filePath: doc.file_path, title: doc.title });
      setPdfViewerData(null); // Clear any PDF viewer
    }
  };

  const closePdfViewer = () => {
    setPdfViewerData(null);
  };

  const closeDocViewer = () => {
    setDocViewerFile(null);
  };

  // ── Cancel handler ────────────────────────────────────────────────────────

  const handleCancel = () => {
    const sid = activeSessionId;
    // Abort the SSE fetch (text / RAG modes)
    abortControllersRef.current.get(sid)?.abort();
    abortControllersRef.current.delete(sid);
    // Stop vision event listener
    visionUnlistenRef.current?.();
    visionUnlistenRef.current = null;
    // Commit whatever partial response was received
    updateSession(sid, (prev) => ({
      messages: prev.streamingContent
        ? [...prev.messages, { role: "assistant" as const, content: prev.streamingContent }]
        : prev.messages,
      streamingContent: "",
      loading: false,
      cancelled: true,
    }));
  };

  // ── Main send handler ─────────────────────────────────────────────────────

  const handleSend = async (text: string) => {
    // Snapshot the session ID at the time of send — this ensures all async
    // callbacks write to the correct session even if the user switches tabs.
    const sid = activeSessionId;
    const session = sessions.find((s) => s.id === sid);
    if (!session || session.loading) return; // Prevent concurrent requests in the same session

    const newMsg: ChatMessage = { role: "user", content: text };

    // Derive title from first user message
    const isFirstMessage = session.messages.length === 0;
    const titlePatch = isFirstMessage ? { title: deriveTitleFromMessage(text) } : {};

    updateSession(sid, (prev) => ({
      messages: [...prev.messages, newMsg],
      loading: true,
      streamingContent: "",
      audioOutput: "",
      cancelled: false,
      ...titlePatch,
    }));

    // Create a fresh AbortController for this session's request
    const ctrl = new AbortController();
    abortControllersRef.current.set(sid, ctrl);

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
          updateSession(sid, { ragResult: { answer: "", sources: setup.sources } });

          // If no relevant docs found, fall through to normal text chat
          if (!setup.prompt || setup.sources.length === 0) {
            // Fall through to normal text chat below
          } else {
            // Phase 2: Stream the answer from llama-server SSE
            let fullAnswer = "";
            let firstTokenTimeMs: number | null = null;
            await Api.streamChat(
              [
                { role: "system", content: "You are a helpful assistant. Answer the question using the provided reference text. Write a clear, natural response without repeating source labels, tags, or brackets. If the user asks for a specific format (table, list, bullet points, etc.), use that format. If the reference text does not cover the question, say you don't know." },
                { role: "user", content: setup.prompt },
              ],
              (chunk) => {
                if (firstTokenTimeMs === null) {
                  firstTokenTimeMs = Date.now();
                }
                fullAnswer += chunk;
                updateSession(sid, (prev) => ({ streamingContent: prev.streamingContent + chunk }));
              },
              () => {
                // Stop timer
                if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
                const totalTime = Math.floor((Date.now() - ragStartTime) / 100) / 10;
                const timeToFirstToken = firstTokenTimeMs ? Math.floor((firstTokenTimeMs - ragStartTime) / 100) / 10 : null;

                setGeneralGenerating(false);
                setGeneralElapsedTime(totalTime);
                setGeneralGenerationTime(totalTime);

                updateSession(sid, (prev) => {
                  const updated: ChatMessage[] = [
                    ...prev.messages,
                    {
                      role: "assistant",
                      content: fullAnswer,
                      generateTime: totalTime,
                      firstTokenTime: timeToFirstToken !== null ? timeToFirstToken : undefined
                    },
                  ];
                  const assistantIdx = updated.length - 1;
                  Api.retrieveMediaForResponse(fullAnswer)
                    .then((assets) => {
                      console.log(`Media retrieval: found ${assets.length} assets`);
                      if (assets.length > 0) {
                        updateSession(sid, (prev2) => ({
                          mediaAssets: {
                            ...prev2.mediaAssets,
                            [assistantIdx]: assets,
                          },
                        }));
                      }
                    })
                    .catch((e) =>
                      console.error("Media retrieval failed:", e)
                    );
                  return {
                    messages: updated,
                    ragResult: prev.ragResult ? { ...prev.ragResult, answer: fullAnswer } : null,
                    streamingContent: "",
                    loading: false,
                  };
                });
              },
              (err) => {
                console.error("RAG stream error:", err);
                updateSession(sid, (prev) => ({
                  messages: [
                    ...prev.messages,
                    { role: "assistant" as const, content: `RAG query error: ${err}` },
                  ],
                  loading: false,
                }));
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

          updateSession(sid, (prev) => ({
            audioOutput: audioUrl,
            messages: [
              ...prev.messages,
              {
                role: "assistant" as const,
                content: `🔊 Audio generated (${ttsVoice}, ${ttsSpeed}x speed).`,
                generateTime: totalTime
              },
            ],
          }));
        } catch (e) {
          console.error(e);
          // Stop timer on error
          if (ttsIntervalRef.current) clearInterval(ttsIntervalRef.current);
          setTtsGenerating(false);
          updateSession(sid, (prev) => ({
            messages: [
              ...prev.messages,
              { role: "assistant" as const, content: `Error generating audio: ${e}` },
            ],
          }));
        }
        updateSession(sid, { loading: false });
        return;
      }

      // ── Vision Mode (streaming via Tauri events) ────────────────────────
      if (chatMode === "vision") {
        if (!imagePath) {
          updateSession(sid, (prev) => ({
            messages: [
              ...prev.messages,
              { role: "assistant" as const, content: "Please select an image first." },
            ],
            loading: false,
          }));
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
          let firstTokenTimeMs: number | null = null;

          const unlisten = await listen<{ chunk: string; done: boolean }>(
            "vision-stream",
            (event) => {
              if (event.payload.done) {
                // Stop timer
                if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
                const totalTime = Math.floor((Date.now() - startTime) / 100) / 10;
                const timeToFirstToken = firstTokenTimeMs ? Math.floor((firstTokenTimeMs - startTime) / 100) / 10 : null;

                setGeneralGenerating(false);
                setGeneralElapsedTime(totalTime);
                setGeneralGenerationTime(totalTime);

                if (visionResponse) {
                  updateSession(sid, (prev) => ({
                    messages: [
                      ...prev.messages,
                      {
                        role: "assistant" as const,
                        content: visionResponse,
                        generateTime: totalTime,
                        firstTokenTime: timeToFirstToken !== null ? timeToFirstToken : undefined
                      },
                    ],
                    streamingContent: "",
                    loading: false,
                  }));
                } else {
                  updateSession(sid, { loading: false });
                }
                visionUnlistenRef.current?.();
                visionUnlistenRef.current = null;
              } else if (event.payload.chunk) {
                if (firstTokenTimeMs === null) {
                  firstTokenTimeMs = Date.now();
                }
                visionResponse += event.payload.chunk;
                updateSession(sid, (prev) => ({ streamingContent: prev.streamingContent + event.payload.chunk }));
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
          updateSession(sid, (prev) => ({
            messages: [
              ...prev.messages,
              { role: "assistant" as const, content: `Vision error: ${e}` },
            ],
            loading: false,
          }));
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
      let textFirstTokenTimeMs: number | null = null;

      // Build the messages array from the session's messages (including the new user msg)
      const sessionMessages = session.messages;
      Api.streamChat(
        [...sessionMessages, newMsg],
        (chunk) => {
          if (textFirstTokenTimeMs === null) {
            textFirstTokenTimeMs = Date.now();
          }
          updateSession(sid, (prev) => ({ streamingContent: prev.streamingContent + chunk }));
          fullResponse += chunk;
        },
        () => {
          // Stop timer
          if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
          const totalTime = Math.floor((Date.now() - chatStartTime) / 100) / 10;
          const timeToFirstToken = textFirstTokenTimeMs ? Math.floor((textFirstTokenTimeMs - chatStartTime) / 100) / 10 : null;

          setGeneralGenerating(false);
          setGeneralElapsedTime(totalTime);
          setGeneralGenerationTime(totalTime);

          if (fullResponse) {
            updateSession(sid, (prev) => ({
              messages: [
                ...prev.messages,
                {
                  role: "assistant" as const,
                  content: fullResponse,
                  generateTime: totalTime,
                  firstTokenTime: timeToFirstToken !== null ? timeToFirstToken : undefined
                },
              ],
              streamingContent: "",
              loading: false,
            }));
          } else {
            updateSession(sid, { loading: false });
          }
        },
        (err) => {
          // Stop timer on error
          if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
          setGeneralGenerating(false);
          console.error("Stream error", err);
          updateSession(sid, (prev) => ({
            messages: [
              ...prev.messages,
              { role: "assistant" as const, content: `Error: ${err}` },
            ],
            loading: false,
          }));
        },
        undefined,
        ctrl.signal
      );
    } catch (err) {
      // Stop timer on error
      if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
      setGeneralGenerating(false);
      console.error(err);
      updateSession(sid, (prev) => ({
        messages: [
          ...prev.messages,
          { role: "assistant" as const, content: "An unexpected error occurred." },
        ],
        loading: false,
      }));
    }
  };

  // ── Derived ────────────────────────────────────────────────────────────────

  const getPlaceholder = (): string => {
    switch (chatMode) {
      case "vision":
        return "Ask about the image (e.g., 'What's in this image?')";
      case "audio":
        return "Type text to generate speech...";
      case "podcast":
        return "What topic should the podcast cover?";
      default:
        return ragDocs.length > 0
          ? "Ask about your documents or chat freely..."
          : "Message NELA...";
    }
  };

  // ── Render ─────────────────────────────────────────────────────────────────

  const currentModeConfig = MODE_CONFIG.find((m) => m.mode === chatMode)!;

  return (
    <div className="flex h-full w-full">
      {/* ══════════ LEFT NAV SIDEBAR ══════════ */}
      <nav className="w-[68px] bg-void-800 border-r border-glass-border flex flex-col items-center py-3 shrink-0 z-30 gap-1">
        {/* Brand */}
        <div className="pt-2 pb-4 mb-1">
          <img
            src="/logo-dark.png"
            alt="NELA"
            className="w-10 h-10 rounded-xl object-contain shadow-[0_4px_20px_rgba(0,212,255,0.3)]"
            draggable={false}
          />
        </div>

        {/* Mode Buttons */}
        <div className="flex flex-col gap-0.5 w-full px-2">
          {MODE_CONFIG.map(({ mode, label, icon: Icon, desc }) => (
            <button
              key={mode}
              className={`nav-mode-btn flex flex-col items-center gap-1 py-2.5 px-1 rounded-xl border transition-all duration-200 relative w-full ${chatMode === mode
                ? "active bg-neon-subtle text-neon border-neon/30 shadow-[0_0_12px_rgba(0,212,255,0.12)]"
                : "bg-transparent border-transparent text-txt-muted hover:bg-glass-hover hover:text-txt-secondary hover:border-glass-border"
                }`}
              onClick={() => setChatMode(mode)}
              title={desc}
            >
              <Icon size={20} strokeWidth={1.8} />
              <span className="text-[0.6rem] font-semibold uppercase tracking-wide leading-none">{label}</span>
            </button>
          ))}
        </div>

        {/* Spacer */}
        <div className="flex-1" />

        {/* New Chat */}
        <button
          className="glass-btn flex flex-col items-center gap-1 py-2.5 px-1 mx-2 mb-2 bg-transparent border border-dashed border-glass-border rounded-xl text-txt-muted cursor-pointer transition-all duration-200 w-[calc(100%-16px)] hover:border-neon hover:text-neon hover:bg-neon-subtle hover:shadow-[0_0_12px_rgba(0,212,255,0.1)]"
          onClick={addNewSession}
          title="New conversation (Ctrl+T)"
        >
          <Plus size={18} strokeWidth={2} />
          <span className="text-[0.6rem] font-semibold uppercase tracking-wide leading-none">New Chat</span>
        </button>
      </nav>

      {/* ══════════ MAIN CONTENT ══════════ */}
      <main className="flex-1 flex flex-col bg-void-900 min-w-0 relative">
        {/* ── Tab Bar (multi-session) ── */}
        {chatMode !== "podcast" && (
          <ChatTabBar
            sessions={sessions}
            activeSessionId={activeSessionId}
            onSelectSession={setActiveSessionId}
            onNewSession={addNewSession}
            onCloseSession={closeSession}
            onReorderSessions={reorderSessions}
          />
        )}

        {/* ── Top Bar ── */}
        <header className="h-14 flex items-center justify-between px-6 border-b border-glass-border bg-void-800/80 backdrop-blur-xl shrink-0 z-20">
          <div className="flex items-center gap-2.5">
            <currentModeConfig.icon size={18} strokeWidth={1.8} className="text-neon" />
            <h1 className="text-[0.95rem] font-semibold m-0 text-txt">{currentModeConfig.label}</h1>
            <span className="text-[0.78rem] text-txt-muted pl-2.5 border-l border-glass-border">{currentModeConfig.desc}</span>
          </div>

          <div className="flex items-center gap-3">
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
              <div className="flex items-center gap-2.5">
                <div className="relative flex items-center">
                  <select
                    value={selectedTtsEngine}
                    onChange={(e) => setSelectedTtsEngine(e.target.value)}
                    className="bg-void-700 text-txt border border-glass-border rounded-lg py-1.5 pl-3.5 pr-8 font-inherit text-sm outline-none cursor-pointer appearance-none transition-all duration-200 min-w-[160px] hover:border-neon"
                    disabled={activeSession.loading}
                  >
                    {ttsEngines.map((m) => (
                      <option key={m.id} value={m.id}>{m.name}</option>
                    ))}
                  </select>
                  <ChevronDown size={14} className="absolute right-2.5 pointer-events-none text-txt-muted" />
                </div>

                {selectedTtsEngine === "kitten-tts" && (
                  <>
                    <div className="relative flex items-center">
                      <select
                        value={ttsVoice}
                        onChange={(e) => setTtsVoice(e.target.value as KittenTtsVoice)}
                        className="bg-void-700 text-txt border border-glass-border rounded-lg py-1.5 pl-3.5 pr-8 font-inherit text-sm outline-none cursor-pointer appearance-none transition-all duration-200 min-w-[100px] hover:border-neon"
                        disabled={activeSession.loading}
                      >
                        {KITTEN_TTS_VOICES.map((v) => (
                          <option key={v} value={v}>{v}</option>
                        ))}
                      </select>
                      <ChevronDown size={14} className="absolute right-2.5 pointer-events-none text-txt-muted" />
                    </div>

                    <div className="flex items-center gap-1.5">
                      <label className="text-[0.78rem] text-txt-secondary min-w-[32px] text-right select-none" title="Speaking speed">
                        {ttsSpeed.toFixed(1)}x
                      </label>
                      <input
                        type="range" min="0.5" max="2.0" step="0.1"
                        value={ttsSpeed}
                        onChange={(e) => setTtsSpeed(parseFloat(e.target.value))}
                        className="w-[72px] h-1 accent-neon cursor-pointer"
                        disabled={activeSession.loading}
                      />
                    </div>
                  </>
                )}
              </div>
            )}
            {chatMode === "vision" && visionModels.length > 0 && (
              <div className="relative flex items-center">
                <select
                  value={selectedVisionModel}
                  onChange={(e) => setSelectedVisionModel(e.target.value)}
                  className="bg-void-700 text-txt border border-glass-border rounded-lg py-1.5 pl-3.5 pr-8 font-inherit text-sm outline-none cursor-pointer appearance-none transition-all duration-200 min-w-[160px] hover:border-neon"
                  disabled={activeSession.loading}
                >
                  {visionModels.map((m) => (
                    <option key={m.id} value={m.id}>{m.name}</option>
                  ))}
                </select>
                <ChevronDown size={14} className="absolute right-2.5 pointer-events-none text-txt-muted" />
              </div>
            )}
          </div>
        </header>

        {/* ── Vision Panel ── */}
        {chatMode === "vision" && (
          <div className="border-b border-glass-border bg-void-800 overflow-hidden animate-panel-slide">
            <div className="flex items-center gap-2 py-2.5 px-6 text-[0.82rem] font-semibold text-txt-secondary border-b border-glass-border">
              <ImageIcon size={16} strokeWidth={1.8} />
              <span>Image Input</span>
            </div>
            <div className="p-3 px-6 max-h-[250px] overflow-y-auto">
              <div className="flex gap-2.5 items-center mb-2.5">
                <button onClick={selectImage} disabled={activeSession.loading}
                  className="glass-btn inline-flex items-center gap-1.5 py-1.5 px-4 text-[0.78rem] font-medium rounded-lg cursor-pointer text-txt-secondary border border-glass-border transition-all duration-200 hover:text-txt hover:border-neon hover:shadow-[0_0_12px_rgba(0,212,255,0.1)] disabled:opacity-45 disabled:cursor-not-allowed">
                  <ImageIcon size={14} /> Select Image
                </button>
                {imagePath && (
                  <button onClick={clearImage} disabled={activeSession.loading}
                    className="glass-btn inline-flex items-center gap-1.5 py-1.5 px-4 text-[0.78rem] font-medium rounded-lg cursor-pointer text-[#fca5a5] border border-[rgba(248,113,113,0.2)] transition-all duration-200 hover:bg-[rgba(248,113,113,0.1)] hover:border-[#f87171] hover:shadow-[0_0_12px_rgba(248,113,113,0.15)] disabled:opacity-45 disabled:cursor-not-allowed">
                    <X size={14} /> Clear
                  </button>
                )}
                {imagePath && (
                  <span className="inline-flex items-center py-0.5 px-2.5 bg-void-700 border border-glass-border rounded-full text-[0.72rem] text-txt-secondary max-w-[200px] overflow-hidden text-ellipsis whitespace-nowrap">
                    {imagePath.split(/[/\\]/).pop()}
                  </span>
                )}
              </div>
              {imagePreview && (
                <img src={imagePreview} alt="Selected" className="max-w-full max-h-[180px] rounded-xl object-contain border border-glass-border" />
              )}
            </div>
          </div>
        )}

        {/* ── Podcast Mode ── */}
        {chatMode === "podcast" ? (
          <PodcastTab hasDocuments={ragDocs.length > 0} />
        ) : (
          <ChatWindow
            key={activeSession.id}
            messages={activeSession.messages}
            streamingContent={activeSession.streamingContent}
            isLoading={activeSession.loading}
            onSend={handleSend}
            onCancel={handleCancel}
            cancelled={activeSession.cancelled}
            audioSrc={activeSession.audioOutput}
            placeholder={getPlaceholder()}
            mediaAssets={activeSession.mediaAssets}
            ragDocs={ragDocs}
            ragIngesting={ragIngesting}
            enrichmentStatus={enrichmentStatus}
            onIngestFile={ingestFile}
            onIngestDir={ingestDir}
            onToggleDocPanel={() => setDocPanelOpen((v) => !v)}
            showRagControls={chatMode === "text"}
            docPanelOpen={docPanelOpen}
          />
        )}

        {/* ── PDF Viewer Overlay ── */}
        {pdfLoading && (
          <div className="absolute inset-0 z-[55] bg-void-900/80 backdrop-blur-sm flex flex-col items-center justify-center gap-3 text-txt-muted text-sm">
            <div className="pdf-spinner" />
            <span>Loading PDF...</span>
          </div>
        )}
        {pdfViewerData && (
          <PdfViewer pdfData={pdfViewerData.data} title={pdfViewerData.title} onClose={closePdfViewer} />
        )}

        {/* ── Document Viewer Overlay ── */}
        {docViewerFile && (
          <DocumentViewer key={docViewerFile.filePath} filePath={docViewerFile.filePath} title={docViewerFile.title} onClose={closeDocViewer} />
        )}
      </main>

      {/* ══════════ RIGHT SIDEBAR — Knowledge Base ══════════ */}
      <aside className={`kb-sidebar overflow-hidden bg-void-800 flex flex-col shrink-0 ${docPanelOpen ? "w-[320px] min-w-[320px] border-l border-glass-border" : "w-0 min-w-0 border-l-0"}`}>
        <div className={`kb-sidebar-inner flex flex-col h-full w-[320px] ${docPanelOpen ? "opacity-100" : "opacity-0"}`}>
          {/* Header */}
          <div className="flex items-center justify-between py-3.5 px-4 border-b border-glass-border shrink-0">
            <div className="flex items-center gap-2 text-[0.85rem] font-semibold text-txt">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
                <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
              </svg>
              Knowledge Base
            </div>
            <button
              className="glass-btn bg-transparent! border border-transparent! text-txt-muted! cursor-pointer p-1.5! rounded-lg! flex items-center justify-center transition-all duration-200 hover:text-txt! hover:border-glass-border! hover:bg-void-700!"
              onClick={() => setDocPanelOpen(false)}
              title="Close panel"
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>

          {/* Actions */}
          <div className="flex gap-1.5 py-3 px-4 border-b border-glass-border shrink-0">
            <button onClick={ingestFile} disabled={ragIngesting}
              className="glass-btn inline-flex items-center gap-1.5 py-1.5 px-3 text-[0.78rem] font-medium rounded-lg cursor-pointer text-txt-secondary border border-glass-border transition-all duration-200 hover:text-txt hover:border-neon hover:shadow-[0_0_12px_rgba(0,212,255,0.1)] disabled:opacity-45 disabled:cursor-not-allowed">
              <FileText size={14} /> Add File
            </button>
            <button onClick={ingestDir} disabled={ragIngesting}
              className="glass-btn inline-flex items-center gap-1.5 py-1.5 px-3 text-[0.78rem] font-medium rounded-lg cursor-pointer text-txt-secondary border border-glass-border transition-all duration-200 hover:text-txt hover:border-neon hover:shadow-[0_0_12px_rgba(0,212,255,0.1)] disabled:opacity-45 disabled:cursor-not-allowed">
              <FolderOpen size={14} /> Add Folder
            </button>
          </div>

          {/* Status */}
          {(ragIngesting || enrichmentStatus) && (
            <div className="flex items-center gap-2 py-2 px-4 shrink-0">
              {ragIngesting && (
                <span className="inline-flex items-center gap-1.5 py-0.5 px-2.5 rounded-full text-[0.72rem] font-medium bg-[rgba(251,191,36,0.1)] text-warning">
                  <Loader2 size={12} className="spin" /> Ingesting...
                </span>
              )}
              {enrichmentStatus && (
                <span className="inline-flex items-center gap-1.5 py-0.5 px-2.5 rounded-full text-[0.72rem] font-medium bg-[rgba(34,197,94,0.1)] text-success">
                  <CheckCircle2 size={12} /> {enrichmentStatus}
                </span>
              )}
            </div>
          )}

          {/* Document List */}
          <div className="kb-sidebar-docs flex-1 overflow-y-auto p-2">
            {ragDocs.length === 0 ? (
              <p className="text-txt-muted text-[0.82rem] m-1">
                No documents ingested yet. Use the buttons above to add files.
              </p>
            ) : (
              <div className="flex flex-col gap-1">
                {ragDocs.map((doc) => {
                  const ext = doc.file_path?.split(".").pop()?.toLowerCase() || "";
                  const isViewable = ext === "pdf" || VIEWABLE_EXTS.has(ext);
                  return (
                    <div
                      key={doc.doc_id}
                      className={`flex items-center gap-2 py-2 px-2.5 bg-void-700 rounded-lg text-[0.78rem] border border-transparent transition-colors duration-150 flex-wrap hover:border-glass-border ${isViewable ? "cursor-pointer hover:bg-[rgba(0,212,255,0.06)] hover:border-[rgba(0,212,255,0.2)]" : ""}`}
                      onClick={() => isViewable && openDocViewer(doc)}
                      title={isViewable ? `Click to view ${ext.toUpperCase()}` : doc.title}
                    >
                      <FileText size={14} className="text-txt-muted shrink-0" />
                      <span className="flex-1 min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-txt font-medium text-[0.78rem]">{doc.title}</span>
                      <span className="text-txt-muted text-[0.7rem] whitespace-nowrap">{doc.total_chunks} chunks</span>
                      <span className={`py-0.5 px-2 rounded-full text-[0.65rem] font-semibold whitespace-nowrap capitalize ${doc.phase.includes("phase2_complete") ? "bg-[rgba(34,197,94,0.15)] text-success" : "bg-[rgba(0,212,255,0.1)] text-[#66e5ff]"}`}>
                        {doc.phase.replace(/_/g, " ")}
                      </span>
                      <button
                        onClick={(e) => { e.stopPropagation(); deleteRagDoc(doc.doc_id); }}
                        className="p-1! bg-transparent! text-txt-muted! border-none! rounded! cursor-pointer flex items-center justify-center transition-all duration-150 hover:text-danger! hover:bg-[rgba(239,68,68,0.1)]!"
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
          {activeSession.ragResult && activeSession.ragResult.sources.length > 0 && (
            <div className="kb-sidebar-sources border-t border-glass-border py-3 px-3 shrink-0 max-h-[250px] overflow-y-auto">
              <div className="flex items-center gap-1.5 mb-2 text-[0.82rem] text-txt-secondary">
                <FileText size={14} />
                <strong>Sources ({activeSession.ragResult.sources.length})</strong>
              </div>
              {activeSession.ragResult.sources.map((src, i) => (
                <details key={src.chunk_id} className="mb-1 text-[0.78rem]">
                  <summary className="cursor-pointer text-[#66e5ff] py-1 transition-colors duration-150 hover:text-[#99eeff]">
                    [Source {i + 1}] {src.doc_title}
                    {src.page_info ? `, ${formatPageLabel(src.page_info)}` : ""}
                    {" "}(score: {src.score.toFixed(4)})
                  </summary>
                  <pre className="whitespace-pre-wrap text-[0.72rem] text-txt-secondary p-2.5 bg-void-900 border border-glass-border rounded-lg mt-1 max-h-[150px] overflow-y-auto">{src.text}</pre>
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
