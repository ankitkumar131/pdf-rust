# Development log

This file is the project's working memory: what is done, what is verified, what
is known to be broken and what comes next. Update it with every milestone.

_Last updated: Milestone 4 (in progress)._

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

`runtime/` is git-ignored; nothing binary is ever committed.

## Milestone status

| # | Milestone | State | Notes |
| --- | --- | --- | --- |
| 1 | Project init, architecture, docs | **done** | Cargo project, ADRs, docs, git |
| 2 | PDF engine + opening documents | **done (engine), UI pending** | PDFium bindings, worker, tests |
| 3 | Navigation, page counter, zoom, rotation | in progress | zoom/rotation model done |
| 4 | SQLite persistence + restore | in progress | schema + sql bindings |
| 5 | Thumbnails, TOC, search, recent | pending | engine side already implemented |
| 6 | UI polish, themes, shortcuts, performance | pending | themes + shortcuts defined |
| 7 | Windows installer, portable ZIP, associations | pending | |
| 8 | Linux .deb / AppImage, MIME registration | pending | |
| 9 | Cross-platform testing, security review, release | pending | |

## Verified so far

* `cargo test` — 39 unit tests + 10 PDF integration tests, all green, run in
  parallel against real PDFium 152.0.7934.
* Font engine: standard metric tests, glyph coverage, PNG frame dumps inspected
  manually (`target/frames/*.png`).
* PDF engine: 20-page and 1000-page documents, zoom 2×, 90° rotation, text
  extraction, search, outline, links, password-protected and corrupt files,
  rotated-copy export that provably leaves the original untouched.

## Known bugs / limitations

* Text rendering has no hinting: chrome text at 11 px is slightly softer than a
  native system font. Document text is unaffected (PDFium renders it).
* No complex-script shaping for UI text (Latin, Greek, Cyrillic covered by the
  embedded DejaVu Sans; CJK shows `.notdef` boxes in the chrome only).
* PDFium's R2 password check compares against the padding constant, so
  encrypted fixtures are generated the way real writers (and pypdf) do it — see
  the comment in `tests/common/mod.rs::crypto`.
* Printing on Linux shells out to `lp`/`lpr`; it is not yet implemented in the
  UI layer (engine export path is ready).

## Next milestones (ordered)

1. Finish Milestone 3: viewer rendering (continuous + single page), page box,
   zoom anchoring, rotation scope, keyboard map.
2. Milestone 4: SQLite bindings, migrations, session save/restore with resume
   prompt, recent documents, window geometry, tests for restart/crash/reboot
   semantics.
3. Milestone 5: thumbnails sidebar, TOC, search panel with next/previous,
   recent list, highlights stored in SQLite.
4. Milestone 6: platform shells (X11 first, then Win32), themes, full screen,
   presentation mode, drag & drop, printing, links, performance metrics.
5. Milestones 7–9: packaging, association tests, security review, release.
