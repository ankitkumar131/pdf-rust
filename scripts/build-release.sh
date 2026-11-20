#!/usr/bin/env bash
# Builds every Linux release artifact from a clean checkout.
#
#   scripts/build-release.sh [version]
#
# Steps: offline tests, release build, Debian package, portable tarball, AppImage
# directory (plus the image itself when appimagetool is installed), source
# tarball and SHA-256 sums. Upload the result with scripts/release.sh.
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
./packaging/linux/build-portable.sh "$version"
./packaging/linux/build-appimage.sh "$version" || true

echo "==> source tarball"
mkdir -p dist
git archive --format=tar.gz --prefix="pdf-rust-$version/" \
  -o "dist/pdf-rust-$version-src.tar.gz" "${GIT_REF:-HEAD}"

echo "==> checksums"
(cd dist && sha256sum ./*.deb ./*.tar.gz ./*.AppImage 2>/dev/null > SHA256SUMS.txt || true)
cat dist/SHA256SUMS.txt
ls -l dist
