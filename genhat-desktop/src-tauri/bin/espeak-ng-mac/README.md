# macOS espeak-ng bundle

Place the macOS `espeak-ng` runtime files here so the Tauri app can use
the bundled phonemizer. The folder should contain:

- the `espeak-ng` executable (native macOS binary)
- `libespeak-ng.dylib` (or similarly named dylib for the binary)
- the `espeak-ng-data/` directory with language/phoneme files

Helper script:

- `upload_espeak_mac.sh` — copy an existing local build or extracted archive
  into this folder and fix permissions.

Usage:

1. If you already have a directory (for example `/path/to/espeak-mac`):

```bash
./upload_espeak_mac.sh /path/to/espeak-mac
```

2. If you have a `.tar.gz` or `.zip` archive containing the runtime:

```bash
./upload_espeak_mac.sh /path/to/espeak-ng-macos.tar.gz
```

What the script does:

- Locates `espeak-ng`, `libespeak-ng*.dylib` and `espeak-ng-data/` inside
  the provided path (or extracted archive).
- Copies them into this directory (`src-tauri/bin/espeak-ng-mac`).
- Makes the `espeak-ng` binary executable.

Notes:

- The repo does not include prebuilt macOS binaries for licensing/release
  reasons; provide your own build or download an official build and run the
  script above.
- After placing files here, the Tauri build will bundle them so `phonemizer`
  can find the bundled binary at runtime.
