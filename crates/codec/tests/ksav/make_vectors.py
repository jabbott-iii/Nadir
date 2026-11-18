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

"""Save file (.ksav, version 1) test vectors for nadir-codec.

    python3 -B crates/codec/tests/ksav/make_vectors.py crates/codec/tests/ksav fuzz/corpus/ksav

writes, into the first directory given:

  NAME.ksav    a save file
  NAME.json    for a file that reads, its payload as JSON text
  vectors.txt  a line per file:
               NAME ok SAVED_AT GAME_VERSION   it reads, to NAME.json; the
                                               game version as the hex of
                                               its UTF-8, or - if empty
               NAME error KIND OFFSET          ksav::parse refuses it with
                                               ErrorKind::KIND at byte OFFSET
               NAME data KIND AT               ksav::parse reads it and
                                               ksav::read refuses its payload
                                               with DataErrorKind::KIND at
                                               the path AT, or - for the root

and, into the second, the files that read, as the fuzz target's seeds.

The files are assembled here, field by field, from the format in
crates/codec/src/ksav.rs, with Python's struct and zlib modules and a KBJ
writer of its own (crates/codec/src/kbj.rs), and share no code with the
reader. Each broken file breaks one rule, its CRC-32 right where the rule
is not the CRC's, so that the reader's first check that fails is its own.
"""

import json
import os
import struct
import sys
import zlib

MAGIC = b"KSAV"
I64_MAX = (1 << 63) - 1
U64_MAX = (1 << 64) - 1


def leb128(n):
    out = bytearray()
    while True:
        byte = n & 0x7F
        n >>= 7
        if n:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def kbj_string(text):
    data = text.encode("utf-8")
    return leb128(len(data)) + data


def kbj_value(value):
    if value is None:
        return b"\x00"
    if value is False:
        return b"\x01"
    if value is True:
        return b"\x02"
    if isinstance(value, int):
        zigzag = value << 1 if value >= 0 else ((-value) << 1) - 1
        return b"\x03" + leb128(zigzag)
    if isinstance(value, float):
        return b"\x04" + struct.pack("<d", value)
    if isinstance(value, str):
        return b"\x05" + kbj_string(value)
    if isinstance(value, list):
        return b"\x06" + leb128(len(value)) + b"".join(kbj_value(v) for v in value)
    if isinstance(value, dict):
        members = b"".join(kbj_string(k) + kbj_value(v) for k, v in value.items())
        return b"\x07" + leb128(len(value)) + members
    raise TypeError(value)


def kbj(value):
    return b"KBJ1" + kbj_value(value)


def save_id(id):
    """A save id as reflection writes a u64: past an i64, its digits."""
    return id if id <= I64_MAX else str(id)


def ksav(payload, game_version=b"0.4.0", saved_at=0, version=1, game_version_len=None,
         payload_size=None, crc=None, magic=MAGIC):
    """A save file of the payload's bytes, each field as given or right."""
    if game_version_len is None:
        game_version_len = len(game_version)
    if payload_size is None:
        payload_size = len(payload)
    after = struct.pack("<qI", saved_at, payload_size) + game_version + payload
    if crc is None:
        crc = zlib.crc32(after) & 0xFFFFFFFF
    return magic + struct.pack("<HHI", version, game_version_len, crc) + after


def with_crc(file):
    """`file` with its CRC-32 made right for the bytes after it."""
    crc = zlib.crc32(file[12:]) & 0xFFFFFFFF
    return file[:8] + struct.pack("<I", crc) + file[12:]


def payload(next_id=1, globals=None, entities=None):
    return {
        "next_id": save_id(next_id),
        "globals": {} if globals is None else globals,
        "entities": [] if entities is None else entities,
    }


def entity(id, components, prefab=None):
    out = {"id": save_id(id)}
    if prefab is not None:
        out["prefab"] = prefab
    out["components"] = components
    return out


def two():
    return payload(
        next_id=5,
        globals={"Score": {"points": 40, "best": 120}},
        entities=[
            entity(1, {"Health": {"current": 7, "max": 10}, "Follow": {"target": 3}}),
            entity(3, {"Health": {"current": 2, "max": 12}}, prefab="slime"),
        ],
    )


def ok_vectors():
    """The files that read: (name, game_version, saved_at, payload)."""
    yield "no_entities", "", 0, payload()
    yield "two", "0.4.0", 1795000000, two()
    yield "far_ids", "1.0-β", -86400, payload(
        next_id=U64_MAX,
        entities=[
            entity(I64_MAX, {"Follow": {"target": None}}),
            entity(I64_MAX + 1, {"Follow": {"target": str(U64_MAX)}}),
            entity(0, {}),
        ],
    )
    yield "every_value", "dev", 1, payload(
        globals={"Notes": {"lines": ["café", "", "☃"], "seen": [True, False]}},
        entities=[
            entity(9, {
                "Mixed": {
                    "nothing": None,
                    "small": -64,
                    "large": -(1 << 63),
                    "half": 0.5,
                    "nested": [[], [1, [2]], {}],
                    "empty": {},
                },
            }, prefab="crate_2"),
        ],
    )
    yield "many", "0.4.0", 1795000000, payload(
        next_id=300,
        entities=[entity(i, {"Tag": {}}) for i in range(300)],
    )


