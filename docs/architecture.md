# Architecture

pdf-rust is a single-process, single-window viewer. It owns a pixel buffer,
paints everything into it with its own rasterizer, and hands the finished frame
to the platform. There is no GUI toolkit, no web view, and no third-party crate
(the only shared libraries are PDFium, X11/sqlite3, and the Windows system DLLs).

This document describes the code as it is; the reasoning behind the choices is
recorded in [`adr/`](adr).

```
                    ┌──────────────────────────── main thread ────────────────────────────┐
  platform shell    │                                                                     │
  (x11.rs /         │   main.rs          app::App                 ui::chrome              │
   win32.rs)        │   ─────────        ──────────               ───────────             │
  ───────────────── │   CLI, options ──► state machine ────────► paint(bmp, ..)           │
  event loop  ─────►│   platform loop    + actions + dialogs     toolbar/sidebar/content  │
  key/mouse/resize  │        ▲                 │                        │                │
  paint request     │        └─────────────────┴─────── ui::Canvas (0xAARRGGBB) ◄──────────┘
        │           │                                  │                                      │
        ▼           │                                  ▼                                      │
  blit to window ◄──┴────────────────────────── platform::Window (XPutImage / StretchDIBits)   │
                    └──────────────────────────────────┬──────────────────────────────────────┘
                                                       │ mpsc command / event
                                                       ▼
                                     ┌──────────── pdf::Engine worker thread ────────────┐
                                     │  PdfiumApi (dlopen'd C entry points) → FPDF_*      │
                                     │  documents, page sizes, bitmaps, text, outline      │
                                     └────────────────────────────────────────────────────┘
                                                       │
                                            db::Store (SQLite, metadata only)
```

## Modules

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | CLI parsing (`--db`, `--no-db`, `--page`, `--stats`, `--selftest`), process setup, `--selftest` harness |
| `src/lib.rs` | Crate root: `app, db, doc, geom, pdf, platform, ui, util` |
| `src/geom.rs` | `Rect`, `Size`, `Point` and the small amount of layout math shared by UI and app |
| `src/app/mod.rs` | `App`: settings, documents, sidebar state, dialogs, toasts, actions, print state, frame loop, `--stats` counters |
| `src/app/docview.rs` | `DocView`: one open document — session (page, scroll, zoom, rotation), page geometry, selection, search, thumbnails, rotation-aware coordinate mapping |
| `src/app/layout.rs` | Page boxes and scroll extents for continuous/single-page layouts, zoom anchoring |
| `src/app/cache.rs` | Byte-bounded LRU of rendered page bitmaps, with `trim_to`/`keep_only` for the global budget |
| `src/app/commands.rs` | Keyboard command table → `Action` |
| `src/app/dialogs.rs` | Modal dialogs (open file, go to page, password, error, about, print) |
| `src/ui/canvas.rs` | Software rasterizer: fills, rounded rects, strokes, gradients, shadows, `blit_scaled` |
| `src/ui/chrome.rs` | Paints the toolbar, tab bar, sidebar panels, page content, overlays and status bar |
| `src/ui/widgets.rs` | Immediate-mode widgets (buttons, rows, tabs, text field, scrollbar, checkbox…) |
| `src/ui/font.rs` | TrueType parser + rasterizer, glyph cache, text measurement and drawing |
| `src/ui/icons.rs` | Vector icons drawn with the rasterizer |
| `src/ui/theme.rs` | Light/dark palettes and layout metrics |
| `src/pdf/engine.rs` | Worker thread, document handles, render/text/search/outline commands, `RenderKey` |
| `src/pdf/ffi.rs` | Hand-declared PDFium C ABI and the `dlopen` symbol table |
| `src/db/mod.rs`, `src/db/sql.rs` | SQLite store: schema migrations, sessions, recent files, window state; `dlopen`'d sqlite3 |
| `src/doc/identity.rs` | SHA-256 fingerprint + path/size/mtime identity of a document |
| `src/platform/x11.rs` | X11 shell (Xlib via `dlopen`, XImage presentation, clipboard, drag & drop) |
| `src/platform/win32.rs` | Win32 shell (window class, message loop, DIB presentation, clipboard, file drop) |
| `src/platform/win32/printing.rs` | GDI printing: `PrintDlgW`, page render, `StartDoc`/`StartPage` blits |
| `src/platform/print.rs` | Platform-neutral `PrintJob` façade (Linux `lp`/`lpr`, Windows GDI) |
| `src/platform/tempfile.rs` | Private (`0700`) scratch directories that delete themselves when dropped |
| `src/platform/dynload.rs` | `dlopen`/`GetProcAddress` wrapper with the trailing-NUL fixup |
| `src/util/*` | SHA-256, PNG encoder, timing/memory counters |

