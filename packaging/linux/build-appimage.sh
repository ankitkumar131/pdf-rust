#!/usr/bin/env bash
# Packages the release binary as an AppImage directory tree plus AppRun.
#
#   packaging/linux/build-appimage.sh [version]
#
# The script builds an AppDir under dist/pdf-rust.AppDir and, when
# `appimagetool` (or `linuxdeploy`) is available, produces
# dist/pdf-rust-<version>-x86_64.AppImage. Without the tool the AppDir is still
# built and can be turned into an AppImage on any machine:
#
#   ARCH=x86_64 appimagetool dist/pdf-rust.AppDir dist/pdf-rust.AppImage
set -euo pipefail

cd "$(dirname "$0")/../.."
root=$(pwd)
version=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
appdir="$root/dist/pdf-rust.AppDir"

bin="$root/target/release/pdf-rust"
pdfium="$root/runtime/pdfium/libpdfium.so"
[ -x "$bin" ] || { echo "error: $bin missing; run cargo build --release" >&2; exit 1; }
[ -f "$pdfium" ] || { echo "error: $pdfium missing" >&2; exit 1; }

rm -rf "$appdir"
install -Dm755 "$bin" "$appdir/usr/bin/pdf-rust"
install -Dm755 "$pdfium" "$appdir/usr/lib/pdf-rust/libpdfium.so"
install -Dm644 packaging/linux/pdf-rust.desktop "$appdir/usr/share/applications/pdf-rust.desktop"
install -Dm644 packaging/linux/pdf-rust.png "$appdir/usr/share/icons/hicolor/256x256/apps/pdf-rust.png"
install -Dm644 packaging/linux/pdf-rust.png "$appdir/pdf-rust.png"
install -Dm644 packaging/linux/pdf-rust.desktop "$appdir/pdf-rust.desktop"
install -Dm644 packaging/linux/org.pdfrust.viewer.metainfo.xml "$appdir/usr/share/metainfo/org.pdfrust.viewer.metainfo.xml"
install -Dm644 README.md "$appdir/usr/share/doc/pdf-rust/README.md"
install -Dm644 LICENSE "$appdir/usr/share/doc/pdf-rust/LICENSE"

cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
# AppImage entry point: make the bundled PDFium visible to the loader.
HERE=$(dirname "$(readlink -f "$0")")
export LD_LIBRARY_PATH="$HERE/usr/lib/pdf-rust${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$HERE/usr/bin/pdf-rust" "$@"
EOF
chmod 755 "$appdir/AppRun"
ln -sf usr/share/icons/hicolor/256x256/apps/pdf-rust.png "$appdir/.DirIcon"

tool=""
for candidate in appimagetool linuxdeploy; do
  if command -v "$candidate" >/dev/null 2>&1; then
    tool=$candidate
    break
  fi
done
if [ -z "$tool" ]; then
  echo "AppDir ready: $appdir"
  echo "appimagetool not found; run: ARCH=x86_64 appimagetool $appdir dist/pdf-rust.AppImage" >&2
  exit 0
fi
out="$root/dist/pdf-rust-${version}-x86_64.AppImage"
if [ "$tool" = appimagetool ]; then
  ARCH=x86_64 "$tool" "$appdir" "$out" >/dev/null
else
  "$tool" --appdir "$appdir" --output appimage >/dev/null
  out=$(ls -t dist/*.AppImage | head -1)
fi
echo "built $out ($(du -h "$out" | cut -f1))"
