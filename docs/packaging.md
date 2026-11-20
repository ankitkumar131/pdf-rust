# Packaging

pdf-rust ships as a small executable plus a PDFium shared library. This document
describes how to build every artifact, what ends up in it, and how the pieces are
generated. The rationale is in [ADR 0006](adr/0006-packaging.md).

## Artifacts

| Platform | Artifact | Built by | Contents |
| --- | --- | --- | --- |
| Linux | `dist/pdf-rust_<version>_amd64.deb` | `packaging/linux/build-deb.sh` | `/usr/bin/pdf-rust`, `/usr/lib/pdf-rust/libpdfium.so`, desktop entry, MIME+AppStream metadata, icon, docs, `postinst`/`postrm` hooks |
| Linux | `dist/pdf-rust-<version>-x86_64.AppImage` | `packaging/linux/build-appimage.sh` | AppDir with `AppRun` (sets `LD_LIBRARY_PATH`), same binary and library |
| Windows | `dist/pdf-rust-<version>-win64.zip` | `packaging/windows/build.ps1` | `pdf-rust.exe`, `pdfium.dll`, `README.md`, `CHANGELOG.md`, `LICENSE`, `pdf-rust.exe.manifest` |
| Windows | `dist/pdf-rust-<version>-setup.exe` | `packaging/windows/build.ps1` + NSIS | installer with shortcuts, uninstaller and optional `.pdf` association |
| any | `dist/SHA256SUMS.txt` | `scripts/build-release.sh` | checksums of the Linux artifacts |

### Sizes (Linux x86-64, release build, PDFium 152.0.7934)

| Item | Size |
| --- | --- |
| `pdf-rust` executable (stripped, fat LTO) | 1.71 MB |
| `libpdfium.so` | 7.62 MB |
| `.deb` download | 3.3 MB |
| `.deb` installed (`dpkg-deb -I` *Installed-Size*) | 9.1 MB |
| AppDir (uncompressed, before squashfs) | 9.3 MB |

The Windows artifacts are not measured here (no Windows machine in the sandbox);
expect a similar split — ~1.7 MB executable plus ~8 MB `pdfium.dll`, and a
portable ZIP of roughly 4 MB thanks to `lzma` in the installer and deflate in the
ZIP.

## Prerequisites

```bash
./scripts/fetch-pdfium.sh          # pinned PDFium into runtime/pdfium/ (git-ignored)
cargo build --release              # target/release/pdf-rust
```

Linux packaging additionally needs `dpkg-deb` (any Debian/Ubuntu host) and,
for the AppImage itself, `appimagetool`. Windows packaging needs PowerShell 5+,
and NSIS (`makensis`) for the installer. A Windows resource compiler (`rc.exe`
from the Windows SDK or `windres` from MinGW) gives the executable its icon and
version stamp; without one the build still succeeds and prints a warning.

## Build

```bash
# everything on Linux: tests, release build, deb, AppDir/AppImage, checksums
scripts/build-release.sh                 # or: scripts/build-release.sh 0.2.0

# individual pieces
packaging/linux/build-deb.sh 0.1.0
packaging/linux/build-appimage.sh 0.1.0

# Windows (from a checkout on that machine)
powershell -ExecutionPolicy Bypass -File packaging\windows\build.ps1
powershell -ExecutionPolicy Bypass -File packaging\windows\build.ps1 -Version 0.1.0 -PdfiumDll C:\pdfium\pdfium.dll
```

`DEB_ARCH` overrides the Debian architecture field (default `amd64`).

## What the Linux package installs

```
/usr/bin/pdf-rust
/usr/lib/pdf-rust/libpdfium.so
/usr/share/applications/pdf-rust.desktop        MimeType=application/pdf
/usr/share/icons/hicolor/256x256/apps/pdf-rust.png
/usr/share/mime/packages/pdf-rust.xml
/usr/share/metainfo/org.pdfrust.viewer.metainfo.xml
/usr/share/doc/pdf-rust/{README.md,CHANGELOG.md,LICENSE,copyright}
```

