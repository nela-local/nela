<div align="center">

<img src="genhat-desktop/public/logo-dark.png" alt="NELA" width="110"/>

# NELA

### Your private AI workspace — entirely on your machine.

[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-6366f1?style=flat-square)](#)
[![Tauri v2](https://img.shields.io/badge/Tauri-v2-24C8D8?style=flat-square&logo=tauri&logoColor=white)](https://tauri.app)
[![React 19](https://img.shields.io/badge/React-19-61DAFB?style=flat-square&logo=react&logoColor=black)](https://react.dev)
[![Rust](https://img.shields.io/badge/Rust-stable-CE422B?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-22c55e?style=flat-square)](LICENSE)

NELA is a local-first AI desktop application — no subscriptions, no data leaving your device, no cloud inference required.  
Chat with your documents, analyse images, generate speech, produce podcasts, build mindmaps, and wire together custom AI pipelines, all powered by models that run on your own hardware.

</div>

---

## What is NELA?

Most AI tools ship as web services. NELA does the opposite. It is a full desktop application built with **Tauri** (a Rust-powered native shell) and a **React** frontend. Every model runs locally via a built-in inference runtime — your prompts and documents never leave your machine.

NELA organises your work into **project workspaces** that can be exported and imported as `.nela` archives, so your chats, documents, podcasts, and mindmaps travel with you like any other file.

> Internet access is only used when you choose to download new models from Hugging Face inside the app. Inference itself is 100% offline.

---

## Features

<table>
<tr>
<td width="50%" valign="top">

### 💬 Chat + Document Grounding

Conversational AI powered by any local LLM you install. Add files or entire folders to build a **retrieval-augmented knowledge base** — NELA indexes them, retrieves the most relevant passages, and cites sources in every answer.

Supports PDF, DOCX, PPTX, Markdown, plain text, code files, CSV, JSON, YAML, HTML, and audio transcripts (MP3, WAV, M4A, and more).

</td>
<td width="50%" valign="top">

### 👁️ Vision Mode

Drop an image into the conversation and ask anything about it. Vision mode uses a multimodal VLM running fully on-device — no upload, no third-party API.

</td>
</tr>
<tr>
<td width="50%" valign="top">

### 🎙️ Audio Mode

Two-way voice interaction in a single mode.

- **Speech-to-text** — dictate prompts with your microphone; a local ASR model transcribes in real time.
- **Text-to-speech** — listen to responses with your choice of local TTS voice and speed.

</td>
<td width="50%" valign="top">

### 🎙️🎙️ Podcast Studio

Turn a knowledge base into a listenable conversation. Give two speaker names, a topic, and let NELA script a multi-turn dialogue from your documents. It then synthesises every line into audio and stitches the segments into a single combined episode track.

</td>
</tr>
<tr>
<td width="50%" valign="top">

### 🧠 Mindmaps

Generate visual concept trees from either your ingested documents or a model's own knowledge. Great for studying, planning, and brainstorming. All maps are saved per-workspace and reopen instantly from the sidebar.

</td>
<td width="50%" valign="top">

### ⚙️ Pipeline Playground

A node-based visual editor for building custom AI pipelines. Wire together **LLM**, **Transcribe**, **TTS**, **RAG Query**, **File Read**, **Script**, **Condition**, **Transform**, and more into reusable automation flows — no code required.

</td>
</tr>
</table>

---

## Model Management

NELA ships with a full in-app model manager.

- **Browse and install** models from Hugging Face directly from the Settings panel.
- **Compatibility scoring** estimates RAM usage, CPU performance, and disk requirements *before* you download, so you never accidentally pull a 70B model onto a 16 GB laptop.
- **Runtime parameter controls** — context size, max tokens, temperature, top-p, top-k, repeat penalty, and backend-specific flags like Flash Attention — are adjustable per-session without restarting the app.
- Parameters you tune are **preserved across model-list refreshes**, so your settings aren't wiped every time NELA re-scans for new models.

Supported model classes:

| Class | Purpose |
|---|---|
| LLM | Text generation and conversation |
| VLM | Multimodal vision + language |
| ASR | Speech-to-text transcription |
| TTS | Text-to-speech synthesis |
| Embedding | Semantic indexing for RAG |
| Grader / Reranker | Chunk relevance scoring |
| Classifier / Router | Intent routing |

---

## Workspaces

Everything in NELA is scoped to a **workspace** — a named project that holds:

- Chat sessions and message history
- Ingested document knowledge base
- Generated podcasts and audio episodes
- Saved mindmaps
- Model preferences and runtime parameters

Workspaces export and import as `.nela` archives, making sharing and backup as simple as copying a file.

---

## Tech Stack

| Layer | Technology |
|---|---|
| Desktop shell | [Tauri v2](https://tauri.app) (Rust) |
| Frontend | [React 19](https://react.dev) + TypeScript + [Vite](https://vitejs.dev) |
| Styling | [Tailwind CSS v4](https://tailwindcss.com) |
| Pipeline canvas | [@xyflow/react](https://reactflow.dev) |
| Inference runtime | llama.cpp-compatible GGUF backend (Rust) |
| Vector search | In-process IVF vector index (Rust) |
| ASR | ONNX-based local transcription |
| TTS | Custom on-device synthesis pipeline |

---

## Run from Source

**Prerequisites:** Node.js 24+, npm, Rust stable toolchain.

Linux also needs a few system libraries:

```sh
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libasound2-dev pkg-config
```

**Start in dev mode:**

```sh
cd genhat-desktop
npm ci
npx tauri dev
```

The app launches with a startup modal where you create or import a workspace. From there, open **Settings** to download models and you're ready.

**Build a distributable package:**

```sh
# Linux .deb
npx tauri build --bundles deb

# macOS .dmg
npx tauri build --bundles dmg

# Windows installer
npx tauri build --bundles msi,nsis
```

---

## First Run Checklist

1. Create a workspace from the startup screen.
2. Go to **Settings → Models** and install the models you want (start with a mid-size LLM like a 7B or 8B Q4 for chat).
3. For RAG, also install an **embedding model** and optionally a **grader model**.
4. Pick a mode from the input bar and start exploring.
5. Use **Help → Tours** for a guided in-app walkthrough if you want one.

---

## Repository Layout

```
nela/
├── genhat-desktop/     # Main desktop app (Tauri + React)
│   ├── src/            # Frontend — components, hooks, app logic
│   └── src-tauri/src/  # Rust backend — inference, RAG, TTS, ASR, commands
├── benchmark/          # Runtime benchmark suite and plotting tools
├── models/             # Local model storage (gitignored)
└── The-Bare/           # Standalone experiments and prototypes
```

---

<div align="center">

*NELA — local intelligence, no strings attached.*

</div>
