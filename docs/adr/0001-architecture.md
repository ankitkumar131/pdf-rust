# ADR 0001 — Application architecture and framework choice

- Status: **accepted**
- Date: 2026-11-20

## Context

We need a native-feeling PDF reader for Windows (primary) and Linux (secondary)
that starts fast, uses little RAM, restores reading positions reliably and can
be registered as the default PDF handler. The instruction document asks for an
explicit evaluation of `eframe/egui`, Tauri and native alternatives, and for the
smallest practical runtime with no unnecessary webview overhead.

Constraints that ended up driving the decision:

1. Binary size and memory budget: target well under 15 MB of application code
   (excluding the PDF engine) and under ~120 MB RSS for a 20-page document.
2. No network at build time and no supply-chain surface beyond the Rust
   standard library — this repository builds offline and audits trivially.
3. One UI implementation for both platforms, because UI details (zoom anchoring,
   thumbnails, search highlighting) are where PDF readers usually diverge.
4. All non-UI behaviour must be testable headlessly (no display server in CI).

## Options considered

| Option | Runtime | Size | Fit for requirements |
| --- | --- | --- | --- |
| **Tauri 2** (Rust core + system webview) | WebView2 / WebKitGTK | ~30–60 MB + webview RSS (150–300 MB) | Fast iteration, but a webview for a page canvas means double memory for every rendered page and no control over text selection metrics; WebKitGTK is a heavy runtime dependency on Linux. Rejected. |
| **eframe/egui** | wgpu or glow | ~12–20 MB binary, 100–250 MB RSS with a GPU backend | Excellent developer ergonomics and true immediate-mode UI. But it pulls in 250+ crates, a GPU stack (or a software GL fallback), and its own text atlas; binary size and dependency surface are an order of magnitude larger than what we need. Rejected for size/dependency reasons, kept as the reference for UI ergonomics. |
| **Native widgets** (Win32 controls + GTK) | OS toolkits | small | Native feel on Windows, but two completely different UI code bases, inconsistent zoom/search behaviour, and GTK is fragile across distributions. Rejected. |
| **Custom immediate-mode UI on a platform shell** (chosen) | std only | ~1.5–3 MB binary + PDFium shared library | One UI code base, full control over page layout/zoom anchoring/selection, direct 32-bit framebuffer blit to `StretchDIBits` (Windows) and `XPutImage` (Linux), and complete headless testability: frames are plain `Vec<u32>`. |

## Decision

Build a **single binary with three layers**:

```
+------------------------------------------------------------+
| app  (plain Rust state machine, no platform code)           |
|   documents, sessions, commands, keybindings, persistence   |
+------------------------------------------------------------+
| ui   (software rasterizer, font engine, widgets, chrome)    |
|   draws into Bitmap 0xAARRGGBB, never blocks                |
+------------------------------------------------------------+
| platform shell (X11 / Win32)   |   pdf worker thread        |
|   window, input, blit, dialogs |   PDFium, rendering, text  |
+------------------------------------------------------------+
```

* The **UI layer** is pure: `Ui::paint(bmp, &AppState)` — so a test can render
  any frame to a PNG and assert on pixels without a display.
* The **application layer** holds no platform handles and no FFI.
* Only the two platform shells contain `unsafe` OS bindings, and only the PDF
  worker contains PDFium `unsafe`.
* No Cargo workspaces: the crate is small enough that a workspace would only add
  ceremony. The layering is enforced by module boundaries instead (see
  `src/lib.rs`).

### Dependency policy

The crate has **zero third-party dependencies**. Everything is implemented in
the standard library:

| Need | Typical crate | What we do instead |
| --- | --- | --- |
| GUI toolkit | `eframe`, `iced` | own rasterizer (`src/ui/canvas.rs`) |
| Text rendering | `ab_glyph`, `fontdue` | own TrueType rasterizer (`src/ui/font.rs`) |
| PDF | `pdfium-static`, `pdfium-render` | hand-written dynamic bindings (`src/pdf/ffi.rs`) |
| SQLite | `rusqlite` | hand-written dynamic bindings (`src/db/sql.rs`) |
| Hashing | `sha2` | own SHA-256 (`src/util/hash.rs`) |
| PNG | `png` | own encoder (`src/util/png.rs`) |
| CLI parsing | `clap` | 40 lines in `src/main.rs` |

Consequences (+): tiny binary, no supply-chain risk, reproducible offline
builds, and every byte of behaviour is auditable in-tree.
Consequences (−): we maintain the font rasterizer and the SQLite bindings
ourselves, and we must be careful with `unsafe` (mitigated by unit tests plus
the single-threaded PDF worker, see ADR 0002).

## Consequences

* Windows and Linux share 100% of the UI, layout and behaviour code.
* High-DPI scaling, native menus and IME are the responsibility of the shells;
  the shells are intentionally small (<1500 lines each).
* Startup does not link PDFium or SQLite: both are opened on demand, so a
  missing engine produces a recoverable error toast rather than a failed start.
