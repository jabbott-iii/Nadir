#!/usr/bin/env python3
"""Hand-made PNG test vectors for nadir-codec's PNG decoder.

    python3 crates/codec/tests/png/make_vectors.py crates/codec/tests/png

writes, into the directory given:

  NAME.png     the vector
  NAME.rgba    the pixels it should decode to: RGBA8, rows from the top,
               computed here from the source samples, never by decoding
  vectors.txt  a line per vector: its name, width, height, and the chunks
               the decoder should report as ignored, in order

It uses only Python's standard library: zlib for deflate (another zlib may
compress differently, but the pixels stay the same), a stored-block writer
of its own, and its own filters, bit packing and Adam7 split. It shares no
code with the decoder. libpng (through GdkPixbuf) decodes every vector to
the same pixels, except that it truncates 16-bit samples where these round
them. So does Go's image/png, except that it does not mask the tRNS of
g2n_trns_masked and rejects the long palette of p2n_pal256.
"""
import os
import struct
import sys
import zlib
from fractions import Fraction

SIG = b"\x89PNG\r\n\x1a\n"
CHANNELS = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}
ADAM7 = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]
WARNED = [b"gAMA", b"cHRM", b"sRGB", b"iCCP", b"cICP", b"mDCV", b"cLLI", b"tEXt", b"zTXt", b"iTXt"]


