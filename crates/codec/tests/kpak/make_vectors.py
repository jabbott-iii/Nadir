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

"""Pack file (.kpak, version 1) test vectors for nadir-codec.

    python3 crates/codec/tests/kpak/make_vectors.py crates/codec/tests/kpak

writes, into the directory given:

  NAME.kpak    a pack
  NAME.files   for a pack that reads, each of its files, a line each:
               its path, a colon, and its bytes in hex
  vectors.txt  a line per pack:
               NAME ok                      it reads, as NAME.files says
               NAME error KIND OFFSET       kpak::parse refuses it with
                                            ErrorKind::KIND at byte OFFSET
               NAME entry PATH KIND OFFSET  it reads, but the entry of PATH
                                            does not: ErrorKind::KIND at
                                            byte OFFSET

The packs are assembled here, field by field, from the format in
crates/codec/src/kpak.rs, and share no code with it; their KLZ streams are
assembled block by block from the grammar in klz.rs. Each broken pack
breaks one rule where it can, and keeps its checksums right unless the
checksum is the rule. One whose rule cannot be broken alone breaks rules
the reader checks after it too, and is refused by its own, the first: an
entry count or a TOC size no file of its size could hold, or a TOC that
starts in the header or off a multiple of 16, which then does not end
where the file does. An entry whose bytes run past the file keeps the CRC
of the bytes it had. Only Python's standard library is used.
"""
import os
import struct
import sys
import zlib

MASK64 = (1 << 64) - 1


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for byte in data:
        h = ((h ^ byte) * 0x100000001B3) & MASK64
    return h


def crc32(data):
    return zlib.crc32(data) & 0xFFFFFFFF


def extra(n):
    """A length's extra bytes, for what is left past 15."""
    out = bytearray()
    while n >= 255:
        out.append(255)
        n -= 255
    out.append(n)
    return bytes(out)


def klz(blocks, last):
    """A KLZ stream: blocks of (literals, offset, length), then a last block
    of literals only."""
    out = bytearray()
    for literals, offset, length in blocks:
        lit, mat = len(literals), length - 4
        out.append((min(lit, 15) << 4) | min(mat, 15))
        if lit >= 15:
            out += extra(lit - 15)
        out += literals
        out += struct.pack("<H", offset)
        if mat >= 15:
            out += extra(mat - 15)
    lit = len(last)
    out.append(min(lit, 15) << 4)
    if lit >= 15:
        out += extra(lit - 15)
    out += last
    return bytes(out)


# "hello, " and a match of 12 bytes 7 back: 11 bytes for 19.
GREETING = b"hello, hello, hello"
GREETING_KLZ = klz([(b"hello, ", 7, 12)], b"")


class Pack:
    """A pack being assembled: its data region, its entries in TOC order,
    and the header's fields, each of which a broken pack may change."""

    def __init__(self, files, names=True):
        """files: (path, raw bytes, KLZ stream or None to store them)."""
        self.magic = b"KPAK"
        self.version = 1
        self.flags = 1 if names else 0
        self.reserved = 0
        self.data = bytearray(32)
        self.entries = []
        self.extra = b""
        blobs = {}
        for path, raw, stream in files:
            stored, codec = (raw, 0) if stream is None else (stream, 1)
            key = (stored, codec, len(raw))
            if key not in blobs:
                self.align()
                blobs[key] = len(self.data)
                self.data += stored
            self.entries.append(
                {
                    "hash": fnv1a64(path.encode()),
                    "offset": blobs[key],
                    "stored": len(stored),
                    "raw": len(raw),
                    "codec": codec,
                    "reserved": b"\0\0\0",
                    "crc": crc32(stored),
                    "name": path.encode(),
                    "path": path,
                }
            )
        self.entries.sort(key=lambda entry: entry["hash"])
        self.align()

    def align(self):
        self.data += bytes(-len(self.data) % 16)

    def index(self, path):
        return [entry["path"] for entry in self.entries].index(path)

    def toc_offset(self):
        return len(self.data)

    def entry_at(self, path, field=0):
        """The byte of a field of the entry of path, in the file."""
        return self.toc_offset() + 32 * self.index(path) + field

    def name_at(self, path):
        """The byte of the length of path's name, in the file."""
        at = self.toc_offset() + 32 * len(self.entries)
        for entry in self.entries[: self.index(path)]:
            at += 2 + len(entry["name"])
        return at

    def moved(self, path):
        """Gives the entry of path the CRC of the bytes it now points at."""
        entry = self.entries[self.index(path)]
        data = self.bytes()
        entry["crc"] = crc32(data[entry["offset"] : entry["offset"] + entry["stored"]])

    def toc(self):
        out = b"".join(
            struct.pack(
                "<QQIIB3sI",
                entry["hash"],
                entry["offset"],
                entry["stored"],
                entry["raw"],
                entry["codec"],
                entry["reserved"],
                entry["crc"],
            )
            for entry in self.entries
        )
        if self.flags & 1:
            out += b"".join(
                struct.pack("<H", len(entry["name"])) + entry["name"]
                for entry in self.entries
            )
        return out + self.extra

    def bytes(self, count=None, toc_offset=None, toc_size=None, toc_crc=None):
        toc = self.toc()
        header = struct.pack(
            "<4sHHIQIII",
            self.magic,
            self.version,
            self.flags,
            len(self.entries) if count is None else count,
            self.toc_offset() if toc_offset is None else toc_offset,
            len(toc) if toc_size is None else toc_size,
            crc32(toc) if toc_crc is None else toc_crc,
            self.reserved,
        )
        assert len(header) == 32
        return header + bytes(self.data[32:]) + toc

    def files(self):
        return "".join(
            f"{path}:{raw.hex()}\n" for path, raw in sorted(self.raw.items())
        )


