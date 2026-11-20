# Testing

Everything is verified without a display. The UI is an immediate-mode painter
driven by `ui::Input`, so a test can build a full `App`, feed it keys, clicks and
wheel events, render real frames into a `Bitmap`, and inspect either the pixels or
the app state. There is no mocking layer: the tests run against the same PDFium
build and the same code paths as the release binary.

```
cargo test                 # everything below, no display, no network
cargo test --offline       # same, in a sandbox without crates.io access
```

## Layers

| Layer | Where | Count | What it proves |
| --- | --- | --- | --- |
| Unit tests | inline `#[cfg(test)]` in `src/` | 116 | geometry, hashing, PNG, font metrics, layout, zoom anchoring, cache eviction, session encoding, print scaling |
| CLI tests | `src/main.rs` | 3 | argument parsing, defaults, `--stats`/`--selftest` acceptance |
| Engine tests | `tests/pdf_engine.rs` | 10 | real PDFs through the PDFium worker |
| Viewer tests | `tests/viewer.rs` | 28 | headless `App` behaviour end to end |

`gen_fixtures` is an extra test target containing only ignored tests: it writes
the synthetic documents used for manual profiling (`cargo test --test
gen_fixtures -- --ignored --nocapture`), including the 200-page benchmark file, to
`$TMPDIR/pdf-rust-bench/`. Nothing is written inside the repository.

### Engine tests (`tests/pdf_engine.rs`)

Each test builds its fixture with `tests/common/mod.rs`, which writes byte-exact
PDFs (no binary assets in the repository):

* multi-page documents with page labels and metadata,
* bookmarks/outline, link annotations, rotated pages,
* RC4-encrypted documents with user and owner passwords,
* truncated and corrupt files.

They then drive the real worker: open, page sizes, render at a zoom and rotation,
text extraction with character boxes, search hits, outline and link enumeration,
export of a rotated copy (asserting the original is untouched), password
rejection, corruption errors and a 1000-page document.

### Viewer tests (`tests/viewer.rs`)

A helper (`harness()`) opens a document in an `App` with `--no-db`, ticks until
the first page is rendered, and returns the app. Tests then:

* press keys (`Ctrl+G`, arrows, `Home`/`End`, `Ctrl+0/1/2`, `Ctrl+R`, `F11`,
  `F5`, `Ctrl+F`, `F3`) and assert on page/zoom/rotation/mode state,
* click toolbar buttons and sidebar rows by coordinate and assert the resulting
  state change (the same `Rect`s the painter would hit-test),
* type into the go-to-page field and assert focus handling (this is where the
  focus-leak bug was caught: a dialog field must lose focus when the dialog
  closes or the user presses elsewhere),
* reopen the same file after a simulated restart and check page, scroll offset,
  zoom and rotation are restored (`reading_position_survives_restart`), that a
  moved/renamed file keeps its position (`moving_a_file_keeps_its_reading_position`)
  and that "start from the beginning" in the resume dialog forgets it
  (`resume_prompt_can_start_over`),
* drag a text selection and copy it, checking the clipboard payload,
* run search and step through hits, checking the page changes and highlights are
  laid out in PDF space, and round-trip a highlight through the database
  (`highlight_round_trips_through_the_database`),
* scroll with the wheel and `Page Down` and assert smooth scrolling eases over
  several frames, is monotonic, lands on the target and can be switched off
  (`smooth_scrolling_eases_towards_the_target`),
* open several documents into tabs, switch between them and check that each keeps
  its own page (`tabs_and_single_page_mode`),
* read 30 pages at 150 % zoom with the cache pinned to 8 MB and assert the budget
  holds, old pages are evicted and the visible page is never dropped
  (`extended_reading_keeps_the_cache_bounded`) — the memory-usage check the
  specification asks for,
