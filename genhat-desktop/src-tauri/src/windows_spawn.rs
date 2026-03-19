//! Windows-only helpers to spawn child processes without showing a console window.
//!
//! The goal is to prevent "terminal pop-ups" when the parent is a GUI/subsystem
//! application (Tauri), by using Windows process creation flags.

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// `CREATE_NO_WINDOW` (suppresses creation of a console window).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `DETACHED_PROCESS` (lets the child run without being attached to the parent console).
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// Hide console window for `std::process::Command` on Windows.
#[cfg(windows)]
pub fn hide_console_std(cmd: &mut std::process::Command) {
    cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
}

/// Hide console window for `tokio::process::Command` on Windows.
#[cfg(windows)]
pub fn hide_console_tokio(cmd: &mut tokio::process::Command) {
    cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
}

#[cfg(not(windows))]
pub fn hide_console_std(_cmd: &mut std::process::Command) {}

#[cfg(not(windows))]
pub fn hide_console_tokio(_cmd: &mut tokio::process::Command) {}

