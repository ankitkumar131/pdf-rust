# Changelog

## [0.2.1] — 2026-11-20

Fixes password-protected documents, which could not be opened at all in 0.2.0.
See the [release notes](docs/release-0.2.1.md).

### Fixed — password-protected documents
- **A protected document now opens once the password is entered.** The prompt
  cleared itself *before* the "password submitted" action was dispatched, so the
  handler found no prompt to read the password from, sent nothing to the engine,
  and the document stayed on the "Password required" placeholder — no matter how
  many times the correct password was typed. The dialog now stays until the action
  is handled, and the action carries the document it belongs to plus the typed
  password, so it cannot silently do nothing.
- The password field **masks what is typed** (one bullet per character through the
  same measurement path as normal text, so the caret, selection and clipping stay
  exact). Previously the password was painted in clear.
- Pressing **Unlock or Enter** with an empty field now says so instead of sending
  an empty password and bouncing off the engine.
- Unlocking shows the loading state in the tab until the engine answers, and a
  successful unlock clears the stale "password required" error, so the page is
  painted immediately instead of the placeholder.
- The prompt is also dismissed when a document opens through any other path, and
  cancelling still closes only that tab.

## [0.2.0] — 2026-11-20

Reliability release: one memory budget for every open document, a hardened PDF
engine boundary, full session recovery, and text that degrades gracefully when a
glyph is missing. See the [release notes](docs/release-0.2.0.md).

### Added — one global memory budget (P0)
- The page cache is no longer per document: `App` owns a single budget
  (default 192 MiB, adjustable 64–2048 MiB in **Settings → Memory**) that every
  open document shares, so ten tabs cost the same as one.
- Under pressure the reader trims background documents to a single page, then
  drops the active document's off-screen pages least-recently-used first, and
  only then the background thumbnails. The page on screen is never evicted.
- `MemoryPressure` (`ok` / `elevated` ≥ 70 % / `critical` ≥ 92 %) drives a dot in
  the status bar, which now also shows `cache used/budget · pages · frame time`.
- **Document information** (Ctrl+I) gained a memory section: used vs budget, a
  usage bar, evictions, peak process memory and a per-document breakdown.
- `trim_to`, `keep_only`, `pages` and an eviction counter make the policy
  testable; `cache.mb` persists in the database.

### Added — session recovery (P1)
- The set of open documents, their tab order and the active tab are stored in a
  new `open_documents` table (schema v2) and restored at startup.
- The sidebar panel, its width and the window rectangle (position and size, on
  both X11 and Win32) come back too; the window reopens where it was left.
- A crash — or a kill — is detected: startup compares a `session.clean` flag, and
  a session that was cut short is restored without asking.
- Documents that have disappeared are dropped from the session list instead of
  producing one error dialog each.

### Added — text rendering and keyboard access (P1)
- Missing glyphs no longer vanish: typographic quotes, dashes and ellipses fall
  back to ASCII, ligatures expand, accented letters fall back to their base
  letter, combining marks are skipped, and anything else draws the font's
  `.notdef` box, so mixed-script text keeps its layout (measured and drawn
  through the same fallback).
- `Tab`/`Shift+Tab` walk every control on screen (including out of a text
  field), the focused control gets a visible ring, and `Enter`/`Space` press it;
  `Ctrl+Tab` still switches documents.

### Added — security and robustness (`#2`)
- PDFium is initialised once per process and now shut down explicitly on the way
  out (`FPDF_DestroyLibrary`); after shutdown every entry point refuses to hand
  out the API instead of touching a destroyed library.
- Page counts above 200 000 are rejected before the per-page bookkeeping is
  allocated, so a hostile page tree cannot exhaust memory at open time.
- Password handling distinguishes "needs a password" from "wrong password": a
  rejected password re-opens the prompt with the field cleared instead of turning
  into a fatal error dialog, and cancelling closes only that tab.
- Rotated print copies are written into a private `0700` temporary directory as a
  `0600` file and deleted as soon as the spooler has them — no PDF fragments left
  in `/tmp`, no predictable names to guess or pre-create.
