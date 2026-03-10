export interface ModelFile {
  name: string;
  path: string;
}

export interface ChatMessage {
  role: "user" | "assistant" | "system";
  content: string;
  generateTime?: number;
  firstTokenTime?: number;
}

export interface RegisteredModel {
  id: string;
  name: string;
  tasks: string[];
  status: string;
  instance_count: number;
  memory_mb: number;
  priority: number;
}

export interface IngestionStatus {
  doc_id: number;
  title: string;
  file_path: string;
  total_chunks: number;
  embedded_chunks: number;
  enriched_chunks: number;
  phase: string;
}

export interface SourceChunk {
  chunk_id: number;
  doc_title: string;
  text: string;
  score: number;
  /** Page/slide provenance from the original document (e.g. "page:3", "slide:2"). */
  page_info?: string;
}

export interface RagResult {
  answer: string;
  sources: SourceChunk[];
}

export interface RagStreamSetup {
  sources: SourceChunk[];
  prompt: string;
  llama_port: number;
  no_retrieval: boolean;
}

/** A media asset (image or table) extracted from an ingested document. */
export interface MediaAsset {
  id: number;
  doc_id: number;
  /** "image" or "table" */
  asset_type: string;
  /** Absolute path to the extracted PNG file on disk. */
  file_path: string;
  /** Context-aware caption derived from surrounding document text. */
  caption: string;
  /** Source metadata (e.g. "page:3:image:2"). */
  metadata: string;
  caption_hash: string | null;
}

export type ChatMode = "text" | "vision" | "audio" | "rag" | "podcast";

// ── Multi-Chat Session ────────────────────────────────────────────────────────

/** Represents a single, independent chat session (tab). */
export interface ChatSession {
  /** Unique session identifier (UUID). */
  id: string;
  /** Display title for the tab — derived from the first user message. */
  title: string;
  /** All messages in this session. */
  messages: ChatMessage[];
  /** Partial content currently being streamed for this session. */
  streamingContent: string;
  /** Whether this session is waiting for an LLM response. */
  loading: boolean;
  /** Audio data URL for the last TTS output in this session. */
  audioOutput: string;
  /** Set to true when user manually cancels generation. */
  cancelled: boolean;
  /** Latest RAG result (sources + answer) for this session. */
  ragResult: RagResult | null;
  /** Media assets keyed by message index. */
  mediaAssets: Record<number, MediaAsset[]>;
  /** Unix timestamp when this session was created (ms). */
  createdAt: number;
}

/** Available KittenTTS voice names. */
export const KITTEN_TTS_VOICES = [
  "Bella",
  "Jasper",
  "Luna",
  "Bruno",
  "Rosie",
  "Hugo",
  "Kiki",
  "Leo",
] as const;

export type KittenTtsVoice = (typeof KITTEN_TTS_VOICES)[number];

export interface ChatCompletionRequest {
  model?: string;
  messages: ChatMessage[];
  max_tokens?: number;
  temperature?: number;
  stream?: boolean;
}

// ── Podcast Types ─────────────────────────────────────────────────────────────

export interface PodcastRequest {
  query: string;
  voice_a: string;
  voice_b: string;
  speaker_a_name: string;
  speaker_b_name: string;
  max_turns: number;
  top_k?: number;
}

export interface PodcastLine {
  speaker: string;
  voice: string;
  text: string;
  index: number;
}

export interface PodcastScript {
  title: string;
  lines: PodcastLine[];
  source_chunks: string[];
}

export interface PodcastSegment {
  line: PodcastLine;
  audio_data_url: string;
}

export interface PodcastResult {
  script: PodcastScript;
  segments: PodcastSegment[];
  combined_audio_data_url: string;
}

export interface PodcastProgress {
  stage: "rag" | "scripting" | "tts" | "merging" | "done";
  detail: string;
  progress: number;
}

