# NELA Repository Instructions

## Required .github Sync Policy

The customization files under `.github/` are part of the repository contract.

- Update `.github/copilot-instructions.md`, `.github/instructions/*.instructions.md`, and `.github/agents/*.agent.md` whenever code, architecture, workflows, or developer commands change.
- Keep `.github/agents/genhat.md` synchronized when architectural or behavior details in that reference become stale.
- If a user asks to revert a change, also revert or delete the matching updates made in these `.github` files in the same revert scope.
- Do not leave stale references to removed files, removed features, or old command paths.

## Repository Snapshot

- `genhat-desktop/` is the main desktop app (React + TypeScript + Vite + Tauri v2).
- `genhat-desktop/src/` contains frontend app logic and components.
- `genhat-desktop/src-tauri/src/` contains Rust backend modules, commands, routing, and RAG pipeline code.
- Text chat supports both document-grounding paths: KB-ingested RAG retrieval and direct file-to-prompt attachments, controlled by a RAG on/off toggle (default off = direct prompting).
- Runtime model parameters panel is hidden by default and opened explicitly by the user.
- Disk-scanned model sync preserves user-applied runtime params (for example `ctx_size`, `max_tokens`, `flash_attn`) instead of resetting them during model-list refreshes.
- `benchmark/` contains runtime benchmark scripts and plotting tools.
- `The-Bare/` contains standalone experiments/prototypes.

## Validation Commands

- Frontend build and lint: `cd genhat-desktop && npm run lint && npm run build`
- Rust compile check: `cd genhat-desktop/src-tauri && cargo check`
- Desktop dev run: `cd genhat-desktop && npx tauri dev`

## Change Hygiene

- Keep changes minimal and scoped to the request.
- Prefer updating existing patterns in nearby code instead of introducing new conventions.
- Verify changed files with targeted checks before finalizing.
