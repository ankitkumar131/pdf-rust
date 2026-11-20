#!/usr/bin/env bash
# Builds every Linux release artifact from a clean checkout.
#
#   scripts/build-release.sh [version]
#
# Steps: offline tests, release build, Debian package, AppImage directory
# (plus the image itself when appimagetool is installed) and SHA-256 sums.
# Windows artifacts are produced on Windows with packaging/windows/build.ps1.
set -euo pipefail

cd "$(dirname "$0")/.."
root=$(pwd)
version=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}

echo "==> tests"
cargo test --offline --quiet

echo "==> release build"
cargo build --offline --release

if [ ! -f runtime/pdfium/libpdfium.so ]; then
  echo "==> fetching PDFium"
  ./scripts/fetch-pdfium.sh linux-x64
fi

echo "==> linux packages ($version)"
./packaging/linux/build-deb.sh "$version"
./packaging/linux/build-appimage.sh "$version" || true

echo "==> checksums"
mkdir -p dist
(cd dist && sha256sum ./*.deb ./*.AppImage 2>/dev/null > SHA256SUMS.txt || true)
ls -l dist
