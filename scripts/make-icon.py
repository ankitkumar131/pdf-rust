#!/usr/bin/env python3
"""Renders the pdf-rust application icon (PNG + ICO) without any dependencies.

The icon is drawn with plain geometry, supersampled 4x and box-filtered, so the
output is deterministic and reproducible: `python3 scripts/make-icon.py` writes
`packaging/linux/pdf-rust.png` and `packaging/windows/pdf-rust.ico`.

Shapes: a rounded blue tile, a white page with a folded corner, a red bookmark
ribbon and a few grey text lines.
"""

import os
import struct
import zlib

SIZES = [16, 24, 32, 48, 64, 128, 256]
SS = 4  # supersampling factor

BG_TOP = (37, 99, 235)      # #2563EB
BG_BOTTOM = (30, 58, 138)   # #1E3A8A
PAPER = (255, 255, 255)
PAPER_SHADE = (226, 232, 240)
FOLD = (203, 213, 225)
RIBBON = (220, 38, 38)
LINE = (148, 163, 184)


def inside_round_rect(x, y, x0, y0, x1, y1, r):
    if x < x0 or x > x1 or y < y0 or y > y1:
        return False
    cx = min(max(x, x0 + r), x1 - r)
    cy = min(max(y, y0 + r), y1 - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def blend(dst, src, alpha):
    return tuple(int(round(d + (s - d) * alpha)) for d, s in zip(dst, src))


def render(size):
    """Renders one square icon at `size` pixels (already supersampled)."""
    n = size * SS
    px = [(0, 0, 0, 0)] * (n * n)
    tile_r = n * 0.20
    inset = n * 0.04
    # Page geometry.
    pw = n * 0.50
    ph = n * 0.66
    px0 = (n - pw) / 2
    py0 = (n - ph) / 2 - n * 0.02
    corner = n * 0.16

    for y in range(n):
        for x in range(n):
            a = 0.0
            colour = (0, 0, 0)
            # 1. Rounded tile with a vertical gradient.
            if inside_round_rect(x + 0.5, y + 0.5, inset, inset, n - inset, n - inset, tile_r):
                t = y / n
                colour = tuple(int(round(BG_TOP[i] + (BG_BOTTOM[i] - BG_TOP[i]) * t)) for i in range(3))
                a = 1.0
            # 2. Paper with a folded top-right corner.
            fx, fy = x + 0.5, y + 0.5
            in_page = px0 <= fx <= px0 + pw and py0 <= fy <= py0 + ph
            if in_page and not (fx > px0 + pw - corner and fy < py0 + corner):
                colour = PAPER
                a = 1.0
            elif in_page:
                colour = FOLD
                a = 1.0
            # 3. Bookmark ribbon.
            ribbon_w = pw * 0.22
            rx0 = px0 + pw * 0.62 - ribbon_w / 2
            if rx0 <= fx <= rx0 + ribbon_w and py0 <= fy <= py0 + ph * 0.30:
                colour = RIBBON
                a = 1.0
            # 4. Text lines.
            line_h = n * 0.028
            for i, width in enumerate((0.62, 0.62, 0.44)):
                ly = py0 + ph * (0.46 + i * 0.14)
                lw = pw * width
                if px0 + pw * 0.12 <= fx <= px0 + pw * 0.12 + lw and ly <= fy <= ly + line_h:
                    colour = LINE if a == 1.0 and colour == PAPER else colour
                    if colour == PAPER:
                        colour = LINE
            px[y * n + x] = (colour[0], colour[1], colour[2], int(a * 255))
    return downsample(px, n, size)


def downsample(px, n, size):
    out = []
    factor = n // size
    for y in range(size):
        for x in range(size):
            r = g = b = a = 0
            for dy in range(factor):
                for dx in range(factor):
                    pr, pg, pb, pa = px[(y * factor + dy) * n + x * factor + dx]
                    r += pr * pa
                    g += pg * pa
                    b += pb * pa
                    a += pa
            count = factor * factor
            if a == 0:
                out.append((0, 0, 0, 0))
            else:
                out.append((r // a, g // a, b // a, a // count))
    return out


def write_png(path, size, pixels):
    raw = bytearray()
    for y in range(size):
        raw.append(0)  # filter type 0
        for x in range(size):
            r, g, b, a = pixels[y * size + x]
            raw += bytes((r, g, b, a))

    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )
    with open(path, "wb") as fh:
        fh.write(png)
    return png


def write_ico(path, images):
    """Writes an ICO containing PNG-compressed entries (supported since Vista)."""
    count = len(images)
    header = struct.pack("<HHH", 0, 1, count)
    offset = 6 + 16 * count
    entries = b""
    blobs = b""
    for size, png in images:
        entries += struct.pack(
            "<BBBBHHII",
            0 if size >= 256 else size,
            0 if size >= 256 else size,
            0,
            0,
            1,
            32,
            len(png),
            offset + len(blobs),
        )
        blobs += png
    with open(path, "wb") as fh:
        fh.write(header + entries + blobs)


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    linux_dir = os.path.join(root, "packaging", "linux")
    win_dir = os.path.join(root, "packaging", "windows")
    os.makedirs(linux_dir, exist_ok=True)
    os.makedirs(win_dir, exist_ok=True)

    images = []
    pngs = {}
    for size in SIZES:
        pixels = render(size)
        png = write_png(os.path.join(win_dir, "pdf-rust-%d.png" % size), size, pixels)
        pngs[size] = png
        images.append((size, png))
    # The PNG used by Linux desktop files and AppStream.
    write_png(os.path.join(linux_dir, "pdf-rust.png"), 256, render(256))
    write_ico(os.path.join(win_dir, "pdf-rust.ico"), images)
    # Keep one loose PNG next to the ICO for documentation screenshots.
    with open(os.path.join(win_dir, "pdf-rust-256.png"), "wb") as fh:
        fh.write(pngs[256])
    print("wrote packaging/linux/pdf-rust.png and packaging/windows/pdf-rust.ico")


if __name__ == "__main__":
    main()
