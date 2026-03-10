# GenHat — The Local Intelligence Engine

> Agent reference document. Read this first before making any changes to the repo.

---

IMPORTANT NOTE: This file has to be updated by the agents on every change to the repo. It serves as the single source of truth for how the application works, how it's structured, and how to develop on it. Always keep it up to date with the latest architectural decisions, file structure, and development guidelines. If anything is removed mention it here, if anything is added mention it here. A new agent must be able to get the complete picture of the current state of the project by just reading this MD file.

## 1. Project Overview

GenHat is a **cross-platform desktop application** that runs LLM inference, RAG, vision, speech-to-text, text-to-speech, and intelligent query routing **entirely on the user's local machine** — no cloud APIs required. It is built with:

- **Tauri v2** (Rust backend + webview frontend)
- **React + TypeScript** (via Vite)
- **llama.cpp** (`llama-server` + `llama-mtmd-cli` + `whisper.cpp` binaries) for LLM / VLM / STT
- **ONNX Runtime** (statically linked via `ort` crate) for DistilBERT query classification + ms-marco cross-encoder relevance grading
- **GGUF model format** for quantized language/vision/embedding/STT models
- **ONNX model format** for the fine-tuned DistilBERT query router + ms-marco-MiniLM-L6-v2 cross-encoder grader
- **Full on-device RAG pipeline** with BM25, vector search, RRF fusion, RAPTOR trees, LLM enrichment, cross-encoder grading

The Rust backend is a modular, multi-model **process control system** (`~8200 LOC across 37 files`). Models are defined declaratively in `models.toml`, lazily spawned on first request, and managed by a central `ProcessManager` with health checks, idle reaping, and graceful shutdown.

---

## 2. Repository Structure

