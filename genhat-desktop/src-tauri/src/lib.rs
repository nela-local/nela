//! GenHat — The Local Intelligence Engine
//!
//! Module structure:
//!   config/     — Configuration loading (models.toml)
//!   registry/   — Model definitions and lookups
//!   backends/   — Model backend implementations (llama-server, etc.)
//!   process/    — Process manager (spawn, health, reap, shutdown)
//!   router/     — Task routing (maps requests to models)
//!   commands/   — Tauri IPC command handlers

pub mod config;
pub mod registry;
pub mod backends;
pub mod process;
pub mod router;
pub mod commands;
pub mod rag;
pub mod tts;
pub mod asr;
