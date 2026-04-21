---
description: "Use when editing Rust backend code in genhat-desktop/src-tauri/src, including commands, routing, model process management, and RAG pipeline modules."
name: "Rust Backend Tauri"
applyTo: "genhat-desktop/src-tauri/src/**/*.rs"
---

# Backend Scope

- Keep backend changes aligned with module boundaries in `commands/`, `router/`, `process/`, `backends/`, and `rag/`.
- Prefer existing task routing and model lifecycle patterns over introducing parallel control paths.
- Maintain compatibility with Tauri command interfaces consumed by the frontend.

# Reliability Rules

- Do not silently change model IDs, task names, or IPC command names without updating all call sites.
- Keep error messages actionable and include enough context for frontend handling.
- Preserve local-first behavior and avoid introducing mandatory cloud dependencies for core flows.
- When reconciling discovered/dynamic models, preserve user-updated runtime params unless the underlying model artifact or task capabilities actually changed.

# Verification

- For Rust backend changes, run: `cd genhat-desktop/src-tauri && cargo check`.
- When backend changes affect frontend behavior, also run frontend lint/build checks.
