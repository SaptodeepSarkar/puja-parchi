#!/usr/bin/env python3
"""Draw the app icon with no image libraries: maroon tile, saffron ring, white 'P'."""
import struct
import zlib
from pathlib import Path

BG = (0x7F, 0x1D, 0x1D)
RING = (0xD9, 0x77, 0x06)
INK = (0xFF, 0xFF, 0xFF)
GLYPH = ["11110", "10001", "10001", "11110", "10000", "10000", "10000"]


def render(size: int, ss: int = 3) -> bytes:
    n = size * ss
    px = [[BG for _ in range(n)] for _ in range(n)]

    def put(x: int, y: int, color):
        if 0 <= x < n and 0 <= y < n:
            px[y][x] = color

    cx = cy = n / 2
    r_out, r_in = n * 0.370, n * 0.318
    for y in range(n):
        for x in range(n):
            d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
            if r_in <= d <= r_out:
                px[y][x] = RING

    cell = n * 0.052
    gw, gh = 5 * cell, 7 * cell
    ox, oy = (n - gw) / 2, (n - gh) / 2
    for row, bits in enumerate(GLYPH):
        for col, bit in enumerate(bits):
            if bit != "1":
                continue
            for yy in range(int(oy + row * cell), int(oy + (row + 1) * cell)):
                for xx in range(int(ox + col * cell), int(ox + (col + 1) * cell)):
                    put(xx, yy, INK)

    out = bytearray()
    for y in range(size):
        out.append(0)
        for x in range(size):
            r = g = b = 0
            for dy in range(ss):
                for dx in range(ss):
                    p = px[y * ss + dy][x * ss + dx]
                    r += p[0]
                    g += p[1]
                    b += p[2]
            k = ss * ss
            out += bytes((r // k, g // k, b // k))
    return bytes(out)


def write_png(path: Path, size: int) -> None:
    raw = render(size)
    def chunk(tag: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)


if __name__ == "__main__":
    out = Path(__file__).resolve().parent.parent / "app" / "public"
    out.mkdir(parents=True, exist_ok=True)
    for s in (192, 512):
        write_png(out / f"icon-{s}.png", s)
        print("wrote", out / f"icon-{s}.png")