def pack(files, names=True):
    made = Pack(files, names)
    made.raw = {path: raw for path, raw, _ in files}
    return made


def two(names=True):
    return pack(
        [
            ("notes/greeting.txt", GREETING, GREETING_KLZ),
            ("notes/one.txt", b"one", None),
        ],
        names,
    )


def vectors():
    """Each vector: (name, the pack's bytes, its line's words after the
    name, its files or None)."""
    out = []

    def ok(name, made):
        out.append((name, made.bytes(), ["ok"], made.files()))

    def refused(name, data, kind, offset):
        out.append((name, data, ["error", kind, str(offset)], None))

    def entry(name, data, path, kind, offset):
        out.append((name, data, ["entry", path, kind, str(offset)], None))

    ok("empty", pack([]))
    ok("two", two())
    ok("no_names", two(names=False))
    # Two blobs stored twice each, and an empty file: four entries share
    # two blobs.
    blob = bytes(range(40))
    ok(
        "dedup",
        pack(
            [
                ("data/a.kbj", blob, None),
                ("data/b.kbj", blob, None),
                ("data/c.kbj", GREETING, GREETING_KLZ),
                ("data/d.kbj", GREETING, GREETING_KLZ),
                ("data/empty.kbj", b"", None),
            ]
        ),
    )
    # Enough entries for a binary search to go both ways, blobs of 16 bytes
    # and of 17, long literals and matches, and a path of 300 bytes.
    files = []
    for i in range(40):
        raw = (f"page {i:02} ".encode() * (i % 5 + 1))[: 16 + i % 3]
        files.append((f"atlas/page_{i:02}.qoi", raw, None))
    run = b"abcdefghijklmnopqrstuvwxyz" + b"z" * 300
    files.append(
        ("data/run.kbj", run, klz([(b"abcdefghijklmnopqrstuvwxyz", 1, 300)], b""))
    )
    files.append(("levels/" + "a" * 293, b"long", None))
    ok("many", pack(files))
    # A KLZ entry larger than its data, all literals: 293 bytes for 290.
    noise = bytes((i * 151 + 7) % 256 for i in range(290))
    ok("klz_literals", pack([("data/noise.kbj", noise, klz([], noise))]))

    base = two()
    data = base.bytes()
    greeting, one = "notes/greeting.txt", "notes/one.txt"
    toc = base.toc_offset()

    refused("short", data[:20], "UnexpectedEnd", 20)
    for name, field, value, kind, offset in [
        ("magic", "magic", b"KPAX", "NotKpak", 0),
        ("version_0", "version", 0, "Version", 4),
        ("version_2", "version", 2, "Version", 4),
        ("flags", "flags", 3, "UnknownFlags", 6),
        ("header_reserved", "reserved", 1, "Reserved", 28),
    ]:
        made = two()
        setattr(made, field, value)
        refused(name, made.bytes(), kind, offset)
    refused("too_many", base.bytes(count=1_000_001), "TooManyEntries", 8)
    refused("toc_in_header", base.bytes(toc_offset=16), "TocOffset", 12)
    refused("toc_misaligned", base.bytes(toc_offset=toc - 8), "TocOffset", 12)
    # Three entries need 102 bytes with their paths' lengths.
    refused("toc_small", base.bytes(count=3), "TocSize", 20)
    made = two(names=False)
    made.extra = bytes(16)
    refused("toc_size_no_names", made.bytes(), "TocSize", 20)
    refused("toc_large", base.bytes(toc_size=(256 << 20) + 1), "TocTooLarge", 20)
    refused("toc_not_at_end", data + bytes(16), "TocPlace", 12)
    refused("toc_cut", data[:-1], "TocPlace", 12)
    refused(
        "toc_crc",
        base.bytes(toc_crc=crc32(base.toc()) ^ 1),
        "TocChecksum",
        toc,
    )

    made = two()
    made.entries.reverse()
    refused("order", made.bytes(), "OutOfOrder", toc + 32)
    made = two(names=False)
    made.entries[1]["hash"] = made.entries[0]["hash"]
    refused("twice", made.bytes(), "OutOfOrder", toc + 32)
    for name, field, value, kind, at in [
        ("codec_2", "codec", 2, "UnknownCodec", 24),
        ("entry_reserved", "reserved", b"\0\1\0", "Reserved", 25),
    ]:
        made = two()
        made.entries[0][field] = value
        refused(name, made.bytes(), kind, toc + at)
    # The stored entry moved into the header, and to 56, clear of the KLZ
    # entry's bytes but not at a multiple of 16.
    for name, offset, kind in [
        ("entry_in_header", 16, "EntryOutside"),
        ("entry_misaligned", 56, "Misaligned"),
    ]:
        made = two()
        made.entries[made.index(one)]["offset"] = offset
        made.moved(one)
        refused(name, made.bytes(), kind, made.entry_at(one, 8))
    # The stored entry of 1,000 bytes, which run past the TOC and the file.
    made = two()
    past = made.entries[made.index(one)]
    past["stored"] = past["raw"] = 1000
    refused("entry_past_toc", made.bytes(), "EntryOutside", made.entry_at(one, 8))
    made = two()
    made.entries[made.index(greeting)]["raw"] = (256 << 20) + 1
    refused(
        "entry_too_large", made.bytes(), "EntryTooLarge", made.entry_at(greeting, 20)
    )
    made = two()
    made.entries[made.index(one)]["raw"] = 4
    refused("size_mismatch", made.bytes(), "SizeMismatch", made.entry_at(one, 20))
    # A blob of 40 bytes at 32, and one of 8 moved to 48, inside it.
    made = pack([("data/a.kbj", blob, None), ("data/b.kbj", b"12345678", None)])
    made.entries[made.index("data/b.kbj")]["offset"] = 48
    made.moved("data/b.kbj")
    refused("overlap", made.bytes(), "Overlap", made.entry_at("data/b.kbj", 8))
    # One blob at one offset, but one entry takes only its first 32 bytes:
    # that one is first by offset and size, so the other is refused.
    made = pack([("data/a.kbj", blob, None), ("data/b.kbj", blob, None)])
    shorter = made.entries[made.index("data/b.kbj")]
    shorter["stored"] = shorter["raw"] = 32
    shorter["crc"] = crc32(blob[:32])
    refused("overlap_shorter", made.bytes(), "Overlap", made.entry_at("data/a.kbj", 8))

    for name, value, kind, at in [
        ("name_empty", b"", "BadName", 0),
        ("name_utf8", b"notes/\xffne.txt", "BadName", 2),
        ("name_hash", b"notes/One.txt", "NameHash", 0),
    ]:
        made = two()
        entry_one = made.entries[made.index(one)]
        entry_one["name"] = value
        if kind == "BadName":
            # Of the bad path's hash, in its place in the order, so that
            # only the path is wrong.
            entry_one["hash"] = fnv1a64(value)
            made.entries.sort(key=lambda entry: entry["hash"])
        refused(name, made.bytes(), kind, made.name_at(one) + at)
    # The last path a byte short, the header and its CRC saying so.
    cut = base.toc()[:-1]
    header = struct.pack("<4sHHIQIII", b"KPAK", 1, 1, 2, toc, len(cut), crc32(cut), 0)
    refused("names_cut", header + data[32:-1], "UnexpectedEnd", len(data) - 1)
    made = two()
    made.extra = b"\0\0\0"
    refused("trailing", made.bytes(), "TrailingBytes", len(data))

    # The entries' own errors: the TOC reads.
    for name, names in [("damaged", True), ("damaged_no_names", False)]:
        made = two(names)
        damaged = bytearray(made.bytes())
        start = made.entries[made.index(greeting)]["offset"]
        damaged[start + 1] ^= 0x20
        entry(name, bytes(damaged), greeting, "Damaged", start)
    # A match at offset 0, its CRC right: KLZ's error, 8 bytes in.
    stream = bytearray(GREETING_KLZ)
    stream[8:10] = b"\0\0"
    made = pack([(greeting, GREETING, bytes(stream)), (one, b"one", None)])
    start = made.entries[made.index(greeting)]["offset"]
    entry("bad_stream", made.bytes(), greeting, "BadStream", start + 8)
    return out


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: make_vectors.py DIR")
    target = sys.argv[1]
    os.makedirs(target, exist_ok=True)
    lines = []
    for name, data, words, files in vectors():
        with open(os.path.join(target, name + ".kpak"), "wb") as f:
            f.write(data)
        if files is not None:
            with open(os.path.join(target, name + ".files"), "w") as f:
                f.write(files)
        lines.append(" ".join([name] + words) + "\n")
    with open(os.path.join(target, "vectors.txt"), "w") as f:
        f.write("".join(lines))


if __name__ == "__main__":
    main()