```
GenHat-The-Local-Intelligence-Engine/
│
├── .github/agents/
│   └── genhat.md                 ← THIS FILE (agent reference)
│
├── README.md                     ← Project README
├── .gitignore                    ← Ignores models/, target/, node_modules/, etc.
│
├── models/                       ← Model files (gitignored)
│   ├── LiquidAI-LLM/
│   │   └── LFM-1.2B-INT8.gguf       ← Default LLM (Liquid Foundation Model 1.2B)
│   ├── LiquidAI-VLM/
│   │   ├── LFM2.5-VL-1.6B-Q4_0.gguf ← Vision-Language Model (Q4)
│   │   ├── LFM2.5-VL-1.6B-Q8_0.gguf ← Vision-Language Model (Q8)
│   │   └── mmproj-LFM2.5-VL-1.6b-Q8_0.gguf ← Multimodal projector
│   ├── bge-small-1.5-Q8/
│   │   └── bge-small-en-v1.5-q8_0.gguf ← Embedding model (384-dim)
│   ├── distilBert-query-router/
│   │   └── onnx_model/              ← ONNX query classifier
│   │       ├── model.onnx           ← DistilBERT ONNX graph (~256 MB incl. data)
│   │       ├── model.onnx.data      ← External weights tensor
│   │       ├── config.json          ← HuggingFace config with id2label mapping
│   │       ├── tokenizer.json       ← WordPiece tokenizer
│   │       ├── tokenizer_config.json
│   │       ├── vocab.txt
│   │       └── special_tokens_map.json
│   ├── grader/
│   │   └── ms-marco-MiniLM-L6-v2-onnx-int8/
│   │       ├── model_quantized.onnx ← Cross-encoder ONNX model (~80 MB INT8)
│   │       ├── config.json          ← HuggingFace config
│   │       ├── tokenizer.json       ← WordPiece tokenizer
│   │       ├── tokenizer_config.json
│   │       ├── vocab.txt
│   │       └── special_tokens_map.json
│   ├── tts-chatterbox-q4-k-m/
│   │   ├── s3gen-bf16.gguf          ← TTS synthesis model
│   │   ├── t3_cfg-q4_k_m.gguf      ← TTS config model
│   │   └── ve_fp32-f16.gguf         ← TTS voice encoder
│   └── whisper/
│       └── model_q4_k.gguf          ← Whisper STT model (Q4_K)
│
├── The-Bare/                     ← Standalone Python inference scripts (prototyping)
│   ├── ASR-Inference/            ← (empty) Automatic Speech Recognition placeholder
│   ├── LLM-Inference/
│   │   └── Liquid-infer-INT8.py  ← CLI chat loop using llama-cpp-python
│   ├── TTS-inference/            ← Text-to-Speech generation script & build tools
│   │   ├── aud_test.py
│   │   ├── requirements.txt
│   │   └── BUILD_INSTRUCTIONS.md
│   └── DistilBERT-fine-tune/     ← Query router training notebook
│       └── train.ipynb           ← Dataset generation, fine-tuning, ONNX export, benchmarks
│
└── genhat-desktop/               ← THE MAIN APPLICATION (Tauri + React)
    ├── package.json              ← npm deps (React 19, Tauri API, Vite 7)
    ├── vite.config.ts            ← Vite config (React plugin only)
    ├── tsconfig.json             ← TypeScript project references
    ├── index.html                ← Main HTML shell (full UI layout)
    ├── src/                      ← Frontend source (TypeScript/React)
    │   ├── main.tsx              ← React entry point (10 lines)
    │   ├── App.tsx               ← Main app with 4 chat modes (850+ lines)
    │   ├── App.css               ← Component styles (180+ lines)
    │   ├── index.css             ← Global styles (68 lines)
    │   ├── types.ts              ← TypeScript type definitions (180+ lines)
    │   ├── api.ts                ← Centralized API layer (420+ lines)
    │   └── components/
    │       ├── ChatWindow.tsx    ← Chat UI component (130+ lines)
    │       ├── ModelSelector.tsx ← Model selection UI (80+ lines)
    │       ├── ModelSelector.css ← ModelSelector styles
    │       └── Sidebar.tsx       ← Sidebar navigation
    │
    └── src-tauri/                ← Rust backend (Tauri)
        ├── Cargo.toml            ← Rust dependencies
        ├── build.rs              ← Tauri build script
        ├── tauri.conf.json       ← Tauri config (window, bundle, resources)
        ├── capabilities/
        │   └── default.json      ← Tauri permissions
        ├── icons/                ← App icons (PNG, ICO, ICNS)
        ├── config/
        │   └── models.toml       ← Declarative model registry (8 models)
        ├── src/
        │   ├── main.rs           ← Bootstrap & lifecycle (135 lines)
        │   ├── lib.rs            ← Module declarations (17 lines)
        │   ├── config/
        │   │   └── mod.rs        ← TOML config parser (135 lines)
        │   ├── registry/
        │   │   ├── mod.rs        ← ModelRegistry (70 lines)
        │   │   └── types.rs      ← Core types: BackendKind, TaskType, ModelDef, etc. (274 lines)
        │   ├── backends/
        │   │   ├── mod.rs            ← ModelBackend trait + create_backend() dispatch (65 lines)
        │   │   ├── llama_server.rs   ← llama-server HTTP child process (517 lines)
        │   │   ├── llama_cli.rs      ← llama-mtmd-cli per-request VLM (423 lines)
        │   │   ├── whisper_cpp.rs    ← whisper.cpp STT backend (410 lines)
        │   │   ├── tts_inference.rs  ← PyInstaller TTS binary (295 lines)
        │   │   ├── onnx_classifier.rs ← ONNX Runtime DistilBERT classifier (273 lines)
        │   │   └── cross_encoder.rs  ← ONNX Runtime ms-marco cross-encoder grader (210 lines)
        │   ├── process/
        │   │   ├── mod.rs        ← ProcessManager: spawn/health/reap/shutdown (542 lines)
        │   │   ├── pool.rs       ← Instance pool utilities (44 lines)
        │   │   └── lifecycle.rs  ← Background health check thread (37 lines)
        │   ├── router/
        │   │   ├── mod.rs        ← TaskRouter: routes requests to models (189 lines)
        │   │   └── tasks.rs      ← Task request builders (embed, enrich, grade, etc.) (122 lines)
        │   ├── commands/
        │   │   ├── mod.rs        ← Command module declarations (8 lines)
        │   │   ├── models.rs     ← Model management IPC commands (284 lines)
        │   │   ├── inference.rs  ← Inference/routing IPC commands (154 lines)
        │   │   ├── audio.rs      ← TTS + STT IPC commands (63 lines)
        │   │   └── rag.rs        ← RAG IPC commands (119 lines)
        │   └── rag/
        │       ├── mod.rs        ← RAG module declarations (26 lines)
        │       ├── pipeline.rs   ← Progressive ingestion + hybrid retrieval (981 lines)
        │       ├── db.rs         ← SQLite vector store + metadata (503 lines)
        │       ├── search.rs     ← Tantivy BM25 full-text search (198 lines)
        │       ├── vecindex.rs   ← In-memory IVF vector index (361 lines)
        │       ├── fusion.rs     ← Reciprocal Rank Fusion (RRF) (120 lines)
        │       ├── chunker.rs    ← Recursive character + semantic chunking (206 lines)
        │       ├── raptor.rs     ← RAPTOR tree building + retrieval (760 lines)
        │       ├── raptor_examples.rs ← RAPTOR usage examples (266 lines)
        │       └── parsers/
        │           ├── mod.rs    ← Parser dispatch (62 lines)
        │           ├── pdf.rs    ← PDF text extraction (41 lines)
        │           ├── docx.rs   ← DOCX parsing (44 lines)
        │           ├── pptx.rs   ← PPTX parsing (97 lines)
        │           ├── text.rs   ← Plain text / markdown (79 lines)
        │           └── audio.rs  ← Audio transcription via Whisper (46 lines)
        └── bin/                  ← Pre-built binaries (per-OS)
            └── llama/            ← llama.cpp tools + shared libs
```

---

## 3. Architecture & Data Flow