def error_vectors():
    """Files ksav::parse refuses: (name, file, kind, offset)."""
    good_payload = kbj(two())
    good = ksav(good_payload)
    payload_at = 24 + len(b"0.4.0")
    yield "not_magic", b"KSAW" + good[4:], "NotKsav", 0
    yield "kbj", good_payload, "NotKsav", 0
    yield "empty_file", b"", "UnexpectedEnd", 0
    yield "magic_only", MAGIC, "UnexpectedEnd", 4
    yield "header_cut", good[:23], "UnexpectedEnd", 23
    yield "version_0", ksav(good_payload, version=0), "Version", 4
    yield "version_2", ksav(good_payload, version=2), "Version", 4
    small = ksav(kbj(payload()))
    cut = ksav(kbj(payload()), game_version_len=len(small))
    yield "game_version_cut", cut, "UnexpectedEnd", len(cut)
    yield "payload_too_large", ksav(good_payload, payload_size=0x10000001), "PayloadTooLarge", 20
    yield "payload_cut", good[:-1], "UnexpectedEnd", len(good) - 1
    shorter = ksav(good_payload, payload_size=len(good_payload) - 1)
    yield "payload_size_short", shorter, "TrailingBytes", len(good) - 1
    yield "trailing", with_crc(good + b"\x00"), "TrailingBytes", len(good)
    damaged = bytearray(good)
    damaged[payload_at + 10] ^= 0x01
    yield "damaged", bytes(damaged), "Damaged", 8
    yield "damaged_time", good[:12] + struct.pack("<q", 1) + good[20:], "Damaged", 8
    crc = struct.unpack("<I", good[8:12])[0]
    yield "damaged_crc", good[:8] + struct.pack("<I", crc ^ 1) + good[12:], "Damaged", 8
    yield "bad_game_version", ksav(good_payload, game_version=b"0.\xff"), "BadGameVersion", 24
    yield "bad_payload", ksav(b"KBJ1\x09"), "BadPayload", payload_at + 4
    yield "payload_empty", ksav(b""), "BadPayload", payload_at
    yield "payload_kbj_2", ksav(b"KBJ2\x00"), "BadPayload", payload_at + 3


def data_vectors():
    """Files whose payload ksav::read refuses: (name, payload, kind, at)."""

    def changed(**members):
        out = payload(entities=[entity(1, {"Health": {"current": 1}})])
        out.update(members)
        return out

    def one(**members):
        out = {"id": 1, "components": {}}
        out.update(members)
        return payload(entities=[{k: v for k, v in out.items() if v is not ...}])

    yield "payload_not_object", [], "WrongType", "-"
    extra = payload()
    extra["version"] = 1
    yield "unknown_member", extra, "UnknownMember", "-"
    yield "no_next_id", {"globals": {}, "entities": []}, "MissingMember", "-"
    yield "no_globals", {"next_id": 1, "entities": []}, "MissingMember", "-"
    yield "no_entities_member", {"next_id": 1, "globals": {}}, "MissingMember", "-"
    yield "next_id_negative", changed(next_id=-1), "BadId", "next_id"
    yield "next_id_float", changed(next_id=1.0), "BadId", "next_id"
    yield "globals_array", changed(globals=[]), "WrongType", "globals"
    yield "global_name", changed(globals={"9lives": {}}), "BadTypeName", "globals"
    yield "entities_object", changed(entities={}), "WrongType", "entities"
    yield "entity_not_object", changed(entities=[1]), "WrongType", "entities[0]"
    yield "entity_member", one(name="hero"), "UnknownMember", "entities[0]"
    yield "no_id", one(id=...), "MissingMember", "entities[0]"
    yield "id_small_string", one(id="12"), "BadId", "entities[0].id"
    yield "id_leading_zero", one(id="09223372036854775808"), "BadId", "entities[0].id"
    yield "id_too_big", one(id="18446744073709551616"), "BadId", "entities[0].id"
    yield "id_negative", one(id=-3), "BadId", "entities[0].id"
    twice = payload(entities=[entity(4, {}), entity(4, {})])
    yield "duplicate_id", twice, "DuplicateId", "entities[1].id"
    yield "prefab_name", one(prefab="Slime"), "BadPrefabName", "entities[0].prefab"
    yield "prefab_empty", one(prefab=""), "BadPrefabName", "entities[0].prefab"
    yield "prefab_null", one(prefab=None), "WrongType", "entities[0].prefab"
    yield "no_components", one(components=...), "MissingMember", "entities[0]"
    yield "components_array", one(components=[]), "WrongType", "entities[0].components"
    yield "component_name", one(components={"bad name": 1}), "BadTypeName", "entities[0].components"


def main():
    out, seeds = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    os.makedirs(seeds, exist_ok=True)
    lines = []

    def write(name, data):
        with open(os.path.join(out, name + ".ksav"), "wb") as f:
            f.write(data)

    for name, game_version, saved_at, value in ok_vectors():
        version = game_version.encode("utf-8")
        file = ksav(kbj(value), game_version=version, saved_at=saved_at)
        write(name, file)
        with open(os.path.join(seeds, name + ".ksav"), "wb") as f:
            f.write(file)
        with open(os.path.join(out, name + ".json"), "w", encoding="utf-8") as f:
            f.write(json.dumps(value, ensure_ascii=False, indent=1) + "\n")
        lines.append("%s ok %d %s" % (name, saved_at, version.hex() or "-"))
    for name, file, kind, offset in error_vectors():
        write(name, file)
        lines.append("%s error %s %d" % (name, kind, offset))
    for name, value, kind, at in data_vectors():
        write(name, ksav(kbj(value)))
        lines.append("%s data %s %s" % (name, kind, at))
    with open(os.path.join(out, "vectors.txt"), "w") as f:
        f.write("\n".join(lines) + "\n")


main()
