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

"""Sound file (.kpcm, version 1) test vectors for nadir-codec.

    python3 -B crates/codec/tests/kpcm/make_vectors.py crates/codec/tests/kpcm fuzz/corpus/kpcm

writes, into the first directory given:

  NAME.kpcm    a sound file
  NAME.i16     for a file that reads, its samples, interleaved, each a
               little-endian 16-bit integer, written apart from the file
  vectors.txt  a line per file:
               NAME ok CHANNELS RATE FRAMES   it reads, to NAME.i16
               NAME error KIND OFFSET         kpcm::decode refuses it with
                                              ErrorKind::KIND at byte OFFSET

and, into the second, the files that read, as the fuzz target's seeds.

The files are assembled here, field by field, from the format in
crates/codec/src/kpcm.rs, with Python's struct module, and share no code
with it. The samples are chosen so that a reader that swapped the bytes of
a sample, or the channels of a frame, would read other ones: left and
right differ, and the bytes of most samples differ from each other. Each
broken file breaks one rule, and the fields after it are as a good file's,
so that the reader's first check is the one that fails.
"""

import os
import struct
import sys

MAGIC = b"KPCM"


def header(version=1, channels=1, reserved=0, rate=48000, frames=1):
    return MAGIC + struct.pack("<HBBII", version, channels, reserved, rate, frames)


def samples_bytes(samples):
    return struct.pack("<%dh" % len(samples), *samples)


def ok_vectors():
    """The files that read: (name, channels, rate, frames, samples)."""
    yield "mono", 1, 48000, 4, [0x0102, -0x0304, 0x7FFE, -0x8000]
    yield "stereo", 2, 44100, 3, [0x0102, -0x0201, 0x1234, 0x4321, -1, 1]
    yield "one_frame", 1, 1, 1, [-32768]
    yield "extremes", 2, 384000, 2, [32767, -32768, -32768, 32767]
    yield "silence", 2, 22050, 5, [0] * 10
    # A tone, quiet on the left and loud on the right, of 100 frames.
    tone = []
    for i in range(100):
        tone.append((i * 997) % 4096 - 2048)
        tone.append(((i * 4099) % 65536) - 32768)
    yield "tone", 2, 48000, 100, tone


def error_vectors():
    """The files that are refused: (name, bytes, kind, offset)."""
    good = header(channels=2, frames=1) + samples_bytes([0x0102, 0x0304])
    yield "not_magic", b"RIF", "NotKpcm", 0
    yield "qoa", b"qoaf" + bytes(12), "NotKpcm", 0
    yield "empty", b"", "UnexpectedEnd", 0
    yield "magic_only", MAGIC, "UnexpectedEnd", 4
    yield "header_cut", good[:15], "UnexpectedEnd", 15
    yield "version_0", header(version=0), "Version", 4
    yield "version_2", header(version=2), "Version", 4
    yield "channels_0", header(channels=0) + bytes(2), "BadChannels", 6
    yield "channels_3", header(channels=3) + bytes(6), "BadChannels", 6
    yield "reserved", header(reserved=1) + bytes(2), "Reserved", 7
    yield "rate_0", header(rate=0) + bytes(2), "BadSampleRate", 8
    yield "rate_384001", header(rate=384001) + bytes(2), "BadSampleRate", 8
    yield "no_frames", header(frames=0), "NoFrames", 12
    yield "samples_cut", good[:-1], "UnexpectedEnd", len(good) - 1
    yield "no_samples", good[:16], "UnexpectedEnd", 16
    yield "trailing", good + b"\x00", "TrailingBytes", len(good)
    # More samples than the 256 MiB of the default limits, and none there.
    yield "too_many", header(channels=2, frames=0x0800_0001), "TooManyBytes", 12


def main():
    out, seeds = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    os.makedirs(seeds, exist_ok=True)
    lines = []
    for name, channels, rate, frames, samples in ok_vectors():
        assert len(samples) == channels * frames, name
        file = header(channels=channels, rate=rate, frames=frames) + samples_bytes(samples)
        with open(os.path.join(out, name + ".kpcm"), "wb") as f:
            f.write(file)
        with open(os.path.join(out, name + ".i16"), "wb") as f:
            f.write(samples_bytes(samples))
        with open(os.path.join(seeds, name + ".kpcm"), "wb") as f:
            f.write(file)
        lines.append("%s ok %d %d %d" % (name, channels, rate, frames))
    for name, file, kind, offset in error_vectors():
        with open(os.path.join(out, name + ".kpcm"), "wb") as f:
            f.write(file)
        lines.append("%s error %s %d" % (name, kind, offset))
    with open(os.path.join(out, "vectors.txt"), "w") as f:
        f.write("\n".join(lines) + "\n")


main()
