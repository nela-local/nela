import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  MessageSquare,
  Eye,
  Volume2,
  Mic,
  FileText,
  FolderOpen,
  Trash2,
  Loader2,
  CheckCircle2,
  Share2,
  SlidersHorizontal,
  ChevronDown,
  ChevronUp,
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
  MindMapGraph,
  MindMapNode,
  PodcastResult,
  WorkspaceRecord,
  ImportModelProfile,
} from "./types";
import { KITTEN_TTS_VOICES } from "./types";
import ChatWindow from "./components/ChatWindow";
import AudioPlayer from "./components/AudioPlayer";
import ChatTabBar from "./components/ChatTabBar";
import ChatHistorySidebar from "./components/ChatHistorySidebar";
import SidebarNav from "./components/SidebarNav";
import ModelSelector from "./components/ModelSelector";
import WorkspaceSelector from "./components/WorkspaceSelector";
import PdfViewer from "./components/PdfViewer";
import DocumentViewer from "./components/DocumentViewer";
import PodcastTab from "./components/PodcastTab";
import MindMapOverlay from "./components/MindMapOverlay";
import StartupModal from "./components/StartupModal";
import ModelsSettingsModal from "./components/ModelsSettingsModal";
import HuggingFaceModal from "./components/HuggingFaceModal";
import ActiveModelParamsDock, { type RuntimeParamsTarget } from "./components/ActiveModelParamsDock";
import AppModal, { type AppModalKind } from "./components/AppModal";
import ToursModal from "./components/ToursModal";
import { useTour } from "./hooks/useTour";
import "./App.css";

const SESSION_STORAGE_PREFIX = "genhat:sessions:v1:";
const STARTUP_OPTIONAL_DOWNLOAD_KEY = "genhat:download-optional-on-start";
const STARTUP_MODEL_SELECTOR = {
  tasks: new Set(["embed", "grade", "classify"]),
  ids: new Set(["kitten-tts", "parakeet-tdt", "qwen3.5-0_8b", "qwen3.5-0_8b-mmproj"]),
};

const formatModelSizeLabel = (memoryMb: number | null | undefined): string => {
  if (typeof memoryMb !== "number" || !Number.isFinite(memoryMb) || memoryMb <= 0) {
    return "Unknown size";
  }
  const mb = memoryMb;
  if (mb >= 1024) return `${(mb / 1024).toFixed(2)} GB`;
  return `${Math.round(mb)} MB`;
};

const formatTotalSizeLabel = (totalMb: number): string => {
  if (totalMb >= 1024) return `${(totalMb / 1024).toFixed(2)} GB`;
  return `${Math.round(totalMb)} MB`;
};

const normalizeModelRef = (raw: string): string => raw.replace(/\\/g, "/").toLowerCase();

const modelRefBasename = (raw: string): string => {
  const normalized = normalizeModelRef(raw);
  const parts = normalized.split("/");
  return parts[parts.length - 1] ?? normalized;
};

const findRegisteredModelByIdentifier = (
  models: RegisteredModel[],
  identifier: string | null | undefined
): RegisteredModel | undefined => {
  if (!identifier) return undefined;

  const exact = models.find((model) => model.id === identifier);
  if (exact) return exact;

  const normalizedIdentifier = normalizeModelRef(identifier);
  const identifierBase = modelRefBasename(identifier);

  return models.find((model) => {
    if (!model.model_file) return false;
    const normalizedFile = normalizeModelRef(model.model_file);
    return (
      normalizedFile === normalizedIdentifier ||
      modelRefBasename(normalizedFile) === identifierBase
    );
  });
};

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
    { mode: "mindmap", label: "Mindmap", icon: Share2, desc: "Visual idea map" },
  ];

function extractTaskText(response: unknown): string {
  if (typeof response === "string") return response;
  if (response && typeof response === "object") {
    const record = response as Record<string, unknown>;
    if (typeof record.Text === "string") return record.Text;
    if (typeof record.Error === "string") throw new Error(record.Error);
  }
  return JSON.stringify(response ?? "");
}

function extractJsonObject(raw: string): string | null {
  const trimmed = raw.trim();
  if (trimmed.startsWith("{")) return trimmed;

  const fenced = trimmed.match(/```(?:json)?\s*([\s\S]*?)\s*```/i);
  if (fenced?.[1]) return fenced[1].trim();

  const start = trimmed.indexOf("{");
  const end = trimmed.lastIndexOf("}");
  if (start >= 0 && end > start) return trimmed.slice(start, end + 1);

  return null;
}

function normalizeMindMapNode(input: unknown): MindMapNode {
  const node = (input ?? {}) as Record<string, unknown>;
  const label = typeof node.label === "string" && node.label.trim().length > 0
    ? node.label.trim()
    : "Untitled";

  const childrenRaw = Array.isArray(node.children) ? node.children : [];
  return {
    id: crypto.randomUUID(),
    label,
    children: childrenRaw.map((child) => normalizeMindMapNode(child)),
  };
}

function parseMindMapGraph(
  raw: string,
  query: string,
  generatedFrom: "documents" | "model",
  sourceCount: number
): MindMapGraph {
  const jsonText = extractJsonObject(raw);
  if (!jsonText) {
    throw new Error("Model did not return JSON mindmap output.");
  }

  const parsed = JSON.parse(jsonText) as Record<string, unknown>;
  const title = typeof parsed.title === "string" && parsed.title.trim().length > 0
    ? parsed.title.trim()
    : query;
  const rootRaw = parsed.root as unknown;
  const root = normalizeMindMapNode(rootRaw ?? { label: title, children: [] });

  return {
    id: crypto.randomUUID(),
    title,
    query,
    generatedFrom,
    sourceCount,
    root,
    createdAt: Date.now(),
  };
}

function normalizeMindMapGraph(raw: unknown): MindMapGraph | null {
  if (!raw || typeof raw !== "object") return null;
  const graph = raw as Partial<MindMapGraph>;
  if (!graph.root || typeof graph.root !== "object") return null;

  return {
    id: typeof graph.id === "string" && graph.id ? graph.id : crypto.randomUUID(),
    title: typeof graph.title === "string" && graph.title ? graph.title : "Mindmap",
    query: typeof graph.query === "string" ? graph.query : "",
    generatedFrom: graph.generatedFrom === "documents" ? "documents" : "model",
    sourceCount: typeof graph.sourceCount === "number" ? graph.sourceCount : 0,
    root: normalizeMindMapNode(graph.root),
    createdAt: typeof graph.createdAt === "number" ? graph.createdAt : Date.now(),
  };
}

function normalizeMindmapsStore(raw: unknown): Record<string, MindMapGraph[]> {
  if (!raw || typeof raw !== "object") return {};
  const store = raw as Record<string, unknown>;
  const normalized: Record<string, MindMapGraph[]> = {};

  Object.entries(store).forEach(([sessionId, value]) => {
    if (Array.isArray(value)) {
      const items = value
        .map((entry) => normalizeMindMapGraph(entry))
        .filter((entry): entry is MindMapGraph => !!entry);
      if (items.length > 0) normalized[sessionId] = items;
      return;
    }

    const single = normalizeMindMapGraph(value);
    if (single) normalized[sessionId] = [single];
  });

  return normalized;
}

// ── Session helpers (pure functions, no hooks) ──────────────────────────────