```
┌──────────────────────────────────────────────────────────────────────┐
│  Tauri Webview (Frontend)                                            │
│  ┌──────────┐                                  ┌──────────────────┐  │
│  │ App.tsx   │──invoke()──┐                     │ fetch()          │  │
│  │ (React)   │            │                     │ → localhost:PORT │  │
│  └──────────┘             ▼                     └────────┬─────────┘  │
│                    ┌──────────────┐                      │            │
│                    │ Tauri IPC    │                      │            │
│                    │ (commands/)  │                      │            │
│                    └──────┬───────┘                      │            │
└───────────────────────────┼──────────────────────────────┼────────────┘
                            │                              │
                            ▼                              │
┌────────────────────────────────────────┐                 │
│          Rust Backend                  │                 │
│  ┌─────────────┐  ┌────────────────┐   │                 │
│  │ TaskRouter   │  │ ProcessManager │   │                 │
│  │ (router/)    │→│ (process/)     │   │                 │
│  └──────┬──────┘  └───────┬────────┘   │                 │
│         │                 │            │                 │
│         ▼                 ▼            │                 │
│  ┌──────────────────────────────┐      │                 │
│  │    Model Backends            │      │                 │
│  │  ┌──────────┐ ┌───────────┐  │      │   ┌─────────────┤
│  │  │llama-    │ │llama-     │  │      │   │llama-server ◄──── HTTP
│  │  │server    │ │mtmd-cli   │  │      │   │(child proc) │
│  │  └──────────┘ └───────────┘  │      │   └─────────────┘
│  │  ┌──────────┐ ┌───────────┐  │      │
│  │  │whisper-  │ │tts-       │  │      │
│  │  │cpp       │ │inference  │  │      │
│  │  └──────────┘ └───────────┘  │      │
│  │  ┌──────────────────────┐   │      │
│  │  │onnx_classifier      │   │      │
│  │  │(in-process, no HTTP) │   │      │
│  │  └──────────────────────┘   │      │
│  │  ┌──────────────────────┐   │      │
│  │  │cross_encoder        │   │      │
│  │  │(in-process, no HTTP) │   │      │
│  │  └──────────────────────┘   │      │
│  └──────────────────────────────┘      │
│                                        │
│  ┌─────────────────────────────┐       │
│  │   RAG Pipeline (rag/)       │       │
│  │  SQLite + Tantivy + IVF     │       │
│  │  → uses TaskRouter for      │       │
│  │    embed/enrich/grade/hyde   │       │
│  └─────────────────────────────┘       │
└────────────────────────────────────────┘
```

**Communication channels:**
1. **Tauri IPC** (`invoke()`): Frontend ↔ Rust for all commands (models, inference, RAG, audio)
2. **HTTP** (`fetch()`): Frontend → `llama-server` child process for streaming chat/completions
3. **Internal**: RAG pipeline → TaskRouter → ProcessManager → Backends (for embed/enrich/grade/hyde/classify)

---

## 4. Rust Backend — Module Reference

### 4.1 `main.rs` — Bootstrap & Lifecycle (135 lines)

On app launch (`setup` hook):
1. Loads `ModelRegistry` from embedded `models.toml`
2. Resolves models directory (`GENHAT_MODEL_PATH` env or `<repo>/models/`)
3. Creates `ProcessManager` (populates backends for all registered models)
4. Creates `TaskRouter` (wraps registry + process manager)
5. Starts lifecycle thread (30s health check + idle reaping)
6. Auto-starts models with `auto_start = true`
7. Initializes `RagPipeline` (opens SQLite DB, BM25 index, loads IVF vector index)
8. Starts background enrichment worker (Phase 2 of RAG ingestion)
9. Registers all Tauri IPC command handlers

On app exit: blocks on `ProcessManager::stop_all()` to kill all child processes.

### 4.2 `config/` — Configuration (145 lines + models.toml)

- `models.toml` is **embedded at compile time** via `include_str!`
- Parsed into `Vec<ModelDef>` with typed enums for `BackendKind`, `ModelKind`, `TaskType`
- Backend-specific params via `[models.params]` key-value map
- **Per-task priority overrides** via optional `[models.task_priorities]` map (task name → priority u32). Falls back to default `priority` field when not specified for a task.

### 4.3 `registry/` — Model Registry (344 lines)

Core type definitions shared across the whole system:

| Type | Purpose |
|---|---|
| `BackendKind` | Enum: `LlamaServer`, `LlamaCli`, `WhisperCpp`, `TtsInference`, `OnnxClassifier`, `CrossEncoder` |
| `ModelKind` | Enum: `ChildProcess`, `InProcess` |
| `TaskType` | Enum: `Chat`, `VisionChat`, `Summarize`, `Mindmap`, `Tts`, `PodcastScript`, `Transcribe`, `Stt`, `Embed`, `Classify`, `Enrich`, `Grade`, `Hyde`, `Custom(String)` |
| `ModelDef` | Full model definition (id, name, backend, tasks, params, limits, task_priorities, etc.). Has `priority_for_task(&TaskType) -> u32` method that checks `task_priorities` map first, falls back to default `priority`. |
| `ModelHandle` | Enum: `ChildProcess { pid, port, child }` or `InMemory { model: Arc<dyn Any> }` |
| `TaskRequest` | Input: `{ task_type, input, params }` |
| `TaskResponse` | Output: `{ Chat, Embedding, Classification, Transcription, AudioFile, Score, Raw }` |
| `ManagedModel` | Runtime model state (definition + live instances) |
| `ManagedInstance` | Single running instance (handle, status, timestamps) |
| `ModelInfo` | Serializable model status for frontend |

