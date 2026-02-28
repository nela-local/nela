export interface ModelFile {
  name: string;
  path: string;
}

export interface ChatMessage {
  role: "user" | "assistant" | "system";
  content: string;
}

export interface RegisteredModel {
  id: string;
  name: string;
  tasks: string[];
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

export type ChatMode = "text" | "vision" | "audio" | "rag";

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
