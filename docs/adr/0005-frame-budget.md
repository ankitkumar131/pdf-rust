# ADR 0005 — Frame budget and blitting strategy

- Status: **accepted**
- Date: 2026-11-20

## Context

A viewer that repaints everything itself must keep a full repaint well under one
frame (16.7 ms at 60 Hz) with room for the platform blit. Profiling a 200-page
document at 1180×860 initially showed ~17 ms steady-state frames - the app could
not hold 60 Hz while scrolling, and idle repaints burned CPU.

## Measurements (release build, 200-page document)

| Stage | Before | After |
| --- | --- | --- |
| page content (2 pages) | 8.4 ms | 0.5 ms |
| sidebar (thumbnails) | 6.8 ms | 0.5 ms |
| toolbar | 0.26 ms | 0.20 ms |
| status bar | 0.05 ms | 0.05 ms |
| **full frame** | **17.1 ms** | **1.6 ms** |

## Decisions

### 1. Strokes visit only the outline band

`stroke_round_rect` used to evaluate two signed distance fields for every pixel
of the bounding box - about a megabyte of work per page border. It now walks only
the border ring, and rectangles (radius ≤ 1.5) become four thin fills.

### 2. Blits copy rows, not pixels

`Canvas::blit_scaled` keeps the general bilinear path for real zooming but adds a
nearest-neighbour row path: when source and destination differ by at most ±2 px
per axis it copies whole rows with `copy_from_slice` if the source is opaque and
blends per pixel otherwise. A 1:1 blit of a 536×758 page dropped from ~0.55 ms to
~0.28 ms, and the common "zoom changed by a hair" case no longer pays for bilinear
sampling (~11 ms per page, still used for genuine rescaling).

### 3. Shadows are edge strips

A full translucent rectangle over a page cost as much as painting the page. Page
and thumbnail shadows are now two short fills along the right and bottom edges.

### 4. Thumbnails are rendered at display size

`DocView::thumb_scale_milli` is derived from the sidebar cell width, so the
sidebar blits thumbnails 1:1 instead of resampling them every frame. Resizing the
sidebar re-renders the visible thumbnails once.

### 5. Idle frames are rare

The shells request a frame on input and while an animation (scrolling inertia,
toast, spinner, page transition) is active; otherwise the timer ticks at 60 ms and
only checks for engine events. Combined with the numbers above this keeps an idle
reader at well under 1 % CPU.

## Consequences

* Frame cost is now dominated by the platform blit, so the UI can repaint
  unconditionally after any input.
* The bilinear path is still the escape hatch for arbitrary zoom; if a future
  profile shows it dominating, the next step is a two-pass resample with a
  precomputed sample table.
* `--selftest` prints the same numbers CI can watch, so regressions are visible
  without a display.