### 4.4 `backends/` — Model Backend Implementations (1786 lines)

The `ModelBackend` trait:
```rust
async fn start(&self, def: &ModelDef, models_dir: &Path) -> Result<ModelHandle, String>;
async fn is_healthy(&self, handle: &ModelHandle) -> bool;
async fn execute(&self, handle: &ModelHandle, request: &TaskRequest, models_dir: &Path) -> Result<TaskResponse, String>;
async fn stop(&self, handle: &ModelHandle) -> Result<(), String>;
fn estimated_memory_mb(&self, def: &ModelDef) -> u32;
```

| Backend | File | Type | Description |
|---|---|---|---|
| `LlamaServer` | `llama_server.rs` (530+ lines) | ChildProcess | Spawns `llama-server` with random port. Supports chat, embeddings, summarize, enrich, hyde, podcast_script. Health-checked via `/health`. Reads all params from `models.toml` including `flash_attn`, `mlock`, `cache_type`, `chat_template_kwargs` (e.g. for disabling Qwen thinking mode). |
| `LlamaCli` | `llama_cli.rs` (423 lines) | ChildProcess | Runs `llama-mtmd-cli` per-request for vision-language queries. Passes base64 images via `--image` flag. |
| `WhisperCpp` | `whisper_cpp.rs` (410 lines) | ChildProcess | Runs whisper.cpp per-request for audio transcription. Outputs JSON, parses segments. |
| `TtsInference` | `tts_inference.rs` (295 lines) | ChildProcess | Spawns PyInstaller-bundled TTS binary. Input text → output .wav file. |
| `OnnxClassifier` | `onnx_classifier.rs` (273 lines) | InProcess | Loads DistilBERT ONNX model + tokenizer into memory. Runs via ONNX Runtime (statically linked). 4 classes: NoRetrieval, SimpleRAG, MultiDoc, Summarization. Uses `Mutex<Session>` for thread safety. |
| `CrossEncoder` | `cross_encoder.rs` (210 lines) | InProcess | Loads ms-marco-MiniLM-L6-v2 ONNX cross-encoder + tokenizer into memory. Takes query+passage pairs, returns relevance scores 0-1. Used for RAG chunk grading. 100-300x faster than LLM grading. Uses `Mutex<Session>` for thread safety. |

### 4.5 `process/` — Process Manager (623 lines)

Central orchestrator for all model instances:
- **File-presence filtering**: At construction time, models whose required files are missing from the models directory are excluded — not registered, not listed, not routable. Uses `ModelDef::files_exist()` which checks the primary `model_file` and every param whose key ends with `_file`.
- **Lazy spawn**: Models start on first matching request
- **Instance pooling**: Up to `max_instances` per model (configurable)
- **Health checks**: Background thread every 30s via `lifecycle.rs`
- **Idle reaping**: Kills ephemeral instances after `idle_timeout_s` of inactivity
- **Memory budgeting**: Tracks estimated memory per model
- **Graceful shutdown**: Stops all processes on app exit
- **Legacy compat**: Maintains `active_llm_id` for `switch_model`/`stop_llama` commands

### 4.6 `router/` — Task Router (311 lines)

Single entry point for all inference:
1. Resolves which model handles a task (by `TaskType` + priority), skipping models not registered in ProcessManager (i.e. files missing)
2. Ensures model is running (delegates to ProcessManager)
3. Executes request against the backend
4. Handles compound tasks (e.g., PodcastScript = LLM script → TTS audio)

`tasks.rs` provides helper builders: `embed_request()`, `enrich_request()`, `grade_request()`, `hyde_request()`, `classify_request()`, `podcast_script_request()`, `chat_request()`

### 4.7 `commands/` — Tauri IPC Commands (628 lines)

All `#[tauri::command]` functions exposed to the frontend:

**Model Management** (`models.rs`):
| Command | Purpose |
|---|---|
| `list_models` | List LLM GGUF models in models dir |
| `list_vision_models` | List VLM models |
| `list_audio_models` | List TTS models |
| `list_registered_models` | List all models from registry with status |
| `get_model_status` | Get a specific model's runtime status |
| `start_model` | Explicitly start a model by ID |
| `stop_model` | Explicitly stop a model by ID |
| `switch_model` | Legacy: switch active LLM |
| `stop_llama` | Legacy: stop active LLM |
| `get_llama_port` | Get active LLM's HTTP port |
| `get_memory_usage` | Get total estimated memory usage |
| `read_image_base64` | Read an image file as base64 |

**Inference** (`inference.rs`):
| Command | Purpose |
|---|---|
| `route_request` | Route any TaskRequest through the TaskRouter |
| `vision_chat` | Send image+prompt to VLM, returns full response |
| `vision_chat_stream` | Send image+prompt to VLM, streams tokens via Tauri events |

