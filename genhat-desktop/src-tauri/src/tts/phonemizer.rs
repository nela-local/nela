//! Phonemization via the bundled `espeak-ng` binary.
//!
//! The binary and its minimal English-only data (~1.3 MB) are shipped inside
//! `bin/espeak-ng-<os>/` alongside the rest of the GenHat pre-built tools.
//! The resolver walks up from `current_exe()` probing several candidate paths
//! (matching the llama-server pattern) and passes `--path=<data_dir>` so
//! espeak-ng finds its phoneme tables without any system-level installation.
//!
//! On Linux the bundled `libespeak-ng.so.1` is found via `LD_LIBRARY_PATH`.
//! On macOS `DYLD_LIBRARY_PATH` is used; on Windows the DLL is a sibling.

use std::path::PathBuf;
use std::process::Command;

// ── Binary resolution ────────────────────────────────────────────────────

/// OS-specific folder name under `bin/`.
fn os_folder() -> &'static str {
    if cfg!(windows) {
        "espeak-ng-win"
    } else if cfg!(target_os = "macos") {
        "espeak-ng-mac"
    } else {
        "espeak-ng-lin"
    }
}

/// Executable file name.
fn exe_name() -> &'static str {
    if cfg!(windows) {
        "espeak-ng.exe"
    } else {
        "espeak-ng"
    }
}

/// Locate the bundled espeak-ng executable + its sibling `espeak-ng-data/` directory.
///
/// Uses the shared `paths::resolve_bundled_binary` helper which checks both
/// dev locations and production Tauri resource directories.
///
/// Returns `(exe_path, data_dir)` on success.
fn resolve_bundled() -> Option<(PathBuf, PathBuf)> {
    let folder = os_folder();
    let name = exe_name();

    let exe = crate::paths::resolve_bundled_binary(folder, &[name]).ok()?;
    let parent = exe.parent()?;
    let data_dir = parent.join("espeak-ng-data");
    if data_dir.is_dir() {
        Some((exe, data_dir))
    } else {
        None
    }
}

// ── Public API ───────────────────────────────────────────────────────────

/// Phonemize English text into IPA using espeak-ng.
///
/// Tries the **bundled** binary first (zero user setup).
/// Falls back to a system-installed `espeak-ng` if the bundle is missing.
///
/// Returns the IPA string with stress markers preserved.
/// Punctuation is preserved in the output.
pub fn phonemize(text: &str) -> Result<String, String> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }

    let mut cmd;

    if let Some((exe, data_dir)) = resolve_bundled() {
        cmd = Command::new(&exe);

        // Tell espeak-ng where its phoneme data lives.
        cmd.arg("--path").arg(&data_dir);

        // Ensure the dynamic library is found next to the binary.
        let lib_dir = exe.parent().unwrap_or(&exe);
        #[cfg(target_os = "linux")]
        {
            cmd.env("LD_LIBRARY_PATH", lib_dir);
        }
        #[cfg(target_os = "macos")]
        {
            cmd.env("DYLD_LIBRARY_PATH", lib_dir);
        }

        log::debug!(
            "Using bundled espeak-ng: {} (data: {})",
            exe.display(),
            data_dir.display()
        );
    } else {
        // Fallback: system-installed espeak-ng
        log::debug!("Bundled espeak-ng not found — falling back to system binary");
        cmd = Command::new("espeak-ng");
    }

    cmd.args(["--ipa", "-v", "en-us", "-q"]);
    cmd.arg(text);

    #[cfg(windows)]
    crate::windows_spawn::hide_console_std(&mut cmd);

    let output = cmd.output().map_err(|e| {
        format!(
            "Failed to run espeak-ng: {e}\n\
             Bundled binary not found and system espeak-ng is not installed.\n\
             Install with: sudo apt install espeak-ng (Linux) / \
             brew install espeak-ng (macOS) / download from GitHub (Windows)"
        )
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "espeak-ng exited with status {}: {stderr}",
            output.status
        ));
    }

    let raw = String::from_utf8_lossy(&output.stdout);

    // espeak-ng outputs one line per input clause; join and trim.
    let ipa = raw
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    Ok(ipa)
}

/// Check if espeak-ng is available (bundled or system).
pub fn is_available() -> bool {
    if resolve_bundled().is_some() {
        // Quick sanity check with the bundled binary.
        phonemize("test").is_ok()
    } else {
        let mut cmd = Command::new("espeak-ng");
        cmd.arg("--version");
        #[cfg(windows)]
        crate::windows_spawn::hide_console_std(&mut cmd);
        cmd.output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phonemize_hello() {
        if !is_available() {
            eprintln!("Skipping test — espeak-ng not installed");
            return;
        }
        let ipa = phonemize("Hello world").unwrap();
        assert!(!ipa.is_empty(), "Should produce non-empty IPA output");
        // The output should contain IPA characters (e.g. ə, ˈ)
        assert!(
            ipa.chars().any(|c| !c.is_ascii()),
            "Should contain IPA characters"
        );
    }

    #[test]
    fn test_empty_input() {
        let ipa = phonemize("").unwrap();
        assert!(ipa.is_empty());
    }

    #[test]
    fn test_resolve_bundled() {
        // This test just checks the resolver doesn't panic.
        let result = resolve_bundled();
        if let Some((exe, data)) = &result {
            assert!(exe.exists(), "Bundled exe should exist: {}", exe.display());
            assert!(data.is_dir(), "Data dir should exist: {}", data.display());
        }
    }
}
