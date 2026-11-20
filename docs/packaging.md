# Packaging

pdf-rust ships as a small executable plus a PDFium shared library. This document
describes how to build every artifact, what ends up in it, and how the pieces are
generated. The rationale is in [ADR 0006](adr/0006-packaging.md).

## Artifacts

Every release attaches these, built by `.github/workflows/release.yml` on GitHub
runners (Linux job: `.deb`, portable tarball, AppImage; Windows job: portable ZIP,
NSIS installer):

| Platform | Artifact | Built by | Contents |
| --- | --- | --- | --- |
| Windows | `pdf-rust-<version>-win64.zip` | `packaging/windows/build.ps1` | `pdf-rust.exe`, `pdfium.dll`, docs, `pdf-rust.exe.manifest` |
| Windows | `pdf-rust-<version>-setup.exe` | `build.ps1` + `installer.nsi` | installer with shortcuts, uninstaller, optional `.pdf` association |
| Linux | `pdf-rust_<version>_amd64.deb` | `packaging/linux/build-deb.sh` | `/usr/bin/pdf-rust`, `/usr/lib/pdf-rust/libpdfium.so`, desktop entry, MIME+AppStream metadata, icon, docs, `postinst`/`postrm` |
| Linux | `pdf-rust-<version>-x86_64.AppImage` | `packaging/linux/build-appimage.sh` + `appimagetool` | single file, `AppRun` sets `LD_LIBRARY_PATH` to the bundled PDFium |
| Linux | `pdf-rust-<version>-linux-x86_64.tar.gz` | `packaging/linux/build-portable.sh` | portable directory: executable, `libpdfium.so`, docs, `install-desktop.sh` |
| any | `pdf-rust-<version>-src.tar.gz` | `git archive` | source of the tagged commit |
| any | `SHA256SUMS.txt` | release workflow (checksums job) | hashes of every attached file |

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
# everything on Linux: tests, release build, deb, tarball, source, checksums
scripts/build-release.sh                 # or: scripts/build-release.sh 0.2.0

# upload the result as a GitHub release (tag + assets + notes)
scripts/release.sh 0.1.0

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
`PDFRUST_RESOURCE_COMPILER`, then `windres` and `rc.exe`. `rc.exe` is *not* on
`PATH` outside a "Developer Command Prompt", so the script also searches
`%WindowsSdkDir%\bin\<version>\<arch>`, `%ProgramFiles(x86)%\Windows Kits\10\bin\<version>\<arch>`
(highest version wins) and, for the conversion step, `cvtres.exe` next to
`link.exe`, in the Windows SDK or in `%VCToolsInstallDir%`. It compiles
`packaging/windows/pdf-rust.rc` (icon + `VERSIONINFO` + manifest) and passes the
object to the linker via `cargo:rustc-link-arg`; MSVC builds convert the `.res`
with `cvtres`. Success and failure are both reported as `cargo:warning=` lines so
the CI log says which tool was used, and a build without a resource compiler still
produces a working executable (just without the icon).

Because the icon is what Windows shows in the taskbar, `packaging/windows/build.ps1`
verifies the result: it fails the build when the `.exe` is not a GUI-subsystem
binary (a console binary opens a terminal window whenever a PDF is opened) and
warns when `VersionInfo.FileDescription` is empty, which means the resources were
not embedded. At runtime the Win32 shell loads icon resource `1` for the window
class and sets it with `WM_SETICON` for the title bar, taskbar and Alt-Tab.

## Publishing a release

`scripts/release.sh <version>` tags the current commit (`v<version>`), pushes the
tag, and publishes a GitHub release whose body is `docs/release-<version>.md`. It
then tries to attach `dist/*.deb`, `dist/*.tar.gz` and `dist/SHA256SUMS.txt` as
release assets.

Release-asset uploads use `uploads.github.com`, which is not reachable from every
build environment. When that fails, the artifacts are published inside the tagged
tree instead: `downloads/<version>/` holds the `.deb`, the portable tarball and
`SHA256SUMS.txt`, and the release notes link to them with
`https://github.com/<owner>/<repo>/raw/v<version>/downloads/<version>/<file>`.
That keeps the download links stable and verifiable (they are checksummed files
inside an immutable tag), at the cost of the binaries living in git history.

```bash
scripts/build-release.sh 0.1.0     # tests, build, deb, tarball, source, sums
scripts/release.sh 0.1.0           # tag + push + gh release create
gh release view v0.1.0 --json assets,url
```

Asset uploads go to `uploads.github.com`, which this development sandbox cannot
reach; the `Release artifacts` workflow runs on GitHub's runners instead, builds
the same artifacts for both platforms (including the Windows ZIP and installer,
which need a Windows host) and attaches them with `gh release upload`.
`scripts/release.sh` therefore only creates the release and pushes the tag — the
assets arrive on the next workflow run, and the checksums job then replaces
`SHA256SUMS.txt` with one file covering everything that was attached.

The published release is
[v0.1.0](https://github.com/ankitkumar131/pdf-rust/releases/tag/v0.1.0) with notes
in [`docs/release-0.1.0.md`](release-0.1.0.md): Windows installer and portable
ZIP, Debian package, AppImage, Linux tarball and a single `SHA256SUMS.txt`.

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
