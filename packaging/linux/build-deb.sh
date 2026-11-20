#!/usr/bin/env bash
# Builds a Debian package from a release binary.
#
#   packaging/linux/build-deb.sh [version]
#
# Expects `target/release/pdf-rust` to exist (cargo build --release) and
# `runtime/pdfium/libpdfium.so` to be the PDFium build to ship. The resulting
# package installs:
#
#   /usr/bin/pdf-rust
#   /usr/lib/pdf-rust/libpdfium.so
#   /usr/share/applications/pdf-rust.desktop
#   /usr/share/icons/hicolor/256x256/apps/pdf-rust.png
#   /usr/share/mime/packages/pdf-rust.xml
#   /usr/share/metainfo/org.pdfrust.viewer.metainfo.xml
#   /usr/share/doc/pdf-rust/{copyright,README.md,CHANGELOG.md}
#
# and registers itself as a handler for application/pdf.
set -euo pipefail

cd "$(dirname "$0")/../.."
root=$(pwd)
version=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
arch=${DEB_ARCH:-amd64}
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

bin="$root/target/release/pdf-rust"
pdfium="$root/runtime/pdfium/libpdfium.so"
[ -x "$bin" ] || { echo "error: $bin missing; run cargo build --release" >&2; exit 1; }
[ -f "$pdfium" ] || { echo "error: $pdfium missing" >&2; exit 1; }

install -Dm755 "$bin" "$stage/usr/bin/pdf-rust"
install -Dm755 "$pdfium" "$stage/usr/lib/pdf-rust/libpdfium.so"
install -Dm644 packaging/linux/pdf-rust.desktop "$stage/usr/share/applications/pdf-rust.desktop"
install -Dm644 packaging/linux/pdf-rust.xml "$stage/usr/share/mime/packages/pdf-rust.xml"
install -Dm644 packaging/linux/org.pdfrust.viewer.metainfo.xml "$stage/usr/share/metainfo/org.pdfrust.viewer.metainfo.xml"
install -Dm644 packaging/linux/pdf-rust.png "$stage/usr/share/icons/hicolor/256x256/apps/pdf-rust.png"
install -Dm644 README.md "$stage/usr/share/doc/pdf-rust/README.md"
install -Dm644 CHANGELOG.md "$stage/usr/share/doc/pdf-rust/CHANGELOG.md"
install -Dm644 LICENSE "$stage/usr/share/doc/pdf-rust/LICENSE"

mkdir -p "$stage/DEBIAN" "$stage/usr/share/doc/pdf-rust"
cat > "$stage/usr/share/doc/pdf-rust/copyright" <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: pdf-rust
Source: https://github.com/ankitkumar131/pdf-rust

Files: *
Copyright: pdf-rust contributors
License: MIT

Files: usr/lib/pdf-rust/libpdfium.so
Copyright: Google LLC and PDFium contributors
License: BSD-3-Clause
 PDFium is distributed under a BSD-style licence; see
 https://pdfium.googlesource.com/pdfium/+/refs/heads/main/LICENSE
EOF
installed_size=$(du -sk "$stage" | cut -f1)
cat > "$stage/DEBIAN/control" <<EOF
Package: pdf-rust
Version: $version
Section: office
Priority: optional
Architecture: $arch
Installed-Size: $installed_size
Depends: libc6 (>= 2.31), libx11-6, libsqlite3-0
Recommends: fonts-dejavu-core
Maintainer: pdf-rust contributors <noreply@example.com>
Homepage: https://github.com/ankitkumar131/pdf-rust
Description: fast native PDF viewer
 pdf-rust is a small PDF viewer written in Rust with no crate dependencies.
 It renders pages with PDFium on a background worker, caches them within a
 fixed memory budget and remembers reading positions per document.
 .
 Features single page and continuous modes, zoom presets, rotation,
 thumbnails, outline navigation, full-text search, text selection and copy,
 presentation mode and printing.
EOF
cat > "$stage/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-mime-database >/dev/null 2>&1; then
  update-mime-database /usr/share/mime || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q /usr/share/icons/hicolor || true
fi
exit 0
EOF
cat > "$stage/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
  if command -v update-mime-database >/dev/null 2>&1; then
    update-mime-database /usr/share/mime || true
  fi
fi
exit 0
EOF
chmod 755 "$stage/DEBIAN/postinst" "$stage/DEBIAN/postrm"

out="$root/dist/pdf-rust_${version}_${arch}.deb"
mkdir -p "$root/dist"
# --root-owner-group keeps the archive reproducible without fakeroot.
if dpkg-deb --help 2>/dev/null | grep -q -- --root-owner-group; then
  dpkg-deb --root-owner-group -Zxz --build "$stage" "$out" >/dev/null
else
  fakeroot dpkg-deb -Zxz --build "$stage" "$out" >/dev/null 2>&1 ||
    dpkg-deb -Zxz --build "$stage" "$out" >/dev/null
fi
echo "built $out ($(du -h "$out" | cut -f1))"
