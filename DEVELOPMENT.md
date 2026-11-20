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
| 7 | Windows installer, portable ZIP, associations | **done (unverified on Windows)** | NSIS + ZIP scripts; resource embedding untested here |
| 8 | Linux .deb / AppImage, MIME registration | **done** | `.deb` built and inspected in the sandbox |
| 9 | Cross-platform testing, security review, release | in progress | see the checklists below |

## Verified so far

* `cargo test` — 111 unit + 3 CLI + 10 PDF engine + 17 viewer tests, all green,
  run in parallel against real PDFium 152.0.7934. Zero compiler warnings.
* `cargo check --features win32-syntax-check` type-checks the Win32 shell and its
  GDI printing path on Linux; a deliberately broken line proves the feature really
  compiles `src/platform/win32.rs`.
* `--selftest` on a 200-page document (release, 1180×860): first frame 4.3 ms,
  document→pixels 13.5 ms, steady frame 1.68 ms, page render 0.3 ms, cache 6.9 MB,
  peak memory 17.8 MB. See `docs/testing.md` for the field descriptions.
* `packaging/linux/build-deb.sh` produces a 3.3 MB `.deb` whose contents were
  checked with `dpkg-deb -I/-c`; `build-appimage.sh` produces a runnable AppDir
  (`dist/pdf-rust.AppDir/AppRun --version`).
* PDF engine: 20-page and 1000-page documents, zoom 2×, 90° rotation, text
  extraction, search, outline, links, password-protected and corrupt files,
  rotated-copy export that provably leaves the original untouched.

## Known bugs / limitations

* Text rendering has no hinting: chrome text at 11 px is slightly softer than a
  native system font. Document text is unaffected (PDFium renders it).
* No complex-script shaping for UI text (Latin, Greek, Cyrillic covered by the
  embedded DejaVu Sans; CJK shows `.notdef` boxes in the chrome only).
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

1. **M4/M5 verification on real hardware**: install X11 + a WM, run the manual
   matrix in `docs/testing.md`; repeat on Windows with the installer.
2. **M9 release checklist**: version bump (see `docs/packaging.md`), build all
   artifacts, sign/checksum, record sizes and selftest numbers in the README.
3. Security review: PDFium load path order, temporary print files (permissions,
   cleanup), SQLite path handling and the `PDFRUST_PDFIUM` override.
4. Optional polish: tooltip delay, keyboard focus rectangle for every control,
   `aui`-style tab overflow for more than ~8 documents, CJK fallback font.
5. Get the first green CI run (Linux: test + selftest + deb; Windows: build +
   portable ZIP) and make it required.