**Audio** (`audio.rs`):
| Command | Purpose |
|---|---|
| `generate_speech` | Text → speech via TTS backend |
| `transcribe_audio` | Audio file → text via Whisper backend |

**RAG** (`rag.rs`):
| Command | Purpose |
|---|---|
| `ingest_document` | Ingest a single file into RAG |
| `ingest_folder` | Ingest all files in a directory |
| `query_rag` | Query the RAG pipeline |
| `list_rag_documents` | List all ingested documents |
| `delete_rag_document` | Remove a document from RAG index |
| `enrich_rag_documents` | Trigger Phase 2 enrichment |
| `build_raptor_tree` | Build RAPTOR hierarchical summary tree |
| `has_raptor_tree` | Check if RAPTOR tree exists |
| `delete_raptor_tree` | Delete RAPTOR tree |
| `query_rag_with_raptor` | Query using RAPTOR-augmented retrieval |

### 4.8 `rag/` — RAG Pipeline (3362 lines)

Fully local, on-device Retrieval-Augmented Generation:

**Progressive Ingestion:**
- **Phase 1 (instant)**: Parse document → chunk → embed via BGE → store in SQLite + BM25 index + IVF vector index
- **Phase 2 (background)**: Enrich chunks via LLM (generates summaries/questions) → re-embed enriched text
- **Phase 3 (lazy auto-trigger)**: Build RAPTOR tree (hierarchical summarization). Triggered automatically by the enrichment worker after 2 idle cycles (~60s of no pending chunks), or after 3 consecutive enrichment failures (fallback). Also buildable on-demand via the `build_raptor_tree` command.

**Hybrid Retrieval:**
1. Query → optional HyDE (Hypothetical Document Embeddings via LLM)
2. BM25 search (Tantivy) + Vector KNN (IVF index)
3. Reciprocal Rank Fusion (RRF) to merge results
4. Cross-encoder chunk grading (relevance scoring: 0-1 → 1-5 scale, ~0.05-0.1s for 8 chunks)
5. **Context window expansion**: For each top-k graded chunk, fetch the preceding (`chunk_index-1`) and following (`chunk_index+1`) chunks from the same document. Neighbors that are themselves selected sources are deduplicated. The expanded text (prev + source + next) is what the LLM sees.
6. Build context prompt → route to LLM for answer generation

**Intelligent Query Routing (micro-classifier):**
- DistilBERT ONNX model classifies incoming queries into 4 classes:
  - `NoRetrieval` → skip RAG, answer directly
  - `SimpleRAG` → single-doc retrieval
  - `MultiDoc` → multi-document cross-reference  
  - `Summarization` → summarize ingested content

**File details:**

| File | Lines | Purpose |
|---|---|---|
| `pipeline.rs` | ~1340 | Orchestrates ingestion + retrieval. Progressive phases. Background enrichment worker with Tauri event emission. HyDE generation. Classify-aware routing. Cross-encoder grading (replaces slow LLM grading). Context window expansion (`build_expanded_context`). RAPTOR lazy auto-trigger with `idle_cycles` reset + `failed_cycles` fallback. |
| `raptor.rs` | 760 | RAPTOR: Recursive Abstractive Processing for Tree-Organized Retrieval. K-means clustering → LLM summarization → hierarchical tree. Confidence-aware retrieval. Lazy auto-build. |
| `db.rs` | ~510 | SQLite storage: documents, chunks, embeddings (BLOB), enrichment metadata. r2d2 connection pooling. Added `get_adjacent_chunks(refs: &[(i64, i32)])` for context window expansion. |
| `vecindex.rs` | 361 | In-memory IVF (Inverted File) vector index. K-means partitioning for fast approximate KNN. Auto-rebuild on threshold. Insert/remove/search operations. |
| `chunker.rs` | 206 | Recursive character splitting with configurable chunk size/overlap. Semantic boundary detection (paragraphs, sentences). |
| `search.rs` | 198 | Tantivy BM25 full-text index. Schema auto-detection + migration on mismatch. |
| `fusion.rs` | 120 | Reciprocal Rank Fusion (RRF) with configurable k-parameter and per-source weighting. |
| `raptor_examples.rs` | 266 | Usage examples and patterns for RAPTOR integration. |
| `parsers/` | 369 | Document parsers: PDF (pdf-extract), DOCX (docx-rs), PPTX (zip+xml), TXT/MD, Audio (→ Whisper STT). |

---

## 5. Model Registry (models.toml)

Eleven models registered:

