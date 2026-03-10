import { invoke } from "@tauri-apps/api/core";
import type {
  ChatMessage,
  ModelFile,
  RegisteredModel,
  IngestionStatus,
  RagResult,
  RagStreamSetup,
  MediaAsset,
  PodcastRequest,
  PodcastResult,
} from "./types";

export const Api = {
  // ── Model Management ───────────────────────────────────────────────────────

  /** List LLM .gguf model files from the LiquidAI-LLM subfolder. */
  async listModels(): Promise<ModelFile[]> {
    return invoke<ModelFile[]>("list_models");
  },

  /** List VLM .gguf model files from the LiquidAI-VLM subfolder. */
  async listVisionModels(): Promise<ModelFile[]> {
    return invoke<ModelFile[]>("list_vision_models");
  },

  /** List all registered models with their current status and supported tasks. */
  async listRegisteredModels(): Promise<RegisteredModel[]> {
    return invoke<RegisteredModel[]>("list_registered_models");
  },

  /** Switch to a different LLM model by registry ID or file path. */
  async switchModel(modelIdentifier: string): Promise<void> {
    await invoke("switch_model", { modelIdentifier });
  },

  /** Stop the currently active LLM server. */
  async stopLlama(): Promise<void> {
    await invoke("stop_llama");
  },

  /** Get the HTTP port of the running llama-server (triggers lazy start). */
  async getLlamaPort(): Promise<number> {
    return invoke<number>("get_llama_port");
  },

  /** Get estimated total memory usage of all loaded models (MB). */
  async getMemoryUsage(): Promise<number> {
    return invoke<number>("get_memory_usage");
  },

  /** Manually start (pre-warm) a model by ID. */
  async startModel(modelId: string): Promise<string> {
    return invoke<string>("start_model", { modelId });
  },

  /** Stop a model (all instances) by ID. */
  async stopModel(modelId: string): Promise<void> {
    await invoke("stop_model", { modelId });
  },

  /** Get the runtime status of a specific model. */
  async getModelStatus(modelId: string): Promise<unknown> {
    return invoke("get_model_status", { modelId });
  },

  // ── Audio ──────────────────────────────────────────────────────────────────

  /** Generate speech from text using the TTS backend. Returns a playable data URL. */
  async generateSpeech(
    input: string,
    options?: { voice?: string; speed?: number }
  ): Promise<string> {
    // Backend returns a data:audio/wav;base64,… URL directly
    return invoke<string>("generate_speech", {
      input,
      voice: options?.voice ?? null,
      speed: options?.speed ?? null,
    });
  },

  /** Transcribe an audio file to text using Whisper. */
  async transcribeAudio(audioPath: string): Promise<unknown> {
    return invoke("transcribe_audio", { audioPath });
  },

  // ── Vision ─────────────────────────────────────────────────────────────────

  /** Read an image file and return it as a base64-encoded data URL (for preview). */
  async readImageBase64(path: string): Promise<string> {
    return invoke<string>("read_image_base64", { path });
  },

  /** Send image + prompt to VLM and return full response (non-streaming). */
  async visionChat(imagePath: string, prompt: string): Promise<string> {
    return invoke<string>("vision_chat", { imagePath, prompt });
  },

  /**
   * Start streaming vision chat. Emits "vision-stream" Tauri events.
   * Frontend should `listen("vision-stream", handler)` before calling this.
   */
  async visionChatStream(
    imagePath: string,
    prompt: string,
    modelId?: string
  ): Promise<void> {
    await invoke("vision_chat_stream", {
      imagePath,
      prompt,
      modelId: modelId || null,
    });
  },

  // ── RAG ────────────────────────────────────────────────────────────────────

  /** Ingest a single document into the RAG knowledge base. */
  async ingestDocument(path: string): Promise<IngestionStatus> {
    return invoke<IngestionStatus>("ingest_document", { path });
  },

  /** Ingest all supported files in a directory. */
  async ingestFolder(path: string): Promise<IngestionStatus[]> {
    return invoke<IngestionStatus[]>("ingest_folder", { path });
  },

  /** Query the RAG pipeline (non-streaming). */
  async queryRag(query: string, topK?: number): Promise<RagResult> {
    return invoke<RagResult>("query_rag", { query, topK });
  },

  /**
   * Streaming RAG query — retrieves sources immediately, then returns
   * the llama-server port + augmented prompt for frontend SSE streaming.
   */
  async queryRagStream(
    query: string,
    topK?: number
  ): Promise<RagStreamSetup> {
    return invoke<RagStreamSetup>("query_rag_stream", { query, topK });
  },

  /** List all ingested documents with their ingestion status. */
  async listRagDocuments(): Promise<IngestionStatus[]> {
    return invoke<IngestionStatus[]>("list_rag_documents");
  },

  /** Delete a document from the knowledge base. */
  async deleteRagDocument(docId: number): Promise<void> {
    await invoke("delete_rag_document", { docId });
  },

  /** Read a file as base64 data URL for the frontend viewer. */
  async readFileBase64(path: string): Promise<string> {
    return invoke<string>("read_file_base64", { path });
  },

  /** Read a text-based file and return its content as a string. */
  async readFileText(path: string): Promise<string> {
    return invoke<string>("read_file_text", { path });
  },

  // ── Podcast ────────────────────────────────────────────────────────────────

  /** Generate a podcast from a RAG query with two-person dialogue + TTS. */
  async generatePodcast(request: PodcastRequest): Promise<PodcastResult> {
    return invoke<PodcastResult>("generate_podcast", { request });
  },

  /** Manually trigger a round of background enrichment. */
  async enrichRagDocuments(batchSize?: number): Promise<number> {
    return invoke<number>("enrich_rag_documents", { batchSize });
  },

  // ── RAPTOR ─────────────────────────────────────────────────────────────────

  /** Build a RAPTOR tree for a specific document (Phase 3). */
  async buildRaptorTree(docId: number): Promise<unknown> {
    return invoke("build_raptor_tree", { docId });
  },

  /** Check if a document has a RAPTOR tree. */
  async hasRaptorTree(docId: number): Promise<boolean> {
    return invoke<boolean>("has_raptor_tree", { docId });
  },

  /** Delete the RAPTOR tree for a document. */
  async deleteRaptorTree(docId: number): Promise<void> {
    await invoke("delete_raptor_tree", { docId });
  },

  /** Query using RAPTOR tree with confidence-aware traversal. */
  async queryRagWithRaptor(
    docId: number,
    query: string,
    topK?: number
  ): Promise<RagResult> {
    return invoke<RagResult>("query_rag_with_raptor", { docId, query, topK });
  },

  /** Streaming RAPTOR query — retrieve + return setup for SSE streaming. */
  async queryRagWithRaptorStream(
    docId: number,
    query: string,
    topK?: number
  ): Promise<RagStreamSetup> {
    return invoke<RagStreamSetup>("query_rag_with_raptor_stream", {
      docId,
      query,
      topK,
    });
  },

  // ── Media Retrieval ────────────────────────────────────────────────────────

  /**
   * Two-phase media retrieval: given the LLM's response text, find images/tables
   * whose captions are semantically similar to the response content.
   * Returns media assets that should be displayed alongside the chat answer.
   */
  async retrieveMediaForResponse(
    responseText: string,
    topK?: number,
    threshold?: number
  ): Promise<MediaAsset[]> {
    return invoke<MediaAsset[]>("retrieve_media_for_response", {
      responseText,
      topK: topK ?? null,
      threshold: threshold ?? null,
    });
  },

  /** Get all media assets for a specific ingested document. */
  async getMediaForDocument(docId: number): Promise<MediaAsset[]> {
    return invoke<MediaAsset[]>("get_media_for_document", { docId });
  },

  /**
   * Convert an absolute file path to a Tauri asset URL for display in an <img> tag.
   * Uses Tauri's convertFileSrc to create a localhost URL the webview can load.
   */
  mediaUrl(filePath: string): string {
    return convertFileSrc(filePath);
  },

  // ── Inference Routing ──────────────────────────────────────────────────────

  /**
   * Route any TaskRequest through the backend TaskRouter.
   * Supports: chat, summarize, mindmap, tts, podcast_script, transcribe,
   * embed, classify, enrich, grade, hyde, vision_chat, and custom tasks.
   */
  async routeRequest(
    taskType: string,
    input: string,
    modelOverride?: string,
    extra?: Record<string, string>
  ): Promise<unknown> {
    return invoke("route_request", {
      taskType,
      input,
      modelOverride: modelOverride || null,
      extra: extra || null,
    });
  },

  // ── Streaming Chat (HTTP → llama-server) ───────────────────────────────────

  /**
   * Stream a chat completion via llama-server's OpenAI-compatible SSE endpoint.
   * Fetches the dynamic port from the backend unless one is provided.
   */
  async streamChat(
    messages: ChatMessage[],
    onChunk: (chunk: string) => void,
    onFinish: () => void,
    onError: (err: unknown) => void,
    port?: number,
    signal?: AbortSignal
  ) {
    try {
      const llamaPort =
        port || (await invoke<number>("get_llama_port"));

      const res = await fetch(
        `http://127.0.0.1:${llamaPort}/v1/chat/completions`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            messages,
            stream: true,
            max_tokens: 2048,
            temperature: 0.7,
          }),
          signal,
        }
      );

      if (!res.ok) {
        const errBody = await res.text().catch(() => res.statusText);
        throw new Error(
          `LLM server returned ${res.status}: ${errBody}`
        );
      }

      if (!res.body)
        throw new Error("No response body received from local LLM");

      const reader = res.body.getReader();
      const decoder = new TextDecoder("utf-8");
      let buffer = "";

      while (true) {
        const { value, done } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split("\n");
        buffer = lines.pop() || "";

        for (const line of lines) {
          if (!line.startsWith("data:")) continue;

          const payload = line.replace("data:", "").trim();
          if (payload === "[DONE]") {
            onFinish();
            return;
          }

          try {
            const parsed = JSON.parse(payload);
            const content = parsed.choices?.[0]?.delta?.content;
            if (content) {
              onChunk(content);
            }
          } catch (e) {
            console.warn("Failed to parse SSE JSON chunk", e);
          }
        }
      }

      onFinish();
    } catch (err) {
      // AbortError means the user cancelled — stop silently without error msg
      if (err instanceof DOMException && err.name === "AbortError") return;
      onError(err);
    }
  },
};
function convertFileSrc(filePath: string): string {
  return new URL(`file://${filePath}`).href;
}

