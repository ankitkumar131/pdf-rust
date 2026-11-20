# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added — Milestone 1: foundation
- Cargo project with zero third-party dependencies and an optimised release
  profile (fat LTO, `codegen-units = 1`, `panic = "abort"`, stripped symbols).
- Architecture decision records: framework, PDF engine, UI rendering.
- Software rasterizer with anti-aliased rectangles, rounded rectangles, circles,
  lines, gradients, drop shadows and bilinear bitmap blits.
- TrueType font engine (cmap 4/12, simple + composite glyphs, quadratics,
  4× vertical supersampling) with an embedded DejaVu Sans face and glyph cache.
- Immediate-mode widget library: buttons, icon buttons, tabs, list rows,
  text field with selection/clipboard, scrollbar, checkbox, tooltips, spinner.
- 28 vector icons drawn procedurally.
- Light and dark themes with shared layout metrics.
- Dependency-free PNG encoder used for headless frame dumps and tests.

### Added — Milestone 2: PDF engine
- Hand-written PDFium C bindings loaded dynamically (`dlopen`/`LoadLibraryW`),
  with graceful, actionable failure when the library is missing.
- Single-threaded PDF worker: documents, page sizes, intrinsic rotations,
  metadata, page labels, rendering (zoom + rotation + clamps), text extraction
  with per-character boxes, in-page search, outline (bookmarks) and link
  enumeration, and export of rotated copies via `FPDF_SaveAsCopy`.
- Test fixture generator that writes valid PDFs (multi-page, outlines, links,
  rotated pages, RC4-encrypted, truncated/corrupt) with no binary assets.
- Integration tests for opening, rendering, rotation, text, search, outlines,
  links, encryption, corruption, large documents and export.

### Added — Milestone 3: reading UI
- Zoom model (fit-width / fit-page / actual / custom), rotation (page and all
  pages), continuous and single-page modes, keyboard shortcuts.
- X11 shell (Xlib through `dlopen`): window, XImage presentation, keyboard/mouse
  input with auto-repeat handling, clipboard in and out, drag & drop, resize,
  DPI-aware metrics and a frame timer that idles at 60 ms.
- Viewer chrome: toolbar, tab bar, sidebar (thumbnails, outline, search, recent),
  page content with selection/search overlays, status bar, toasts, modal dialogs
  (open, go to page, password, error, about, print).
- Text selection and copy, find-as-you-type search with next/previous, outline
  navigation, page thumbnails, link activation, presentation mode.

### Added — Milestone 4: persistence
- SHA-256 document fingerprints, SQLite schema with forward-only migrations
  (WAL, `synchronous=NORMAL`), session save/restore, recent documents, window
  geometry, batched autosave every 2 s and on close/quit.
- Session-only fallback when sqlite3 cannot be loaded.

### Added — Milestone 5: platforms and printing
- Win32 shell: window class, message loop, DIB presentation, clipboard, file drop,
  DPI awareness and the same interaction model as the X11 shell.
- Printing: Linux exports a temporary PDF to `lp`/`lpr`; Windows uses `PrintDlgW`
  and GDI page blits at `dpi/72`. `Esc` cancels; rotated documents print rotated.
- `--features win32-syntax-check` compiles and type-checks the Win32 shell (and its
  GDI printing path) on Linux.

### Added — packaging
- `scripts/make-icon.py` renders the application icon (PNG + multi-size ICO) with
  plain geometry, no imaging library.
- Debian package builder (`packaging/linux/build-deb.sh`) with desktop entry,
  MIME registration, AppStream metainfo, icon, docs and `postinst`/`postrm` hooks.
- AppImage builder (`packaging/linux/build-appimage.sh`) and `scripts/build-release.sh`.
- Windows packaging: `build.rs` resource embedding (icon, `VERSIONINFO`,
  per-monitor-DPI manifest) via `windres`/`rc.exe`, `packaging/windows/build.ps1`,
  portable ZIP and NSIS installer with optional `.pdf` association.
- `LICENSE` (MIT) and third-party notices.

### Changed — performance
- Steady-state frame on a 200-page document: **17.1 ms → 1.6 ms**
  (page content 8.4 → 0.5 ms, sidebar 6.8 → 0.5 ms).
  - `stroke_round_rect` visits only the outline band instead of the whole box.
  - `Canvas::blit_scaled` copies rows for near-1:1 blits and only resamples on
    real zoom.
  - page and thumbnail shadows are two edge strips instead of blurred rectangles.
  - thumbnails render at the sidebar cell width, so the sidebar blits 1:1.
- `--screenshot <out.png> [--dark] [--size WxH] [--page N]` paints one frame with
  the real paint path and writes it as a PNG, so screenshots and visual checks
  need no display server; `scripts/screenshot.sh` regenerates `docs/screenshots/`.
- `--selftest [doc.pdf]` runs the viewer headlessly and reports first frame,
  document load, steady frame, page render, cache size, peak memory and page count;
  `scripts/bench.sh` wraps it.
- Removed all compiler warnings (including the PDFium C names in the FFI table,
  now annotated instead of suppressed globally).
- The page cache never evicts the page being displayed, even when one bitmap is
  larger than the whole budget; `extended_reading_keeps_the_cache_bounded` and two
  cache unit tests cover the bound.

### Documentation
- `docs/architecture.md`, `docs/testing.md`, `docs/packaging.md`.
- ADR 0004 (persistence and document identity), ADR 0005 (frame budget), ADR 0006
  (distribution and packaging).
