//! Tauri command handlers — the frontend-facing API.
//!
//! All `#[tauri::command]` functions live here, organized by domain.

pub mod models;
pub mod inference;
pub mod audio;
