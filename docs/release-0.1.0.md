# pdf-rust 0.1.0

A fast, small, native PDF reader for Linux and Windows, written in Rust with
**zero third-party crates**. One window, one toolbar, no web view, no telemetry,
and it remembers exactly where you stopped — page, scroll offset, zoom, rotation
and reading mode, per document, across restarts and reboots.

![pdf-rust viewing a 200-page document](https://github.com/ankitkumar131/pdf-rust/raw/v0.1.0/docs/screenshots/light.png)

## Download

Windows and Linux binaries are attached to this release; every file is covered by
[SHA256SUMS.txt](https://github.com/ankitkumar131/pdf-rust/releases/download/v0.1.0/SHA256SUMS.txt).

### Windows 10 / 11 (x64)

| Package | Size | Direct download |
| --- | --- | --- |
| Installer (adds shortcuts, uninstaller, *Open with pdf-rust*) | 2.99 MB | [pdf-rust-0.1.0-setup.exe](https://github.com/ankitkumar131/pdf-rust/releases/download/v0.1.0/pdf-rust-0.1.0-setup.exe) |
| Portable ZIP (unpack and run, no install) | 3.78 MB | [pdf-rust-0.1.0-win64.zip](https://github.com/ankitkumar131/pdf-rust/releases/download/v0.1.0/pdf-rust-0.1.0-win64.zip) |

### Linux (x86-64)

| Package | Distribution | Size | Direct download |
| --- | --- | --- | --- |
| `.deb` | Debian, Ubuntu, Zorin | 2.89 MB | [pdf-rust_0.1.0_amd64.deb](https://github.com/ankitkumar131/pdf-rust/releases/download/v0.1.0/pdf-rust_0.1.0_amd64.deb) |
| AppImage | any distribution, no install | 4.40 MB | [pdf-rust-0.1.0-x86_64.AppImage](https://github.com/ankitkumar131/pdf-rust/releases/download/v0.1.0/pdf-rust-0.1.0-x86_64.AppImage) |
| Portable tarball | any distribution (glibc ≥ 2.31) | 3.71 MB | [pdf-rust-0.1.0-linux-x86_64.tar.gz](https://github.com/ankitkumar131/pdf-rust/releases/download/v0.1.0/pdf-rust-0.1.0-linux-x86_64.tar.gz) |

### Source

| | Size | Direct download |
| --- | --- | --- |
| Source archive (tag `v0.1.0`) | 663 KB | [pdf-rust-0.1.0-src.tar.gz](https://github.com/ankitkumar131/pdf-rust/archive/refs/tags/v0.1.0.tar.gz) |
| Checksums | 649 B | [SHA256SUMS.txt](https://github.com/ankitkumar131/pdf-rust/releases/download/v0.1.0/SHA256SUMS.txt) |

```bash
# verify every download at once
curl -LO https://github.com/ankitkumar131/pdf-rust/releases/download/v0.1.0/SHA256SUMS.txt
sha256sum -c SHA256SUMS.txt
```

All packages bundle PDFium 152.0.7934 — no separate renderer install, no
`LD_LIBRARY_PATH`, no GTK/Qt, no web view.

## Install

**Windows — installer**

Download `pdf-rust-0.1.0-setup.exe` and run it. It installs into
`%ProgramFiles%\pdf-rust`, adds Start Menu (and optionally desktop) shortcuts, an
uninstaller, and an optional *Open PDF files with pdf-rust* component that
registers the app under *Open with* so it can be chosen as the default PDF
application.

**Windows — portable**

```powershell
Expand-Archive pdf-rust-0.1.0-win64.zip -DestinationPath "$env:LOCALAPPDATA\pdf-rust"
& "$env:LOCALAPPDATA\pdf-rust\pdf-rust.exe" document.pdf
```

`pdfium.dll` sits next to the executable; nothing else is needed.

**Linux — Debian / Ubuntu / Zorin**

```bash
sudo dpkg -i pdf-rust_0.1.0_amd64.deb
sudo apt-get -f install          # only if dpkg reports missing dependencies
pdf-rust document.pdf
```

The package installs `/usr/bin/pdf-rust`, ships PDFium in
`/usr/lib/pdf-rust/`, and registers the desktop entry, icon, `application/pdf`
MIME type and AppStream metadata — pdf-rust appears in *Open with* and can be
chosen as the default PDF application.

**Linux — AppImage**

```bash
chmod +x pdf-rust-0.1.0-x86_64.AppImage
./pdf-rust-0.1.0-x86_64.AppImage document.pdf
```

**Linux — portable tarball**

```bash
tar -xzf pdf-rust-0.1.0-linux-x86_64.tar.gz
cd pdf-rust-0.1.0-linux-x86_64
./pdf-rust document.pdf          # runs in place, no root, no LD_LIBRARY_PATH
./install-desktop.sh             # optional: add it to the application menu
```

## Highlights

* **Reading position that survives everything** — page, scroll offset, zoom,
  rotation and reading mode are stored per document (SHA-256 identity, so a
  renamed or moved file keeps its place) in a WAL-mode SQLite database, autosaved
  every 2 s and on exit.
* **Fast** — ~1.6 ms steady-state frame on a 200-page document; first frame in
  ~5 ms once the renderer is mapped; idle at well under 1% CPU.
* **Small** — 1.71 MB stripped executable, 18 MB peak RSS on the benchmark
  document, no runtime besides X11 (or the Windows system DLLs).
* Continuous and single-page reading with smooth scrolling, thumbnails, table of
  contents, full-text search with next/previous, text selection and copy,
  highlights, rotation (page or all pages), links, presentation mode, printing.
* Light and dark themes, keyboard-first, full-screen, drag & drop, tabs for
  multiple documents, encrypted PDFs, graceful errors for corrupt files.
* All processing is local. No analytics, no network requests, and documents are
  never modified — rotated copies and print jobs go to new files.

## Keyboard

| Action | Shortcut |
| --- | --- |
| Open | `Ctrl+O` · Next / previous page `→` / `←` · First / last `Home` / `End` |
| Go to page | `Ctrl+G` · Zoom in/out `Ctrl++` / `Ctrl+-` (or `Ctrl`+wheel) |
| Fit page / width / 100% | `Ctrl+0` / `Ctrl+1` / `Ctrl+2` |
| Rotate / rotate all | `Ctrl+R` / `Ctrl+Shift+R` · Continuous or single page `Ctrl+M` |
| Search | `Ctrl+F`, `F3` / `Shift+F3` · Copy `Ctrl+C` · Highlight `Ctrl+H` |
| Print `Ctrl+P` · Full screen `F11` · Presentation `F5` · Sidebar `F9` | |
| Theme `Ctrl+D` · Close document `Ctrl+W` · Quit `Ctrl+Q` | |

## Requirements

| | |
| --- | --- |
| Linux | x86-64, glibc ≥ 2.31, `libX11.so.6`; `libsqlite3.so.0` optional (session-only without it) |
| Build | Rust 1.75+, no network access needed (no third-party crates) |
| Renderer | PDFium 152.0.7934 — bundled in the packages, BSD-3-Clause |

## Verify it works

Every artifact was smoke-tested before upload. You can repeat it:

```bash
pdf-rust --version
pdf-rust --selftest document.pdf     # headless: timings, memory, page count, text
pdf-rust --screenshot frame.png document.pdf
```

Reference numbers for the 200-page benchmark document, release build, 1180×860:

```
first frame        4.3 ms
document -> pixels 13.5 ms
frame (steady)     1.68 ms
page render        0.1 ms
page cache         6.9 MB
peak memory        17.8 MB
document           200 pages
text selection     ready
```

## Known limitations

* The Windows binaries are built by the release workflow on Windows and the
  installer compiles cleanly, but the interactive parts (window, DPI scaling,
  clipboard, `Ctrl+P` printing, file associations) have not been verified by hand
  on Windows hardware yet — please report anything that misbehaves.
* The UI text has no hinting (slightly softer at 11 px) and no complex-script
  shaping; CJK appears as boxes in the chrome only — document text is rendered by
  PDFium and is unaffected.
* The X11 shell has not been exercised against a real X server in the development
  sandbox (no display); painting and interaction are covered by 146 headless
  tests.

## Documentation

* [README](https://github.com/ankitkumar131/pdf-rust/blob/v0.1.0/README.md) — features, build, usage
* [Architecture](https://github.com/ankitkumar131/pdf-rust/blob/v0.1.0/docs/architecture.md) · [Testing](https://github.com/ankitkumar131/pdf-rust/blob/v0.1.0/docs/testing.md) · [Packaging](https://github.com/ankitkumar131/pdf-rust/blob/v0.1.0/docs/packaging.md)
* [ADRs](https://github.com/ankitkumar131/pdf-rust/tree/v0.1.0/docs/adr) — framework, engine, rendering, persistence, frame budget, packaging
* [CHANGELOG](https://github.com/ankitkumar131/pdf-rust/blob/v0.1.0/CHANGELOG.md) · [Development log](https://github.com/ankitkumar131/pdf-rust/blob/v0.1.0/DEVELOPMENT.md)

## Licence

MIT for the application code. PDFium is BSD-3-Clause; the embedded DejaVu Sans
font is under the Bitstream Vera licence. Third-party notices are in
[LICENSE](https://github.com/ankitkumar131/pdf-rust/blob/v0.1.0/LICENSE).
