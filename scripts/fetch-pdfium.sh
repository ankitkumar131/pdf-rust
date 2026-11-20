#!/usr/bin/env bash
# Fetches a pinned PDFium shared library into runtime/pdfium/.
#
#   ./scripts/fetch-pdfium.sh            # host platform (linux-x64 or win-x64)
#   ./scripts/fetch-pdfium.sh linux-x64  # explicit target
#   ./scripts/fetch-pdfium.sh all
#
# Binaries come from the bblanchon/pdfium-binaries releases (BSD-3-Clause
# licensed, same source as the engine used by Chromium). The archives are small
# enough to ship alongside the executable; they are never committed to git.
set -euo pipefail

PDFIUM_TAG="${PDFIUM_TAG:-chromium/7690}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/runtime/pdfium"
mkdir -p "$DEST"

target="${1:-auto}"
if [[ "$target" == "auto" ]]; then
  case "$(uname -s)" in
    Linux) target="linux-x64" ;;
    *) echo "unsupported host $(uname -s); pass linux-x64 or win-x64" >&2; exit 1 ;;
  esac
fi

fetch() {
  local name="$1" lib="$2"
  local url="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_TAG}/pdfium-${name}.tgz"
  local tmp; tmp="$(mktemp -d)"
  echo "==> downloading $url"
  curl -fL --retry 3 -o "$tmp/pdfium.tgz" "$url"
  tar -xzf "$tmp/pdfium.tgz" -C "$tmp"
  if [[ -f "$tmp/lib/$lib" ]]; then
    cp "$tmp/lib/$lib" "$DEST/$lib"
  elif [[ -f "$tmp/bin/$lib" ]]; then
    cp "$tmp/bin/$lib" "$DEST/$lib"
  else
    echo "!! $lib not found in the archive" >&2; ls -R "$tmp" | head -20; exit 1
  fi
  [[ -d "$tmp/licenses" ]] && cp -r "$tmp/licenses" "$DEST/licenses" 2>/dev/null || true
  echo "==> installed $DEST/$lib ($(du -h "$DEST/$lib" | cut -f1))"
  rm -rf "$tmp"
}

case "$target" in
  linux-x64) fetch linux-x64 libpdfium.so ;;
  win-x64)   fetch win-x64 pdfium.dll ;;
  all)       fetch linux-x64 libpdfium.so; fetch win-x64 pdfium.dll ;;
  *) echo "unknown target: $target" >&2; exit 2 ;;
esac
