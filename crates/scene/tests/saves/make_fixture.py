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

"""The version 1 save fixture for nadir-scene's tests.

    python3 -B crates/scene/tests/saves/make_fixture.py crates/scene/tests/saves

writes v1.ksav into the directory given: a save of version 1 of the
format (crates/codec/src/ksav.rs), of the world crates/scene/tests/saves.rs
builds, assembled here field by field with a KBJ writer of its own.

Once a version is shipped, its fixture is never written again: a later
version of the format adds its own fixture and a migration, and the tests
keep loading this one (docs/subsystems/scene.md, "Saves").
"""

import os
import struct
import sys
import zlib

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
    if isinstance(value, str):
        return b"\x05" + kbj_string(value)
    if isinstance(value, list):
        return b"\x06" + leb128(len(value)) + b"".join(kbj_value(v) for v in value)
    if isinstance(value, dict):
        members = b"".join(kbj_string(k) + kbj_value(v) for k, v in value.items())
        return b"\x07" + leb128(len(value)) + members
    raise TypeError(value)


def ksav(payload, game_version, saved_at):
    body = b"KBJ1" + kbj_value(payload)
    after = struct.pack("<qI", saved_at, len(body)) + game_version + body
    crc = zlib.crc32(after) & 0xFFFFFFFF
    return b"KSAV" + struct.pack("<HHI", 1, len(game_version), crc) + after


PAYLOAD = {
    "next_id": 5,
    "globals": {"Score": {"points": 40, "best": 120}},
    "entities": [
        {"id": 1, "components": {"Health": {"current": 7, "max": 10}, "Follow": {"target": 3}}},
        {"id": 3, "prefab": "slime", "components": {"Health": {"current": 2, "max": 12}}},
        {"id": str(U64_MAX), "components": {"Follow": {"target": None}}},
    ],
}


def main():
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    with open(os.path.join(out, "v1.ksav"), "wb") as f:
        f.write(ksav(PAYLOAD, b"0.4.0", 1795000000))


main()
