# ADR 0003 — UI rendering, text and input model

- Status: **accepted**
- Date: 2026-11-20

## Context

We chose a custom immediate-mode UI (ADR 0001). That leaves three questions:
how text is rasterized, how input is delivered, and how frames reach the screen.

## Decisions

### 1. Software rasterizer with one shared pixel format

All drawing writes `u32` pixels in `0xAARRGGBB` order:

* **Windows**: a DIB section (`CreateDIBSection`, `BI_RGB`, 32 bpp) is selected
  into a memory DC and blitted with `StretchDIBits`/`BitBlt`.
* **Linux**: `XCreateImage` + `XPutImage` on a 32-bit TrueColor visual; the alpha
  byte is simply ignored padding.

Because both consume the same buffer layout, no per-platform drawing code
exists, and screenshots of test frames match production pixel for pixel.

### 2. Own TrueType rasterizer

`src/ui/font.rs` parses `cmap` (formats 4/12), `glyf`/`loca` (simple and
composite glyphs), `hmtx`, `head`, `hhea` and `maxp`, flattens quadratic
splines, and rasterizes with 4× vertical supersampling plus exact horizontal
coverage accumulation (non-zero winding). Glyphs are cached per
(glyph, size-in-quarter-pixels, bold) with a bounded cache.

This avoids GDI/Xft differences, my-font-your-font inconsistencies and an FFI
surface, and gives identical text metrics for layout on both platforms. The
embedded DejaVu Sans face (Bitstream Vera derived licence, redistributable)
covers Latin/Greek/Cyrillic.

Trade-offs: no hinting (text is slightly softer at 11–13 px than system fonts)
and no complex-script shaping (fine for UI chrome; *document* text is rendered
by PDFium, which does have full shaping).

### 3. Immediate-mode widgets with stable string ids

`Ui::interact(id, rect, enabled)` maintains `hot`/`active`/`focus`/`drag` state
between frames; widgets are plain functions (`button`, `icon_button`, `tab`,
`list_row`, `text_field`, `scrollbar`, `checkbox`). Ids are FNV-1a hashes of
stable labels, so state survives layout changes (e.g. sidebar resize).

### 4. Input and event loop

The shells translate platform events into a `ui::Input` snapshot per frame
(pointer position/buttons/wheel, modifiers, logical `Key` events, typed text,
paste buffer). The application maps keys to `Command`s (see
`docs/architecture.md#keybindings`). Frames are drawn on demand — when input or
worker events arrive — plus a 500 ms tick for the loading spinner and caret
blink, so an idle reader consumes ~0% CPU.

## Consequences

* Instant startup: no GPU context, no shader compilation, no font config scan.
* Predictable behaviour across both targets; visual regressions are catchable in
  unit tests (`tests/ui_frames.rs` renders frames to PNG).
* We accept a slightly softer text look and maintain ~900 lines of rasterizer
  code, backed by standard-vector unit tests (CRC/SHA/PNG/font metrics).
