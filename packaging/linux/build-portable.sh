#!/usr/bin/env bash
# Builds a portable Linux tarball: unpack anywhere and run ./pdf-rust.
#
#   packaging/linux/build-portable.sh [version]
#
# Layout inside dist/pdf-rust-<version>-linux-x86_64.tar.gz:
#
#   pdf-rust-<version>-linux-x86_64/
#     pdf-rust                 the executable
#     libpdfium.so             the renderer (found automatically: same directory)
#     README.md CHANGELOG.md LICENSE
#     install-desktop.sh       optional: register the desktop entry for this user
#
# No root, no package manager, no LD_LIBRARY_PATH.
set -euo pipefail

cd "$(dirname "$0")/../.."
root=$(pwd)
version=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
name="pdf-rust-${version}-linux-x86_64"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
pkg="$stage/$name"

bin="target/release/pdf-rust"
pdfium="runtime/pdfium/libpdfium.so"
[ -x "$bin" ] || { echo "error: $bin missing; run cargo build --release" >&2; exit 1; }
[ -f "$pdfium" ] || { echo "error: $pdfium missing; run scripts/fetch-pdfium.sh" >&2; exit 1; }

mkdir -p "$pkg"
install -m755 "$bin" "$pkg/pdf-rust"
install -m644 "$pdfium" "$pkg/libpdfium.so"
for doc in README.md CHANGELOG.md LICENSE; do
  install -m644 "$doc" "$pkg/$doc"
done

cat > "$pkg/install-desktop.sh" <<'INSTALL'
#!/bin/sh
# Registers pdf-rust for the current user: desktop entry, icon and MIME type.
# Run it from this directory; uninstall with `--uninstall`.
set -e
here=$(cd "$(dirname "$0")" && pwd)
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
icons="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps"
mime="${XDG_DATA_HOME:-$HOME/.local/share}/mime/packages"

if [ "$1" = "--uninstall" ]; then
  rm -f "$apps/pdf-rust.desktop" "$icons/pdf-rust.png" "$mime/pdf-rust.xml"
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" || true
  command -v update-mime-database >/dev/null 2>&1 && update-mime-database "${XDG_DATA_HOME:-$HOME/.local/share}/mime" || true
  echo "pdf-rust desktop integration removed"
  exit 0
fi

mkdir -p "$apps" "$icons" "$mime"
cat > "$apps/pdf-rust.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=pdf-rust
GenericName=PDF Viewer
Comment=Fast, native PDF reader
Exec=$here/pdf-rust %F
Icon=pdf-rust
Terminal=false
Categories=Office;Viewer;Graphics;
MimeType=application/pdf;
StartupWMClass=pdf-rust
DESKTOP
if [ -f "$here/pdf-rust.png" ]; then cp "$here/pdf-rust.png" "$icons/pdf-rust.png"; fi
cat > "$mime/pdf-rust.xml" <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/pdf"><icon name="pdf-rust"/></mime-type>
</mime-info>
XML
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" || true
command -v update-mime-database >/dev/null 2>&1 && update-mime-database "${XDG_DATA_HOME:-$HOME/.local/share}/mime" || true
echo "pdf-rust registered in $apps (remove with ./install-desktop.sh --uninstall)"
INSTALL
chmod 755 "$pkg/install-desktop.sh"
install -m644 packaging/linux/pdf-rust.png "$pkg/pdf-rust.png"

mkdir -p dist
out="dist/$name.tar.gz"
tar -czf "$out" -C "$stage" "$name"
echo "built $out ($(du -h "$out" | cut -f1))"
