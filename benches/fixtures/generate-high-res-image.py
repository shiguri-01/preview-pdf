#!/usr/bin/env python3
"""Generate a deterministic high-resolution PNG for PDF render benchmarks."""

from __future__ import annotations

import struct
import sys
import zlib
from pathlib import Path


WIDTH = 4096
HEIGHT = 4096


def png_chunk(kind: bytes, data: bytes) -> bytes:
    checksum = zlib.crc32(kind)
    checksum = zlib.crc32(data, checksum) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", checksum)


def pixel(x: int, y: int) -> tuple[int, int, int]:
    grid = 48 if (x // 128 + y // 128) % 2 == 0 else 18
    r = (x * 5 + y * 2 + grid) % 256
    g = (x * 3 + y * 7 + 96) % 256
    b = ((x ^ y) + (x * y) // 8192 + 32) % 256

    if abs(x - y) < 12 or abs((WIDTH - 1 - x) - y) < 12:
        return (250, 250, 250)
    if x % 512 < 16 or y % 512 < 16:
        return (20, 20, 20)
    return (r, g, b)


def main() -> None:
    out = (
        Path(sys.argv[1])
        if len(sys.argv) > 1
        else Path("target/bench/assets/high-res-bench.png")
    )
    out.parent.mkdir(parents=True, exist_ok=True)

    compressor = zlib.compressobj(level=6)
    compressed_parts: list[bytes] = []
    for y in range(HEIGHT):
        row = bytearray([0])
        for x in range(WIDTH):
            row.extend(pixel(x, y))
        compressed_parts.append(compressor.compress(bytes(row)))
    compressed_parts.append(compressor.flush())
    image_data = b"".join(compressed_parts)

    ihdr = struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 2, 0, 0, 0)
    payload = (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", ihdr)
        + png_chunk(b"IDAT", image_data)
        + png_chunk(b"IEND", b"")
    )
    out.write_bytes(payload)


if __name__ == "__main__":
    main()