* paint frames and check pixels (for example that the page content region is not
  uniform, that the selection overlay changes the frame, that the toast appears
  and expires, and that a page scrolled halfway off the viewport leaves the two
  toolbar rows above the reading area untouched — the "page painted over the
  toolbar" regression),
* click the ✕ in the Settings dialog and in the toolbar and assert the theme,
  smooth scrolling and resume policy change *and persist*, that only the clicked
  document closes, that the toolbar ✕ never quits with a document open, that the
  checkbox widget reports clicks rather than values, that clicking a thumbnail
  navigates to its page, and that search finds a match on the last page of a
  twelve page document.

### Unit tests

The bulk are in `app::layout` (page boxes, scroll extents, zoom anchoring around
the pointer, fit modes), `app::docview` (rotation-aware coordinate mapping,
selection ordering, search wrapping), `app::cache` (byte accounting, LRU order,
eviction of everything but the visible page), `ui::font` (advance widths, kerning
of missing glyphs, line breaking, PNG dump), `util::hash` (SHA-256 vectors),
`util::png` (encoder against known bytes, filter and CRC handling) and
`platform::print` (a print job for a missing file fails with an actionable
message instead of spawning a backend).

## Headless performance check

`--selftest` runs the real app against a real document without a window:

```bash
cargo build --release
./target/release/pdf-rust --selftest --no-db "$TMPDIR/pdf-rust-bench/benchmark-200.pdf"
```

It reports:

| Field | Meaning |
| --- | --- |
| first frame | wall time from process start to the first painted frame |
| document → pixels | open + first page render |
| frame (steady) | average of the last frames (chrome + page blits) |
| page render | PDFium render for one page |
| page cache | bytes held by the render cache |
| peak memory | process peak RSS |
| document | page count read from the document |
| text selection | whether text extraction for page 1 succeeded |

Reference numbers on the development machine (release build, 1180×860,
200-page 99 KB synthetic document):

```
first frame        4.3 ms
document -> pixels 13.5 ms
frame (steady)     1.68 ms
page render        0.3 ms
page cache         6.9 MB
peak memory        17.8 MB
document           200 pages
text selection     ready
```

If PDFium cannot be found, `--selftest` exits immediately (status 1) and prints
every path it tried instead of waiting for a render that will never come.
`scripts/bench.sh` regenerates the fixture and runs the selftest three times.
`PDFRUST_STATS=1` (or `--stats`) prints startup time and peak memory to stderr
on exit; useful in a shell loop or under `/usr/bin/time`.

## Screenshots and visual checks

`--screenshot <out.png> [--dark] [--size WxH] [--page N]` opens a document,
waits for the page, thumbnails and prefetches to settle, paints one frame with the
real paint path and writes it as a PNG. It needs no display server, which makes it
usable for documentation and for eyeballing a rendering change:

```bash
cargo test --test gen_fixtures -- --ignored --nocapture
./target/release/pdf-rust --screenshot /tmp/light.png --no-db "$TMPDIR/pdf-rust-bench/benchmark-200.pdf"
./target/release/pdf-rust --screenshot /tmp/dark.png --dark --size 1280x820 --no-db "$TMPDIR/pdf-rust-bench/benchmark-200.pdf"
```

`scripts/screenshot.sh` regenerates `docs/screenshots/` this way. The encoder in
`util::png` stores uncompressed zlib streams (tiny code, valid but large files),
so the script recompresses the output with Python's `zlib` before committing.

## Manual matrix

Some things cannot be automated here because the sandbox has no X server. They
are verified by inspection on a desktop and are listed here so a release check
does not forget them:

| Area | Check |
| --- | --- |
| X11 window | open/maximise/resize, DPI change, dark theme, title and icon |
| Clipboard | copy selection out, paste in the go-to field, paste from another app |
| Drag & drop | drop a PDF on the window; second document opens in a tab |
| Printing (Linux) | `Ctrl+P` sends the document through `lp`/`lpr`, Esc cancels |
| Printing (Windows) | `Ctrl+P` opens `PrintDlgW`, page range, DPI-correct output |
| File association | double-click a `.pdf` in the file manager after installing the package |
| Persistence | kill -9 mid-scroll, restart, verify the page and offset are restored |
| Full screen | F11, presentation mode F5 with arrow keys and Esc |

## Conventions

* Fixtures are generated, never committed: `tests/common/mod.rs` writes documents
  to a temporary directory and returns the path.
* Tests must not depend on wall-clock timing; when a test needs the worker to
  make progress it ticks the app in a bounded loop and asserts on state.
* Every test uses `--no-db` (or a temporary database) so a test run never touches
  the user's real reading positions.
