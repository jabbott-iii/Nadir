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

"""QOA test vectors for nadir-codec, and the fuzz target's seeds.

    python3 crates/codec/tests/qoa/make_vectors.py crates/codec/tests/qoa fuzz/corpus/qoa

writes, into the first directory given:

  NAME.src     the samples encoded: i16 little-endian, interleaved
  NAME.qoa     the file this script's encoder writes for them
  NAME.i16     the samples that file decodes to: i16 little-endian,
               interleaved, the encoder's own reconstruction as it chose
               each residual, never got by decoding
  vectors.txt  a line per vector: name, channels, sample rate, samples a
               channel, and the most any LMS weight held at a frame's start

and the small vectors' files again into the second, as `NAME.qoa`.

Its encoder is a port of the reference's, `qoa.h`'s `qoa_encode`, written
from its description and sharing no code with nadir-codec's. It computes
in Python's integers, wrapping where C's 32-bit `int` does. It differs
from the reference where nadir-codec's encoder does: at a frame's start,
weights that no longer fit in 16 bits go on as the 16 bits the file holds.
"""
import math
import os
import random
import struct
import sys

SLICE_LEN = 20
FRAME_LEN = 256 * SLICE_LEN
MAX_CHANNELS = 8

SCALEFACTORS = [int(math.floor((s + 1) ** 2.75 + 0.5)) for s in range(16)]
RECIPROCALS = [((1 << 16) + sf - 1) // sf for sf in SCALEFACTORS]
QUANTIZE = [7, 7, 7, 5, 5, 3, 3, 1, 0, 0, 2, 2, 4, 4, 6, 6, 6]
MULTIPLIERS = [0.75, -0.75, 2.5, -2.5, 4.5, -4.5, 7.0, -7.0]


def round_away(x):
    """To the nearest integer, ties away from zero (exact here: the
    multipliers are dyadic)."""
    return int(math.copysign(math.floor(abs(x) + 0.5), x))


DEQUANTIZE = [[round_away(sf * m) for m in MULTIPLIERS] for sf in SCALEFACTORS]


def wrap32(x):
    """C's `int`: two's complement in 32 bits."""
    x &= 0xFFFFFFFF
    return x - (1 << 32) if x & 0x80000000 else x


def wrap16(x):
    x &= 0xFFFF
    return x - (1 << 16) if x & 0x8000 else x


def sign(x):
    return (x > 0) - (x < 0)


def clamp(x, low, high):
    return low if x < low else high if x > high else x


class Lms:
    def __init__(self, history, weights):
        self.history = list(history)
        self.weights = list(weights)

    def copy(self):
        return Lms(self.history, self.weights)

    def predict(self):
        total = 0
        for w, h in zip(self.weights, self.history):
            total = wrap32(total + wrap32(w * h))
        return total >> 13  # arithmetic, as C's on these machines

    def update(self, sample, residual):
        delta = residual >> 4
        self.weights = [wrap32(w + (-delta if h < 0 else delta)) for w, h in zip(self.weights, self.history)]
        self.history = self.history[1:] + [sample]


def divide(v, factor):
    n = wrap32(wrap32(v * RECIPROCALS[factor]) + (1 << 15)) >> 16
    return n + sign(v) - sign(n)


def penalty(weights):
    squares = 0
    for w in weights:
        squares = wrap32(squares + wrap32(w * w))
    p = (squares >> 18) - 0x8FF
    return p * p if p > 0 else 0


def encode(channels, rate, samples):
    """Returns the file, the reconstructed samples, and the largest weight
    carried into a frame before it was cut to 16 bits."""
    count = len(samples) // channels
    assert count * channels == len(samples) and 0 < count < 1 << 32
    out = bytearray(b"qoaf" + struct.pack(">I", count))
    lms = [Lms([0, 0, 0, 0], [0, 0, -(1 << 13), 1 << 14]) for _ in range(channels)]
    decoded = [0] * len(samples)
    largest = 0
    for first in range(0, count, FRAME_LEN):
        frame_len = min(FRAME_LEN, count - first)
        slices = (frame_len + SLICE_LEN - 1) // SLICE_LEN
        size = 8 + channels * (16 + 8 * slices)
        out += struct.pack(">B", channels) + struct.pack(">I", rate)[1:] + struct.pack(">HH", frame_len, size)
        for state in lms:
            largest = max(largest, *(abs(w) for w in state.weights))
            state.weights = [wrap16(w) for w in state.weights]
            out += struct.pack(">4h", *state.history) + struct.pack(">4h", *state.weights)
        previous = [0] * channels
        for start in range(0, frame_len, SLICE_LEN):
            length = min(SLICE_LEN, frame_len - start)
            for c in range(channels):
                at = [(first + start + i) * channels + c for i in range(length)]
                best = None  # (rank, bits, state, factor, reconstructed)
                for step in range(16):
                    factor = (previous[c] + step) % 16
                    state = lms[c].copy()
                    bits, rank, made = factor, 0, []
                    for index in at:
                        sample = samples[index]
                        predicted = state.predict()
                        quantized = QUANTIZE[clamp(divide(sample - predicted, factor), -8, 8) + 8]
                        dequantized = DEQUANTIZE[factor][quantized]
                        reconstructed = clamp(predicted + dequantized, -32768, 32767)
                        rank += (sample - reconstructed) ** 2 + penalty(state.weights)
                        if best is not None and rank > best[0]:
                            break
                        state.update(reconstructed, dequantized)
                        bits = (bits << 3) | quantized
                        made.append(reconstructed)
                    if best is None or rank < best[0]:
                        best = (rank, bits, state, factor, made)
                rank, bits, state, factor, made = best
                lms[c], previous[c] = state, factor
                for index, value in zip(at, made):
                    decoded[index] = value
                out += struct.pack(">Q", bits << ((SLICE_LEN - length) * 3))
    return bytes(out), decoded, largest


def tone(rng, count, channels, rate, amplitude):
    """A chord of a few partials per channel, with a little noise."""
    out = []
    for i in range(count):
        for c in range(channels):
            t = i / rate
            x = sum(math.sin(math.tau * f * (c + 1) * t + c) / (k + 1) for k, f in enumerate((220.0, 331.0, 997.0)))
            out.append(clamp(int(round(amplitude * x / 1.83 + rng.gauss(0, 30))), -32768, 32767))
    return out


def noise(rng, count, channels, amplitude):
    return [clamp(int(round(rng.gauss(0, amplitude))), -32768, 32767) for _ in range(count * channels)]


def vectors():
    rng = random.Random(0x514F41)
    yield "mono_one", 1, 44100, [1234]
    yield "mono_slice", 1, 48000, tone(rng, 20, 1, 48000, 9000)
    yield "stereo_short", 2, 44100, tone(rng, 37, 2, 44100, 12000)
    yield "eight_channels", 8, 8000, tone(rng, 45, 8, 8000, 6000)
    yield "rate_max", 1, 0xFFFFFF, noise(rng, 64, 1, 4000)
    yield "extremes", 1, 22050, [(-32768, 32767)[i // 3 % 2] for i in range(120)]
    yield "mono_frames", 1, 44100, tone(rng, FRAME_LEN + 50, 1, 44100, 16000)
    yield "stereo_frames", 2, 48000, tone(rng, FRAME_LEN + 1, 2, 48000, 20000)
    # A square wave at the highest frequency, at full scale, across two
    # frames: the clamps at both ends.
    yield "nyquist_frames", 1, 44100, [(32767, -32768)[i % 2] for i in range(FRAME_LEN + 300)]
    # A sine clipped at full scale: the weights grow until the search's
    # penalty counts.
    yield "clipped_sine", 1, 44100, [clamp(int(round(32767 * 1.25 * math.sin(0.45 * i))), -32768, 32767) for i in range(2000)]
    # A low sine, clipped, across two frames: the second frame's search
    # starts again from factor 0.
    yield "low_frames", 1, 44100, [clamp(int(32767 * 1.2 * math.sin(math.tau * 20 * i / 44100)), -32768, 32767) for i in range(FRAME_LEN + 300)]
    # Two clipped sines, one high and one low, across two frames: each
    # channel's search starts from its own last factor.
    yield "penalty_stereo", 2, 44100, [
        clamp(int(60000 * math.sin(math.tau * (21000, 50)[c] * i / 44100)), -32768, 32767)
        for i in range(FRAME_LEN + 200)
        for c in range(2)
    ]
    # A pattern of period 4 at full scale: the weights it carries into the
    # second frame no longer fit in 16 bits, and the frame goes on from the
    # 16 bits it holds.
    yield "wide_weights", 1, 44100, [(-9697, 3435, -16185, -31732)[i % 4] for i in range(FRAME_LEN + 100)]


SEEDS = {
    "mono_one", "mono_slice", "stereo_short", "eight_channels", "rate_max", "extremes",
    "mono_frames", "stereo_frames", "nyquist_frames",
}


def main():
    out_dir, seed_dir = sys.argv[1], sys.argv[2]
    lines = []
    for name, channels, rate, samples in vectors():
        file, decoded, largest = encode(channels, rate, samples)
        pack = lambda values: struct.pack("<%dh" % len(values), *values)
        for extension, data in (("src", pack(samples)), ("qoa", file), ("i16", pack(decoded))):
            with open(os.path.join(out_dir, f"{name}.{extension}"), "wb") as f:
                f.write(data)
        if name in SEEDS:
            with open(os.path.join(seed_dir, f"{name}.qoa"), "wb") as f:
                f.write(file)
        lines.append(f"{name} {channels} {rate} {len(samples) // channels} {largest}")
    with open(os.path.join(out_dir, "vectors.txt"), "w") as f:
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
