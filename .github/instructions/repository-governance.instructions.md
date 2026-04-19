---
description: "Use when making any repository change. Enforces synchronization of .github instructions and agents with code updates and reverts."
name: "Repository Governance"
applyTo: "**"
---

# Repository Governance

- Treat `.github/copilot-instructions.md`, `.github/instructions/*.instructions.md`, `.github/agents/*.agent.md`, and `.github/agents/genhat.md` as versioned code artifacts.
- For every meaningful code change, update affected `.github` customization files in the same change so guidance matches the current repository state.
- If a user asks to revert a change, also remove or revert the associated `.github` customization edits in the same revert scope.
- When features, file paths, scripts, or workflows are removed, delete outdated references from these `.github` files.
- When features, file paths, scripts, or workflows are added, document them in these `.github` files where relevant.

# Consistency Checks Before Final Response

- Confirm documented commands still exist in `README.md`, `package.json`, or build manifests.
- Confirm documented paths exist in the current workspace tree.
- Avoid copying stale architecture details from older docs without re-validating against code.