/** Create a fresh, empty ChatSession with a unique ID. */
function createEmptySession(): ChatSession {
  return {
    id: crypto.randomUUID(),
    title: "New Chat",
    messages: [],
    streamingContent: "",
    loading: false,
    audioOutputs: [],
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

/** Ensure persisted sessions are safely shaped after loading from localStorage. */
function normalizeSession(raw: Partial<ChatSession>): ChatSession {
  const messages = Array.isArray(raw.messages)
    ? raw.messages.filter((m): m is ChatMessage =>
      !!m &&
      (m.role === "user" || m.role === "assistant" || m.role === "system") &&
      typeof m.content === "string"
    )
    : [];

  return {
    id: typeof raw.id === "string" && raw.id ? raw.id : crypto.randomUUID(),
    title: typeof raw.title === "string" && raw.title ? raw.title : "New Chat",
    messages,
    streamingContent: "",
    loading: false,
    audioOutputs: Array.isArray(raw.audioOutputs)
      ? raw.audioOutputs
      : (typeof raw.audioOutput === "string" && raw.audioOutput ? [raw.audioOutput] : []),
    cancelled: false,
    ragResult: raw.ragResult ?? null,
    mediaAssets: raw.mediaAssets ?? {},
    createdAt: typeof raw.createdAt === "number" ? raw.createdAt : Date.now(),
  };
}

function App() {
  // ── Model state ────────────────────────────────────────────────────────────
  const [models, setModels] = useState<ModelFile[]>([]);
  const [selectedModel, setSelectedModel] = useState("");
  const [registeredModels, setRegisteredModels] = useState<RegisteredModel[]>([]);
  const [modelCatalog, setModelCatalog] = useState<RegisteredModel[]>([]);
  const [sessionModelParamOverrides, setSessionModelParamOverrides] = useState<Record<string, Record<string, string>>>({});
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [hfModalOpen, setHfModalOpen] = useState(false);
  const [hfModalPreset, setHfModalPreset] = useState<{
    folder: string;
    profile: "none" | ImportModelProfile;
  }>({
    folder: "LLM",
    profile: "llm",
  });
  const [downloadOptionalOnStart] = useState(() => {
    return localStorage.getItem(STARTUP_OPTIONAL_DOWNLOAD_KEY) === "true";
  });
  const [appModal, setAppModal] = useState({
    open: false,
    kind: "info" as AppModalKind,
    title: "",
    message: "",
    confirmLabel: "OK",
    cancelLabel: "Cancel",
    showCancel: false,
  });
  const modalResolveRef = useRef<((value: boolean) => void) | null>(null);

  // ── Tours ─────────────────────────────────────────────────────────────────
  const { startTour, setBindings } = useTour();
  const [toursOpen, setToursOpen] = useState(false);
  const [suppressStartupModal, setSuppressStartupModal] = useState(false);

  useEffect(() => {
    setBindings({
      openSettings: () => setSettingsOpen(true),
      openTours: () => setToursOpen(true),
      openDocPanel: () => setDocPanelOpen(true),
      switchMode: (mode: ChatMode) => {
        setChatMode(mode);
        if (mode !== "vision") {
          setImagePath(null);
          setImagePreview(null);
        }
        if (mode !== "text" && mode !== "mindmap") {
          setDocPanelOpen(false);
        }
      },
    });
  }, [setBindings]);

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
  const [workspaceScope, setWorkspaceScope] = useState<string | null>(null);
  const [workspaces, setWorkspaces] = useState<WorkspaceRecord[]>([]);
  const [activeWorkspace, setActiveWorkspace] = useState<WorkspaceRecord | null>(null);
  const [startupContinueWorkspace, setStartupContinueWorkspace] = useState<WorkspaceRecord | null>(null);
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const [modelLoadingStatus, setModelLoadingStatus] = useState<{
    loading: boolean;
    modelId: string;
    message: string;
  }>({ loading: false, modelId: "", message: "" });
  const [sessionStoreReady, setSessionStoreReady] = useState(false);
  const [sessions, setSessions] = useState<ChatSession[]>(() => [createEmptySession()]);
  const [openSessionIds, setOpenSessionIds] = useState<string[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string>("");
  /** AbortControllers keyed by session ID — persists across renders. */
  const abortControllersRef = useRef<Map<string, AbortController>>(new Map());
  
  // ── Thinking/Reasoning state ───────────────────────────────────────────────
  const [thinkingEnabled, setThinkingEnabled] = useState(false);
  const [streamingThinking, setStreamingThinking] = useState<string>("");

  // ── Vision state ───────────────────────────────────────────────────────────
  const [imagePath, setImagePath] = useState<string | null>(null);
  const [imagePreview, setImagePreview] = useState<string | null>(null);
  const visionUnlistenRef = useRef<(() => void) | null>(null);

  // ── RAG state ──────────────────────────────────────────────────────────────
  const [ragDocs, setRagDocs] = useState<IngestionStatus[]>([]);
  const [ragIngesting, setRagIngesting] = useState(false);
  const [enrichmentStatus, setEnrichmentStatus] = useState<string | null>(null);
  const [mindmapsBySession, setMindmapsBySession] = useState<Record<string, MindMapGraph[]>>({});
  const [activeMindmapOverlay, setActiveMindmapOverlay] = useState<{
    sessionId: string;
    mindmapId: string | null;
    isGenerating?: boolean;
    query?: string;
  } | null>(null);

  // ── Right sidebar (Knowledge Base) ─────────────────────────────────────────
  const [docPanelOpen, setDocPanelOpen] = useState(false);
  const [paramsDockOpen, setParamsDockOpen] = useState(true);
  const [modeSwitchNotice, setModeSwitchNotice] = useState<string | null>(null);
  const modeSwitchTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const startupToastTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const startupPresenceNoticeShownRef = useRef(false);
  const legacySessionStorageDisabledRef = useRef(false);
  const sessionQuotaPromptedRef = useRef(false);
  const [startupModelToast, setStartupModelToast] = useState<{
    open: boolean;
    phase: "prompt" | "downloading" | "done" | "declined" | "info";
    message: string;
    missingIds: string[];
    missingNames: string[];
    missingSizesMb: number[];
    selectedIds: string[];
    doneIds: string[];
    failedIds: string[];
    completed: number;
    total: number;
    failed: number;
  }>({
    open: false,
    phase: "info",
    message: "",
    missingIds: [],
    missingNames: [],
    missingSizesMb: [],
    selectedIds: [],
    doneIds: [],
    failedIds: [],
    completed: 0,
    total: 0,
    failed: 0,
  });
  const [startupToastMinimized, setStartupToastMinimized] = useState(false);

  // ── Session accessor helpers ───────────────────────────────────────────────

  /** Get the currently active session object (read-only snapshot). */
  const activeSession = sessions.find((s) => s.id === activeSessionId) ?? null;

  const promptClearSessionStorage = useCallback(() => {
    if (sessionQuotaPromptedRef.current) return;
    if (modalResolveRef.current) return;
    sessionQuotaPromptedRef.current = true;

    modalResolveRef.current = (confirmed) => {
      sessionQuotaPromptedRef.current = false;
      if (!confirmed) return;
      try {
        for (let i = localStorage.length - 1; i >= 0; i -= 1) {
          const key = localStorage.key(i);
          if (!key) continue;
          if (key.startsWith(SESSION_STORAGE_PREFIX)) {
            localStorage.removeItem(key);
          }
        }
        legacySessionStorageDisabledRef.current = false;
        setAppModal({
          open: true,
          kind: "info",
          title: "Session storage cleared",
          message: "Local cached session storage was cleared. You can continue normally.",
          confirmLabel: "OK",
          cancelLabel: "Cancel",
          showCancel: false,
        });
      } catch (err) {
        console.warn("Failed to clear session storage cache:", err);
      }
    };

    setAppModal({
      open: true,
      kind: "confirm",
      title: "Session storage is full",
      message: "Local session cache is full. Do you want to clear cached session storage now?",
      confirmLabel: "Clear storage",
      cancelLabel: "Not now",
      showCancel: true,
    });
  }, []);

  const parseModelParamNumber = useCallback((raw: string | undefined): number | undefined => {
    if (!raw) return undefined;
    const parsed = Number.parseFloat(raw);
    return Number.isFinite(parsed) ? parsed : undefined;
  }, []);

  const getModelParams = useCallback(
    (modelIdentifier: string | null | undefined): Record<string, string> => {
      if (!modelIdentifier) return {};
      const registered = findRegisteredModelByIdentifier(registeredModels, modelIdentifier);
      const persisted = registered?.params ?? {};
      const override =
        sessionModelParamOverrides[registered?.id ?? modelIdentifier] ??
        sessionModelParamOverrides[modelIdentifier] ??
        {};
      return {
        ...persisted,
        ...override,
      };
    },
    [registeredModels, sessionModelParamOverrides]
  );

  const getChatGenerationOptions = useCallback(
    (modelIdentifier: string | null | undefined) => {
      const params = getModelParams(modelIdentifier);
      return {
        maxTokens: parseModelParamNumber(params.max_tokens),
        temperature: parseModelParamNumber(params.temp),
        topP: parseModelParamNumber(params.top_p),
        topK: parseModelParamNumber(params.top_k),
        repeatPenalty: parseModelParamNumber(params.repeat_penalty),
      };
    },
    [getModelParams, parseModelParamNumber]
  );

  const activeRuntimeParamTarget = useMemo<RuntimeParamsTarget | null>(() => {
    const createTarget = (
      identifier: string | null | undefined,
      fallbackBackend: string,
      fallbackName?: string
    ): RuntimeParamsTarget | null => {
      if (!identifier) return null;

      const resolved = findRegisteredModelByIdentifier(registeredModels, identifier);
      const discoveredName =
        fallbackName ||
        models.find((model) => model.path === identifier)?.name ||
        identifier;

      return {
        key: `${chatMode}:${resolved?.id ?? identifier}`,
        identifier,
        displayName: discoveredName.replace(/\s*\(Unregistered\)$/i, ""),
        backend: resolved?.backend ?? fallbackBackend,
        modelFile: resolved?.model_file,
        memoryMb: resolved?.memory_mb,
        params: getModelParams(identifier),
        isRegistered: !!resolved,
      };
    };

    if (chatMode === "text" || chatMode === "mindmap") {
      return createTarget(selectedModel, "LlamaServer");
    }

    if (chatMode === "audio") {
      const selectedEngine = ttsEngines.find((engine) => engine.id === selectedTtsEngine);
      return createTarget(selectedTtsEngine, selectedEngine?.backend ?? "KittenTts", selectedEngine?.name);
    }

    if (chatMode === "vision") {
      const selectedVision = visionModels.find((model) => model.id === selectedVisionModel);
      return createTarget(selectedVisionModel, selectedVision?.backend ?? "LlamaServer", selectedVision?.name);
    }

    return null;
  }, [
    chatMode,
    getModelParams,
    models,
    registeredModels,
    selectedModel,
    selectedTtsEngine,
    selectedVisionModel,
    ttsEngines,
    visionModels,
  ]);

  const lastRuntimeTargetKeyRef = useRef<string | null>(null);
  useEffect(() => {
    if (!activeRuntimeParamTarget) {
      lastRuntimeTargetKeyRef.current = null;
      setParamsDockOpen(false);
      return;
    }

    // Re-open the panel only when switching to a different active model target.
    if (lastRuntimeTargetKeyRef.current !== activeRuntimeParamTarget.key) {
      lastRuntimeTargetKeyRef.current = activeRuntimeParamTarget.key;
      setParamsDockOpen(true);
    }
  }, [activeRuntimeParamTarget]);

  const handleApplyRuntimeParams = async (nextParams: Record<string, string>) => {
    if (!activeRuntimeParamTarget) return;

    const targetIdentifier = activeRuntimeParamTarget.identifier;
    let resolved = findRegisteredModelByIdentifier(registeredModels, targetIdentifier);

    // Auto-bind path-only models (for manually placed/HF-downloaded GGUF files)
    // by forcing a model switch first, then refreshing the registry.
    if (!resolved && (chatMode === "text" || chatMode === "mindmap" || chatMode === "vision")) {
      try {
        await Api.switchModel(targetIdentifier);
        const refreshed = await refreshModels();
        resolved = findRegisteredModelByIdentifier(refreshed, targetIdentifier);
      } catch (err) {
        console.warn("Failed to auto-bind model before applying params", err);
      }
    }

    if (resolved) {
      await Api.updateModelParams(resolved.id, nextParams);
      setSessionModelParamOverrides((prev) => {
        const next = { ...prev };
        delete next[targetIdentifier];
        delete next[resolved.id];
        return next;
      });

      await refreshModels();

      if (
        (chatMode === "text" || chatMode === "mindmap") &&
        selectedModel === targetIdentifier
      ) {
        setSelectedModel(resolved.id);
      }
      return;
    }

    // Fallback for models that still cannot be bound: keep session-local overrides.
    setSessionModelParamOverrides((prev) => ({
      ...prev,
      [targetIdentifier]: { ...nextParams },
    }));
  };

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

  /** Create a new session, add it to the list, and activate it. */
  const addNewSession = useCallback(() => {
    // No active workspace means we keep the app in the "empty window" state.
    if (!activeWorkspace) return;
    const newSession = createEmptySession();
    setSessions((prev) => [...prev, newSession]);
    setOpenSessionIds((prev) => [...prev, newSession.id]);
    setActiveSessionId(newSession.id);
  }, [activeWorkspace]);

  /** Open a chat from history into the top viewer tabs and activate it. */
  const openSessionInViewer = useCallback((sessionId: string) => {
    setOpenSessionIds((prev) => (prev.includes(sessionId) ? prev : [...prev, sessionId]));
    setActiveSessionId(sessionId);
  }, []);

  const openMindmapOverlay = useCallback((sessionId: string, mindmapId: string) => {
    openSessionInViewer(sessionId);
    setActiveMindmapOverlay({ sessionId, mindmapId, isGenerating: false });
  }, [openSessionInViewer]);

  /** Close only the viewer tab (does not delete chat history). */
  const closeViewerTab = useCallback(
    (sessionId: string) => {
      setOpenSessionIds((prev) => {
        if (!prev.includes(sessionId)) return prev;
        const next = prev.filter((id) => id !== sessionId);

        if (activeSessionId === sessionId) {
          if (next.length === 0) {
            setActiveSessionId("");
          } else {
            const closedIdx = prev.findIndex((id) => id === sessionId);
            const nextIdx = Math.min(closedIdx, next.length - 1);
            setActiveSessionId(next[nextIdx]);
          }
        }

        return next;
      });
    },
    [activeSessionId]
  );

  /** Close a session by ID. Allow zero sessions. */
  const closeSession = useCallback(
    (sessionId: string) => {
      // Abort any in-flight request for this session
      abortControllersRef.current.get(sessionId)?.abort();
      abortControllersRef.current.delete(sessionId);
      setMindmapsBySession((prev) => {
        const next = { ...prev };
        delete next[sessionId];
        return next;
      });
      setActiveMindmapOverlay((prev) => (prev?.sessionId === sessionId ? null : prev));

      setSessions((prev) => {
        const remaining = prev.filter((s) => s.id !== sessionId);
        setOpenSessionIds((openPrev) => openPrev.filter((id) => id !== sessionId));
        if (remaining.length === 0) {
          setOpenSessionIds([]);
          setActiveSessionId("");
          return [];
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

  /** Reorder only open viewer tabs (VS Code style). */
  const reorderViewerTabs = useCallback((reordered: ChatSession[]) => {
    setOpenSessionIds(reordered.map((s) => s.id));
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

  // ── Keyboard shortcut: Ctrl+T to open a new chat ──────────────────────────
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

  const buildWorkspaceFrontendState = useCallback(
    (safeActive: string) =>
      JSON.stringify({
        sessions,
        activeSessionId: safeActive,
        openSessionIds,
        mindmapsBySession,
        selectedModel,
        selectedTtsEngine,
        selectedVisionModel,
      }),
    [sessions, openSessionIds, mindmapsBySession, selectedModel, selectedTtsEngine, selectedVisionModel]
  );

  const refreshWorkspaceRegistry = useCallback(async () => {
    try {
      const [all, active] = await Promise.all([
        Api.listWorkspaces(),
        Api.getActiveWorkspace(),
      ]);
      setWorkspaces(all);
      setActiveWorkspace(active);
    } catch (err) {
      console.warn("Failed to refresh workspace registry:", err);
    }
  }, []);

  // ── RAG helpers ────────────────────────────────────────────────────────────
  // NOTE: This must be declared before any hook dependency arrays that reference
  // `loadRagDocs` (e.g. workspace switching), otherwise it hits TDZ at runtime.
  const loadRagDocs = useCallback(async () => {
    try {
      const docs = await Api.listRagDocuments();
      // Preserve placeholder entries (doc_id < 0) for files still being ingested
      // that haven't appeared in the backend list yet.
      setRagDocs((prev) => {
        const completedPaths = new Set(docs.map((d) => d.file_path));
        const remainingPlaceholders = prev.filter(
          (d) => d.doc_id < 0 && !completedPaths.has(d.file_path)
        );
        return [...remainingPlaceholders, ...docs];
      });
    } catch (e) {
      console.error("Failed to load RAG docs:", e);
    }
  }, []);

  useEffect(() => {
    refreshModels();
    loadRagDocs(); // Always load RAG docs on startup
    return () => {
      visionUnlistenRef.current?.();
      visionUnlistenRef.current = null;
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Listen for backend events (model loading, workspace ready)
  useEffect(() => {
    let unlistenModelLoading: (() => void) | null = null;
    let unlistenWorkspaceReady: (() => void) | null = null;

    const setupListeners = async () => {
      // Model loading progress events
      unlistenModelLoading = await listen<{
        model_id: string;
        status: "starting" | "ready" | "error" | "timeout";
        message: string;
      }>("model-loading", (event) => {
        const { status, model_id, message } = event.payload;
        if (status === "starting") {
          setModelLoadingStatus({ loading: true, modelId: model_id, message });
        } else {
          // Clear loading state on ready, error, or timeout
          setModelLoadingStatus({ loading: false, modelId: "", message: "" });
        }
      });

      // Workspace ready events (emitted after RAG pipeline is reloaded)
      unlistenWorkspaceReady = await listen<{
        workspace_id: string;
        status: string;
      }>("workspace-ready", (event) => {
        console.log("Workspace ready:", event.payload.workspace_id);
        // The workspace is now fully initialized - state restoration can proceed
      });
    };

    void setupListeners();

    return () => {
      unlistenModelLoading?.();
      unlistenWorkspaceReady?.();
    };
  }, []);

  // Start from a neutral scope and let startup actions choose the real workspace scope.
  useEffect(() => {
    setSessionStoreReady(false);
    setWorkspaceScope("workspace:none");
  }, []);

  useEffect(() => {
    const initializeApp = async () => {
      try {
        const [all, active] = await Promise.all([
          Api.listWorkspaces(),
          Api.getActiveWorkspace().catch(() => null),
        ]);
        setWorkspaces(all);
        setStartupContinueWorkspace(
          active && all.some((workspace) => workspace.id === active.id) ? active : null
        );
        setActiveWorkspace(null);
      } catch (err) {
        console.warn("Failed to initialize workspace state:", err);
      }
    };
    void initializeApp();
  }, [loadRagDocs]);

  // Restore persisted chat sessions for the active workspace.
  useEffect(() => {
    if (!workspaceScope) return;

    let cancelled = false;

    const applyRawState = (raw: string | null) => {
      if (!raw) {
        const fresh = createEmptySession();
        setSessions([fresh]);
        setOpenSessionIds([fresh.id]);
        setActiveSessionId(fresh.id);
        setMindmapsBySession({});
        setActiveMindmapOverlay(null);
        return;
      }

      const parsed = JSON.parse(raw) as {
        sessions?: Partial<ChatSession>[];
        activeSessionId?: string;
        openSessionIds?: string[];
        mindmapsBySession?: Record<string, unknown>;
        selectedModel?: string;
        selectedTtsEngine?: string;
        selectedVisionModel?: string;
      };
      const loaded = Array.isArray(parsed.sessions)
        ? parsed.sessions.map(normalizeSession)
        : [];
      const restoredMindmaps = normalizeMindmapsStore(parsed.mindmapsBySession);

      if (loaded.length === 0) {
        const fresh = createEmptySession();
        setSessions([fresh]);
        setOpenSessionIds([fresh.id]);
        setActiveSessionId(fresh.id);
        setMindmapsBySession({});
      } else {
        setSessions(loaded);
        const nextActive =
          parsed.activeSessionId && loaded.some((s) => s.id === parsed.activeSessionId)
            ? parsed.activeSessionId
            : loaded[0].id;
        const restoredOpen = Array.isArray(parsed.openSessionIds)
          ? parsed.openSessionIds.filter((id) => loaded.some((s) => s.id === id))
          : [];
        setOpenSessionIds(restoredOpen.length > 0 ? restoredOpen : [nextActive]);
        setActiveSessionId(nextActive);
        setMindmapsBySession(restoredMindmaps);
      }

      // Restore per-workspace model selections
      if (parsed.selectedModel) setSelectedModel(parsed.selectedModel);
      if (parsed.selectedTtsEngine) setSelectedTtsEngine(parsed.selectedTtsEngine);
      if (parsed.selectedVisionModel) setSelectedVisionModel(parsed.selectedVisionModel);

      setActiveMindmapOverlay(null);
    };

    (async () => {
      try {
        // Primary store: workspace backend state blob.
        const backendState = await Api.getWorkspaceFrontendState();
        if (cancelled) return;
        if (backendState) {
          applyRawState(backendState);
          return;
        }

        // Compatibility fallback: legacy localStorage key.
        const storageKey = `${SESSION_STORAGE_PREFIX}${workspaceScope}`;
        const raw = localStorage.getItem(storageKey);
        if (cancelled) return;
        applyRawState(raw);
      } catch (err) {
        console.error("Failed to restore workspace sessions:", err);
        if (cancelled) return;
        const fresh = createEmptySession();
        setSessions([fresh]);
        setOpenSessionIds([fresh.id]);
        setActiveSessionId(fresh.id);
        setMindmapsBySession({});
        setActiveMindmapOverlay(null);
      } finally {
        if (!cancelled) setSessionStoreReady(true);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [workspaceScope]);

  // Persist sessions whenever they change, scoped to the current workspace.
  useEffect(() => {
    if (!workspaceScope || !sessionStoreReady || sessions.length === 0) return;
    if (workspaceScope === "workspace:none") return;

    const safeActive = sessions.some((s) => s.id === activeSessionId)
      ? activeSessionId
      : sessions[0].id;

    const storageKey = `${SESSION_STORAGE_PREFIX}${workspaceScope}`;
    const fullLegacyState = JSON.stringify({
      sessions,
      activeSessionId: safeActive,
      openSessionIds,
      mindmapsBySession,
      selectedModel,
      selectedTtsEngine,
      selectedVisionModel,
    });

    if (!legacySessionStorageDisabledRef.current) {
      try {
        localStorage.setItem(storageKey, fullLegacyState);
      } catch (err) {
        const isQuotaError =
          err instanceof DOMException &&
          (err.name === "QuotaExceededError" || err.name === "NS_ERROR_DOM_QUOTA_REACHED");

        if (isQuotaError) {
          promptClearSessionStorage();
          try {
            for (let i = localStorage.length - 1; i >= 0; i -= 1) {
              const key = localStorage.key(i);
              if (!key) continue;
              if (key.startsWith(SESSION_STORAGE_PREFIX) && key !== storageKey) {
                localStorage.removeItem(key);
              }
            }
            localStorage.setItem(storageKey, fullLegacyState);
          } catch {
            try {
              localStorage.setItem(
                storageKey,
                JSON.stringify({
                  sessions: [],
                  activeSessionId: safeActive,
                  openSessionIds: [safeActive],
                  selectedModel,
                  selectedTtsEngine,
                  selectedVisionModel,
                })
              );
            } catch (retryErr) {
              legacySessionStorageDisabledRef.current = true;
              console.warn("Disabling legacy localStorage session mirror due to quota:", retryErr);
            }
          }
        } else {
          console.warn("Failed to persist legacy localStorage session state:", err);
        }
      }
    }

    // Mirror into active workspace cache for .nela save/open flows.
    void Api.saveWorkspaceFrontendState(
      buildWorkspaceFrontendState(safeActive)
    ).catch((err) => {
      console.warn("Failed to persist workspace frontend state to backend:", err);
    });
  }, [workspaceScope, sessionStoreReady, sessions, activeSessionId, openSessionIds, mindmapsBySession, selectedModel, selectedTtsEngine, selectedVisionModel, buildWorkspaceFrontendState, promptClearSessionStorage]);

  const switchWorkspaceById = useCallback(async (workspaceId: string) => {
    if (workspaceBusy) return;
    try {
      setWorkspaceBusy(true);
      setSessionStoreReady(false);
      setRagDocs([]);
      // Clear per-workspace frontend state immediately to avoid any brief cross-workspace mix.
      setSessions([]);
      setOpenSessionIds([]);
      setActiveSessionId("");
      setMindmapsBySession({});
      setActiveMindmapOverlay(null);
      const opened = await Api.openWorkspace(workspaceId);
      const scope = await Api.getWorkspaceScope();
      setActiveWorkspace(opened);
      setWorkspaceScope(scope || `workspace:${opened.id}`);
      await refreshWorkspaceRegistry();
      await loadRagDocs();
    } catch (err) {
      console.error("Failed to switch workspace:", err);
      setSessionStoreReady(true);
    } finally {
      setWorkspaceBusy(false);
    }
  }, [workspaceBusy, refreshWorkspaceRegistry, loadRagDocs]);

  const createNewWorkspace = useCallback(async () => {
    if (workspaceBusy) return;
    try {
      setWorkspaceBusy(true);
      setSessionStoreReady(false);
      setRagDocs([]);
      setSessions([]);
      setOpenSessionIds([]);
      setActiveSessionId("");
      setMindmapsBySession({});
      setActiveMindmapOverlay(null);
      const created = await Api.createWorkspace();
      const scope = await Api.getWorkspaceScope();
      setActiveWorkspace(created);
      setWorkspaceScope(scope || `workspace:${created.id}`);
      await refreshWorkspaceRegistry();
      await loadRagDocs();
    } catch (err) {
      console.error("Failed to create workspace:", err);
      setSessionStoreReady(true);
    } finally {
      setWorkspaceBusy(false);
    }
  }, [workspaceBusy, refreshWorkspaceRegistry, loadRagDocs]);

  const saveWorkspaceAsFile = useCallback(async () => {
    if (workspaceBusy || !activeSession) return;
    try {
      setWorkspaceBusy(true);
      const path = await save({
        title: "Save NELA Workspace As",
        filters: [{ name: "NELA Workspace", extensions: ["nela"] }],
        defaultPath: `${activeWorkspace?.name ?? "workspace"}.nela`,
      });
      if (!path) return;

      const safeActive = sessions.some((s) => s.id === activeSessionId)
        ? activeSessionId
        : sessions[0]?.id ?? "";
      const frontendState = buildWorkspaceFrontendState(safeActive);
      const savedWorkspace = await Api.saveWorkspaceAsNela(path, frontendState);
      setActiveWorkspace(savedWorkspace);
      await refreshWorkspaceRegistry();
    } catch (err) {
      console.error("Failed to save workspace as .nela:", err);
    } finally {
      setWorkspaceBusy(false);
    }
  }, [workspaceBusy, activeSession, activeWorkspace?.name, sessions, activeSessionId, buildWorkspaceFrontendState, refreshWorkspaceRegistry]);

  const saveWorkspaceFile = useCallback(async () => {
    if (workspaceBusy || !activeWorkspace) return;
    try {
      if (!activeWorkspace.nela_path) {
        await saveWorkspaceAsFile();
        return;
      }

      setWorkspaceBusy(true);
      const safeActive = sessions.some((s) => s.id === activeSessionId)
        ? activeSessionId
        : sessions[0]?.id ?? "";
      const frontendState = buildWorkspaceFrontendState(safeActive);
      const savedWorkspace = await Api.saveWorkspaceNela(frontendState);
      setActiveWorkspace(savedWorkspace);
      await refreshWorkspaceRegistry();
    } catch (err) {
      console.error("Failed to save workspace .nela:", err);
    } finally {
      setWorkspaceBusy(false);
    }
  }, [workspaceBusy, activeWorkspace, sessions, activeSessionId, buildWorkspaceFrontendState, saveWorkspaceAsFile, refreshWorkspaceRegistry]);

  const openWorkspaceFromFile = useCallback(async () => {
    if (workspaceBusy) return;
    try {
      setWorkspaceBusy(true);
      setSessionStoreReady(false);
      setRagDocs([]);
      setSessions([]);
      setOpenSessionIds([]);
      setActiveSessionId("");
      setMindmapsBySession({});
      setActiveMindmapOverlay(null);
      const selected = await open({
        title: "Open NELA Workspace",
        multiple: false,
        filters: [{ name: "NELA Workspace", extensions: ["nela"] }],
      });
      if (!selected || Array.isArray(selected)) return;

      const result = await Api.openWorkspaceNela(selected);
      const scope = await Api.getWorkspaceScope();
      setActiveWorkspace(result.workspace);
      setWorkspaceScope(scope || `workspace:${result.workspace.id}`);
      await refreshWorkspaceRegistry();
      await loadRagDocs();
    } catch (err) {
      console.error("Failed to open .nela workspace:", err);
      setSessionStoreReady(true);
    } finally {
      setWorkspaceBusy(false);
    }
  }, [workspaceBusy, refreshWorkspaceRegistry, loadRagDocs]);

  const refreshWorkspaceListOnly = useCallback(async () => {
    try {
      const all = await Api.listWorkspaces();
      setWorkspaces(all);
    } catch (err) {
      console.warn("Failed to refresh workspace list:", err);
    }
  }, []);

  const renameWorkspaceById = useCallback(
    async (workspaceId: string, newName: string) => {
      if (workspaceBusy) return;
      const trimmed = newName.trim();
      if (!trimmed) return;

      try {
        setWorkspaceBusy(true);
        await Api.renameWorkspace(workspaceId, trimmed);

        // In the "empty window" state, do NOT ask backend for active workspace,
        // otherwise we'd accidentally exit the empty state.
        if (activeWorkspace) {
          await refreshWorkspaceRegistry();
        } else {
          await refreshWorkspaceListOnly();
        }
      } catch (err) {
        console.error("Failed to rename workspace:", err);
      } finally {
        setWorkspaceBusy(false);
      }
    },
    [workspaceBusy, activeWorkspace, refreshWorkspaceRegistry, refreshWorkspaceListOnly]
  );

  const deleteWorkspaceById = useCallback(
    async (workspaceId: string) => {
      if (workspaceBusy) return;
      const deletingActive = activeWorkspace?.id === workspaceId;

      try {
        setWorkspaceBusy(true);
        const nextActiveFromBackend = await Api.deleteWorkspace(workspaceId);

        if (!deletingActive) {
          // Non-active deletion: keep current workspace scope/session state untouched.
          await refreshWorkspaceListOnly();
          return;
        }

        // Active deletion: clear state immediately, then bind to backend-selected fallback workspace.
        setSessionStoreReady(false);
        setSessions([]);
        setOpenSessionIds([]);
        setActiveSessionId("");
        setMindmapsBySession({});
        setActiveMindmapOverlay(null);
        setRagDocs([]);

        if (nextActiveFromBackend) {
          const scope = await Api.getWorkspaceScope();
          setActiveWorkspace(nextActiveFromBackend);
          setWorkspaceScope(scope || `workspace:${nextActiveFromBackend.id}`);
          await refreshWorkspaceRegistry();
          await loadRagDocs();
        } else {
          setActiveWorkspace(null);
          setStartupContinueWorkspace(null);
          setWorkspaceScope("workspace:none");
          await refreshWorkspaceListOnly();
          setSessionStoreReady(true);
        }
      } catch (err) {
        console.error("Failed to delete workspace:", err);
        setSessionStoreReady(true);
      } finally {
        setWorkspaceBusy(false);
      }
    },
    [
      workspaceBusy,
      activeWorkspace,
      refreshWorkspaceRegistry,
      refreshWorkspaceListOnly,
      loadRagDocs,
    ]
  );

  // Keep active session aligned with currently open viewer tabs.
  useEffect(() => {
    if (openSessionIds.length === 0) {
      if (activeSessionId) setActiveSessionId("");
      return;
    }

    const isActiveOpen = openSessionIds.includes(activeSessionId);
    if (!isActiveOpen) {
      setActiveSessionId(openSessionIds[0]);
    }
  }, [openSessionIds, activeSessionId]);

  // Keep open viewer tabs valid if chat history changes.
  useEffect(() => {
    if (sessions.length === 0) return;
    setOpenSessionIds((prev) => {
      const valid = prev.filter((id) => sessions.some((s) => s.id === id));
      return valid.length > 0 ? valid : [sessions[0].id];
    });
  }, [sessions]);

  useEffect(() => {
    const validSessionIds = new Set(sessions.map((session) => session.id));
    setMindmapsBySession((prev) => {
      const next: Record<string, MindMapGraph[]> = {};
      let changed = false;
      Object.entries(prev).forEach(([sessionId, maps]) => {
        if (validSessionIds.has(sessionId) && maps.length > 0) {
          next[sessionId] = maps;
        } else {
          changed = true;
        }
      });
      return changed ? next : prev;
    });

    setActiveMindmapOverlay((prev) => {
      if (!prev) return prev;
      if (!validSessionIds.has(prev.sessionId)) return null;
      if (!prev.mindmapId) return prev;
      const list = mindmapsBySession[prev.sessionId] ?? [];
      return list.some((map) => map.id === prev.mindmapId) ? prev : null;
    });
  }, [sessions, mindmapsBySession]);

  // Clear one-time mode notice after a short duration.
  useEffect(() => {
    if (!modeSwitchNotice) return;
    if (modeSwitchTimeoutRef.current) clearTimeout(modeSwitchTimeoutRef.current);
    modeSwitchTimeoutRef.current = setTimeout(() => {
      setModeSwitchNotice(null);
      modeSwitchTimeoutRef.current = null;
    }, 1800);
    return () => {
      if (modeSwitchTimeoutRef.current) {
        clearTimeout(modeSwitchTimeoutRef.current);
      }
    };
  }, [modeSwitchNotice]);

  // Never carry mode-switch text into another chat tab.
  useEffect(() => {
    setModeSwitchNotice(null);
  }, [activeSessionId]);

  // On first entry to the chat screen, notify user which optional models are present.
  useEffect(() => {
    if (startupPresenceNoticeShownRef.current) return;
    if (!activeWorkspace || !sessionStoreReady) return;
    if (modelCatalog.length === 0) return;

    const optionalForStartup = modelCatalog.filter((model) =>
      model.tasks.some((task) => STARTUP_MODEL_SELECTOR.tasks.has(task)) ||
      STARTUP_MODEL_SELECTOR.ids.has(model.id)
    );
    if (optionalForStartup.length === 0) return;

    const present = optionalForStartup.filter((model) => model.is_downloaded);
    const missing = optionalForStartup.filter((model) => !model.is_downloaded);
    if (missing.length > 0) {
      setStartupModelToast({
        open: true,
        phase: "prompt",
        message: `${present.length}/${optionalForStartup.length} models are present.`,
        missingIds: missing.map((model) => model.id),
        missingNames: missing.map((model) => model.name),
        missingSizesMb: missing.map((model) => model.memory_mb),
        selectedIds: missing.map((model) => model.id),
        doneIds: [],
        failedIds: [],
        completed: 0,
        total: missing.length,
        failed: 0,
      });
    } else {
      setStartupModelToast({
        open: true,
        phase: "info",
        message: `All ${optionalForStartup.length} models are already present.`,
        missingIds: [],
        missingNames: [],
        missingSizesMb: [],
        selectedIds: [],
        doneIds: [],
        failedIds: [],
        completed: 0,
        total: 0,
        failed: 0,
      });
    }
    startupPresenceNoticeShownRef.current = true;
  }, [activeWorkspace, sessionStoreReady, modelCatalog]);

  useEffect(() => {
    if (!startupModelToast.open) return;
    if (startupModelToast.phase === "prompt" || startupModelToast.phase === "downloading") return;
    if (startupToastTimeoutRef.current) clearTimeout(startupToastTimeoutRef.current);
    startupToastTimeoutRef.current = setTimeout(() => {
      setStartupModelToast((prev) => ({ ...prev, open: false }));
      startupToastTimeoutRef.current = null;
    }, 5000);
    return () => {
      if (startupToastTimeoutRef.current) {
        clearTimeout(startupToastTimeoutRef.current);
      }
    };
  }, [startupModelToast]);

  useEffect(() => {
    if (startupModelToast.phase !== "downloading") {
      setStartupToastMinimized(false);
    }
  }, [startupModelToast.phase]);

  // Reload RAG docs periodically when in text/mindmap modes
  useEffect(() => {
    if (chatMode === "text" || chatMode === "mindmap") loadRagDocs();
  }, [chatMode, loadRagDocs]);

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
  }, [loadRagDocs]);

  // Clean up TTS and general timers on unmount
  useEffect(() => {
    return () => {
      if (ttsIntervalRef.current) clearInterval(ttsIntervalRef.current);
      if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
    };
  }, []);


  // ── Download state ─────────────────────────────────────────────────────────
  const [downloads, setDownloads] = useState<Record<string, { progress: number; status: string }>>({});
  const refreshModelsOnDownloadRef = useRef<() => Promise<RegisteredModel[]>>(async () => []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listen<{ model_id: string; progress: number; status: string }>(
      "model-download-progress",
      (e) => {
        setDownloads((prev) => ({
          ...prev,
          [e.payload.model_id]: { progress: e.payload.progress, status: e.payload.status },
        }));
        if (e.payload.progress >= 100 && e.payload.status === "Complete") {
          setTimeout(() => {
            void refreshModelsOnDownloadRef.current();
          }, 1000);
          setTimeout(() => {
            setDownloads((prev) => {
              const newD = { ...prev };
              delete newD[e.payload.model_id];
              return newD;
            });
          }, 3000);
        }
      }
    ).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    try {
      localStorage.setItem(
        STARTUP_OPTIONAL_DOWNLOAD_KEY,
        downloadOptionalOnStart ? "true" : "false"
      );
    } catch (err) {
      console.warn("Failed to persist startup optional-download preference:", err);
    }
  }, [downloadOptionalOnStart]);

  // ── Model helpers ──────────────────────────────────────────────────────────


  const handleDownloadModel = async (modelId: string) => {
    try {
      await Api.downloadModel(modelId);
    } catch (e) {
      console.error("Failed to download model", e);
      showError(`Failed to download model: ${String(e)}`);
    }
  };

  const handleCancelDownload = async (modelId: string) => {
    try {
      await Api.cancelDownload(modelId);
      setDownloads((prev) => {
        const next = { ...prev };
        delete next[modelId];
        return next;
      });
    } catch (e) {
      console.error("Failed to cancel download", e);
      showError(`Failed to cancel download: ${String(e)}`);
    }
  };

  const handleUninstall = async (modelId: string) => {
    try {
      await Api.uninstallModel(modelId);
      setTimeout(refreshModels, 1000);
    } catch (e) {
      console.error("Failed to uninstall model", e);
      showError(`Failed to uninstall model: ${String(e)}`);
    }
  };

  const handleStartupToastDecline = () => {
    setStartupModelToast((prev) => ({
      ...prev,
      open: true,
      phase: "declined",
      message: "You can download these models from Settings or clicking on the drop-down any time.",
    }));
  };

  const handleStartupToastAccept = async () => {
    const ids = startupModelToast.selectedIds;
    if (ids.length === 0) {
      setStartupModelToast((prev) => ({
        ...prev,
        open: true,
        phase: "declined",
        message: "No models selected. You can download models from Settings any time.",
      }));
      return;
    }

    setStartupModelToast((prev) => ({
      ...prev,
      open: true,
      phase: "downloading",
      message: "Starting parallel downloads...",
      total: ids.length,
      doneIds: [],
      failedIds: [],
      completed: 0,
      failed: 0,
    }));
    setStartupToastMinimized(false);

    let completed = 0;
    let failed = 0;
    await Promise.all(
      ids.map(async (modelId) => {
        try {
          await Api.downloadModel(modelId);
          completed += 1;
          setStartupModelToast((prev) => ({
            ...prev,
            doneIds: prev.doneIds.includes(modelId) ? prev.doneIds : [...prev.doneIds, modelId],
          }));
        } catch (e) {
          failed += 1;
          console.error("Failed startup model download", e);
          setStartupModelToast((prev) => ({
            ...prev,
            failedIds: prev.failedIds.includes(modelId) ? prev.failedIds : [...prev.failedIds, modelId],
          }));
        } finally {
          setStartupModelToast((prev) => ({
            ...prev,
            completed,
            failed,
          }));
        }
      })
    );

    if (failed > 0) {
      setStartupModelToast((prev) => ({
        ...prev,
        open: true,
        phase: "done",
        message: `Downloads finished: ${completed}/${ids.length} completed, ${failed} failed. You can retry from Settings.`,
      }));
    } else {
      setStartupModelToast((prev) => ({
        ...prev,
        open: true,
        phase: "done",
        message: `All ${ids.length} model download(s) completed.`,
      }));
    }
  };

  const toggleStartupModelSelection = (modelId: string) => {
    setStartupModelToast((prev) => {
      if (prev.phase !== "prompt") return prev;
      const selected = prev.selectedIds.includes(modelId)
        ? prev.selectedIds.filter((id) => id !== modelId)
        : [...prev.selectedIds, modelId];
      return { ...prev, selectedIds: selected };
    });
  };

  const startupSelectedTotalMb = useMemo(() => {
    let total = 0;
    startupModelToast.selectedIds.forEach((modelId) => {
      const idx = startupModelToast.missingIds.indexOf(modelId);
      if (idx < 0) return;
      const mb = startupModelToast.missingSizesMb[idx];
      if (typeof mb === "number" && Number.isFinite(mb) && mb > 0) total += mb;
    });
    return total;
  }, [startupModelToast.selectedIds, startupModelToast.missingIds, startupModelToast.missingSizesMb]);

  const getOptionalModels = (list: RegisteredModel[]) =>
    list.filter(
      (model) =>
        model.tasks.some((t) => STARTUP_MODEL_SELECTOR.tasks.has(t)) ||
        STARTUP_MODEL_SELECTOR.ids.has(model.id)
    );

  const downloadMissingOptionalModels = async () => {
    const list = registeredModels.length > 0
      ? registeredModels
      : await Api.listRegisteredModels();
    if (registeredModels.length === 0) setRegisteredModels(list);

    const missing = getOptionalModels(list)
      .filter((model) => model.gdrive_id && !model.is_downloaded)
      .filter((model) => !downloads[model.id]);

    for (const model of missing) {
      await handleDownloadModel(model.id);
    }
  };

  const showModal = useCallback((
    kind: AppModalKind,
    title: string,
    message: string,
    options?: { confirmLabel?: string; cancelLabel?: string; showCancel?: boolean }
  ) => {
    setAppModal({
      open: true,
      kind,
      title,
      message,
      confirmLabel: options?.confirmLabel ?? "OK",
      cancelLabel: options?.cancelLabel ?? "Cancel",
      showCancel: options?.showCancel ?? false,
    });
  }, []);

  const showError = (message: string, title = "Error") => {
    showModal("error", title, message);
  };

  const confirmAction = (
    title: string,
    message: string,
    confirmLabel = "OK",
    cancelLabel = "Cancel"
  ) =>
    new Promise<boolean>((resolve) => {
      modalResolveRef.current = resolve;
      showModal("confirm", title, message, {
        confirmLabel,
        cancelLabel,
        showCancel: true,
      });
    });

  const handleModalConfirm = () => {
    const resolver = modalResolveRef.current;
    modalResolveRef.current = null;
    if (resolver) resolver(true);
    setAppModal((prev) => ({ ...prev, open: false }));
  };

  const handleModalCancel = () => {
    const resolver = modalResolveRef.current;
    modalResolveRef.current = null;
    if (resolver) resolver(false);
    setAppModal((prev) => ({ ...prev, open: false }));
  };

  const refreshModels = useCallback(async (): Promise<RegisteredModel[]> => {
    try {
      const [list, discoveredUnits, catalog] = await Promise.all([
        Api.listRegisteredModels(),
        Api.discoverLocalModelUnits().catch(() => []),
        Api.listModelCatalog().catch(() => []),
      ]);
      setRegisteredModels(list);
      setModelCatalog(catalog);

      // Vision models
      const vision = list.filter((m) => m.tasks.includes("vision_chat"));
      setVisionModels(vision);
      if (vision.length > 0) {
        setSelectedVisionModel((prev) => prev || vision[0].id);
      }

      // Text models from registry
      const chatModels = list
        .filter((m) => m.tasks.includes("chat"))
        .sort((a, b) => b.priority - a.priority)
        .map((m) => ({
          name:
            m.model_source === "custom"
              ? `${m.name} (Custom${m.model_profile ? ` ${m.model_profile.toUpperCase()}` : ""})`
              : m.name,
          path: m.id,
          is_downloaded: m.is_downloaded,
          gdrive_id: m.gdrive_id,
        }));

      // Also include any discovered local model files that are not currently
      // runtime-registered, so manual file drops still appear for selection.
      const registeredTokens = new Set<string>();
      const registeredRepoIds = new Set<string>();
      for (const model of list) {
        registeredTokens.add(normalizeModelRef(model.id));
        if (model.model_file) {
          registeredTokens.add(normalizeModelRef(model.model_file));
          registeredTokens.add(modelRefBasename(model.model_file));
        }
        const repoId = model.params?.hf_repo_id;
        if (repoId) {
          registeredRepoIds.add(repoId.toLowerCase());
        }
      }

      const unregistered = discoveredUnits
        .filter((unit) => {
          const normalizedRel = normalizeModelRef(unit.llm_rel_path);
          const normalizedAbs = normalizeModelRef(unit.llm_abs_path);
          const basename = modelRefBasename(unit.llm_rel_path);
          const byPath =
            registeredTokens.has(normalizedRel) ||
            registeredTokens.has(normalizedAbs) ||
            registeredTokens.has(basename);
          const byRepo = registeredRepoIds.has(unit.repo_id.toLowerCase());
          return !byPath && !byRepo;
        })
        .map((unit) => ({
          name: `${unit.repo_id} (${unit.llm_file_name}) (Unregistered)`,
          path: unit.llm_abs_path,
          is_downloaded: true,
          gdrive_id: null as string | null,
        }))
        .filter(
          (entry, index, all) =>
            all.findIndex((candidate) => candidate.path === entry.path) === index
        );

      const allChatModels = [...chatModels, ...unregistered];
      setModels(allChatModels);
      if (allChatModels.length > 0) {
        setSelectedModel((prev) => prev || allChatModels[0].path);
      }

      // TTS engines from the registry
      const tts = list.filter((m) => m.tasks.includes("tts"));
      setTtsEngines(tts);
      if (tts.length > 0) {
        setSelectedTtsEngine((prev) => prev || tts[0].id);
      }

      return list;
    } catch (error) {
      console.error(error);
      return [];
    }
  }, []);
  refreshModelsOnDownloadRef.current = refreshModels;

  useEffect(() => {
    if (!selectedTtsEngine) return;
    const engine = registeredModels.find((m) => m.id === selectedTtsEngine);
    if (!engine) return;

    const configuredVoice = engine.params?.voice;
    if (configuredVoice && KITTEN_TTS_VOICES.includes(configuredVoice as KittenTtsVoice)) {
      setTtsVoice(configuredVoice as KittenTtsVoice);
    }

    const configuredSpeed = parseModelParamNumber(engine.params?.speed);
    if (configuredSpeed !== undefined) {
      const clamped = Math.max(0.5, Math.min(2.0, configuredSpeed));
      setTtsSpeed(clamped);
    }
  }, [selectedTtsEngine, registeredModels, parseModelParamNumber]);

  const handleModelChange = async (path: string) => {
    try {
      setSelectedModel(path);
      await Api.switchModel(path);

      const refreshed = await refreshModels();
      const resolved = findRegisteredModelByIdentifier(refreshed, path);
      if (resolved) {
        setSelectedModel(resolved.id);
      }
    } catch (err) {
      console.error(err);
      showError("Failed to switch model");
    }
  };

  const handleAddModel = () => {
    setHfModalPreset({ folder: "LLM", profile: "llm" });
    setHfModalOpen(true);
  };

  const handleAddVisionModel = () => {
    setHfModalPreset({ folder: "LiquidAI-VLM", profile: "vlm" });
    setHfModalOpen(true);
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

  const ingestFile = async () => {
    try {
      const selected = await open({
        multiple: true,
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
      if (!selected) return;
      const files = Array.isArray(selected) ? selected : [selected];
      if (files.length === 0) return;

      // Add placeholder entries to the side panel immediately so users can
      // click to view the file while ingestion is still running.
      const placeholders: IngestionStatus[] = files.map((f, i) => ({
        doc_id: -(i + 1), // negative IDs to avoid clashing with real docs
        title: f.split(/[\\/]/).pop() || f,
        file_path: f,
        total_chunks: 0,
        embedded_chunks: 0,
        enriched_chunks: 0,
        phase: "ingesting",
      }));
      setRagDocs((prev) => [...placeholders, ...prev]);

      setRagIngesting(true);

      // Ingest all files in parallel so the UI doesn't hang waiting on each one.
      // As each file finishes, refresh the doc list to replace its placeholder.
      const results = await Promise.allSettled(
        files.map((f) =>
          Api.ingestDocument(f).then(async (res) => {
            await loadRagDocs(); // refresh side panel as each file completes
            return res;
          })
        )
      );

      await loadRagDocs();
      setRagIngesting(false);

      const failures = results.filter((r) => r.status === "rejected");
      if (failures.length > 0) {
        const msgs = failures.map((r) => (r as PromiseRejectedResult).reason).join("\n");
        alert(`${failures.length} file(s) failed to ingest:\n${msgs}`);
      }
    } catch (e) {
      console.error(e);
      setRagIngesting(false);
      await loadRagDocs();
      showError(`Ingest failed: ${e}`);
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
      showError(`Folder ingest failed: ${e}`);
    }
  };

  const deleteRagDoc = async (docId: number) => {
    try {
      // Close the viewer if the deleted document is currently open
      const doc = ragDocs.find((d) => d.doc_id === docId);
      if (doc) {
        const delPath = doc.file_path;
        if (pdfViewerData && doc.title === pdfViewerData.title) {
          setPdfViewerData(null);
        }
        if (docViewerFile && docViewerFile.filePath === delPath) {
          setDocViewerFile(null);
        }
      }

      await Api.deleteRagDocument(docId);
      await loadRagDocs();
    } catch (e) {
      console.error(e);
      showError(`Delete failed: ${e}`);
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
        showError(`Failed to open PDF: ${e}`);
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
    if (!sid) return;
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

    const currentVisionImagePath = chatMode === "vision" ? imagePath : null;

    const visionAttachment =
      chatMode === "vision" && currentVisionImagePath
        ? {
            path: currentVisionImagePath,
            name: currentVisionImagePath.split(/[/\\]/).pop() ?? "image",
          }
        : undefined;

    const newMsg: ChatMessage = {
      role: "user",
      content: text,
      ...(visionAttachment ? { visionImage: visionAttachment } : {}),
    };

    // Derive title from first user message
    const isFirstMessage = session.messages.length === 0;
    const titlePatch = isFirstMessage ? { title: deriveTitleFromMessage(text) } : {};

    updateSession(sid, (prev) => ({
      messages: [...prev.messages, newMsg],
      loading: true,
      streamingContent: "",
      audioOutputs: prev.audioOutputs ?? [],
      cancelled: false,
      ...titlePatch,
    }));

    // In vision mode, clear the input attachment chip after sending while
    // preserving the image on the stored user message above.
    if (chatMode === "vision" && currentVisionImagePath) {
      clearImage();
    }

    // Create a fresh AbortController for this session's request
    const ctrl = new AbortController();
    abortControllersRef.current.set(sid, ctrl);
    const generationOptions = getChatGenerationOptions(selectedModel);

    try {
      // ── Mindmap Mode (RAG-grounded when relevant) ──────────────────────
      if (chatMode === "mindmap") {
        try {
          setActiveMindmapOverlay({ sessionId: sid, mindmapId: null, isGenerating: true, query: text });
          setGeneralGenerating(true);
          setGeneralElapsedTime(0);
          setGeneralGenerationTime(null);
          const startTime = Date.now();

          if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
          generalIntervalRef.current = setInterval(() => {
            const elapsed = Math.floor((Date.now() - startTime) / 100) / 10;
            setGeneralElapsedTime(elapsed);
          }, 100);

          let generatedFrom: "documents" | "model" = "model";
          let sourceCount = 0;
          let sourceContext = "";

          if (ragDocs.length > 0) {
            try {
              const setup = await Api.queryRagStream(text);
              updateSession(sid, { ragResult: { answer: "", sources: setup.sources } });
              if (!setup.no_retrieval && setup.sources.length > 0) {
                generatedFrom = "documents";
                sourceCount = setup.sources.length;
                sourceContext = setup.sources
                  .map((source, index) => `Source ${index + 1} (${source.doc_title}):\n${source.text}`)
                  .join("\n\n");
              }
            } catch (e) {
              console.warn("Mindmap RAG grounding failed; using model knowledge.", e);
            }
          }

          const prompt = generatedFrom === "documents"
            ? [
                `User query: ${text}`,
                "Build a concise mindmap grounded ONLY in the provided sources.",
                "Return ONLY valid JSON and no markdown/code fences.",
                "Schema:",
                '{"title":"string","root":{"label":"string","children":[{"label":"string","children":[...]}]}}',
                "Rules:",
                "- 3 to 6 first-level branches.",
                "- Keep labels short (2 to 8 words).",
                "- Depth max 3.",
                "- Do not invent unsupported facts.",
                "Sources:",
                sourceContext,
              ].join("\n")
            : [
                `User query: ${text}`,
                "Create a concise conceptual mindmap from your own knowledge.",
                "Return ONLY valid JSON and no markdown/code fences.",
                "Schema:",
                '{"title":"string","root":{"label":"string","children":[{"label":"string","children":[...]}]}}',
                "Rules:",
                "- 3 to 6 first-level branches.",
                "- Keep labels short (2 to 8 words).",
                "- Depth max 3.",
              ].join("\n");

          let graph: MindMapGraph | undefined;
          let lastError: unknown;
          
          for (let attempt = 1; attempt <= 3; attempt++) {
            try {
              const raw = await Api.routeRequest("mindmap", prompt, selectedModel || undefined);
              const modelText = extractTaskText(raw);
              graph = parseMindMapGraph(modelText, text, generatedFrom, sourceCount);
              break; // Success! Exit loop.
            } catch (e) {
              console.warn(`Mindmap generation attempt ${attempt} failed:`, e);
              lastError = e;
            }
          }
          
          if (!graph) {
            throw lastError; // Propagate the error to trigger the fallback UI
          }

          setMindmapsBySession((prev) => ({
            ...prev,
            [sid]: [...(prev[sid] ?? []), graph],
          }));
          setActiveMindmapOverlay({
            sessionId: sid,
            mindmapId: graph.id,
            isGenerating: false,
            query: text,
          });

          if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
          const totalTime = Math.floor((Date.now() - startTime) / 100) / 10;
          setGeneralGenerating(false);
          setGeneralElapsedTime(totalTime);
          setGeneralGenerationTime(totalTime);

          updateSession(sid, (prev) => ({
            messages: [
              ...prev.messages,
              {
                role: "assistant" as const,
                content:
                  generatedFrom === "documents"
                    ? `Mindmap generated from ${sourceCount} retrieved document source${sourceCount === 1 ? "" : "s"}.`
                    : "Mindmap generated from model knowledge.",
                generateTime: totalTime,
              },
            ],
            streamingContent: "",
            loading: false,
          }));
        } catch (e) {
          setActiveMindmapOverlay(null);
          if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
          setGeneralGenerating(false);
          console.error("Mindmap generation failed:", e);
          updateSession(sid, (prev) => ({
            messages: [
              ...prev.messages,
              {
                role: "assistant" as const,
                content: "Mindmap generation failed. The model produced malformed data. Try selecting a larger model or rewording your input.",
              },
            ],
            loading: false,
          }));
        }
        return;
      }

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
            let fullThinking = "";
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
              (thinkingChunk) => {
                fullThinking += thinkingChunk;
                setStreamingThinking((prev) => prev + thinkingChunk);
              },
              () => {
                // Stop timer
                if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
                const totalTime = Math.floor((Date.now() - ragStartTime) / 100) / 10;
                const timeToFirstToken = firstTokenTimeMs ? Math.floor((firstTokenTimeMs - ragStartTime) / 100) / 10 : null;

                setGeneralGenerating(false);
                setGeneralElapsedTime(totalTime);
                setGeneralGenerationTime(totalTime);
                setStreamingThinking(""); // Clear streaming thinking

                updateSession(sid, (prev) => {
                  const updated: ChatMessage[] = [
                    ...prev.messages,
                    {
                      role: "assistant",
                      content: fullAnswer,
                      thinking: fullThinking || undefined,
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
              ctrl.signal,
              !thinkingEnabled,
              generationOptions
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
            audioOutputs: [(prev.audioOutputs ?? []), audioUrl].flat(),
            audioOutput: audioUrl, // for backward compatibility
            messages: [
              ...prev.messages,
              {
                role: "assistant" as const,
                content: `🔊 Audio generated (${ttsVoice}, ${ttsSpeed}x speed).`,
                generateTime: totalTime,
                audioUrl: audioUrl,
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
          const visionPrompt = text || (currentVisionImagePath ? "What's in this image?" : "Hello! Let's chat.");
          await Api.visionChatStream(
            currentVisionImagePath || undefined,
            visionPrompt,
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
      let fullThinking = "";
      let textFirstTokenTimeMs: number | null = null;

      // Build the messages array from the session's messages (including the new user msg)
      const sessionMessages = session.messages;
      const apiMessages = [...sessionMessages, newMsg].map((msg) => ({
        role: msg.role,
        content: msg.content,
      }));
      Api.streamChat(
        apiMessages,
        (chunk) => {
          if (textFirstTokenTimeMs === null) {
            textFirstTokenTimeMs = Date.now();
          }
          updateSession(sid, (prev) => ({ streamingContent: prev.streamingContent + chunk }));
          fullResponse += chunk;
        },
        (thinkingChunk) => {
          fullThinking += thinkingChunk;
          setStreamingThinking((prev) => prev + thinkingChunk);
        },
        () => {
          // Stop timer
          if (generalIntervalRef.current) clearInterval(generalIntervalRef.current);
          const totalTime = Math.floor((Date.now() - chatStartTime) / 100) / 10;
          const timeToFirstToken = textFirstTokenTimeMs ? Math.floor((textFirstTokenTimeMs - chatStartTime) / 100) / 10 : null;

          setGeneralGenerating(false);
          setGeneralElapsedTime(totalTime);
          setGeneralGenerationTime(totalTime);
          setStreamingThinking(""); // Clear streaming thinking

          if (fullResponse) {
            updateSession(sid, (prev) => ({
              messages: [
                ...prev.messages,
                {
                  role: "assistant" as const,
                  content: fullResponse,
                  thinking: fullThinking || undefined,
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
          setStreamingThinking(""); // Clear streaming thinking on error
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
        ctrl.signal,
        !thinkingEnabled,
        generationOptions
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

  const handleModeSwitch = (mode: ChatMode) => {
    if (mode === chatMode) return;

    // Preserve existing behavior: stop any active generation before switching mode.
    if (activeSession?.loading) {
      handleCancel();
    }

    const switchedTo = MODE_CONFIG.find((m) => m.mode === mode)?.label ?? mode;
    setModeSwitchNotice(`Switched to ${switchedTo} mode`);

    if (mode !== "vision") {
      setImagePath(null);
      setImagePreview(null);
    }
    if (mode !== "text" && mode !== "mindmap") {
      setDocPanelOpen(false);
    }

    setChatMode(mode);
  };

  const getPlaceholder = (): string => {
    switch (chatMode) {
      case "vision":
        return "Ask about the image (e.g., 'What's in this image?')";
      case "audio":
        return "Type text to generate speech...";
      case "podcast":
        return "What topic should the podcast cover?";
      case "mindmap":
        return ragDocs.length > 0
          ? "Ask for a mindmap (auto-grounds on relevant documents)..."
          : "Describe a topic to generate a mindmap...";
      default:
        return ragDocs.length > 0
          ? "Ask about your documents or chat freely..."
          : "Message NELA...";
    }
  };

  // ── Render ─────────────────────────────────────────────────────────────────

  const currentModeConfig = MODE_CONFIG.find((m) => m.mode === chatMode)!;
  const openViewerSessions = openSessionIds
    .map((id) => sessions.find((s) => s.id === id))
    .filter((s): s is ChatSession => !!s);

  // Sidebar section state with toggle logic
  const [sidebarSection, setSidebarSection] = useState<"chats" | "audio" | "mindmaps" | null>("chats");

  // Toggle handler for sidebar
  const handleSidebarNav = (section: "chats" | "audio" | "mindmaps") => {
    setSidebarSection((prev) => (prev === section ? null : section));
  };

  const activeSessionMindmaps = activeSession ? (mindmapsBySession[activeSession.id] ?? []) : [];
  const handleStartupAction = async (action: () => Promise<void>) => {
    await action();
    if (downloadOptionalOnStart) {
      await downloadMissingOptionalModels();
    }
  };

  const canContinueStartupWorkspace = !!(
    startupContinueWorkspace &&
    workspaces.some((workspace) => workspace.id === startupContinueWorkspace.id)
  );

  const continueExistingWorkspace = () => {
    if (!startupContinueWorkspace || !canContinueStartupWorkspace) return;
    void handleStartupAction(() => switchWorkspaceById(startupContinueWorkspace.id));
  };

  const createWorkspaceFromStartup = () => {
    void handleStartupAction(createNewWorkspace);
  };

  const importWorkspaceFromStartup = () => {
    void handleStartupAction(openWorkspaceFromFile);
  };

  const startTourFromStartup = () => {
    setSuppressStartupModal(true);
    startTour("getting-started", {
      source: "startup",
      onExit: () => setSuppressStartupModal(false),
      onComplete: () => setSuppressStartupModal(false),
    });
  };

  const mindmaps = activeSessionMindmaps
    .map((map) => ({
      id: map.id,
      sessionId: activeSession?.id ?? "",
      name: map.title,
      query: map.query,
      generatedFrom: map.generatedFrom,
      createdAt: map.createdAt,
    }))
    .sort((a, b) => b.createdAt - a.createdAt);

  const activeMindmapGraph = activeMindmapOverlay
    ? (mindmapsBySession[activeMindmapOverlay.sessionId] ?? []).find(
        (map) => map.id === activeMindmapOverlay.mindmapId
      ) ?? null
    : null;

  // Handler to save audio to sidebar (set audioSaved=true)
  const handleSaveAudioToSidebar = (msgIdx: number) => {
    if (!activeSession) return;
    updateSession(activeSession.id, (prev) => ({
      messages: prev.messages.map((m, i) => i === msgIdx ? { ...m, audioSaved: true } : m)
    }));
  };

  const handlePodcastGenerated = useCallback(
    ({ query, result }: { query: string; result: PodcastResult }) => {
      const trimmedQuery = query.trim();
      if (!trimmedQuery) return;

      const combinedAudioUrl = result.combined_audio_data_url?.trim() || "";
      const transcriptMessages: ChatMessage[] = result.segments.map((segment) => ({
        role: "assistant",
        content: `🎙️ ${segment.line.speaker}: ${segment.line.text}`,
      }));
      const combinedAudioMessage: ChatMessage = {
        role: "assistant",
        content: result.script?.title
          ? `🎧 Podcast generated: ${result.script.title}`
          : "🎧 Podcast generated.",
        ...(combinedAudioUrl ? { audioUrl: combinedAudioUrl, audioSaved: true } : {}),
      };

      if (!combinedAudioUrl && transcriptMessages.length === 0) return;

      const targetSessionId = activeSessionId && sessions.some((session) => session.id === activeSessionId)
        ? activeSessionId
        : null;

      if (targetSessionId) {
        updateSession(targetSessionId, (prev) => {
          const shouldSetTitle = prev.messages.length === 0;
          return {
            title: shouldSetTitle ? deriveTitleFromMessage(trimmedQuery) : prev.title,
            messages: [
              ...prev.messages,
              { role: "user", content: trimmedQuery },
              combinedAudioMessage,
              ...transcriptMessages,
            ],
            audioOutputs: combinedAudioUrl
              ? [...(prev.audioOutputs ?? []), combinedAudioUrl]
              : prev.audioOutputs ?? [],
            audioOutput: combinedAudioUrl || prev.audioOutput,
          };
        });
        return;
      }

      const newSession = createEmptySession();
      newSession.title = deriveTitleFromMessage(trimmedQuery);
      newSession.messages = [
        { role: "user", content: trimmedQuery },
        combinedAudioMessage,
        ...transcriptMessages,
      ];
      newSession.audioOutputs = combinedAudioUrl ? [combinedAudioUrl] : [];
      newSession.audioOutput = combinedAudioUrl || undefined;

      setSessions((prev) => [...prev, newSession]);
      setOpenSessionIds((prev) => (prev.includes(newSession.id) ? prev : [...prev, newSession.id]));
      setActiveSessionId(newSession.id);
    },
    [activeSessionId, sessions, updateSession]
  );

  // Show startup modal if no active workspace yet (unless we're running the tour from it)
  const showStartupModal = !activeWorkspace && !suppressStartupModal;
  const showParamsDock = !!activeRuntimeParamTarget && paramsDockOpen;
  const showRightSidebar = showParamsDock || docPanelOpen;

  return (
    <div className="relative w-full h-full overflow-hidden">
      {/* Startup Modal Overlay */}
      {showStartupModal && (
        <StartupModal
          onContinueWorkspace={continueExistingWorkspace}
          canContinueWorkspace={canContinueStartupWorkspace}
          continueWorkspaceName={startupContinueWorkspace?.name ?? null}
          onNewProject={createWorkspaceFromStartup}
          onImportProject={importWorkspaceFromStartup}
          onStartTour={startTourFromStartup}
          busy={workspaceBusy}
        />
      )}

      <AppModal
        isOpen={appModal.open}
        kind={appModal.kind}
        title={appModal.title}
        message={appModal.message}
        confirmLabel={appModal.confirmLabel}
        cancelLabel={appModal.cancelLabel}
        showCancel={appModal.showCancel}
        onConfirm={handleModalConfirm}
        onCancel={handleModalCancel}
      />

      <ModelsSettingsModal
        isOpen={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        models={registeredModels}
        modelCatalog={modelCatalog}
        onModelsUpdated={refreshModels}
        downloads={downloads}
        onDownload={handleDownloadModel}
        onCancelDownload={handleCancelDownload}
        onUninstall={handleUninstall}
        onDownloadMissingOptional={downloadMissingOptionalModels}
        onConfirm={confirmAction}
        workspaceId={activeWorkspace?.id}
      />

      <HuggingFaceModal
        isOpen={hfModalOpen}
        onClose={() => setHfModalOpen(false)}
        onModelImported={refreshModels}
        defaultFolder={hfModalPreset.folder}
        defaultImportProfile={hfModalPreset.profile}
      />

      <ToursModal isOpen={toursOpen} onClose={() => setToursOpen(false)} />

      {/* Main app content stays visible in background behind startup modal */}
      <div className="flex h-full w-full relative z-10">
          <SidebarNav
        selected={sidebarSection}
        onSelect={handleSidebarNav}
        onImportProject={() => void openWorkspaceFromFile()}
        onExportProject={() => void saveWorkspaceFile()}
        onOpenSettings={() => setSettingsOpen(true)}
        onOpenTours={() => setToursOpen(true)}
        onOpenHuggingFaceSearch={handleAddModel}
        workspaceBusy={workspaceBusy}
        canExport={!!activeWorkspace}
      />
      {/* Vertical blue line when sidebar is minimized */}
          {sidebarSection === null && (
        <div className="w-[4px] min-w-[4px] h-full bg-[#00d4ff] rounded-full mx-1 shadow-[0_0_16px_#00d4ff88] transition-all duration-200 opacity-100" />
      )}
      {/* Side section (chats/audio/mindmaps) */}
      {sidebarSection !== null && (
        <>
          {/* Side section content */}
          {sidebarSection === "chats" && (
            <ChatHistorySidebar
              sessions={sessions}
              activeSessionId={activeSessionId}
              onSelectSession={openSessionInViewer}
              onNewSession={addNewSession}
              onDeleteSession={closeSession}
            />
          )}
          {sidebarSection === "audio" && (
            <aside className="w-[280px] min-w-[280px] border-r border-glass-border bg-void-800/80 backdrop-blur-xl flex flex-col">
              <div className="h-10 px-4 flex items-center justify-between shrink-0">
                <div className="flex items-center gap-2 text-txt">
                  <span className="text-2xl font-semibold mt-2">Audio</span>
                </div>
              </div>
              <div className="flex-1 p-2 flex flex-col">
                <div className="flex-1 bg-void-900 border border-glass-border rounded-xl p-2 flex flex-col gap-2 shadow-md overflow-y-auto overflow-x-hidden">
                  {(() => {
                    // Gather all audio messages from all sessions
                    const allAudio = sessions.flatMap((session) =>
                      session.messages
                        .map((msg, idx, arr) => {
                          if (!msg.audioUrl || msg.audioSaved === false) return null;
                          // Find the most recent user message before this assistant message
                          let userMsg = null;
                          for (let i = idx - 1; i >= 0; i--) {
                            if (arr[i].role === "user") {
                              userMsg = arr[i];
                              break;
                            }
                          }
                          return {
                            audioUrl: msg.audioUrl,
                            sessionId: session.id,
                            sessionTitle: session.title,
                            msgIdx: idx,
                            userQuery: userMsg ? userMsg.content : "(Unknown query)"
                          };
                        })
                        .filter(Boolean)
                    );
                    const filteredAudio = allAudio.filter(Boolean);
                    return filteredAudio.length > 0 ? (
                      <ul className="flex flex-col gap-2">
                        {filteredAudio.map((item) => (
                          <li key={item!.audioUrl} className="flex flex-col gap-1 group relative">
                            <span className="text-[0.82rem] font-medium truncate">
                              {item!.userQuery}
                            </span>
                            <span className="text-[0.75rem] text-txt-muted truncate">{item!.sessionTitle}</span>
                            <AudioPlayer src={item!.audioUrl} barCount={20} />
                            <button
                              className="absolute top-1 right-1 opacity-60 group-hover:opacity-100 transition-opacity text-danger hover:text-danger/80"
                              title="Delete audio"
                              onClick={() => {
                                updateSession(item!.sessionId, (prev) => ({
                                  messages: prev.messages.map((m, i) => i === item!.msgIdx ? { ...m, audioSaved: false } : m)
                                }));
                              }}
                            >
                              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
                            </button>
                          </li>
                        ))}
                      </ul>
                    ) : (
                      <div className="text-[0.9rem] text-txt-muted">No audio generated yet for any session.</div>
                    );
                  })()}
                </div>
              </div>
            </aside>
          )}
          {sidebarSection === "mindmaps" && (
            <aside className="w-[280px] min-w-[280px] border-r border-glass-border bg-void-800/80 backdrop-blur-xl flex flex-col">
              <div className="h-10 px-4 flex items-center justify-between shrink-0">
                <div className="flex items-center gap-2 text-txt">
                  <span className="text-2xl font-semibold mt-2">Mindmaps</span>
                </div>
              </div>

              <div className="flex-1 p-2 flex flex-col">
                <div className="flex-1 bg-void-900 border border-glass-border rounded-xl p-2 flex flex-col gap-1.5 shadow-md overflow-y-auto">
                  {mindmaps.length === 0 ? (
                    <div className="text-[0.9rem] text-txt-muted p-2">No mindmaps generated yet.</div>
                  ) : (
                    mindmaps.map((mm) => {
                      const isOpen =
                        activeMindmapOverlay?.mindmapId === mm.id &&
                        activeMindmapOverlay?.sessionId === mm.sessionId;

                      return (
                        <button
                          key={mm.id}
                          className={`group relative w-full text-left rounded-xl border px-3 py-2.5 transition-all duration-150 ${
                            isOpen
                              ? "bg-neon-subtle border-neon/30 text-txt shadow-[0_0_14px_rgba(0,212,255,0.08)]"
                              : "bg-void-700/65 border-glass-border text-txt-secondary hover:border-neon/20 hover:text-txt"
                          }`}
                          onClick={() => openMindmapOverlay(mm.sessionId, mm.id)}
                          title={mm.name}
                        >
                          <div className="flex flex-col min-w-0">
                            <div className="flex items-center justify-between gap-2">
                              <span className="text-[0.82rem] font-medium truncate">{mm.name}</span>
                              <span className="text-[0.68rem] text-txt-muted shrink-0">
                                {new Date(mm.createdAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
                              </span>
                            </div>
                            <p className="mt-1 text-[0.72rem] text-txt-muted leading-snug max-h-[2.4em] overflow-hidden">
                              {mm.query || "No query"}
                            </p>
                            <div className="mt-1.5 text-[0.68rem] text-txt-muted">
                              {mm.generatedFrom === "documents" ? "Document-grounded" : "Model knowledge"}
                            </div>
                          </div>
                        </button>
                      );
                    })
                  )}
                </div>
              </div>
            </aside>
          )}
          {/* Vertical blue line between side section and chat window (less blue) */}
          <div className="w-[4px] min-w-[4px] h-full bg-[#00d4ff]/40 rounded-full mx-1 shadow-[0_0_8px_#00d4ff33] transition-all duration-200 opacity-60" />
        </>
      )}

      {/* ══════════ MAIN CONTENT ══════════ */}
      <main className="flex-1 flex flex-col bg-void-900 min-w-0 relative">
        {chatMode !== "podcast" && (
          <ChatTabBar
            sessions={openViewerSessions}
            activeSessionId={activeSessionId}
            onSelectSession={setActiveSessionId}
            onNewSession={addNewSession}
            onCloseSession={closeViewerTab}
            onReorderSessions={reorderViewerTabs}
          />
        )}

        {/* ── Top Bar ── */}
        <header className="min-h-14 py-2 flex items-center justify-between px-6 border-b border-glass-border bg-void-800/80 backdrop-blur-xl shrink-0 z-20">
          <div className="flex flex-col items-start gap-1.5">
            <div className="flex items-center gap-2.5">
              <currentModeConfig.icon size={18} strokeWidth={1.8} className="text-neon" />
              <h1 className="text-[0.95rem] font-semibold m-0 text-txt">{currentModeConfig.label}</h1>
              <span className="text-[0.78rem] text-txt-muted pl-2.5 border-l border-glass-border">{currentModeConfig.desc}</span>
            </div>
            <WorkspaceSelector
              workspaces={workspaces}
              activeWorkspaceId={activeWorkspace?.id ?? null}
              onSelectWorkspace={(id) => void switchWorkspaceById(id)}
              onCreateWorkspace={() => void createNewWorkspace()}
              onDeleteWorkspace={(id) => void deleteWorkspaceById(id)}
              onRenameWorkspace={(id, name) => renameWorkspaceById(id, name)}
              busy={workspaceBusy}
            />
            {modelLoadingStatus.loading && (
              <div className="flex items-center gap-2 px-3 py-1.5 bg-amber-900/30 border border-amber-500/40 rounded-lg text-amber-300 text-xs">
                <div className="w-3 h-3 border-2 border-amber-400 border-t-transparent rounded-full animate-spin" />
                <span>{modelLoadingStatus.message || "Loading model..."}</span>
              </div>
            )}
          </div>

          <div className="flex items-center gap-3">
            {(chatMode === "text" || chatMode === "mindmap") && (
              <ModelSelector
                models={models}
                selectedModel={selectedModel}
                onSelect={handleModelChange}
                type="llm"
                onAdd={handleAddModel}
                onDownload={handleDownloadModel}
                onCancelDownload={handleCancelDownload}
                onUninstall={handleUninstall}
                onConfirm={confirmAction}
                downloads={downloads}
              />
            )}
            {chatMode === "audio" && ttsEngines.length > 0 && (
              <div className="flex items-center gap-2.5">
                <ModelSelector
                    models={ttsEngines.map(m => ({ name: m.name, path: m.id, is_downloaded: m.is_downloaded, gdrive_id: m.gdrive_id }))}
                    selectedModel={selectedTtsEngine}
                    onSelect={setSelectedTtsEngine}
                    type="audio"
                    onDownload={handleDownloadModel}
                    onCancelDownload={handleCancelDownload}
                    onUninstall={handleUninstall}
                    onConfirm={confirmAction}
                    downloads={downloads}
                  />

                {selectedTtsEngine === "kitten-tts"}
              </div>
            )}
            {chatMode === "vision" && visionModels.length > 0 && (
              <ModelSelector
                  models={visionModels.map(m => ({ name: m.name, path: m.id, is_downloaded: m.is_downloaded, gdrive_id: m.gdrive_id }))}
                  selectedModel={selectedVisionModel}
                  onSelect={setSelectedVisionModel}
                  type="vision"
                  onAdd={handleAddVisionModel}
                  onDownload={handleDownloadModel}
                  onCancelDownload={handleCancelDownload}
                  onUninstall={handleUninstall}
                  onConfirm={confirmAction}
                  downloads={downloads}
                />
            )}

            {activeRuntimeParamTarget && (
              <button
                className={`glass-btn inline-flex items-center gap-1.5 py-1.5 px-3 text-[0.78rem] font-medium rounded-lg cursor-pointer transition-all duration-200 border backdrop-blur-md ${paramsDockOpen ? "bg-neon-subtle text-neon border-neon/30 shadow-[0_0_12px_rgba(0,212,255,0.12)]" : "bg-glass-bg text-txt-secondary border-glass-border hover:border-neon hover:text-neon hover:shadow-[0_0_12px_rgba(0,212,255,0.08)]"}`}
                onClick={() => setParamsDockOpen((open) => !open)}
                title="Toggle runtime parameter panel"
              >
                <SlidersHorizontal size={14} />
                {paramsDockOpen ? "Hide Params" : "Show Params"}
              </button>
            )}
          </div>
        </header>

        {/* ── Podcast Mode ── */}
        {chatMode === "podcast" ? (
          <PodcastTab
            hasDocuments={ragDocs.length > 0}
            modeOptions={MODE_CONFIG.map(({ mode, label }) => ({ mode, label }))}
            currentMode={chatMode}
            onSelectMode={handleModeSwitch}
            onPodcastGenerated={handlePodcastGenerated}
          />
        ) : !activeSession ? (
          <div className="flex-1 flex items-center justify-center text-txt-muted text-sm">
            {activeWorkspace
              ? "Open a chat from the left sidebar or create a new chat."
              : "No workspace selected. Create a workspace from the left sidebar."}
          </div>
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
            audioOutputs={activeSession.audioOutputs}
            placeholder={getPlaceholder()}
            mediaAssets={activeSession.mediaAssets}
            ragDocs={ragDocs}
            ragIngesting={ragIngesting}
            enrichmentStatus={enrichmentStatus}
            onIngestFile={ingestFile}
            onIngestDir={ingestDir}
            onSelectVisionImage={selectImage}
            visionImagePath={imagePath}
            visionImagePreview={imagePreview}
            onClearVisionImage={clearImage}
            onToggleDocPanel={() => setDocPanelOpen((v) => !v)}
            chatMode={chatMode}
            showRagControls={chatMode === "text" || chatMode === "mindmap"}
            docPanelOpen={docPanelOpen}
            modeOptions={MODE_CONFIG.map(({ mode, label }) => ({ mode, label }))}
            currentMode={chatMode}
            onSelectMode={handleModeSwitch}
            modeSwitchNotice={modeSwitchNotice}
            saveAudioToSidebar={handleSaveAudioToSidebar}
            session={activeSession}
            streamingThinking={streamingThinking}
            thinkingEnabled={thinkingEnabled}
            onToggleThinking={() => setThinkingEnabled(!thinkingEnabled)}
          />
        )}

        {activeMindmapOverlay && (activeMindmapGraph || activeMindmapOverlay.isGenerating) && (
          <MindMapOverlay
            graph={activeMindmapGraph}
            isGenerating={!!activeMindmapOverlay.isGenerating}
            query={activeMindmapOverlay.query}
            onClose={() => setActiveMindmapOverlay(null)}
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

      {startupModelToast.open && (
        <div className="fixed bottom-4 right-4 z-[90] w-[360px] max-w-[92vw] rounded-xl border border-neon/60 bg-void-800/95 shadow-[0_12px_36px_rgba(0,0,0,0.45)] backdrop-blur-md">
          <div className="px-4 py-3 text-sm text-txt">
            <div className="mb-1 flex items-center justify-between gap-2">
              <div className="font-medium">
                {startupModelToast.phase === "prompt"
                  ? "Model(s) absent"
                  : startupModelToast.phase === "downloading"
                    ? "Downloading models"
                    : "Model setup"}
              </div>
            </div>
            <div className="flex items-center justify-between gap-2">
              <div className="text-txt-muted leading-relaxed">
                {startupModelToast.phase === "downloading" && startupToastMinimized
                  ? `Progress: ${startupModelToast.completed}/${startupModelToast.total}`
                  : startupModelToast.message}
              </div>
              {startupModelToast.phase === "downloading" && (
                <button
                  type="button"
                  className="p-1 rounded text-txt-muted hover:text-txt hover:bg-void-700/50"
                  onClick={() => setStartupToastMinimized((prev) => !prev)}
                  aria-label={startupToastMinimized ? "Expand download notification" : "Minimize download notification"}
                  title={startupToastMinimized ? "Expand" : "Minimize"}
                >
                  {startupToastMinimized ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
                </button>
              )}
            </div>

            {startupModelToast.phase === "prompt" && startupModelToast.missingNames.length > 0 && (
              <div className="mt-2 text-xs leading-relaxed">
                <div className="text-txt-muted">The following models are not present:</div>
                <ul className="mt-2 space-y-1">
                  {startupModelToast.missingIds.map((modelId, idx) => {
                    const name = startupModelToast.missingNames[idx] ?? modelId;
                    const sizeLabel = formatModelSizeLabel(startupModelToast.missingSizesMb[idx]);
                    const checked = startupModelToast.selectedIds.includes(modelId);
                    return (
                      <li key={modelId}>
                        <label className="flex items-center gap-2 cursor-pointer rounded px-1 py-0.5 hover:bg-void-700/40">
                          <input
                            type="checkbox"
                            className="accent-neon"
                            checked={checked}
                            onChange={() => toggleStartupModelSelection(modelId)}
                          />
                          <span className="text-neon font-medium truncate" title={name}>{name}</span>
                          <span className="ml-auto text-[11px] text-txt-muted">{sizeLabel}</span>
                        </label>
                      </li>
                    );
                  })}
                </ul>
                <div className="mt-2 text-[11px] text-txt-muted">
                  Total selected size:{" "}
                  <span className="text-neon font-medium">{formatTotalSizeLabel(startupSelectedTotalMb)}</span>
                </div>
              </div>
            )}

            {startupModelToast.phase === "downloading" && !startupToastMinimized && (
              <div className="mt-2 space-y-2">
                <div className="flex items-center gap-2 text-neon text-xs">
                  <Loader2 size={13} className="animate-spin" />
                  <span>
                    Progress: {startupModelToast.completed}/{startupModelToast.total}
                  </span>
                </div>
                {startupModelToast.selectedIds.map((modelId) => {
                  const dl = downloads[modelId];
                  const isDone = startupModelToast.doneIds.includes(modelId);
                  const isFailed = startupModelToast.failedIds.includes(modelId);
                  const pct = isDone
                    ? 100
                    : typeof dl?.progress === "number"
                      ? Math.max(0, Math.min(100, dl.progress))
                      : 0;
                  const idx = startupModelToast.missingIds.indexOf(modelId);
                  const name = idx >= 0 ? startupModelToast.missingNames[idx] ?? modelId : modelId;
                  return (
                    <div key={modelId} className="space-y-1">
                      <div className="flex items-center justify-between text-[11px] text-txt-muted">
                        <span className="truncate max-w-[220px]" title={name}>{name}</span>
                        <span>{isDone ? "Done" : isFailed ? "Failed" : dl ? `${pct.toFixed(0)}%` : "Queued"}</span>
                      </div>
                      <div className="h-1.5 w-full rounded bg-void-700/80 overflow-hidden">
                        <div
                          className={`h-full transition-all duration-300 ${isFailed ? "bg-red-500" : "bg-neon"}`}
                          style={{ width: `${pct}%` }}
                        />
                      </div>
                    </div>
                  );
                })}
              </div>
            )}

            {startupModelToast.phase === "prompt" && (
              <div className="mt-3 flex items-center justify-end gap-2">
                <button
                  className="px-3 py-1.5 rounded-md border border-glass-border text-txt-muted text-xs hover:text-txt"
                  onClick={handleStartupToastDecline}
                >
                  No
                </button>
                <button
                  className="px-3 py-1.5 rounded-md bg-neon text-void-900 text-xs font-semibold hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed"
                  onClick={() => void handleStartupToastAccept()}
                  disabled={startupModelToast.selectedIds.length === 0}
                >
                  Yes, download ({startupModelToast.selectedIds.length})
                </button>
              </div>
            )}
          </div>
        </div>
      )}

      {/* ══════════ RIGHT SIDEBAR — Runtime Params / Knowledge Base ══════════ */}
      {showRightSidebar && (
        <aside className={`kb-sidebar overflow-hidden bg-void-800 flex shrink-0 ${showParamsDock && docPanelOpen ? "w-[640px] min-w-[640px]" : "w-[320px] min-w-[320px]"} border-l border-glass-border`}>
          {showParamsDock && activeRuntimeParamTarget && (
            <div className="w-[320px] min-w-[320px] h-full">
              <ActiveModelParamsDock
                target={activeRuntimeParamTarget}
                onApply={handleApplyRuntimeParams}
                onClose={() => setParamsDockOpen(false)}
              />
            </div>
          )}

      <div className={`overflow-hidden bg-void-800 flex flex-col shrink-0 ${docPanelOpen ? "w-[320px] min-w-[320px]" : "w-0 min-w-0"} ${showParamsDock && docPanelOpen ? "border-l border-glass-border" : "border-l-0"}`} data-tour="kb-sidebar">
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
              <FileText size={14} /> Add Files
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
                  const isPlaceholder = doc.doc_id < 0;
                  return (
                    <div
                      key={doc.doc_id}
                      className={`flex items-center gap-2 py-2 px-2.5 bg-void-700 rounded-lg text-[0.78rem] border border-transparent transition-colors duration-150 flex-wrap hover:border-glass-border ${isViewable ? "cursor-pointer hover:bg-[rgba(0,212,255,0.06)] hover:border-[rgba(0,212,255,0.2)]" : ""}`}
                      onClick={() => isViewable && openDocViewer(doc)}
                      title={isViewable ? `Click to view ${ext.toUpperCase()}` : doc.title}
                    >
                      <FileText size={14} className="text-txt-muted shrink-0" />
                      <span className="flex-1 min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-txt font-medium text-[0.78rem]">{doc.title}</span>
                      {!isPlaceholder && (
                        <span className="text-txt-muted text-[0.7rem] whitespace-nowrap">{doc.total_chunks} chunks</span>
                      )}
                      <span className={`py-0.5 px-2 rounded-full text-[0.65rem] font-semibold whitespace-nowrap capitalize ${doc.phase.includes("phase2_complete") ? "bg-[rgba(34,197,94,0.15)] text-success" : "bg-[rgba(0,212,255,0.1)] text-[#66e5ff]"}`}>
                        {isPlaceholder ? (
                          <span className="inline-flex items-center gap-1"><Loader2 size={10} className="animate-spin" /> ingesting</span>
                        ) : doc.phase.replace(/_/g, " ")}
                      </span>
                      {!isPlaceholder && (
                        <button
                          onClick={(e) => { e.stopPropagation(); deleteRagDoc(doc.doc_id); }}
                          className="p-1! bg-transparent! text-txt-muted! border-none! rounded! cursor-pointer flex items-center justify-center transition-all duration-150 hover:text-danger! hover:bg-[rgba(239,68,68,0.1)]!"
                          title="Remove document"
                        >
                          <Trash2 size={12} />
                        </button>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>

          {/* RAG Source Citations */}
          {activeSession?.ragResult && activeSession.ragResult.sources.length > 0 && (
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
      </div>
        </aside>
      )}
    </div>

    </div>
  );
}


export default App;
