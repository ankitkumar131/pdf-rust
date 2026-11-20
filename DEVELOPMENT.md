# Development log

This file is the project's working memory: what is done, what is verified, what
is known to be broken and what comes next. Update it with every milestone.

_Last updated: Milestone 6 (platform shells, printing, packaging, performance)._

## How to reproduce the build in this sandbox

The development sandbox has no access to `static.rust-lang.org` or `crates.io`,
so the toolchain is installed from the npm registry and the project is built
with **zero dependencies** (which is also a product decision, see ADR 0001):

```bash
./scripts/bootstrap-toolchain.sh                  # Rust 1.95 into ~/.local/rust1.95
export PATH="$HOME/.local/rust1.95/bin:$PATH"
./scripts/fetch-pdfium.sh linux-x64               # -> runtime/pdfium/libpdfium.so
cargo test && cargo build --release
```

`runtime/` and `dist/` are git-ignored; nothing binary is ever committed.
There is no display, no `rustfmt` and no `clippy` in the sandbox: formatting is
kept by hand and the code is verified by `cargo test`, `cargo build` (zero
warnings) and `--features win32-syntax-check` for the Windows shell.

## Milestone status

| # | Milestone | State | Notes |
| --- | --- | --- | --- |
| 1 | Project init, architecture, docs | **done** | Cargo project, ADRs, docs, git |
| 2 | PDF engine + opening documents | **done** | PDFium bindings, worker, tests |
| 3 | Navigation, page counter, zoom, rotation | **done** | zoom/rotation model, X11 shell, chrome |
| 4 | SQLite persistence + restore | **done** | schema, migrations, autosave, recent list |
| 5 | Thumbnails, TOC, search, recent | **done** | all sidebar panels, search navigation |
| 6 | UI polish, themes, shortcuts, performance | **done** | 17.1 ms → 1.6 ms frames (ADR 0005) |
| 7 | Windows installer, portable ZIP, associations | **done (unverified on Windows)** | NSIS + ZIP scripts; `build.ps1` now fails the build if the exe is not a GUI-subsystem binary and warns when the icon resource is missing |
| 8 | Linux .deb / AppImage, MIME registration | **done** | `.deb` built and inspected in the sandbox |
| 9 | Cross-platform testing, security review, release | **done (0.2.0)** | see the checklists below |
| 10 | One global memory budget, session recovery, keyboard focus, glyph fallback | **done (0.2.0)** | `App::enforce_cache_budget`, `open_documents` table, `UiState::end_frame`, `Font::drawable` |
| 11 | Password-protected documents fixed, prompt masked | **done (0.2.1)** | painter no longer drops the dialog before the action; `password_field` masks input |
| 12 | Annotations, page management, OCR, reading modes, library | planned | deliberately after the reliability round in 0.2.0 |

## Verified so far

* `cargo test` — 131 unit + 3 CLI + 10 PDF engine + 34 viewer tests (plus the
  fixture generators), all green, run in parallel against real PDFium 152.0.7934.
  Zero compiler warnings.
* `cargo check --features win32-syntax-check` type-checks the Win32 shell and its
  GDI printing path on Linux; a deliberately broken line proves the feature really
  compiles `src/platform/win32.rs`.
* `--selftest` on a 200-page document (release, 1180×860): first frame 7.7 ms,
  steady frame 1.36 ms, peak memory 19.0 MB. See `docs/testing.md` for the field
  descriptions.
* The panel clip, the widget contract, whole-document search, single-document
  close, thumbnail navigation/paging and the Windows shell changes are covered by
  headless tests (`tests/viewer.rs`, `src/ui/canvas.rs`); rendering a dark,
  scrolled frame headlessly confirms the reading area stays inside its box.
* `packaging/linux/build-deb.sh` produces a 3.3 MB `.deb` whose contents were
  checked with `dpkg-deb -I/-c`; `build-appimage.sh` produces a runnable AppDir
  (`dist/pdf-rust.AppDir/AppRun --version`).
* The packaged layout works end to end: the `.deb` was unpacked with
  `dpkg-deb -x` into a scratch root and `usr/bin/pdf-rust --selftest` rendered a
  page with no `LD_LIBRARY_PATH` and no `PDFRUST_PDFIUM`, proving the
  `../lib/pdf-rust/libpdfium.so` search path.
* `--selftest` fails fast with the list of paths it tried when PDFium is missing
  (previously it waited two minutes and then reported only a timeout).
* Release `v0.1.0` is published with **Windows and Linux binaries attached as
  release assets**: `pdf-rust-0.1.0-setup.exe` (NSIS), `pdf-rust-0.1.0-win64.zip`,
  `pdf-rust_0.1.0_amd64.deb`, `pdf-rust-0.1.0-x86_64.AppImage`,
  `pdf-rust-0.1.0-linux-x86_64.tar.gz` and one `SHA256SUMS.txt`. This sandbox
  cannot reach `uploads.github.com`, so the upload is done by
  `.github/workflows/release.yml` on GitHub's runners (Linux job + Windows job +
  a checksums job that collects both). Run
  `gh run list --workflow "Release artifacts"` to see it. Binaries are no longer
  committed to the tree.
* The Windows binaries were produced by that workflow; the interactive parts still
  need a hands-on check on real Windows hardware (see limitations).
* PDF engine: 20-page and 1000-page documents, zoom 2×, 90° rotation, text
  extraction, search, outline, links, password-protected and corrupt files,
  rotated-copy export that provably leaves the original untouched.

## Acceptance criteria

Mapped to the specification in [`instruction.md`](instruction.md) §16, with how
each one is verified today:

