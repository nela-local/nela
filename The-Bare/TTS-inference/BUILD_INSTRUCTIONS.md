# TTS Inference Build Instructions

This document provides step-by-step instructions for setting up the environment and building the standalone executable for the TTS inference engine (`aud_test.py`) on Linux, macOS, and Windows.

The resulting executable removes the need for users to have Python installed and allows the application to run the TTS engine via a binary.

## Prerequisites

- **Python 3.10+** installed.
- **Git** (to clone the repo).

## 1. Setup Virtual Environment

Open your terminal or command prompt and navigate to the `The-Bare/TTS-inference` directory.

### Linux / macOS

```bash
cd The-Bare/TTS-inference

# Create virtual environment
python3 -m venv venv

# Activate virtual environment
source venv/bin/activate

# Upgrade base tools
pip install --upgrade pip setuptools wheel
```

### Windows (PowerShell)

```powershell
cd The-Bare\TTS-inference

# Create virtual environment
python -m venv venv

# Activate virtual environment
.\venv\Scripts\Activate.ps1

# Upgrade base tools
pip install --upgrade pip setuptools wheel
```

## 2. Install Dependencies

Install the required packages from `requirements.txt` and install `pyinstaller`.

```bash
pip install -r requirements.txt
pip install pyinstaller
```

> **Important:** Ensure `huggingface_hub==0.36.2` and `transformers==4.57.6` are installed.
> - `transformers>=5.0` has a new backend import system that breaks inside PyInstaller.
> - `huggingface_hub>=0.27` removed `is_offline_mode`, which older `transformers` expects.
> - `huggingface_hub>=1.0` is rejected by `transformers<5` at runtime.
>
> The pinned versions in `requirements.txt` are tested and compatible.

## 3. Prepare Encoder Files

The TTS models require two encoder files (`tokenizer.json` and `conds.pt`) from the Chatterbox encoder. These must be downloaded once and placed alongside your TTS model GGUF files.

### Download Encoder Files

```bash
# Into the GenHat models directory (used at runtime by the Tauri app)
mkdir -p ../../models/tts-chatterbox-q4-k-m
cd ../../models/tts-chatterbox-q4-k-m
wget https://huggingface.co/callgg/chatterbox-encoder/resolve/main/tokenizer.json
wget https://huggingface.co/callgg/chatterbox-encoder/resolve/main/conds.pt
cd -
```

### Required Model Directory Structure

The `models/tts-chatterbox-q4-k-m/` directory must contain:

```
models/tts-chatterbox-q4-k-m/
├── s3gen-bf16.gguf            # S3Gen model (selectable in frontend)
├── t3_cfg-q4_k_m.gguf         # T3 model
├── ve_fp32-f16.gguf            # Voice encoder
├── tokenizer.json              # Chatterbox encoder tokenizer
└── conds.pt                    # Chatterbox encoder conditionals
```

The Rust backend passes `--encoder_dir` pointing to this directory when invoking the TTS binary.

## 4. Build the Executable

We use PyInstaller to build a "One-Directory" (`--onedir`) bundle. The command collects package metadata that PyInstaller doesn't detect automatically.

### Command

```bash
pyinstaller --clean -y --onedir --name tts-inference \
  --collect-data requests \
  --collect-data diffusers \
  --collect-data chichat \
  --collect-data transformers \
  --collect-data huggingface_hub \
  --hidden-import requests \
  --hidden-import diffusers \
  --hidden-import transformers \
  --hidden-import huggingface_hub \
  aud_test.py
```

**Why `--collect-data` is needed:**
- `diffusers` checks for `requests` package metadata at import time.
- `transformers` checks `huggingface_hub` version via `importlib.metadata`.
- Without `--collect-data`, PyInstaller bundles the code but not the `.dist-info` metadata directories, causing `PackageNotFoundError` at runtime.

## 5. Output Location

The build process will create:
- A `build/` directory (temporary intermediate files).
- A `dist/` directory containing the final output.
- A `tts-inference.spec` file.

The runnable executable will be located at:

- **Linux**: `dist/tts-inference/tts-inference`
- **macOS**: `dist/tts-inference/tts-inference`
- **Windows**: `dist/tts-inference/tts-inference.exe`

## 6. Integration with GenHat

To use this built binary in the GenHat desktop app, you must:
1. Move the entire `tts-inference` directory from `dist/` to the appropriate platform folder in `src-tauri/bin/`.
2. **Clean the Tauri target/debug cache** to avoid stale metadata (see Troubleshooting).

### Directory Structure

The destination structure must look like this:

```
genhat-desktop/src-tauri/bin/
├── tts-lin/
│   └── tts-inference/       <-- The folder from dist/
│       ├── tts-inference    <-- The executable
│       └── _internal/       <-- Dependencies
├── tts-mac/
│   └── tts-inference/
│       ├── tts-inference
│       └── ...
└── tts-win/
    └── tts-inference/
        ├── tts-inference.exe
        └── ...
```

### Automation Commands

**Linux:**
```bash
# From The-Bare/TTS-inference

# 1. Deploy to src-tauri/bin
rm -rf ../../genhat-desktop/src-tauri/bin/tts-lin/tts-inference
mkdir -p ../../genhat-desktop/src-tauri/bin/tts-lin
cp -r dist/tts-inference ../../genhat-desktop/src-tauri/bin/tts-lin/

# 2. IMPORTANT: Clean Tauri's cached copy
rm -rf ../../genhat-desktop/src-tauri/target/debug/bin/tts-lin/tts-inference
```

