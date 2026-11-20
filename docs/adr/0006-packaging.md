# ADR 0006 — Distribution and packaging

- Status: **accepted**
- Date: 2026-11-20

## Context

The viewer ships as a single executable plus a PDFium shared library. We need
packages that a user can install without a toolchain, on both target platforms,
built with tools that are normally present on each platform (no new build
dependencies for the project itself).

## Decisions

### 1. The binary is the product; PDFium is a sibling file

`scripts/fetch-pdfium.sh` downloads a pinned PDFium build into `runtime/pdfium/`
(git-ignored). Packages install it next to the binary and the loader is asked for
it explicitly:

* Linux: `/usr/bin/pdf-rust` + `/usr/lib/pdf-rust/libpdfium.so` (the loader tries
  the executable's directory and `../lib/pdf-rust` before `libpdfium.so`).
* Windows: `pdf-rust.exe` + `pdfium.dll` in the same directory.

Bundling PDFium statically would remove a file but also remove the ability to
swap in a patched engine, and it complicates licensing. A sibling shared library
keeps the MIT application under MIT.

### 2. Icons and version metadata are generated, not stored by hand

`scripts/make-icon.py` draws the icon with plain geometry (no PIL, no network),
supersamples 4×, and writes `packaging/linux/pdf-rust.png` plus a multi-size
`packaging/windows/pdf-rust.ico` (16–256 px, PNG-compressed entries). Re-running
the script is the only way the icons change, so they cannot drift from the design.

`build.rs` embeds the icon and the `VERSIONINFO` block on Windows when `windres`
(GNU) or `rc.exe` + `cvtres` (MSVC) is available, and otherwise builds without
them instead of failing. No crate (`winres`, `embed-resource`) is involved.

### 3. Linux: `.deb` first, AppImage second

* `packaging/linux/build-deb.sh` builds a package with `dpkg-deb` only (no
  `fakeroot`, no `dh`), installing the binary, PDFium, desktop entry, MIME
  registration, AppStream metainfo, icon and copyright, plus `postinst` hooks
  that refresh the MIME, desktop and icon caches.
* `packaging/linux/build-appimage.sh` builds an AppDir with an `AppRun` that adds
  the bundled library directory to `LD_LIBRARY_PATH`, and calls `appimagetool`
  if it is installed.

Both are shell scripts with no dependencies beyond what a Debian/Ubuntu build
host already has, and both are exercised by `scripts/build-release.sh`.

### 4. Windows: NSIS installer plus portable ZIP

`packaging/windows/build.ps1` runs the tests, builds the release binary, prepares
`dist/pdf-rust-<version>-win64/` (exe, dll, docs, manifest), zips it, and calls
`makensis` when available. `installer.nsi` installs per-machine under
`%ProgramFiles%\pdf-rust`, offers Start Menu/desktop shortcuts and an optional
"Open PDF files with pdf-rust" component, writes the uninstall entry, and
registers the manifest-declared per-monitor DPI awareness.

### 5. Reproducibility

Release builds are `--release` with fat LTO, one codegen unit, `panic=abort` and
stripped symbols; packages are built from a clean `cargo build` with the pinned
PDFium. `scripts/build-release.sh` writes `dist/SHA256SUMS.txt`, and the README
quotes artifact sizes so a surprise (for example PDFium growing) is noticeable.

## Consequences

* Two extra files per platform to keep in sync (desktop entry/MIME on Linux,
  manifest/RC on Windows), all checked into `packaging/`.
* A missing `makensis`, `dpkg-deb` or `appimagetool` degrades gracefully: the
  script explains what is missing and produces whatever it can.