## Frame lifecycle

1. The platform shell waits for input (X11 `XNextEvent`, Win32 `GetMessage`) with
   a 16 ms timer while animations are running and a 60 ms timer when idle
   (ADR 0003).
2. Input is translated into `ui::Input` (pointer, wheel, keys, text, dropped
   files) and pushed into `App`.
3. `App::frame()` runs the state machine: handles input, applies `Action`s,
   polls the engine, updates animations, layout and toasts, and returns the list
   of `Action`s that the shell must execute (quit, print, clipboard write).
4. `App::paint()` clears the buffer and calls `ui::chrome::paint`, which paints
   the toolbar, tab bar, sidebar, page content, overlays and status bar in that
   order.
5. The shell sends the buffer to the window: `XPutImage` with a shared XImage or
   `StretchDIBits` from a top-down 32-bit DIB. The buffer is `0xAARRGGBB`
   premultiplied-free, which maps exactly onto both back ends.

Nothing paints outside `App::paint`, which makes frames reproducible from a
`ui::Input` sequence — that is how the headless viewer tests work.

Each panel is *clipped* to its own rectangle (`Bitmap::push_clip`): the reading
area, the sidebar body and the dialogs set a clip for the duration of their
painting, and every primitive (`fill_rect`, `blit_scaled`, `blend`,
`fill_rect_gradient`, …) intersects with it. Pages are laid out from the top of
the scrollable document, so a page scrolled halfway off the viewport would
otherwise be blitted over the toolbar and the sidebar — clipping is what keeps
content inside the window box.

### Cost model

Measured on a 1180×860 window with a 200-page document (`--selftest`,
release build, see [`testing.md`](testing.md)):

| Stage | Cost |
| --- | --- |
| steady-state frame (chrome + 2 page blits) | ~1.6 ms |
| page content | ~0.5 ms |
| toolbar | ~0.2 ms |
| status bar | ~0.05 ms |
| page render (PDFium, 536×758) | ~0.1–0.3 ms |
| one glyph line (65 chars) | ~0.05 ms |

Two rules keep the frame cheap:

* **Only the border band of a stroke is visited.** `stroke_round_rect` evaluates
  the signed distance field for the outline ring only; the rectangle fast path
  (radius ≤ 1.5) degenerates to four fills.
* **Blits are row copies whenever possible.** `Canvas::blit_scaled` uses
  `copy_from_slice` per row when the source is opaque and the destination is
  within ±2 px of the source size; it falls back to bilinear sampling only for
  real zooming. Thumbnails are *rendered* at the sidebar cell width
  (`DocView::thumb_scale_milli`), so the sidebar blits 1:1 as well.

Shadows are drawn as two edge strips rather than a blurred translucent
rectangle, which used to cost more than painting the page itself.

## Document state

`DocView` holds everything about one open document:

* `session`: page index, scroll offset, zoom mode and value, rotation, view mode.
* `scroll` / `smooth_target`: the current offset and, while a smooth scroll is in
  flight, where it is heading (`App::tick` eases towards it with a 60 ms time
  constant; jumps and page changes cancel it).
* `info`: page count, page sizes and intrinsic rotations, metadata, page labels.
* `layout`: the page rectangles for the current mode/zoom (recomputed only when
  geometry changes).
* `cache`: byte-bounded LRU of `Arc<Bitmap>` keyed by `RenderKey`.
* `pending`: in-flight renders (at most `MAX_INFLIGHT_RENDERS`), plus a priority
  hint so the visible page is rendered first.
* selection (`Selection`: anchor/focus with per-character boxes), search state
  (`SearchState`: query, hits, current hit), thumbnails and outline.

Coordinates are handled in PDF space (points, origin bottom-left) everywhere and
converted at the edges: `screen_to_pdf` / `pdf_to_screen` in `docview.rs` know
about zoom and rotation, so overlays (selection, links, search highlights) are
stored once and stay correct at any zoom or rotation.

## Memory management

One budget, every document. `App::cache_budget` (default 192 MiB, settable from
64 MiB to 2 GiB, persisted as `cache.mb`) is the *only* page-cache limit;
`DocView::cache` gets the whole budget so a single document can use all of it
when nothing else is open, and `App::enforce_cache_budget` (called from `tick`)
keeps the **sum** inside it:

```
total = Σ doc.cache.memory_bytes()
if total > budget:
    for each background doc: cache.trim_to(INACTIVE_DOC_CACHE_BYTES, [its page])
    if still over: active cache.trim_to(budget - others, visible pages + current)
    if still over: drop background thumbnails
```