def chunk(kind, data):
    crc = zlib.crc32(kind + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", crc)


class Rng:
    def __init__(self, seed):
        self.s = seed & 0xFFFFFFFF

    def next(self):
        self.s = (self.s * 1664525 + 1013904223) & 0xFFFFFFFF
        return self.s >> 8


def source(w, h, ct, depth, seed, entries=None):
    """Pixels as tuples of samples at `depth` (palette: (index,))."""
    rng = Rng(seed)
    top = (1 << depth) - 1
    rows = []
    for y in range(h):
        row = []
        for x in range(w):
            px = []
            for c in range(CHANNELS[ct]):
                if ct == 3:
                    v = rng.next() % entries
                elif rng.next() % 3 == 0:
                    v = rng.next() & top
                else:
                    # A gradient, so that Sub, Up, Average and Paeth all
                    # predict something other than 0.
                    v = ((x * 37 + y * 19 + c * 71) * top) // (w * 37 + h * 19 + 3 * 71 + 1)
                px.append(v)
            row.append(tuple(px))
        rows.append(row)
    return rows


def pack(row, depth, pad_ones=False):
    if depth < 8:
        out = bytearray()
        acc = 0
        bits = 0
        for px in row:
            acc = (acc << depth) | px[0]
            bits += depth
            if bits == 8:
                out.append(acc)
                acc = 0
                bits = 0
        if bits:
            pad = (1 << (8 - bits)) - 1 if pad_ones else 0
            out.append((acc << (8 - bits)) | pad)
        return bytes(out)
    if depth == 8:
        return bytes(s for px in row for s in px)
    return b"".join(struct.pack(">H", s) for px in row for s in px)


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    if pb <= pc:
        return b
    return c


def filter_line(kind, line, prev, bpp):
    out = bytearray([kind])
    for i, x in enumerate(line):
        a = line[i - bpp] if i >= bpp else 0
        b = prev[i] if prev is not None else 0
        c = prev[i - bpp] if prev is not None and i >= bpp else 0
        pred = [0, a, b, (a + b) // 2, paeth(a, b, c)][kind]
        out.append((x - pred) & 0xFF)
    return bytes(out)


def passes(w, h, interlace):
    for x0, y0, dx, dy in ADAM7 if interlace else [(0, 0, 1, 1)]:
        pw = (w - x0 + dx - 1) // dx if w > x0 else 0
        ph = (h - y0 + dy - 1) // dy if h > y0 else 0
        if pw and ph:
            yield x0, y0, dx, dy, pw, ph


def raw_data(rows, w, h, ct, depth, interlace, filters, pad_ones):
    """The filtered rows. `filters(pass, row)` picks each row's filter."""
    bpp = max(1, CHANNELS[ct] * depth // 8)
    out = bytearray()
    for p, (x0, y0, dx, dy, pw, ph) in enumerate(passes(w, h, interlace)):
        prev = None
        for r in range(ph):
            line = pack([rows[y0 + r * dy][x0 + i * dx] for i in range(pw)], depth, pad_ones)
            out += filter_line(filters(p, r), line, prev, bpp)
            prev = line
    return bytes(out)


def stored_zlib(data, block):
    out = bytearray(b"\x78\x01")
    blocks = [data[i:i + block] for i in range(0, len(data), block)] or [b""]
    for i, b in enumerate(blocks):
        out.append(1 if i == len(blocks) - 1 else 0)
        out += struct.pack("<HH", len(b), len(b) ^ 0xFFFF)
        out += b
    out += struct.pack(">I", zlib.adler32(data) & 0xFFFFFFFF)
    return bytes(out)


def to8(v, depth):
    if depth == 16:
        # Nearest: round(v * 255 / 65535), halves cannot occur.
        return int(Fraction(v * 255, 65535) + Fraction(1, 2))
    return v * 255 // ((1 << depth) - 1)


def expected(rows, ct, depth, palette, trns):
    # A gray or RGB tRNS value counts only in the bits the depth has.
    mask = (1 << depth) - 1
    if trns is not None and ct == 0:
        trns = trns & mask
    elif trns is not None and ct == 2:
        trns = tuple(v & mask for v in trns)
    out = bytearray()
    for row in rows:
        for px in row:
            if ct == 3:
                r, g, b = palette[px[0]]
                a = trns[px[0]] if trns is not None and px[0] < len(trns) else 255
            elif ct == 0:
                r = g = b = to8(px[0], depth)
                a = 0 if trns is not None and px[0] == trns else 255
            elif ct == 2:
                r, g, b = (to8(s, depth) for s in px)
                a = 0 if trns is not None and tuple(px) == tuple(trns) else 255
            elif ct == 4:
                r = g = b = to8(px[0], depth)
                a = to8(px[1], depth)
            else:
                r, g, b, a = (to8(s, depth) for s in px)
            out += bytes([r, g, b, a])
    return bytes(out)


def make(name, ct, depth, interlace, w=13, h=11, seed=None, filters=None, entries=None,
         palette_len=None, trns=None, compress=None, split=None, extra=(), pad_ones=False,
         plte_for_color=False, tweak=None):
    seed = seed if seed is not None else (hash_name(name))
    rng = Rng(seed ^ 0x5A5A)
    palette = None
    if ct == 3:
        n = palette_len or entries or (1 << depth)
        palette = [(rng.next() & 255, rng.next() & 255, rng.next() & 255) for _ in range(n)]
        entries = entries or n
    rows = source(w, h, ct, depth, seed, entries)
    trns_value = None
    if trns == "auto":
        if ct == 0:
            trns_value = rows[h // 2][w // 2][0]
            if depth == 16 and w > 1:
                # Same high byte, other low byte: must stay opaque.
                rows[0][0] = (trns_value ^ 1,)
        elif ct == 2:
            trns_value = tuple(rows[h // 2][w // 2])
            if w > 1:
                near = list(trns_value)
                near[2] ^= 1
                rows[0][0] = tuple(near)
            rows[h - 1][w - 1] = trns_value
        elif ct == 3:
            trns_value = [rng.next() & 255 for _ in range(max(1, len(palette) // 2))]
    elif trns == "unused":
        present = {px[0] for row in rows for px in row}
        trns_value = min(set(range(1 << depth)) - present)
    elif trns is not None:
        trns_value = trns
    if tweak:
        tweak(rows)
    # Each pass's first row gets another filter, so that Up, Average and
    # Paeth meet a first row with nothing above it.
    offset = hash_name(name) % 5
    filt = filters or (lambda p, r: (p + r + offset) % 5)
    raw = raw_data(rows, w, h, ct, depth, interlace, filt, pad_ones)
    if compress is None:
        z = zlib.compress(raw, 9)
    elif compress[0] == "stored":
        z = stored_zlib(raw, compress[1])
    else:
        level, strategy = compress
        c = zlib.compressobj(level, zlib.DEFLATED, 15, 9, strategy)
        z = c.compress(raw) + c.flush()
    assert zlib.decompress(z) == raw
    out = bytearray(SIG)
    written = []

    def put(kind, data):
        out.extend(chunk(kind, data))
        written.append(kind)

    put(b"IHDR", struct.pack(">IIBBBBB", w, h, depth, ct, 0, 0, interlace))
    for kind, data, where in extra:
        if where == "start":
            put(kind, data)
    if palette is not None:
        put(b"PLTE", b"".join(bytes(c) for c in palette))
    elif plte_for_color:
        put(b"PLTE", bytes(range(48)))
    for kind, data, where in extra:
        if where == "after_plte":
            put(kind, data)
    if trns_value is not None:
        if ct == 0:
            put(b"tRNS", struct.pack(">H", trns_value))
        elif ct == 2:
            put(b"tRNS", struct.pack(">HHH", *trns_value))
        else:
            put(b"tRNS", bytes(trns_value))
    if split is None:
        put(b"IDAT", z)
    else:
        for part in split(z):
            put(b"IDAT", part)
    for kind, data, where in extra:
        if where == "after_idat":
            put(kind, data)
    put(b"IEND", b"")
    want = expected(rows, ct, depth, palette, trns_value)
    ignored = []
    for kind in written:
        if kind in WARNED and kind not in ignored:
            ignored.append(kind)
    return name, w, h, bytes(out), want, ignored


def hash_name(name):
    h = 2166136261
    for ch in name.encode():
        h = ((h ^ ch) * 16777619) & 0xFFFFFFFF
    return h


def split_ones(z):
    return [z[i:i + 1] for i in range(len(z))]


def split_with_empty(z):
    third = len(z) // 3
    return [b"", z[:third], b"", z[third:], b""]


def vectors():
    out = []
    kinds = [(0, 1), (0, 2), (0, 4), (0, 8), (0, 16), (2, 8), (2, 16), (3, 1), (3, 2), (3, 4), (3, 8),
             (4, 8), (4, 16), (6, 8), (6, 16)]
    prefix = {0: "g", 2: "rgb", 3: "p", 4: "ga", 6: "rgba"}
    for ct, depth in kinds:
        for interlace in (0, 1):
            out.append(make(f"{prefix[ct]}{depth}{'i' if interlace else 'n'}", ct, depth, interlace))
    # Sizes that leave Adam7 passes empty: any side of 4 or less.
    for w, h in [(1, 1), (2, 1), (1, 2), (3, 2), (5, 1), (1, 9), (8, 8), (9, 9)]:
        out.append(make(f"rgba8i_{w}x{h}", 6, 8, 1, w=w, h=h))
        out.append(make(f"p2i_{w}x{h}", 3, 2, 1, w=w, h=h))
    for w, h in [(1, 1), (4, 1), (1, 8), (3, 2), (4, 8)]:
        out.append(make(f"g1i_{w}x{h}", 0, 1, 1, w=w, h=h))
        out.append(make(f"rgba16i_{w}x{h}", 6, 16, 1, w=w, h=h))
    # One filter for every row: each filter on its own, at 1, 3 and 8
    # bytes a pixel.
    for kind in range(5):
        out.append(make(f"filter{kind}_rgb8n", 2, 8, 0, filters=lambda p, r, f=kind: f))
        out.append(make(f"filter{kind}_g8n", 0, 8, 0, filters=lambda p, r, f=kind: f))
        out.append(make(f"filter{kind}_rgba16i", 6, 16, 1, filters=lambda p, r, f=kind: f))
    # Transparency.
    for name, ct, depth, interlace in [("g1n_trns", 0, 1, 0), ("g4i_trns", 0, 4, 1), ("g8n_trns", 0, 8, 0),
                                       ("g16n_trns", 0, 16, 0), ("rgb8n_trns", 2, 8, 0),
                                       ("rgb16i_trns", 2, 16, 1), ("p4n_trns", 3, 4, 0), ("p8i_trns", 3, 8, 1)]:
        out.append(make(name, ct, depth, interlace, trns="auto"))
    # A tRNS value no pixel has: everything stays opaque.
    out.append(make("g8n_trns_unused", 0, 8, 0, trns="unused"))
    # tRNS values past the bit depth count only in its bits: 0x0100 is 0
    # at 8 bits, and 7 is 3 at 2 bits.
    out.append(make("g8n_trns_masked", 0, 8, 0, trns=0x0100, tweak=lambda rows: rows[2].__setitem__(3, (0,))))
    out.append(make("g2n_trns_masked", 0, 2, 0, trns=7))
    out.append(make("rgb8n_trns_masked", 2, 8, 0, trns=(0x0100, 0x0105, 0x01FF),
                    tweak=lambda rows: rows[4].__setitem__(4, (0, 5, 255))))
    # Palettes longer and shorter than the bit depth allows for.
    out.append(make("p2n_pal256", 3, 2, 0, palette_len=256, entries=4))
    out.append(make("p8n_pal3", 3, 8, 0, entries=3))
    out.append(make("p1n_pal1", 3, 1, 0, entries=1))
    # Padding bits at the end of each row set to 1: ignored, even where
    # they would be a palette index past the palette.
    out.append(make("p1n_padding", 3, 1, 0, pad_ones=True))
    out.append(make("p1n_pal1_padding", 3, 1, 0, entries=1, pad_ones=True))
    out.append(make("p4n_pal5_padding", 3, 4, 0, entries=5, pad_ones=True))
    out.append(make("p2i_pal3_padding", 3, 2, 1, entries=3, pad_ones=True))
    out.append(make("g2i_padding", 0, 2, 1, pad_ones=True))
    # A suggested palette in a color image: ignored.
    out.append(make("rgb8n_plte", 2, 8, 0, plte_for_color=True))
    out.append(make("rgba8n_plte", 6, 8, 0, plte_for_color=True))
    # Ancillary chunks.
    iccp = b"sRGB profile\x00\x00" + zlib.compress(b"not really a profile")
    extra = [
        (b"gAMA", struct.pack(">I", 45455), "start"),
        (b"cHRM", struct.pack(">8I", 31270, 32900, 64000, 33000, 30000, 60000, 15000, 6000), "start"),
        (b"iCCP", iccp, "start"),
        (b"sRGB", b"\x00", "start"),
        (b"cICP", bytes([1, 13, 0, 1]), "start"),
        (b"mDCV", bytes(24), "start"),
        (b"cLLI", bytes(8), "start"),
        (b"pHYs", struct.pack(">IIB", 2835, 2835, 1), "start"),
        (b"tEXt", b"Software\x00a hand-made vector", "start"),
        (b"bKGD", struct.pack(">HHH", 1, 2, 3), "after_plte"),
        (b"prVt", b"private ancillary chunk", "after_plte"),
        (b"tIME", struct.pack(">HBBBBB", 2026, 10, 30, 12, 0, 0), "after_idat"),
        (b"zTXt", b"Comment\x00\x00" + zlib.compress(b"compressed text"), "after_idat"),
        (b"iTXt", b"Title\x00\x00\x00\x00\x00a title", "after_idat"),
        (b"tEXt", b"Author\x00twice, reported once", "after_idat"),
    ]
    out.append(make("rgba8n_ancillary", 6, 8, 0, extra=extra))
    out.append(make("g8n_srgb", 0, 8, 0, extra=[(b"sRGB", b"\x00", "start")]))
    # How the image data is stored.
    out.append(make("rgb8n_split1", 2, 8, 0, split=split_ones))
    out.append(make("rgb8i_split_empty", 2, 8, 1, split=split_with_empty))
    out.append(make("rgb8n_stored", 2, 8, 0, compress=("stored", 100)))
    out.append(make("rgba16i_stored", 6, 16, 1, compress=("stored", 65535)))
    out.append(make("p2i_stored", 3, 2, 1, compress=("stored", 65535)))
    out.append(make("g16i_trns_stored", 0, 16, 1, trns="auto", compress=("stored", 65535)))
    out.append(make("rgb8n_level0", 2, 8, 0, compress=(0, zlib.Z_DEFAULT_STRATEGY)))
    out.append(make("rgb8n_level1", 2, 8, 0, compress=(1, zlib.Z_DEFAULT_STRATEGY)))
    out.append(make("rgb8n_huffman", 2, 8, 0, compress=(9, zlib.Z_HUFFMAN_ONLY)))
    out.append(make("rgb8n_rle", 2, 8, 0, compress=(9, zlib.Z_RLE)))
    out.append(make("rgb8n_fixed", 2, 8, 0, compress=(9, zlib.Z_FIXED)))
    out.append(make("rgb8n_filtered", 2, 8, 0, compress=(9, zlib.Z_FILTERED)))
    # Larger.
    out.append(make("rgba8n_64x64", 6, 8, 0, w=64, h=64))
    out.append(make("rgba8i_63x65", 6, 8, 1, w=63, h=65))
    return out


def first_row_filters(png):
    """The filters of the first row of each pass, read back from a vector
    made with the default schedule (zlib-compressed, one IDAT)."""
    at = 8
    data = b""
    while at < len(png):
        n, kind = struct.unpack(">I4s", png[at:at + 8])
        if kind == b"IHDR":
            w, h, depth, ct, _, _, interlace = struct.unpack(">IIBBBBB", png[at + 8:at + 21])
        if kind == b"IDAT":
            data += png[at + 8:at + 8 + n]
        at += 12 + n
    raw = zlib.decompress(data)
    bits = CHANNELS[ct] * depth
    seen = []
    pos = 0
    for _, _, _, _, pw, ph in passes(w, h, interlace):
        seen.append(raw[pos])
        pos += ph * (1 + (pw * bits + 7) // 8)
    return seen


def main():
    target = sys.argv[1]
    os.makedirs(target, exist_ok=True)
    lines = []
    names = set()
    made = vectors()
    # Every interlaced base vector puts each filter on some pass's first
    # row.
    for name, w, h, png, _, _ in made:
        if name[-1] == "i" and name[0] in "gpr" and "_" not in name:
            assert set(first_row_filters(png)) == {0, 1, 2, 3, 4}, name
    for name, w, h, png, want, ignored in made:
        assert name not in names, name
        names.add(name)
        assert len(want) == w * h * 4
        with open(os.path.join(target, name + ".png"), "wb") as f:
            f.write(png)
        with open(os.path.join(target, name + ".rgba"), "wb") as f:
            f.write(want)
        lines.append(" ".join([name, str(w), str(h)] + [k.decode() for k in ignored]))
    with open(os.path.join(target, "vectors.txt"), "w") as f:
        f.write("\n".join(lines) + "\n")
    print(len(lines), "vectors")


if __name__ == "__main__":
    main()
