# Nela Help Guide

Welcome to Nela. This guide explains what each major part of the app does so you can work confidently even if you are new to AI tools.

## 1. Quick Start

1. Create or continue a workspace.
2. Pick a mode (Chat, Vision, Audio, Podcast, Mindmap).
3. Choose the model for that mode.
4. Ask your question or run your task.

## 2. Workspaces

Workspaces keep your chats, documents, generated outputs, and model preferences organized by project.

- Use **New Project** to start fresh.
- Use **Import Project** to open a saved `.nela` file.
- Use **Export Project** to save your current workspace.

## 3. Modes

### Chat
Use for normal text conversations, Q&A, reasoning, summarization, and document-grounded responses.

### Vision
Use when you want to ask questions about an image.

### Audio
Use text-to-speech and speech-related workflows.

### Podcast
Generate a two-speaker podcast script and audio from your ingested documents.

### Mindmap
Generate visual concept trees from either your documents or model knowledge.

## 4. Models

Nela supports different model types for different tasks.

- **LLM models**: text generation and conversation.
- **Vision models (VLM)**: image + text understanding.
- **TTS/STT models**: speech generation/transcription.

You can install models from Hugging Face and switch models from the top bar selectors.

## 5. Advanced Model Classes (Settings)

Inside **Settings → Advanced Models**, you may see optional classes:

- **Embedding models**: convert text into vectors for semantic search.
- **Grader models**: rerank retrieved chunks so better evidence is used.
- **Classifier / Router models**: classify intent and route tasks to the right model path.
- **Other advanced models**: specialized task models used in specific pipelines.

These are not always required, but they can improve retrieval quality and workflow accuracy.

## 6. Runtime Parameters (What They Mean)

Use the **Model Parameters** panel to tune generation.

- **Context Size**: how much prior text the model can remember.
- **Max Output Tokens**: max response length.
- **Temperature**: creativity/randomness.
- **Top P / Top K**: token sampling controls.
- **Repeat Penalty**: reduces repetitive loops.

Tip: Use the small **?** next to each parameter for a plain-language explanation.

## 7. Documents (RAG)

RAG means the model answers using your ingested files.

- Add files/folders in Chat mode.
- Nela indexes them for retrieval.
- Answers can cite relevant chunks from your docs.

## 8. Podcast Studio

To generate a podcast:

1. Ingest documents first.
2. Open Podcast mode.
3. Set speaker names/voices and turns.
4. Enter topic/query.
5. Click **Generate Podcast**.

You can play the full output or individual lines.

## 9. Mindmaps

Mindmaps summarize ideas as a tree structure.

- Great for studying, planning, and brainstorming.
- Reopen saved mindmaps from the sidebar.

## 10. Help Options

- **Tours**: guided step-by-step walkthroughs.
- **Help Guide** (this document): quick reference whenever needed.

If you are ever unsure, start with a tour, then use this guide as your lookup reference.
