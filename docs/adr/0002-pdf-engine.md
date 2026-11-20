# ADR 0002 — PDF rendering engine

- Status: **accepted**
- Date: 2026-11-20

## Context

The engine must render pages accurately (fonts, transparency, colour spaces,
form XObjects), extract text with positions, expose bookmarks and links, and
work on 64-bit Windows and Linux without bloating our binary.

Candidates:

| Engine | Licence | Notes |
| --- | --- | --- |
| **PDFium** | BSD-3-Clause | Chromium's engine: best-in-class rendering fidelity and text extraction, small C API, prebuilt shared libraries exist for Windows/Linux. No dependency on system font stacks. |
| MuPDF | AGPL-3.0 (or commercial) | Excellent renderer, but AGPL is incompatible with shipping a freely licensed binary. |
| Poppler | GPL-2.0 | Same licensing problem; also a large C++ dependency chain and per-distro ABI drift. |
| pdf.js | Apache-2.0 | Needs a JS runtime; unusable from Rust without a JS engine dependency. |
| Pure-Rust (pdf-rs, lopdf, pdfium-render) | MIT | No complete, high-fidelity rasterizer; text layout and CJK fonts would be a multi-year project. |

## Decision

Use **PDFium**, loaded dynamically at runtime:

* `src/pdf/ffi.rs` declares the ~70 C functions we need with `extern "C"`
  signatures copied from `public/fpdf_*.h`.
* The library is opened with `dlopen` (Linux) / `LoadLibraryW` (Windows) from
  (in order) `PDFRUST_PDFIUM`, the executable's directory, `./runtime/pdfium/`,
  `./lib/`, `/usr/lib`, `/usr/local/lib`, `Program Files\pdf-rust`.
* No link-time dependency: the binary starts even if the library is missing and
  reports a clear, actionable error in the UI.
* `scripts/fetch-pdfium.sh` downloads a pinned, checksum-verified build from
  `bblanchon/pdfium-binaries` for packaging.

### Threading model

PDFium is **not thread-safe**, so every call happens on one worker thread
(`src/pdf/engine.rs`). UI → worker communication is a `std::sync::mpsc` command
channel; results come back as events carrying `Arc<Bitmap>`/`Arc<PageText>`.
The worker additionally takes a process-wide `Mutex` around each command
(`ffi::CALLS`) so that extra workers (tests, future multi-window support) cannot
race.

Benefits: the UI never blocks, no locks around document state, stale render
results are simply dropped by the UI (natural cancellation), and memory is
bounded in one place (LRU page cache + text cache).

### Rendering contract

* Pages are rendered into a `Bitmap` (BGRA, `0xAARRGGBB` in memory) sized
  `page_points × zoom`, swapped for 90°/270°.
* Dimensions are clamped (≤ 8192 px/side, ≤ 40 Mpx) so pathological zoom levels
  or malformed `/MediaBox` entries cannot exhaust memory.
* Flags: `FPDF_ANNOT | FPDF_LCD_TEXT` — existing annotations are honoured, text
  stays crisp with subpixel rendering.
* The alpha channel is forced opaque after rendering (`FPDFBitmap_BGRA` leaves
  it undefined in places).

## Consequences

* The application's download size is dominated by the engine (~7.6 MB
  compressed for Linux x64), not by our code (~2 MB).
* Shipping is a two-artifact story (`pdf-rust` + `libpdfium.so`/`pdfium.dll`),
  which the packaging scripts and installers handle.
* If a future packaging policy requires a single binary, the bindings can be
  compiled in by linking the static library without touching the call sites.
