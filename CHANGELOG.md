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

### Added — Milestone 3: reading UI (in progress)
- Zoom model (fit-width / fit-page / actual / custom), rotation (page and all
  pages), continuous and single-page modes, keyboard shortcuts.

### Added — Milestone 4: persistence (in progress)
- SHA-256 document fingerprints, SQLite schema with migrations, session restore.