`trim_to` evicts least-recently-used pages first and refuses to drop a protected
page (one that is on screen) or the newest entry, so the page the user is looking
at can never disappear — even when a single bitmap is larger than the budget.
`MemoryPressure::from_usage` turns the ratio into `ok`/`elevated`/`critical`,
which is what the status bar dot and the diagnostics panel read; `MemoryStats`
also reports evictions and peak RSS. Per-document accounting is exposed, so the
information dialog can show which tab costs what.

## Threading

All PDFium calls happen on one worker thread; the UI thread only ever talks to
it through `mpsc` channels (`PdfCommand` in, `PdfEvent` out). This is ADR 0002
and it is what makes PDFium safe without a mutex: PDFium is not thread-safe, but
a single thread with a message queue is.

```
UI: engine.send(Render { id, key })        worker: loop { recv() → render → send(Event) }
UI: engine.pump() → Vec<Event>             events: Opened, Rendered, Text, Search, Outline, …
```

Results are handed back as `Arc<Bitmap>` / `Arc<PageText>` so the UI can put them
in the cache without copying. Page bitmaps are clamped to 8192 px per side and
40 MP total; oversized requests are scaled down and the UI is told the effective
ratio.

## Persistence

`db::Store` is metadata-only SQLite ([ADR 0004](adr/0004-persistence.md)):

* A document is identified by a SHA-256 fingerprint of its bytes
  (`doc/identity.rs`), with path/size/mtime kept for fast lookup and recent lists.
* Tables: `documents`, `sessions`, `highlights`, `settings`, `open_documents`
  (schema v2: the tabs that were open, their order and which one was active);
  migrations run forward-only at open.
* WAL mode with `synchronous=NORMAL`; writes are batched and flushed at most
  every 2 s and on `save_all()` (quit, close, crash-safe panic handler).
* If sqlite3 cannot be loaded the app runs session-only and says so once.
* Session recovery is crash-safe in both directions: the list of open documents is
  rewritten (in one transaction) as tabs open, close and change, and a
  `session.clean` flag is set to `0` at startup and back to `1` by `save_all()`. A
  start that finds `0` knows the previous run was cut short, restores the tabs
  without asking, and skips documents that no longer exist. The window rectangle
  is written by the shells on `ConfigureNotify`/`WM_CLOSE` and reopened at the
  same position and size.

## Platform abstraction

`platform/mod.rs` defines the small surface both shells implement: create a
window, run the event loop, present a bitmap, set the title, set the cursor,
write the clipboard, and report display metrics. `platform::print::PrintJob` is
the same idea for printing: the Linux implementation exports a temporary PDF and
hands it to `lp`/`lpr`; the Windows implementation renders through GDI at
`dpi/72` with `PrintDlgW`.

The Win32 shell is compiled on Linux for type checking with
`--features win32-syntax-check`, which swaps the printing implementation so the
GDI path is type checked too.

## Trust boundary

The engine boundary is treated as hostile input:

* `ffi::global()` initialises PDFium exactly once per process, serialises every
  call through a process-wide mutex, and refuses to hand out the API after
  `ffi::shutdown()` (called once on the exit path) — so a destroyed library is
  never touched.
* Handles are null-checked at every step; page counts are capped
  (`MAX_PAGES = 200 000`) before per-page bookkeeping is allocated; bitmaps are
  clamped to 8192 px per side and 40 MP total.
* Open errors are classified (`OpenFailure::PasswordRequired/WrongPassword/Other`)
  instead of guessed from text, which is what lets a wrong password re-prompt
  rather than fail the document.
* Print scratch files live in a `PrivateTempDir` (`0700`, unpredictable name, file
  restricted to `0600`) that removes itself when the job finishes.
* External links are limited to `http`/`https`/`mailto`; JavaScript and launch
  actions are never executed because the engine is never asked to run them.

## Invariants

* The UI thread never calls PDFium, and never blocks on the worker.
* Every pixel the user sees came from `App::paint` in this frame; there is no
  retained widget tree.
* A document's on-disk bytes are never written. Rotated exports and print jobs
  go to new files.
* Actions that can be triggered from more than one place (toolbar, key, menu,
  shell) are expressed once as `Action` and dispatched in one place.
* Widgets report *interactions*, never state: `checkbox` returns `true` on the
  frame it is clicked, and the caller owns the value. A widget that echoed its
  value looked like a click to every `if widget(..) { action }` call site and
  made the settings dialog re-fire its actions every frame.
* A panel clip is always narrower than or equal to the surface: painting is
  bounded by `Regions`, and the shells blit at most the client area.
* The sum of every document's page cache stays within `App::cache_budget`, and the
  visible page is never evicted to make room.
* Text is measured through the same fallback path it is drawn with, so a missing
  glyph can never shift the layout of the rest of the line.