**macOS:**
```bash
# From The-Bare/TTS-inference

# 1. Deploy to src-tauri/bin
rm -rf ../../genhat-desktop/src-tauri/bin/tts-mac/tts-inference
mkdir -p ../../genhat-desktop/src-tauri/bin/tts-mac
cp -r dist/tts-inference ../../genhat-desktop/src-tauri/bin/tts-mac/

# 2. IMPORTANT: Clean Tauri's cached copy
rm -rf ../../genhat-desktop/src-tauri/target/debug/bin/tts-mac/tts-inference
```

**Windows (PowerShell):**
```powershell
# From The-Bare\TTS-inference

# 1. Deploy to src-tauri\bin
if (Test-Path ..\..\genhat-desktop\src-tauri\bin\tts-win\tts-inference) {
    Remove-Item -Recurse -Force ..\..\genhat-desktop\src-tauri\bin\tts-win\tts-inference
}
New-Item -ItemType Directory -Force -Path ..\..\genhat-desktop\src-tauri\bin\tts-win
Copy-Item -Recurse -Path dist\tts-inference -Destination ..\..\genhat-desktop\src-tauri\bin\tts-win\

# 2. IMPORTANT: Clean Tauri's cached copy
if (Test-Path ..\..\genhat-desktop\src-tauri\target\debug\bin\tts-win\tts-inference) {
    Remove-Item -Recurse -Force ..\..\genhat-desktop\src-tauri\target\debug\bin\tts-win\tts-inference
}
```

## 7. Troubleshooting

### Stale `target/debug` Cache (All OSes)

**Symptom:** After rebuilding the TTS binary, you still get errors referencing old package versions (e.g., `huggingface-hub==1.4.1` when you've pinned `0.36.2`).

**Cause:** Tauri copies resource files from `src-tauri/bin/` into `src-tauri/target/debug/bin/` during development builds. When you replace the binary in `src-tauri/bin/`, the old copy in `target/debug/bin/` persists and may contain stale `.dist-info` directories with conflicting version metadata. The stale copy can accumulate multiple `.dist-info` versions (e.g., `huggingface_hub-0.36.0.dist-info`, `huggingface_hub-0.36.2.dist-info`, `huggingface_hub-1.4.1.dist-info`), and `importlib.metadata` may pick the wrong one.

**Fix:** Always delete the cached copy after deploying a new binary:

```bash
# Linux
rm -rf genhat-desktop/src-tauri/target/debug/bin/tts-lin/tts-inference

# macOS
rm -rf genhat-desktop/src-tauri/target/debug/bin/tts-mac/tts-inference

# Windows (PowerShell)
Remove-Item -Recurse -Force genhat-desktop\src-tauri\target\debug\bin\tts-win\tts-inference
```

### `PackageNotFoundError: No package metadata was found for requests`

**Symptom:** The bundled executable crashes at startup with `importlib.metadata.PackageNotFoundError`.

**Cause:** PyInstaller didn't bundle the `.dist-info` metadata for the package. Libraries like `diffusers` and `transformers` use `importlib.metadata` to check dependency versions at import time.

**Fix:** Rebuild with `--collect-data` for the missing package:

```bash
pyinstaller --collect-data requests --collect-data diffusers ...
```

### `ImportError: cannot import name 'is_offline_mode' from 'huggingface_hub'`

**Symptom:** Crash at startup with an import error from `transformers/utils/hub.py`.

**Cause:** `huggingface_hub >= 0.27` removed the `is_offline_mode` function, but `transformers < 4.46` still imports it directly.

**Fix:** Ensure compatible versions:

```bash
pip install "transformers>=4.51,<5" "huggingface_hub>=0.34,<1.0"
```

### `ValueError: Backend should be defined in the BACKENDS_MAPPING`

**Symptom:** Crash referencing `tf` backend in `transformers`.

**Cause:** `transformers >= 5.0` introduced a new lazy backend import system that doesn't work inside PyInstaller's frozen environment.

**Fix:** Downgrade to `transformers < 5`:

```bash
pip install "transformers>=4.51,<5"
```

### `ImportError: huggingface-hub>=0.34.0,<1.0 is required`

**Symptom:** Runtime error saying `huggingface-hub==1.4.1` was found when `<1.0` is required.

**Cause:** Either (a) the venv has the wrong version installed, or (b) stale `.dist-info` in the Tauri `target/debug` cache (see above).

**Fix:**
1. Verify venv: `pip show huggingface-hub` should show `0.36.2`.
2. Clean rebuild: `rm -rf build dist && pyinstaller --clean -y ...`
3. Clean Tauri cache: Delete `target/debug/bin/tts-*/tts-inference/`.

### Tauri Build Error: `glob pattern ... path not found`

**Symptom:** `cargo build` fails with `glob pattern bin/tts-lin/tts-inference/** path not found or didn't match any files`.

**Cause:** The `tauri.conf.json` references TTS binary paths that don't exist on disk. Tauri resource globs fail when they match nothing.

**Fix:**
1. Ensure the TTS binary has been built and copied to the correct `src-tauri/bin/tts-{platform}/tts-inference/` directory.
2. Only include resource globs for platforms you've built. In `tauri.conf.json`:

```json
"resources": [
  "bin/llama-lin/*",
  "bin/tts-lin/tts-inference/*",
  "bin/tts-lin/tts-inference/_internal/**/*"
]
```

3. For the `**` glob to work, use `_internal/**/*` (not just `**`) — Tauri requires the file-level `*` to match actual files.
