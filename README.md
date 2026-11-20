# pdf-rust

A fast, lightweight, native PDF reader for **Windows 10/11** and **Linux**,
written in Rust with **zero third-party crates**.

* Opens instantly and stays out of your way: one window, one toolbar, no web
  view, no telemetry.
* **Remembers exactly where you stopped** — page, scroll offset, zoom, rotation
  and reading mode per document, restored after a restart, a crash or a reboot.
* Continuous and single-page reading, thumbnails, table of contents, full-text
  search, text selection, highlights, rotation, printing and link handling.
* Light and dark themes, full-screen and presentation modes, keyboard-first.

```
┌──────────────────────────────────────────────────────────────────┐
│ ☰  Open  ‹ ›  [page] / 20   −100%+  Fit  ⟳  🔍   ☾  ⛶           │
├───────────┬──────────────────────────────────────────────────────┤
│ Thumbs    │                                                      │
│ Contents  │                  rendered PDF page                   │
│ Search    │                                                      │
│ Recent    │                                                      │
├───────────┴──────────────────────────────────────────────────────┤
│ document.pdf · Page 10 / 20 · 120% · continuous          12 MB   │
└──────────────────────────────────────────────────────────────────┘
```

## Status

Milestone 1–4 of the plan in [`DEVELOPMENT.md`](DEVELOPMENT.md) are implemented:
architecture, PDFium rendering, navigation/zoom/rotation, and SQLite-backed
reading-position persistence. See that file for the exact, current state — it is
the single source of truth, and [`CHANGELOG.md`](CHANGELOG.md) tracks releases.

## Requirements

| | Requirement |
| --- | --- |
| Build | Rust 1.75+ (`cargo`), no network needed (no dependencies) |
| Runtime | `libpdfium.so` (Linux) or `pdfium.dll` (Windows) |
| Persistence | `libsqlite3.so.0` / `winsqlite3.dll` (falls back to a session-only store if absent) |
| Linux GUI | X11 (`libX11.so.6`), no GTK/Qt needed |
| Windows GUI | user32/gdi32/comdlg32 (part of every Windows install) |

Fetch the PDF engine (pinned, checksum-verified):

```bash
./scripts/fetch-pdfium.sh            # Linux x64, Windows x64, or both
```

## Build

```bash
cargo build --release                # target/release/pdf-rust
cargo test                           # unit + integration tests
./target/release/pdf-rust document.pdf
```

Release builds use fat LTO, one codegen unit, `panic=abort` and symbol stripping
(see `Cargo.toml`).

## Install

* **Windows**: `packaging/windows/build.ps1` produces an Inno Setup installer
  (`pdf-rust-<version>-setup.exe`) and a portable ZIP. The installer registers
  `pdf-rust` as a PDF handler, adds *Open with*, and ships the icon and version
  metadata.
* **Linux**: `packaging/linux/build.sh` produces a `.deb` and an AppImage. Both
  install a `.desktop` entry, icons, MIME types (`application/pdf`,
  `application/x-pdf`) and register `pdf-rust` as a candidate PDF handler.

## Usage

| Action | Shortcut |
| --- | --- |
| Open document | `Ctrl+O` (or drag & drop, or `pdf-rust file.pdf`) |
| Next / previous page | `→` / `←`, `Page Down` / `Page Up` |
| First / last page | `Home` / `End` |
| Go to page | `Ctrl+G` |
| Zoom in / out | `Ctrl++` / `Ctrl+-` (or `Ctrl`+wheel) |
| Fit page / fit width / 100% | `Ctrl+0` / `Ctrl+1` / `Ctrl+2` |
| Rotate clockwise / counter-clockwise | `Ctrl+R` / `Ctrl+Shift+R` |
| Continuous / single page | `Ctrl+M` |
| Search | `Ctrl+F`, `F3` / `Shift+F3` for next/previous match |
| Copy selection | `Ctrl+C` |
| Highlight selection | `Ctrl+H` |
| Print | `Ctrl+P` |
| Full screen | `F11`, presentation mode `F5` |
| Toggle sidebar | `F9` |
| Toggle theme | `Ctrl+D` |
| Close document | `Ctrl+W` |
| Quit | `Ctrl+Q` |

## Documentation

* [`docs/architecture.md`](docs/architecture.md) — module map, data flow, schema.
* [`docs/adr/`](docs/adr) — architecture decision records (framework, engine, UI,
  persistence).
* [`docs/testing.md`](docs/testing.md) — automated and manual test matrix.
* [`docs/packaging.md`](docs/packaging.md) — installers, file associations, sizes.
* [`DEVELOPMENT.md`](DEVELOPMENT.md) — progress, known issues, next milestones.

## Privacy

Everything runs locally. No analytics, no telemetry, no network requests. The
reader never modifies your documents; rotated copies, print jobs and exports are
written to new files.

## Licence

MIT for the application code. PDFium is BSD-3-Clause; the embedded DejaVu Sans
font is under the Bitstream Vera licence (see `assets/fonts/DejaVuSans.LICENSE`).