| ID | Name | Backend | Tasks | Kind | Priority | Auto-Start |
|---|---|---|---|---|---|---|
| `lfm-1_2b` | LFM 1.2B INT8 | llama_server | chat, summarize, mindmap, enrich, hyde, podcast_script | child_process | 20 (default) | No |
| `qwen3.5-0_8b` | Qwen3.5 0.8B Q4 | llama_server | chat, summarize, mindmap, enrich, hyde, podcast_script | child_process | 30 (podcast_script: 10) | No |
| `qwen3.5-2b` | Qwen3.5 2B Q4 | llama_server | chat, summarize, mindmap, hyde, podcast_script | child_process | 15 (podcast_script: 30) | No |
| `qwen3.5-4-b` | Qwen3.5 4B Q4 | llama_server | chat, summarize, mindmap, hyde | child_process | 25 | No |
| `lfm-2_5-vl-q4` | LFM 2.5 VL INT4 | llama_cli | vision_chat | child_process | 10 | No |
| `lfm-2_5-vl-q8` | LFM 2.5 VL INT8 | llama_cli | vision_chat | child_process | 5 | No |
| `bge-small-embed` | BGE-small-en-v1.5 Q8 | llama_server | embed | child_process | 10 | No |
| `bge-base-embed` | BGE-base-en-v1.5 Q8 | llama_server | embed | child_process | 15 | No |
| `query-router` | GenHat Query Router (DistilBERT ONNX) | onnx_classifier | classify | in_process | 10 | No |
| `ms-marco-grader` | MS MARCO MiniLM L6 v2 INT8 | cross_encoder | grade | in_process | 20 | No |
| `kitten-tts` | KittenTTS Mini v0.8 | kitten_tts | tts | in_process | 15 | No |
| `parakeet-tdt` | Parakeet TDT 0.6B INT8 | parakeet | transcribe, stt | in_process | 10 | No |

**Per-task priority routing** (effective priority for each task, sorted highest-first):
- **Chat/Summarize/Mindmap/Hyde**: Qwen 0.8B (30) > Qwen 4B (25) > LFM (20) > Qwen 2B (15)
- **PodcastScript**: Qwen 2B (30) > LFM (20) > Qwen 0.8B (10)
- **Enrich**: Qwen 0.8B (30) > LFM (20)

All models are **lazily loaded on first request**. Set `auto_start = true` to pre-load at app launch.

---

## 6. Frontend Architecture

### 6.1 React App (`App.tsx` — 850+ lines)

The React app mounted into `#root` provides:
- **4 Chat Modes**: Text, Vision, Audio, RAG with tabbed UI
- Model selection dropdowns (LLM, Vision, Audio)
- Start/stop model controls
- Chat interface with streaming responses
- Audio generation with playback
- Image upload for vision queries
- Document ingestion for RAG (single file + folder)
- RAG query interface with source citations

### 6.2 Modular Frontend Structure

**types.ts** (180+ lines)
- Complete TypeScript definitions for all API types
- Interfaces: `RegisteredModel`, `IngestionStatus`, `SourceChunk`, `RagResult`, `RagStreamSetup`, `ChatMode`, etc.

**api.ts** (420+ lines)
- Centralized API layer for all Tauri IPC calls
- 30+ methods covering models, inference, vision, audio, RAG, RAPTOR
- Dynamic port resolution (no hardcoded ports)
- Streaming support with port parameter

