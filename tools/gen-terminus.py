#!/usr/bin/env python3
"""Generate protolith's Terminus glyph tables (`src/font/terminus/*.bin`)
from the upstream Terminus Font BDF sources.

Upstream (verify before regenerating):

    https://sourceforge.net/projects/terminus-font/files/terminus-font-4.49/terminus-font-4.49.1.tar.gz/download
    version 4.49.1
    SHA-256 d961c1b781627bf417f9b340693d64fc219e0113ad3a3af1a3424c7aa373ef79

Usage:

    python3 tools/gen-terminus.py <extracted terminus-font-4.49.1 dir>

Writes one file per (size, weight): `ter-u{size}{n,b}.bin`. Each holds 191
glyphs — printable ASCII 0x20..=0x7E, then Latin-1 0xA0..=0xFF — and each
glyph is `height` rows of `ceil(width / 8)` bytes, exactly as BDF stores
them: top row first, most significant bit of a row's first byte = leftmost
pixel. No bit reversal, no padding, no per-glyph headers: glyph `i` starts
at byte `i * height * ceil(width / 8)`.

Terminus is monospaced (SPACING "C"); this tool checks that every glyph's
BBX and DWIDTH equal the font bounding box, and that the cell sizes and
ascents are the ones `src/font/terminus.rs` hard-codes, and fails loudly
otherwise. The BDF sources are not committed; only this tool and its
output are.
"""

import re
import sys
from pathlib import Path

SIZES = [12, 16, 20, 24, 32]
WEIGHTS = ["n", "b"]
# Upstream has no 12 px bold: its ter-u12b is a copy of ter-u12n (README,
# "A copy of the normal 6x12 font is installed as bold"), so it is skipped.
SKIP = {(12, "b")}
CODES = list(range(0x20, 0x7F)) + list(range(0xA0, 0x100))
# (width, height, ascent) per size, as src/font/terminus.rs declares them.
EXPECTED = {12: (6, 12, 10), 16: (8, 16, 12), 20: (10, 20, 16), 24: (12, 24, 19), 32: (16, 32, 26)}


def parse(path):
    text = path.read_text(encoding="ascii")
    prop = lambda name: int(re.search(rf"^{name} (-?\d+)$", text, re.M).group(1))
    fbb = tuple(int(v) for v in re.search(r"^FONTBOUNDINGBOX (\d+) (\d+) (-?\d+) (-?\d+)$", text, re.M).groups())
    width, height = fbb[0], fbb[1]
    ascent = prop("FONT_ASCENT")
    if ascent + prop("FONT_DESCENT") != height:
        sys.exit(f"{path}: ascent + descent != height")
    glyphs = {}
    for block in text.split("STARTCHAR")[1:]:
        code = int(re.search(r"^ENCODING (-?\d+)$", block, re.M).group(1))
        if code not in CODES:
            continue
        bbx = tuple(int(v) for v in re.search(r"^BBX (\d+) (\d+) (-?\d+) (-?\d+)$", block, re.M).groups())
        dwidth = int(re.search(r"^DWIDTH (-?\d+) ", block, re.M).group(1))
        if bbx != fbb or dwidth != width:
            sys.exit(f"{path}: glyph {code:#x} is not a full {width}x{height} cell")
        rows = re.search(r"^BITMAP\n(.*?)^ENDCHAR", block, re.M | re.S).group(1).split()
        data = b"".join(bytes.fromhex(r) for r in rows)
        if len(rows) != height or len(data) != height * ((width + 7) // 8):
            sys.exit(f"{path}: glyph {code:#x} has a malformed bitmap")
        glyphs[code] = data
    missing = [hex(c) for c in CODES if c not in glyphs]
    if missing:
        sys.exit(f"{path}: missing glyphs {missing}")
    return (width, height, ascent), b"".join(glyphs[c] for c in CODES)


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    src = Path(sys.argv[1])
    out = Path(__file__).resolve().parent.parent / "src" / "font" / "terminus"
    out.mkdir(parents=True, exist_ok=True)
    for size in SIZES:
        for weight in WEIGHTS:
            if (size, weight) in SKIP:
                continue
            metrics, data = parse(src / f"ter-u{size}{weight}.bdf")
            if metrics != EXPECTED[size]:
                sys.exit(f"ter-u{size}{weight}: metrics {metrics} != expected {EXPECTED[size]}")
            path = out / f"ter-u{size}{weight}.bin"
            path.write_bytes(data)
            print(f"{path.relative_to(out.parent.parent.parent)}: {len(data)} bytes")


if __name__ == "__main__":
    main()
