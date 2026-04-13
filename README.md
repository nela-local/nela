# NELA

NELA is a local-first desktop AI workspace for chat, documents, vision, speech, podcast generation, and mindmaps.

It runs on-device using Tauri (Rust + React), so your inference pipeline can work without cloud APIs for normal usage. Internet is only needed for downloading models (and optionally searching Hugging Face inside the app).

## Why NELA

- Local intelligence with no required cloud inference backend.
- Workspace-oriented UX with import/export via .nela project files.
- Multi-mode assistant in one app:
  - Chat with optional document grounding (RAG).
  - Vision chat over images.
  - Audio mode for TTS output and voice input transcription.
  - Podcast Studio to generate two-speaker audio episodes from your docs.
  - Mindmap generation for study/planning/brainstorming.
- Model management UI with compatibility scoring and runtime parameter controls.
- Guided onboarding tours built into the app.

## Repository Layout

- [genhat-desktop](genhat-desktop): Main NELA desktop app (Tauri + React).
- [models](models): Model storage (gitignored in normal development).
- [benchmark](benchmark): Runtime benchmark suite and plotting tools.
- [The-Bare](The-Bare): Standalone experiments/prototypes.

## Quick Start For Users

If you already have a release build, install and launch it directly.

If you want to run from source, follow the steps below.

## Run From Source

### 1. Prerequisites

- Node.js 24 or newer
- npm
- Rust stable toolchain (Cargo), rustc 1.77.2+ recommended

Linux system packages (Ubuntu/Debian):

	sudo apt-get update
	sudo apt-get install -y \
	  libwebkit2gtk-4.1-dev \
	  libgtk-3-dev \
	  libayatana-appindicator3-dev \
	  librsvg2-dev \
	  libasound2-dev \
	  pkg-config

### 2. Install Dependencies

	cd genhat-desktop
	npm ci

### 3. Start NELA In Development Mode

	npx tauri dev

The app opens with the startup modal where you can create a new workspace or import an existing .nela project.

## First-Time In-App Flow

1. Create a workspace (or import a .nela file).
2. Open Settings and download required models for your workflow.
3. Choose a mode from the input bar: Chat, Vision, Audio, Podcast, or Mindmap.
4. Select an installed model for that mode.
5. Start interacting.

Tip: Use Help -> Tours if you want a guided walkthrough.

## Core Features

### Workspaces And Project Files

- Workspaces isolate chats, documents, podcasts, and generated assets.
- Export/import supported via .nela archives.

### Chat + RAG

- Ask normal LLM questions.
- Add files/folders to build a local knowledge base.
- RAG pipeline supports chunking, retrieval, enrichment, and source-backed answers.

Single-file ingestion supports document/code/audio types including:

- pdf, docx, pptx
- txt, md, csv, json, toml, yaml/yml, xml, html, css, log
- rs, py, js, ts, c, cpp, h, java, go, rb, sh, bat
- mp3, wav, m4a, ogg, flac, aac, wma, webm

### Vision Mode

- Select an image and run multimodal Q&A with a vision-capable model.

### Audio Mode

- Generate speech from text with selectable voices and speed.
- Use microphone input to transcribe voice prompts.

### Podcast Studio

- Generate a two-speaker scripted discussion from ingested docs.
- Produces per-line audio segments and a combined episode track.

### Mindmaps

- Generate concept trees from model knowledge or document-grounded context.
- Saved mindmaps are attached to workspace sessions.

### Model Controls

- Install/uninstall models inside the app.
- Configure runtime parameters (context size, max tokens, temperature, top-p, top-k, repeat penalty, and backend-specific controls).
- Built-in compatibility estimator helps prevent downloading models that are likely too heavy for the current machine.

## Models And Storage

By default, models are resolved from the repository-level models directory during development.

You can override model location with an environment variable before launching the app:

Linux/macOS:

	export GENHAT_MODEL_PATH=/absolute/path/to/models

Windows PowerShell:

	$env:GENHAT_MODEL_PATH="D:\\path\\to\\models"

## Developer Section

This section focuses on running and compiling source code manually.

### A. Fast Dev Loop

Use Tauri dev mode with live frontend updates:

	cd genhat-desktop
	npm ci
	npx tauri dev

### B. Manual Compilation (No Tauri Dev Runner)

This path compiles frontend and backend explicitly, then runs the compiled desktop binary.

1. Build frontend assets:

	cd genhat-desktop
	npm ci
	npm run build

2. Compile Rust backend manually:

	cd src-tauri
	cargo build

3. Run compiled app from source tree:

	cargo run

Notes:

- Step 1 is required because Tauri loads frontend files from [genhat-desktop/dist](genhat-desktop/dist) in non-dev runs.
- In debug mode, model discovery falls back to repository [models](models) automatically.

### C. Create Installable Bundles

From [genhat-desktop](genhat-desktop):

Linux deb:

	npx tauri build --bundles deb

Windows msi/nsis:

	npx tauri build --bundles msi,nsis

macOS dmg:

	npx tauri build --bundles dmg

Bundle outputs are written under:

- [genhat-desktop/src-tauri/target/release/bundle](genhat-desktop/src-tauri/target/release/bundle)

## Benchmarking

Use the benchmark suite in [benchmark](benchmark) to capture startup, memory, CPU, process-tree stats, per-model load metrics, and plots.

Quick setup:

	python3 -m venv .venv-benchmark
	source .venv-benchmark/bin/activate
	pip install -r benchmark/requirements.txt

Quick run (launch mode):

	python3 benchmark/run_benchmark.py \
	  --repo-root . \
	  --mode launch \
	  --launch-cmd "cd genhat-desktop && npx tauri dev" \
	  --interactive \
	  --shutdown-after-benchmark \
	  --sanitize-launch-env

Full details are in [benchmark/README.md](benchmark/README.md).

## Troubleshooting

- App fails to open on Linux: verify the Linux packages listed in prerequisites are installed.
- Models not visible: check your model directory and GENHAT_MODEL_PATH.
- RAG answers are weak: verify embedding/grader/classifier models are installed in Settings.
- Very large model is slow or unstable: use the compatibility hints and reduce context size/max tokens.

## Additional References

- Main desktop app: [genhat-desktop](genhat-desktop)
- In-app help guide source: [genhat-desktop/src/content/help-guide.md](genhat-desktop/src/content/help-guide.md)
- Model compatibility notes: [genhat-desktop/docs/MODEL_COMPATIBILITY.md](genhat-desktop/docs/MODEL_COMPATIBILITY.md)
- Benchmark docs: [benchmark/README.md](benchmark/README.md)

## Status

This repository is actively evolving. If you are onboarding a new team member, start with this README, then open the in-app tours after launching NELA.