**components/**
- `ChatWindow.tsx` — Message display + input with mode-aware placeholders
- `ModelSelector.tsx` — Model selection UI component
- `Sidebar.tsx` — Navigation sidebar

### 6.3 External CDN Dependencies (loaded in index.html)
- **PDF.js v3.11.174** — PDF rendering
- **Lucide Icons** — Icon library

### 6.4 npm Dependencies
- `lucide-react 0.575.0` — React icon components

---

## 7. Pre-built Binaries (bin/)

Located at `src-tauri/bin/llama/`:
- `llama-server` — HTTP inference server (LLM + embeddings)
- `llama-mtmd-cli` — Multimodal CLI (vision-language)
- Various other llama.cpp tools (llama-cli, llama-bench, llama-quantize, etc.)
- **Shared libraries** (must be colocated with the executable):
  - Linux: `libggml-base.so`, `libggml.so`, `libllama.so`, `libmtmd.so`
  - Windows: corresponding `.dll` files
  - macOS: corresponding `.dylib` files

TTS binary: `bin/tts-<os>/tts-inference/` (PyInstaller-bundled Python + Torch)

The backends set `current_dir` to the binary folder so the OS linker finds sibling shared libraries.

---

## 8. Models

- Stored in `<repo>/models/` (gitignored)
- **LLM**: GGUF format via llama.cpp.
  - `LLM/LFM-1.2B-INT8.gguf` — Liquid Foundation Model 1.2B (enrichment fallback)
  - `LLM/Qwen3.5-0.8B-UD-Q4_K_XL.gguf` — Primary chat model (thinking disabled, flash-attn + mlock + Q8 KV cache)
  - `LLM/Qwen3.5-2B-Q4_K_M.gguf` — Primary podcast script model (thinking disabled, flash-attn + mlock + Q8 KV cache)
  - `LLM/Qwen3.5-4B-Q4_K_M.gguf` — High-quality chat fallback
- **VLM**: GGUF format. `LiquidAI-VLM/` with model + multimodal projector
- **Embeddings**: GGUF format. `bge-1.5-embed/` (both small 384-dim and base 768-dim variants)
- **Query Router**: ONNX format. `distilBert-query-router/onnx_model/model.onnx` (4-class DistilBERT)
- **Grader**: ONNX format. `grader/ms-marco-MiniLM-L6-v2-onnx-int8/model_quantized.onnx` (cross-encoder for relevance scoring, INT8 quantized, ~80 MB)
- **TTS**: ONNX format. `kittenTTS/mini/` (KittenTTS Mini v0.8)
- **STT**: ONNX format. `parakeet/` (Parakeet TDT 0.6B INT8, encoder + decoder + joiner)
- Custom path: set `GENHAT_MODEL_PATH` env var

---

## 9. Commands to Run

### Prerequisites
- **Node.js** (v18+) and **npm**
- **Rust** (stable, 1.77.2+) via `rustup`
- **Tauri v2 CLI**: `npm install -g @tauri-apps/cli` (or use npx)
- **System dependencies** (Linux): `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev`

### Development
```bash
cd genhat-desktop

# Install npm dependencies (first time)
npm install

# Run in development mode (starts Vite dev server + Tauri app)
npx tauri dev
```

### Production Build
```bash
cd genhat-desktop
npx tauri build
```
Output will be in `src-tauri/target/release/bundle/`.

### Rust-only Build & Test
```bash
cd genhat-desktop/src-tauri
cargo build          # Compile (ort downloads ONNX Runtime automatically)
cargo test --lib     # Run 19 unit tests
```

### Training / Exporting the Query Router (optional)
```bash
cd The-Bare/DistilBERT-fine-tune
# Open train.ipynb in Jupyter/VS Code
# Cell 1: Generate dataset + fine-tune DistilBERT
# Cell 5: Export to ONNX → models/distilBert-query-router/onnx_model/
# Cell 6: Benchmark ONNX inference
```

---

## 10. Configuration Reference

### tauri.conf.json
- `productName`: "GenHat"
- `identifier`: "com.genhat.dev"
- Dev server: `http://localhost:5173` (Vite)
- Frontend dist: `../dist`
- Bundle resources: `bin/llama-lin/*`, `bin/llama-win/*`, `bin/llama-mac/*`
- Window: 800×600, resizable, not fullscreen

### Cargo.toml Key Dependencies
- **Framework**: `tauri 2.10`, `tokio 1` (full), `async-trait 0.1`
- **Serialization**: `serde 1.0`, `serde_json 1.0`, `toml 0.8`
- **HTTP**: `reqwest 0.12` (json)
- **Concurrency**: `dashmap 6`, `crossbeam-channel 0.5`
- **RAG**: `rusqlite 0.32` (bundled), `r2d2 0.8`, `r2d2_sqlite 0.25`, `tantivy 0.22`
- **Document parsing**: `pdf-extract 0.8`, `docx-rs 0.4`, `zip 2`, `xml-rs 0.8`
- **ML Inference**: `ort 2.0.0-rc.11` (download-binaries), `tokenizers 0.22`
- **Utilities**: `uuid 1`, `portpicker 0.1`, `base64 0.22`, `tempfile 3`
- **Logging**: `log 0.4`, `tauri-plugin-log 2`

### package.json
- `react 19.2`, `@tauri-apps/api 2.10.1`
- Vite 7, TypeScript 5.9

---

## 11. Known Issues & Gotchas

1. **"Exec format error"**: The `llama-server` binary architecture must match the host OS. The code selects the correct OS folder via `cfg!()` at compile time.

2. **"Text file busy" build error**: If `llama-server` is running when you rebuild, the build fails. Kill it first: `pkill -9 llama-server`

3. **Bundle resources**: `tauri.conf.json` resources must point to existing paths at build time. Update before cross-platform builds.

4. **Port allocation**: `llama-server` uses random ports (via `portpicker`). The port is stored in `ModelHandle::ChildProcess { port }`. Frontend gets it via `get_llama_port` command.

5. **ONNX Runtime**: The `ort` crate with `download-binaries` feature automatically downloads the correct ONNX Runtime library during `cargo build`. The binary is statically linked — no runtime dependency needed.

6. **BM25 schema migration**: The Tantivy BM25 index auto-detects schema mismatches and wipes/recreates the index. No manual migration needed.

7. **IVF vector index**: The in-memory vector index rebuilds its k-means partitions when the insert count exceeds a threshold. This is automatic.

8. **TTS Architecture**:
    - The TTS engine uses PyInstaller (`--onedir`) to bundle Python + Torch dependencies.
    - Located in `src-tauri/bin/tts-<os>/tts-inference/`.
    - Requires sibling GGUF models in `models/tts-chatterbox-q4-k-m/`.

9. **RAG data directory**: RAG stores its SQLite DB and BM25 index in the Tauri app data directory (`~/.local/share/com.genhat.dev/rag/` on Linux).

---

## 12. Environment Variables

| Variable | Purpose | Default |
|---|---|---|
| `GENHAT_MODEL_PATH` | Override models directory | `<repo>/models/` |
| `RUST_BACKTRACE` | Enable Rust stack traces | Not set |
| `ORT_DYLIB_PATH` | Override ONNX Runtime library path (only if using `load-dynamic` feature) | Not needed with `download-binaries` |

---

## 13. Test Suite

19 unit tests, all passing:
- `rag::chunker::tests` (4) — paragraph splitting, overlap, short text, sequential indices
- `rag::fusion::tests` (3) — RRF basic, weighted, empty
- `rag::raptor::tests` (4) — kmeans basic, empty, group_by_cluster, estimate_confidence
- `rag::vecindex::tests` (5) — empty index, insert+search, remove, rebuild, IVF rebuild
- `registry::tests` (2) — registry load, find_for_task
- `config::tests` (1) — load embedded config

Run with: `cd genhat-desktop/src-tauri && cargo test --lib`

---

## 14. Development Guidelines for Future Agents

1. **Always kill `llama-server` before rebuilding**: `pkill -9 llama-server`
2. **Test changes in dev mode**: `cd genhat-desktop && npx tauri dev`
3. **Modular backend**: The Rust code is modular — each module has a clear responsibility. New backends go in `backends/`, new commands in `commands/`, new RAG features in `rag/`.
4. **Adding a new model**: Add a `[[models]]` entry to `config/models.toml`. If it uses an existing backend, no Rust code changes needed.
5. **Adding a new backend**: Create `backends/new_backend.rs`, implement `ModelBackend` trait, add variant to `BackendKind` enum in `registry/types.rs`, register in `backends/mod.rs::create_backend()`, add string mapping in `config/mod.rs::parse_backend()`.
6. **Adding a new task type**: Add variant to `TaskType` in `registry/types.rs`, add string mapping in `config/mod.rs::parse_task()`, add builder in `router/tasks.rs`.
7. **RAG pipeline**: Ingestion is lock-serialized. Background enrichment uses a crossbeam channel + Tauri event emission. RAPTOR trees are built on-demand.
8. **Binary compatibility**: When updating llama.cpp binaries, update ALL OS folders to the same version.
9. **ONNX model updates**: Re-train with `The-Bare/DistilBERT-fine-tune/train.ipynb`, export to ONNX (Cell 5), files go to `models/distilBert-query-router/onnx_model/`.
10. **The-Bare scripts**: Standalone prototypes, not used by the desktop app. Used for quick testing and model training.
11. **Cross-encoder grading**: RAG chunk grading now uses ms-marco cross-encoder (100-300x faster than LLM). Converts 0-1 scores to 1-5 scale: score < 0.2 → 1, 0.2-0.4 → 2, 0.4-0.6 → 3, 0.6-0.8 → 4, > 0.8 → 5.
12. **Context window expansion**: After cross-encoder grading, `build_expanded_context()` fetches neighboring chunks (`chunk_index±1`) from `db.get_adjacent_chunks()` in a single DB call. Neighbors already selected as top-k sources are skipped (deduplicated via a `HashSet`). This applies to both `query()` (non-streaming) and `query_retrieve()` (streaming) paths, including their RAG Fusion retry branches. The RAPTOR path (`query_with_raptor`) does NOT use this — RAPTOR summaries already provide hierarchical context expansion.
13. **RAPTOR worker**: The enrichment worker tracks `idle_cycles` (increments each 30s poll with no pending chunks) and `failed_cycles` (consecutive enrichment errors). RAPTOR auto-build triggers at `idle_cycles >= 2` (reset to 0 afterwards) OR `failed_cycles >= 3` (LLM unavailable fallback). Previously, a bug caused `idle_cycles` to never be reset, so repeated triggers would occur. Fixed.
14. **Frontend architecture**: Modular structure with types.ts (type definitions), api.ts (centralized API), components/ (reusable UI). App.tsx uses 4 chat modes with tabbed navigation.
15. **Per-task priority**: Models can override their default priority for specific tasks via `[models.task_priorities]` in `models.toml` and the `task_priorities: HashMap<TaskType, u32>` field in `ModelDef`. The registry's `find_for_task()` uses `priority_for_task()` which checks task-specific overrides first. This enables different priority ordering for chat vs podcast_script without duplicating model entries.
16. **Podcast script routing**: The podcast engine (`podcast/engine.rs`) routes script generation via `TaskType::PodcastScript` (not `Chat`), so it respects the separate podcast priority chain (Qwen 2B > LFM > Qwen 0.8B).
17. **llama-server params from TOML**: All llama-server CLI flags are driven from `[models.params]` in `models.toml`. Supported params: `ctx_size`, `max_tokens`, `temp`, `top_p`, `top_k`, `repeat_penalty`, `embedding`, `batch_size`, `flash_attn`, `mlock`, `cache_type`, `chat_template_kwargs`. No hardcoded values — defaults are in `param_or()` calls.
18. **Model file-presence filtering**: `ModelDef::files_exist(models_dir)` / `missing_files(models_dir)` checks the primary `model_file` (file or directory) and every param key ending with `_file`. `ProcessManager::new()` skips models with missing files (logged at `warn` level). The `TaskRouter` iterates registry candidates and only picks models that are present in ProcessManager. This means models simply disappear from listings and routing when their files are absent — no error at runtime.
19. **Keep this file updated**: Every architectural change, new module, renamed file, or removed feature must be reflected here.