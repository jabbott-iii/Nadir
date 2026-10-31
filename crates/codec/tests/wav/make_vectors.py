#!/usr/bin/env python3
"""WAV test vectors for nadir-codec.

    python3 crates/codec/tests/wav/make_vectors.py crates/codec/tests/wav

writes, into the directory given:

  NAME.wav     the vector
  NAME.f32     the samples it should decode to: f32 little-endian,
               interleaved, computed here from the samples written (integers
               divided by 2^15 or 2^23, floats as they are), never by decoding
  vectors.txt  a line per vector: name, sample rate, channels, format

It writes RIFF itself, with Python's standard library only, and shares no
code with the decoder.
"""
import os
import random
import struct
import sys

PCM, FLOAT, EXTENSIBLE = 1, 3, 0xFFFE
GUID_TAIL = bytes([0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71])


def chunk(kind, data):
    """A chunk, with the pad byte an odd length needs."""
    return kind + struct.pack("<I", len(data)) + data + (b"\0" if len(data) % 2 else b"")


def fmt(tag, channels, rate, bits, extensible=False, valid=None, cb_size=True):
    align = channels * bits // 8
    base = struct.pack("<HHIIHH", EXTENSIBLE if extensible else tag, channels, rate, rate * align, align, bits)
    if not extensible:
        # A float format often carries cbSize = 0; arecord leaves it out.
        return chunk(b"fmt ", base + (struct.pack("<H", 0) if tag == FLOAT and cb_size else b""))
    mask = 0x4 if channels == 1 else 0x3
    ext = struct.pack("<HHI", 22, valid or bits, mask) + struct.pack("<H", tag) + GUID_TAIL
    return chunk(b"fmt ", base + ext)


def riff(chunks):
    body = b"WAVE" + b"".join(chunks)
    return b"RIFF" + struct.pack("<I", len(body)) + body


def samples(fmt_name, count, rng):
    if fmt_name == "f32":
        edges = [0.0, -0.0, 1.0, -1.0, 0.5, 1.5, -2.25, 1e-40, 3.4e38]
        values = edges + [rng.uniform(-1, 1) for _ in range(count - len(edges))]
        # As f32, which is what the file holds.
        return [struct.unpack("<f", struct.pack("<f", v))[0] for v in values[:count]]
    top = 1 << (15 if fmt_name == "i16" else 23)
    edges = [0, 1, -1, top - 1, -top, top // 2, -top // 2]
    return (edges + [rng.randrange(-top, top) for _ in range(count - len(edges))])[:count]


def encode(fmt_name, values):
    if fmt_name == "i16":
        return b"".join(struct.pack("<h", v) for v in values)
    if fmt_name == "i24":
        return b"".join(struct.pack("<i", v)[:3] for v in values)
    return b"".join(struct.pack("<f", v) for v in values)


def expected(fmt_name, values):
    if fmt_name == "i16":
        return [v / 32768 for v in values]
    if fmt_name == "i24":
        return [v / 8388608 for v in values]
    return list(values)


FORMATS = {"i16": (PCM, 16, "Int16"), "i24": (PCM, 24, "Int24"), "f32": (FLOAT, 32, "Float32")}


def make(name, fmt_name, channels, rate=44100, frames=37, extensible=False, before=(), after=(), seed=None,
         valid=None, cb_size=True, pad=True):
    rng = random.Random(seed if seed is not None else name)
    tag, bits, label = FORMATS[fmt_name]
    values = samples(fmt_name, frames * channels, rng)
    if valid is not None:
        # Fewer valid bits: the low ones are 0.
        values = [v >> (bits - valid) << (bits - valid) for v in values]
    chunks = list(before) + [fmt(tag, channels, rate, bits, extensible, valid, cb_size)]
    if fmt_name == "f32":
        chunks.append(chunk(b"fact", struct.pack("<I", frames)))
    data = chunk(b"data", encode(fmt_name, values))
    # Some writers leave out the pad byte after odd data.
    chunks.append(data if pad else data[:8 + len(encode(fmt_name, values))])
    chunks += list(after)
    want = b"".join(struct.pack("<f", v) for v in expected(fmt_name, values))
    return name, rate, channels, label, riff(chunks), want


def vectors():
    out = []
    for fmt_name in FORMATS:
        for channels in (1, 2):
            for extensible in (False, True):
                name = f"{fmt_name}_{'stereo' if channels == 2 else 'mono'}{'_ext' if extensible else ''}"
                out.append(make(name, fmt_name, channels, extensible=extensible))
    # Odd data: 24-bit mono of an odd number of frames, so a pad byte
    # follows, and the RIFF size counts it.
    out.append(make("i24_mono_odd", "i24", 1, frames=11))
    # Chunks to skip, before and after, of odd and even lengths.
    info = chunk(b"LIST", b"INFO" + chunk(b"ISFT", b"make_vectors.py\0"))
    out.append(make("i16_stereo_chunks", "i16", 2, before=[chunk(b"JUNK", b"\0" * 3), info],
                    after=[chunk(b"cue ", struct.pack("<I", 0)), chunk(b"id3 ", b"ID3\x04\0\0\0\0\0\0\0")]))
    # Rates and lengths: one frame, no frames, a high rate.
    out.append(make("i16_mono_one_frame", "i16", 1, frames=1))
    out.append(make("f32_stereo_empty", "f32", 2, frames=0))
    out.append(make("i24_stereo_96k", "i24", 2, rate=96000, frames=64))
    out.append(make("i16_mono_8k", "i16", 1, rate=8000, frames=200))
    # As GStreamer writes odd 24-bit mono: no pad byte after the data, and
    # a LIST chunk right after it, at an odd offset.
    out.append(make("i24_mono_odd_no_pad", "i24", 1, frames=13, pad=False,
                    after=[chunk(b"LIST", b"INFO" + chunk(b"ISFT", b"GStreamer\0"))]))
    # As arecord writes float: a fmt chunk of 16 bytes, without cbSize.
    out.append(make("f32_mono_fmt16", "f32", 1, cb_size=False))
    # 20 valid bits in samples of 24.
    out.append(make("i24_stereo_ext_valid20", "i24", 2, extensible=True, valid=20))
    return out


def main():
    target = sys.argv[1]
    os.makedirs(target, exist_ok=True)
    lines = []
    for name, rate, channels, label, wav, want in vectors():
        with open(os.path.join(target, name + ".wav"), "wb") as f:
            f.write(wav)
        with open(os.path.join(target, name + ".f32"), "wb") as f:
            f.write(want)
        lines.append(f"{name} {rate} {channels} {label}")
    with open(os.path.join(target, "vectors.txt"), "w") as f:
        f.write("\n".join(lines) + "\n")
    print(len(lines), "vectors")


if __name__ == "__main__":
    main()