| Criterion | Status | Evidence |
| --- | --- | --- |
| PDF opens through the UI | met | `tests/viewer.rs::opens_a_document_and_draws_it`, open dialog test |
| PDF opens by double-clicking in Explorer / file manager | met (Linux `.deb` inspected; Windows installer not run here) | `.desktop` + MIME + `Open with` registration, NSIS association component, per-OS command-line parsing (`document_from_args`) |
| A 20-page PDF opened on page 10 restores page 10 after closing and reopening | met | `reading_position_survives_restart`, `db::reopening_the_same_file_keeps_the_session`, `--page 10` + `session` row |
| Restoration survives a full machine restart | met by construction | session rows are committed to SQLite (WAL, `synchronous=NORMAL`, autosave ≤2 s, `save_all()` on quit); a restart is indistinguishable from the process restart the tests perform |
| Zoom and rotation persist | met | session row stores zoom mode/value, rotation and view mode; covered by `reading_position_survives_restart` and `settings_round_trip_through_the_database` |
| Large PDFs read without excessive memory | met | 1000-page engine test; bounded cache (192 MB, never evicts the visible page) + 8192 px/40 MP clamps; `extended_reading_keeps_the_cache_bounded`; `--selftest` reports 17.8 MB peak on the 200-page benchmark |
| Multiple PDFs open | met | tabs (`tabs_and_single_page_mode`), tab bar, drag & drop of a second file |
| Windows installer and portable distribution work | built by CI, **not hand-tested** | `.github/workflows/release.yml` builds `pdf-rust-0.1.0-setup.exe` and `pdf-rust-0.1.0-win64.zip` on `windows-latest` and attaches them to the release; installation itself needs a Windows machine (see limitations) |
| Linux package works | met (packages built and inspected; install not performed here) | `.deb` built with `dpkg-deb`, AppDir runs in place (`AppRun --selftest`) |
| Recognised as a PDF reader by both systems | met on Linux, scripted on Windows | `application/pdf` MIME + desktop entry + AppStream; Windows ProgID + `OpenWithProgids` + `Applications` entry |
| Critical features have automated or documented manual tests | met | 159 automated tests plus the manual matrix in `docs/testing.md` |
| Source pushed to the configured remote | met | fast-forward pushes to `origin` after every milestone |

* 0.2.0 reliability round, each covered by tests: one shared cache budget over all
  documents with protected visible pages, memory-pressure bands, full session
  restore (tabs/order/active tab/sidebar/window rectangle) including detection of
  an unclean shutdown and of documents that disappeared, Tab/Shift+Tab focus
  traversal with Enter/Space activation, per-glyph fallback for characters the
  embedded font lacks, typed password errors that re-prompt instead of failing the
  document, PDFium init/shutdown ordering, the page-count cap, and the `0700`/
  `0600` print scratch directory with guaranteed cleanup.

## Known bugs / limitations

* Text rendering has no hinting: chrome text at 11 px is slightly softer than a
  native system font. Document text is unaffected (PDFium renders it).
* No complex-script shaping for UI text (Latin, Greek, Cyrillic covered by the
  embedded DejaVu Sans; other scripts fall back per character — accents to their
  base letter, everything else to the `.notdef` box — in the chrome only).
* No screen-reader bridge yet (AT-SPI/UIA): the chrome is keyboard navigable and
  every control is reachable with Tab, but assistive technology cannot read it.
* PDFium's R2 password check compares against the padding constant, so encrypted
  fixtures are generated the way real writers (and pypdf) do it — see the comment
  in `tests/common/mod.rs::crypto`.
* The Win32 shell, its printing path and the NSIS installer have never run on a
  Windows machine from this sandbox; they are type-checked only. First Windows
  smoke test must cover: window creation and DPI scaling, clipboard, file drop,
  `Ctrl+P` through `PrintDlgW`, and the installer's associations.
* The X11 shell has not been run against a real X server here (no `Xvfb`); the
  painting and interaction code is covered headlessly by the viewer tests.
* Cold start is dominated by loading PDFium: the first run after a reboot takes a
  few hundred milliseconds, warm starts paint the first frame in ~4–6 ms.
* The CI workflow (`.github/workflows/ci.yml`) has not run yet — it was added
  together with this document and needs one green run before it can gate merges.

## Next steps

0. **Features after 0.2.0** (planned, in this order): annotations (colours,
   underline/strikethrough, sticky notes, freehand, edit/delete, export to a new
   PDF), page management (extract/merge/split/reorder/delete/rotate — always to a
   new file), optional OCR that does not raise the baseline footprint, more
   reading modes (two-page, cover, RTL, focus, per-document background), and a
   document library (bookmarks, progress, pins, collections, metadata search).
1. **M4/M5 verification on real hardware**: install X11 + a WM, run the manual
   matrix in `docs/testing.md`; repeat on Windows with the installer.
2. **M9 release checklist**: version bump (see `docs/packaging.md`), build all
   artifacts, sign/checksum, record sizes and selftest numbers in the README.
3. Security review: PDFium load path order, temporary print files (permissions,
   cleanup), SQLite path handling and the `PDFRUST_PDFIUM` override — mostly done
   in 0.2.0; an isolated render process is the remaining item and is deliberately
   out of scope for this release.
4. Optional polish: tooltip delay, tab overflow for more than ~8 documents, a
   real fallback font for CJK, screen-reader bridge (AT-SPI/UIA).
5. Get the first green CI run (Linux: test + selftest + deb; Windows: build +
   portable ZIP) and make it required.
