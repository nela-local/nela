---
description: "Use when editing NELA frontend code in genhat-desktop/src. Covers React + TypeScript + Tauri API integration patterns."
name: "Frontend React Tauri"
applyTo:
  - "genhat-desktop/src/**/*.ts"
  - "genhat-desktop/src/**/*.tsx"
  - "genhat-desktop/src/**/*.css"
---

# Frontend Scope

- Keep frontend edits within `genhat-desktop/src/` unless the task explicitly requires cross-layer changes.
- Use existing TypeScript types from local `types.ts` files before introducing new duplicated types.
- Preserve established component structure under `src/components/`, `src/app/`, and `src/hooks/`.

# Integration Rules

- Use Tauri command boundaries via existing API wrappers in `src/api.ts` and adjacent app utilities.
- Keep mode behavior aligned with current app flows (chat, vision, audio, podcast, mindmap, RAG knowledge base).
- In text chat, support both document grounding paths via the RAG toggle: `on` uses ingested knowledge-base retrieval (`query_rag_stream`), `off` uses direct file-to-prompt attachments (`prepare_direct_document_prompt`).
- Keep RAG toggle default to `off` and keep runtime parameter dock hidden by default until explicitly opened.
- Runtime parameter apply actions should resolve to a registered runtime model ID and must fail loudly instead of silently falling back to session-only overrides.
- When command names or payload contracts change, update frontend call sites and matching Rust commands together.

# Verification

- For frontend-impacting changes, run: `cd genhat-desktop && npm run lint && npm run build`.
- If skipping a command due to time or environment limits, state it explicitly in the final response.
