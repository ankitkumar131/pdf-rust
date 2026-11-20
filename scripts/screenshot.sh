#!/usr/bin/env bash
# Regenerates the documentation screenshots.
#
#   scripts/screenshot.sh [document.pdf]
#
# The frames are painted by the real viewer through `--screenshot` (no display
# server involved) and then recompressed: `util::png` deliberately writes
# uncompressed zlib streams, which are ~60x larger than necessary.
set -euo pipefail

cd "$(dirname "$0")/.."
pdf=${1:-}
out=docs/screenshots
mkdir -p "$out"

cargo build --offline --release
bin=./target/release/pdf-rust

if [ -z "$pdf" ]; then
  dir=${TMPDIR:-/tmp}/pdf-rust-bench
  mkdir -p "$dir"
  cargo test --offline --quiet --test gen_fixtures -- --ignored --nocapture >/dev/null
  pdf="$dir/benchmark-200.pdf"
fi

"$bin" --screenshot "$out/light.png" --no-db "$pdf"
"$bin" --screenshot "$out/dark.png" --dark --size 1280x820 --no-db "$pdf"

python3 - "$out" <<'PY'
import glob, struct, sys, zlib

def chunk(kind, payload):
    return (struct.pack('>I', len(payload)) + kind + payload +
            struct.pack('>I', zlib.crc32(kind + payload) & 0xffffffff))

for path in sorted(glob.glob(sys.argv[1] + "/*.png")):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", path
    i, chunks = 8, []
    while i < len(data):
        length = struct.unpack('>I', data[i:i + 4])[0]
        chunks.append((data[i + 4:i + 8], data[i + 8:i + 8 + length]))
        i += 12 + length
    ihdr = dict(chunks)[b"IHDR"]
    raw = b"".join(payload for kind, payload in chunks if kind == b"IDAT")
    pixels = zlib.decompress(raw)  # filter type 0 on every row
    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) +
           chunk(b"IDAT", zlib.compress(pixels, 9)) + chunk(b"IEND", b""))
    open(path, "wb").write(png)
    print(f"{path}: {len(data) / 1e6:.2f} MB -> {len(png) / 1e3:.0f} KB")
PY
