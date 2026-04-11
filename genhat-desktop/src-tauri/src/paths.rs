//! Shared path resolution utilities for bundled binaries and resources.
//!
//! In **development** (`cargo tauri dev`), the current_exe is somewhere inside
//! `src-tauri/target/debug/`, and bundled binaries live in `src-tauri/bin/`.
//! Walking up `ancestors()` and probing `src-tauri/bin`, `bin`, `resources/bin`
//! naturally finds them.
//!
//! In **production** (deb/AppImage/DMG), Tauri v2 installs:
//!   - **Linux deb**: exe → `/usr/bin/<name>`, resources → `/usr/lib/<ProductName>/`
//!   - **Linux AppImage**: exe → `<mount>/usr/bin/<name>`, resources → `<mount>/usr/lib/<ProductName>/`
//!   - **macOS**: exe → `<App>.app/Contents/MacOS/<name>`, resources → `<App>.app/Contents/Resources/`
//!   - **Windows**: exe → `<install>/GenHat.exe`, resources → `<install>/`
//!
//! The ancestor-walk strategy works for dev and macOS/Windows, but on Linux the
//! resource dir is a *sibling* (`../lib/<ProductName>/`) rather than a child of
//! any ancestor.  This module adds the Tauri resource directory to the search.

use std::path::PathBuf;

/// The Tauri product name (must match `productName` in tauri.conf.json).
const PRODUCT_NAME: &str = "GenHat";

/// Collect all candidate directories that might contain bundled files.
///
/// Returns a list of base directories.  The caller should then look for
/// `<base>/<os_folder>/<file_name>` in each.
///
/// Candidates (in order):
///   1. Tauri resource dir (Linux: `<exe_dir>/../lib/<ProductName>/`)
///   2. Tauri macOS resource dir (`<exe_dir>/../Resources/`)
///   3. All ancestors of `current_exe()` ×  `{src-tauri/bin, bin, resources/bin}`
pub fn candidate_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return dirs,
    };

    let exe_dir = match exe_path.parent() {
        Some(d) => d,
        None => return dirs,
    };

    // ── Tauri v2 production resource directories ──

    // Linux deb / AppImage: /usr/bin/<exe> → /usr/lib/<ProductName>/
    // Resources are mapped into `bin/` subdirectory, so add both the
    // resource root AND its `bin/` child.
    if cfg!(target_os = "linux") {
        let lib_dir = exe_dir.join("..").join("lib").join(PRODUCT_NAME);
        if let Ok(canonical) = lib_dir.canonicalize() {
            dirs.push(canonical.join("bin"));
            dirs.push(canonical);
        } else {
            dirs.push(lib_dir.join("bin"));
            dirs.push(lib_dir);
        }
    }

    // macOS .app bundle: Contents/MacOS/<exe> → Contents/Resources/
    if cfg!(target_os = "macos") {
        let resources_dir = exe_dir.join("..").join("Resources");
        if let Ok(canonical) = resources_dir.canonicalize() {
            dirs.push(canonical.join("bin"));
            dirs.push(canonical);
        } else {
            dirs.push(resources_dir.join("bin"));
            dirs.push(resources_dir);
        }
    }

    // Windows NSIS install: exe sits in the install dir, resources right there
    if cfg!(windows) {
        dirs.push(exe_dir.join("bin"));
        dirs.push(exe_dir.to_path_buf());
    }

    // ── Dev / generic ancestor walk ──
    for ancestor in exe_path.ancestors() {
        for sub in &["src-tauri/bin", "bin", "resources/bin"] {
            dirs.push(ancestor.join(sub));
        }
    }

    dirs
}

/// Resolve a bundled binary file given an OS-specific folder name and
/// a list of possible executable names.
///
/// Example: `resolve_bundled_binary("llama-lin", &["llama-server"])`
/// will search all candidate directories for `<dir>/llama-lin/llama-server`.
///
/// Returns the full path to the first match, or an error listing all checked paths.
pub fn resolve_bundled_binary(os_folder: &str, exe_names: &[&str]) -> Result<PathBuf, String> {
    let dirs = candidate_bin_dirs();
    let mut checked = Vec::new();

    for dir in &dirs {
        for &name in exe_names {
            let candidate = dir.join(os_folder).join(name);
            if candidate.exists() {
                return Ok(candidate);
            }
            checked.push(candidate);
        }
    }

    Err(format!(
        "Bundled binary not found ({os_folder}/{exe_names:?}). Checked:\n{}",
        checked
            .iter()
            .map(|p| format!("  {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

/// Resolve a bundled shared library file.
///
/// Example: `resolve_bundled_library("pdfium-lin", "libpdfium.so")`
pub fn resolve_bundled_library(os_folder: &str, lib_name: &str) -> Result<PathBuf, String> {
    resolve_bundled_binary(os_folder, &[lib_name])
}

/// Resolve the "models" directory.
///
/// Search order:
///   1. `GENHAT_MODEL_PATH` environment variable
///   2. (debug only) Dev workspace `../../models` relative to CARGO_MANIFEST_DIR
///   3. `models/` next to the running executable
///   4. Tauri resource dir + `models/` (Linux: `/usr/lib/GenHat/models/`)
///   5. Fallback: return the Tauri resource path even if empty
pub fn resolve_models_dir() -> PathBuf {
    // 1. Explicit override
    if let Ok(val) = std::env::var("GENHAT_MODEL_PATH") {
        let p = PathBuf::from(val);
        if p.is_file() {
            if let Some(parent) = p.parent() {
                return parent.to_path_buf();
            }
        } else if p.is_dir() {
            return p;
        }
    }

    // 1b. Local Windows workspace fallback for shared models.
    // This keeps category folders like `grader/` rooted at D:\nela\models\...
    if cfg!(windows) {
        let local_models = PathBuf::from(r"D:\nela\models");
        if local_models.is_dir() {
            return local_models;
        }
    }

    // 2. Dev fallback — only compiled into debug builds.
    //    Checked early so it wins over the empty placeholder dirs
    //    that Tauri copies next to the debug binary.
    #[cfg(debug_assertions)]
    {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dev_models = manifest_dir.join("../../models");
        if let Ok(canonical) = dev_models.canonicalize() {
            if canonical.is_dir() {
                return canonical;
            }
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            // 3. Sibling of the executable
            let candidate = exe_dir.join("models");
            if candidate.is_dir() {
                return candidate;
            }

            // 4. Tauri resource dir (Linux: ../lib/GenHat/models/)
            if cfg!(target_os = "linux") {
                let candidate = exe_dir
                    .join("..")
                    .join("lib")
                    .join(PRODUCT_NAME)
                    .join("models");
                if let Ok(canonical) = candidate.canonicalize() {
                    if canonical.is_dir() {
                        return canonical;
                    }
                }
            }

            // macOS: Contents/Resources/models/
            if cfg!(target_os = "macos") {
                let candidate = exe_dir.join("..").join("Resources").join("models");
                if let Ok(canonical) = candidate.canonicalize() {
                    if canonical.is_dir() {
                        return canonical;
                    }
                }
            }
        }
    }

    // 5. No models directory found — return the Tauri resource path
    //    even if it doesn't exist yet (the user will add models there).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            if cfg!(target_os = "linux") {
                return exe_dir
                    .join("..")
                    .join("lib")
                    .join(PRODUCT_NAME)
                    .join("models");
            }
            if cfg!(target_os = "macos") {
                return exe_dir.join("..").join("Resources").join("models");
            }
            return exe_dir.join("models");
        }
    }

    PathBuf::from("models")
}