- External links stay restricted to `http`/`https`/`mailto`; embedded JavaScript
  and launch actions are never executed because the reader never asks PDFium to
  run them.

## [0.1.0] — 2026-11-20

First tagged release: Linux `.deb`, portable tarball and source; Windows build
scripts (installer + portable ZIP) are in the tree. Highlights: reading-position
persistence with SHA-256 document identity, continuous/single-page reading with
smooth scrolling, thumbnails/outline/search/recent, text selection and copy,
highlights, printing, presentation mode, and a steady-state frame of ~1.6 ms on a
200-page document. See the [release notes](docs/release-0.1.0.md).

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed — reported defects (Windows and UI)
- Pages no longer paint outside the reading area while scrolling: every panel is
  clipped to its own rectangle, so a page can no longer appear over the toolbar,
  the tab bar or the sidebar (and the toolbar buttons stay visible).
- `pdf-rust.exe` no longer opens a console window: the binary is built for the
  Windows GUI subsystem and re-attaches the parent console only when it is
  launched from a terminal, so `--help`, `--selftest` and warnings still print.
  Fatal startup errors that would otherwise be invisible show a message box.
- The application icon is embedded by default on Windows: `build.rs` now finds
  `rc.exe`/`cvtres.exe` in the standard Windows SDK and MSVC locations instead of
  requiring a "Developer Command Prompt", and the window installs the embedded
  icon for the title bar, taskbar and Alt-Tab.
- The dark theme now darkens the window frame too (`DWMWA_USE_IMMERSIVE_DARK_MODE`,
  caption and border colours where Windows supports them) and the class
  background brush follows the theme, so resizing never flashes white.
- The settings dialog works: the checkbox widget reported its *value* instead of
  a click, so every checked option re-fired its action on every frame, unchecking
  was impossible and the dark theme toggled itself. The same bug disabled
  "Match case" in the search panel and "Remember my choice" in the resume dialog.
- Search covers the whole document: it starts at the visible page and then scans
  every other page (wrapping at the end) while accumulating matches, instead of
  stopping at the first page that matched. The sidebar shows `n/total pages`
  progress.
- Closing a document no longer disturbs the others: the toolbar's close button
  closes the open document (and only quits when none is open), the tab close
  button keeps the same document selected when a tab to its left goes away, and
  `Ctrl+W` still closes just the active document.
- Thumbnails follow the reader: pages visible in the thumbnail strip are rendered
  first (previously only pages near the reading position were requested, so
  scrolling the strip left spinners), and rotating a document re-renders them in
  the new orientation.
- Wheel messages are translated from screen to client coordinates before hit
  testing, and `WM_DPICHANGED` applies the suggested window rectangle instead of
  leaving a mis-sized window after a monitor change.
- Documents are stored under their ordinary absolute path: `std::fs::canonicalize`
  returns a verbatim path on Windows (`\\?\C:\...`), which is now stripped
  before the path is written to the database or shown in the recent list. This was
  the one Windows test failure in CI (`reading_position_survives_restart`), and it
  also made the recent-documents list unreadable there.

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
- Smooth scrolling: wheel and keyboard scrolling eases towards the target over a
  few frames (60 ms time constant) instead of jumping, with a real toggle in the
  Settings dialog. Page jumps, zoom and outline clicks cancel an animation in
  flight.
- Removed the unused `confirm_close` preference (it had no effect).
- Smooth scrolling: wheel and `Page Up`/`Page Down` motion eases towards the target
  over a few frames (60 ms time constant) instead of jumping, and the setting is a
  real toggle in the Settings dialog. Page jumps, zoom and outline clicks cancel an
  animation in flight.
- Removed the unused `confirm_close` preference (it had no effect).

### Documentation
- `docs/architecture.md`, `docs/testing.md`, `docs/packaging.md`.
- ADR 0004 (persistence and document identity), ADR 0005 (frame budget), ADR 0006
  (distribution and packaging).
