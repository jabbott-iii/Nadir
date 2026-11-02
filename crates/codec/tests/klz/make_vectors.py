#!/usr/bin/env python3
# Copyright 2026 Joseph Anthony Abbott III
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""KLZ test vectors for nadir-codec.

    python3 crates/codec/tests/klz/make_vectors.py crates/codec/tests/klz

writes, into the directory given:

  NAME.klz     a stream
  NAME.bin     the bytes it decodes to
  vectors.txt  a line per vector: its name, and "encoder" if NAME.klz is
               what klz::compress must write for NAME.bin, or "stream" if it
               is a stream assembled by hand, block by block, for the decoder

The hand-assembled streams follow the grammar in klz.rs; the encoder's are
from a port of its greedy hash encoder written here from the description,
which shares no code with it. Only Python's standard library is used.
"""
import os
import random
import sys

MIN_MATCH = 4
MAX_OFFSET = 0xFFFF


def extra(n):
    """The extra bytes of a length part n >= 15: 255s, then the rest."""
    out = bytearray()
    n -= 15
    while n >= 255:
        out.append(255)
        n -= 255
    out.append(n)
    return bytes(out)


def block(literals, match=None):
    """A block: literals, then (offset, length) or nothing for the last."""
    lit = len(literals)
    m = 0 if match is None else match[1] - MIN_MATCH
    out = bytearray([(min(lit, 15) << 4) | min(m, 15)])
    if lit >= 15:
        out += extra(lit)
    out += literals
    if match is not None:
        out += match[0].to_bytes(2, "little")
        if m >= 15:
            out += extra(m)
    return bytes(out)


def decode(stream):
    """A reference decoder, to check the hand-made streams."""
    out = bytearray()
    i = 0
    ended = not stream
    while i < len(stream):
        token = stream[i]
        i += 1
        lit = token >> 4
        if lit == 15:
            while True:
                b = stream[i]
                i += 1
                lit += b
                if b != 255:
                    break
        out += stream[i:i + lit]
        i += lit
        if i == len(stream):
            assert token & 15 == 0
            ended = True
            break
        offset = int.from_bytes(stream[i:i + 2], "little")
        i += 2
        length = (token & 15) + MIN_MATCH
        if token & 15 == 15:
            while True:
                b = stream[i]
                i += 1
                length += b
                if b != 255:
                    break
        assert 0 < offset <= len(out)
        for _ in range(length):
            out.append(out[-offset])
    assert ended, "the last block holds literals only"
    return bytes(out)


def encode(data):
    """The greedy encoder: a table of 2^16 positions, indexed by the top 16
    bits of the next 4 bytes (little-endian) times 2654435761, mod 2^32."""
    if not data:
        return b""
    table = [None] * (1 << 16)
    out = bytearray()
    anchor = 0
    i = 0
    while i + MIN_MATCH <= len(data):
        word = int.from_bytes(data[i:i + 4], "little")
        slot = ((word * 2654435761) & 0xFFFFFFFF) >> 16
        start = table[slot]
        table[slot] = i
        if start is None or i - start > MAX_OFFSET or data[start:start + 4] != data[i:i + 4]:
            i += 1
            continue
        length = MIN_MATCH
        while i + length < len(data) and data[i + length] == data[start + length]:
            length += 1
        out += block(data[anchor:i], (i - start, length))
        i += length
        anchor = i
    out += block(data[anchor:])
    return bytes(out)


def hand_made():
    vectors = []
    # Literals only, short and long enough for extra bytes.
    vectors.append(("literals_1", block(b"a")))
    vectors.append(("literals_14", block(b"abcdefghijklmn")))
    vectors.append(("literals_15", block(bytes(range(15)))))
    vectors.append(("literals_269", block(bytes(i % 251 for i in range(15 + 254)))))
    vectors.append(("literals_270", block(bytes(i % 251 for i in range(15 + 255)))))
    vectors.append(("literals_525", block(bytes(i % 251 for i in range(15 + 255 + 255)))))
    # A last block of no literals, after a match.
    vectors.append(("match_then_empty_last", block(b"ab", (2, 4)) + block(b"")))
    # A run: offset 1 overlaps what it writes.
    vectors.append(("run_offset_1", block(b"x", (1, 4)) + block(b"")))
    vectors.append(("run_offset_1_long", block(b"x", (1, 4 + 15 + 255 + 3)) + block(b"y")))
    # Match lengths at the edges of the 4-bit part and the extra bytes.
    for length in (4, 18, 19, 20, 19 + 254, 19 + 255, 19 + 256):
        vectors.append((f"match_{length}", block(b"abcd", (4, length)) + block(b"!")))
    # The farthest offset.
    history = bytes((i * 7) % 253 for i in range(0xFFFF))
    vectors.append(("offset_65535", block(history, (0xFFFF, 9)) + block(b"")))
    # 274 bytes from 6, the most 6 bytes of stream can give.
    vectors.append(("most_from_6_bytes", block(b"x", (1, 273)) + block(b"")))
    # Several blocks, matches into each other's output.
    vectors.append(("chain", block(b"one two ", (4, 4)) + block(b"three ", (14, 8)) + block(b"end")))
    return vectors


def window_edge(rng):
    """Random bytes with WXYZ again exactly 65,535 back, which the encoder
    must match, and QRST again exactly 65,536 back, which it must not. No
    other position hashes to either one's slot, so the first stays in the
    table until it is needed."""
    def slot(quad):
        return ((int.from_bytes(quad, "little") * 2654435761) & 0xFFFFFFFF) >> 16
    plants = {100: b"WXYZ", 100 + 65_535: b"WXYZ", 200: b"QRST", 200 + 65_536: b"QRST"}
    slots = {slot(quad) for quad in plants.values()}
    while True:
        data = bytearray(rng.getrandbits(8) for _ in range(66_000))
        for at, quad in plants.items():
            data[at:at + 4] = quad
        if all(slot(data[i:i + 4]) not in slots or i in plants for i in range(len(data) - 3)):
            break
    data = bytes(data)
    # Where each match of the encoder's output lands, and how far back.
    stream, i, at, found = encode(data), 0, 0, []
    while True:
        token = stream[i]
        i += 1
        literals = token >> 4
        if literals == 15:
            while stream[i] == 255:
                literals += 255
                i += 1
            literals += stream[i]
            i += 1
        i += literals
        at += literals
        if i == len(stream):
            break
        offset = int.from_bytes(stream[i:i + 2], "little")
        i += 2
        length = (token & 15) + MIN_MATCH
        if token & 15 == 15:
            while stream[i] == 255:
                length += 255
                i += 1
            length += stream[i]
            i += 1
        found.append((at, offset))
        at += length
    assert (100 + 65_535, 65_535) in found, found
    assert all(at != 200 + 65_536 for at, _ in found), found
    return data


def samples():
    rng = random.Random(52)
    far = bytes(rng.getrandbits(8) for _ in range(300))
    text = b" ".join(rng.choice([b"tile", b"slime", b"sword", b"door", b"key", b"wall"]) for _ in range(400))
    return [
        ("enc_empty", b""),
        ("enc_one", b"z"),
        ("enc_three", b"abc"),
        ("enc_four_same", b"aaaa"),
        ("enc_run", b"\0" * 1000),
        ("enc_text", text),
        ("enc_random", bytes(rng.getrandbits(8) for _ in range(600))),
        ("enc_period_3", b"abc" * 200),
        # The same 300 bytes twice, too far apart to match.
        ("enc_far", far + bytes(65_600) + far),
        # Among bytes with no repeats of 4, one sequence again exactly
        # 65,535 back, which must match, and one exactly 65,536 back, which
        # must not.
        ("enc_window_edge", window_edge(rng)),
        ("enc_ends_in_match", b"0123456789" * 30),
    ]


def main():
    target = sys.argv[1]
    os.makedirs(target, exist_ok=True)
    lines = []
    for name, stream in hand_made():
        data = decode(stream)
        lines.append((name, "stream", stream, data))
    for name, data in samples():
        stream = encode(data)
        assert decode(stream) == data, name
        lines.append((name, "encoder", stream, data))
    for name, kind, stream, data in lines:
        with open(os.path.join(target, name + ".klz"), "wb") as f:
            f.write(stream)
        with open(os.path.join(target, name + ".bin"), "wb") as f:
            f.write(data)
    with open(os.path.join(target, "vectors.txt"), "w") as f:
        f.write("".join(f"{name} {kind}\n" for name, kind, _, _ in lines))
    print(len(lines), "vectors")


if __name__ == "__main__":
    main()