The runtime library is found without `LD_LIBRARY_PATH`: `pdf::ffi::search_paths`
tries, in order, `$PDFRUST_PDFIUM`, the executable's directory, its
`runtime/pdfium/` and `lib/` subdirectories, `../lib/pdf-rust/` (which is how
`/usr/bin/pdf-rust` finds `/usr/lib/pdf-rust/libpdfium.so`), the working
directory, `/usr/lib/pdf-rust`, `/usr/lib`, `/usr/lib/x86_64-linux-gnu` and
`/usr/local/lib`.

`postinst` refreshes the MIME, desktop and icon caches when the corresponding
tools exist; `postrm` repeats the MIME refresh on removal. Neither fails an
install if a tool is missing.

## What the Windows installer does

`packaging/windows/installer.nsi` installs per-machine into
`%ProgramFiles%\pdf-rust`, requiring 64-bit Windows (the shipped PDFium is x64).
Components:

* **pdf-rust (required)** — executable, `pdfium.dll`, docs, manifest, uninstaller
  and the `Software\Microsoft\Windows\CurrentVersion\Uninstall\pdf-rust` entry
  (display name, version, publisher, icon, estimated size).
* **Start Menu shortcut** — folder with the app and its uninstaller.
* **Desktop shortcut**.
* **Open PDF files with pdf-rust** — registers `pdf-rust.pdf` under
  `Software\Classes`, adds it to `.pdf\OpenWithProgids` and registers the
  `Applications\pdf-rust.exe` entry, then notifies the shell to refresh
  associations. It is opt-in and reversible from the uninstaller.

The executable carries a manifest (`pdf-rust.exe.manifest`) declaring
per-monitor-v2 DPI awareness, long-path awareness and supported OS versions, and
is embedded as `RT_MANIFEST` by `build.rs`.

## Icons

`packaging/linux/pdf-rust.png` (256×256) and `packaging/windows/pdf-rust.ico`
(16, 24, 32, 48, 64, 128, 256 px, PNG-compressed entries) are produced by

```bash
python3 scripts/make-icon.py
```

which draws the tile/page/bookmark shapes with plain Python geometry, supersamples
4× and box-filters down. No imaging library and no network access is required, and
the output is deterministic, so the icons can be regenerated and diffed in review.
The script also writes loose `packaging/windows/pdf-rust-<size>.png` files used by
documentation.

## Resource embedding (`build.rs`)

`build.rs` only acts when the target OS is Windows. It looks for
`PDFRUST_RESOURCE_COMPILER`, then `windres` and `rc.exe` on `PATH`, compiles
`packaging/windows/pdf-rust.rc` (icon + `VERSIONINFO` + manifest) and passes the
object to the linker via `cargo:rustc-link-arg`. MSVC builds convert the `.res`
with `cvtres`. Any failure is downgraded to a `cargo:warning` so a build without a
resource compiler still produces a working executable.

## Verifying a package

On Linux the generated `.deb` can be inspected without installing it:

```bash
dpkg-deb -I dist/pdf-rust_0.1.0_amd64.deb     # control fields, hooks
dpkg-deb -c dist/pdf-rust_0.1.0_amd64.deb     # file list and modes
dpkg-deb -x dist/pdf-rust_0.1.0_amd64.deb /tmp/pkg && /tmp/pkg/usr/bin/pdf-rust --version
```

The AppDir can be run in place: `dist/pdf-rust.AppDir/AppRun --selftest` reports a
rendered page without any environment setup. The `.deb` layout can be checked the
same way without installing:

```bash
dpkg-deb -x dist/pdf-rust_0.1.0_amd64.deb /tmp/pkgroot
env -u LD_LIBRARY_PATH -u PDFRUST_PDFIUM /tmp/pkgroot/usr/bin/pdf-rust --selftest doc.pdf
```

This exercises the `../lib/pdf-rust/` lookup the installed package relies on. On Windows, install into a
scratch VM, double-click a `.pdf`, and check *Settings → Apps* lists pdf-rust with
its icon before uninstalling.

## Versioning

`Cargo.toml` is the single source of truth: the packaging scripts read the version
from it when no argument is given. When bumping, update `Cargo.toml`,
`CHANGELOG.md`, the `FILEVERSION`/`PRODUCTVERSION` block in
`packaging/windows/pdf-rust.rc` and the `<release>` element in the AppStream
metainfo — those two are the only places where the number is duplicated. The NSIS
installer takes the version from the `/DAPP_VERSION` define that `build.ps1`
passes.
