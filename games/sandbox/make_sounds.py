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

"""The sandbox's sounds, made from arithmetic alone.

    python3 games/sandbox/make_sounds.py [ASSETS]

writes, into ASSETS (the workspace's `assets/` if none is given), mono
16-bit WAVs at 48 kHz, the rate the cook cooks to, so that it keeps them
sample for sample:

  audio/sfx/step.wav               a soft footstep on grass, 80 ms, which
                                   ends at 0
  audio/ambience/pond.wav          4 s of water lapping at the pond's edge,
                                   a loop
  audio/ambience/pond.sound.jsonc  its sidecar: it loops
  audio/music/north.wav            the north's tune, 8 s, a loop: bright and
                                   light, C major pentatonic at 120 BPM
  audio/music/south.wav            the south's, 8 s, a loop: calmer and
                                   darker, A minor at 90 BPM in 3/4

and prints each WAV's size, its peak and loudness, and its seam: the jump
from its last sample to its first, beside the largest jump between two
neighbours within it, and the loudness of its first and last 10 ms.

The WAVs are committed, as the sandbox's other sources are: this script is
how they were made, and it is run by hand to remake them, never by a build.
It is seeded, so it writes the same bytes each time; another machine's sines
may round otherwise in their last bit.

A loop is seamless by its making: whatever passes its end, a note's tail
or the reverb's, is added at its start, and each filter runs twice around
it, the second pass kept, which starts in the state the first ended in. A
tune's loop starts where it crosses 0, for QOA's sake (see `tune`).
"""
import math
import os
import random
import struct
import sys

RATE = 48_000
TAU = 2 * math.pi


# Writing

def chunk(kind, data):
    """A chunk, with the pad byte an odd length needs."""
    return kind + struct.pack("<I", len(data)) + data + (b"\0" if len(data) % 2 else b"")


def wav(samples):
    """A WAV of `samples`, 16-bit mono at RATE."""
    fmt = struct.pack("<HHIIHH", 1, 1, RATE, RATE * 2, 2, 16)
    body = b"WAVE" + chunk(b"fmt ", fmt) + chunk(b"data", struct.pack(f"<{len(samples)}h", *samples))
    return b"RIFF" + struct.pack("<I", len(body)) + body


def to_i16(signal, gain):
    """`signal` times `gain` of full scale, as 16-bit samples."""
    return [max(-32768, min(32767, round(x * gain * 32768))) for x in signal]


def db(level):
    """`level`, a share of full scale, in dBFS."""
    return 20 * math.log10(level) if level > 0 else -math.inf


def rms(signal):
    """`signal`'s root mean square."""
    return math.sqrt(sum(x * x for x in signal) / len(signal))


def peak(signal):
    """`signal`'s largest magnitude."""
    return max(abs(x) for x in signal)


# Shaping

def coefficient(hz):
    """A one-pole low-pass's coefficient, for a cutoff of `hz`."""
    return 1 - math.exp(-TAU * hz / RATE)


def passes(loop):
    """How many times a filter runs over a sound: twice around a loop."""
    return 2 if loop else 1


def low_pass(signal, cutoff, loop):
    """`signal` through two one-pole low-passes at `cutoff` Hz: a number, or
    one for each sample."""
    if isinstance(cutoff, (int, float)):
        cutoff = [cutoff] * len(signal)
    a = [coefficient(hz) for hz in cutoff]
    out = [0.0] * len(signal)
    y1 = y2 = 0.0
    for _ in range(passes(loop)):
        for i, x in enumerate(signal):
            y1 += a[i] * (x - y1)
            y2 += a[i] * (y1 - y2)
            out[i] = y2
    return out


def high_pass(signal, hz, loop):
    """`signal` less its one-pole low-pass at `hz`."""
    a = coefficient(hz)
    out = [0.0] * len(signal)
    y = 0.0
    for _ in range(passes(loop)):
        for i, x in enumerate(signal):
            y += a * (x - y)
            out[i] = x - y
    return out


def scaled(signal, level):
    """`signal`, its RMS made `level`."""
    gain = level / rms(signal)
    return [x * gain for x in signal]


def add(into, start, sound):
    """Adds `sound` into the loop `into` from the sample `start`; what passes
    the loop's end, from its start."""
    n = len(into)
    for i, x in enumerate(sound):
        into[(start + i) % n] += x


def mix(*parts):
    """`parts`, of one length, added sample by sample."""
    return [sum(xs) for xs in zip(*parts)]


def fade_in(t, attack):
    """0 to 1 over `attack` s, as a sine squared."""
    return math.sin(min(t / attack, 1.0) * math.pi / 2) ** 2


def note(hz, partials, attack, held, damp):
    """A note of `hz`: the sum of `partials`, sines of (a multiple of `hz`,
    a level, the time in s each takes to fall by e); it rises over `attack`
    s, is held for `held` s, then damped, falling by e every `damp` s, and
    ends 5 of those later, faded out over its last 20 ms to 0."""
    frames = round((held + 5 * damp) * RATE)
    fade = round(0.02 * RATE)
    steps = [TAU * hz * multiple / RATE for multiple, _, _ in partials]
    falls = [math.exp(-1 / (decay * RATE)) for _, _, decay in partials]
    levels = [level for _, level, _ in partials]
    out = [0.0] * frames
    for i in range(frames):
        t = i / RATE
        envelope = fade_in(t, attack)
        if t > held:
            envelope *= math.exp(-(t - held) / damp)
        if i >= frames - fade:
            envelope *= math.cos((i - (frames - fade) + 1) / fade * math.pi / 2) ** 2
        out[i] = envelope * sum(levels[k] * math.sin(steps[k] * i) for k in range(len(steps)))
        for k, fall in enumerate(falls):
            levels[k] *= fall
    return out


def hz(midi):
    """The equal-tempered pitch of a MIDI note, A4 (69) at 440 Hz."""
    return 440 * 2 ** ((midi - 69) / 12)


def comb(signal, delay, feedback, damping):
    """Freeverb's comb, damped, twice around the loop `signal`."""
    line = [0.0] * delay
    out = [0.0] * len(signal)
    at, store = 0, 0.0
    for _ in range(2):
        for i, x in enumerate(signal):
            y = line[at]
            store = y * (1 - damping) + store * damping
            line[at] = x + store * feedback
            at = at + 1 if at + 1 < delay else 0
            out[i] = y
    return out


def all_pass(signal, delay, feedback=0.5):
    """Freeverb's all-pass, twice around the loop `signal`."""
    line = [0.0] * delay
    out = [0.0] * len(signal)
    at = 0
    for _ in range(2):
        for i, x in enumerate(signal):
            y = line[at]
            line[at] = x + y * feedback
            at = at + 1 if at + 1 < delay else 0
            out[i] = y - x
    return out


def reverb(signal, wet):
    """The loop `signal` in a small, soft room, after Freeverb: four damped
    combs side by side, two all-passes after them, a tail of about a second;
    the room `wet` of the loudness of what goes in."""
    room = mix(*(comb(signal, delay, 0.78, 0.35) for delay in (1693, 1759, 1621, 1549)))
    for delay in (607, 479):
        room = all_pass(room, delay)
    return mix(signal, scaled(room, wet * rms(signal)))


# The sounds

def step():
    """A soft footstep on grass: a muffled thud of low-passed noise, and the
    grass under it, a crunch of noise between 600 Hz and 3.2 kHz, at the
    heel and, softer, at the toe; each rising in a few ms and gone in a few
    tens; faded out over the last 25 ms to 0."""
    frames = round(0.080 * RATE)
    rng = random.Random("step")
    noise = [rng.uniform(-1, 1) for _ in range(2 * frames)]

    def burst(start, attack, decay):
        return [0.0 if t < start else fade_in(t - start, attack) * math.exp(-(t - start) / decay)
                for t in (i / RATE for i in range(frames))]

    thud = low_pass(noise[:frames], 220, loop=False)
    grass = high_pass(low_pass(noise[frames:], 3200, loop=False), 600, loop=False)
    heel, crunch, toe = burst(0.0, 0.003, 0.016), burst(0.004, 0.002, 0.020), burst(0.030, 0.003, 0.016)
    thud = scaled([x * e for x, e in zip(thud, heel)], 1.0)
    grass = scaled([x * (c + 0.55 * t) for x, c, t in zip(grass, crunch, toe)], 0.45)
    fade = round(0.025 * RATE)
    out = []
    for i, x in enumerate(mix(thud, grass)):
        left = frames - 1 - i
        out.append(x * (math.sin(left / fade * math.pi / 2) ** 2 if left < fade else 1.0))
    return out, 10 ** (-4 / 20) / peak(out)


def pond():
    """Water lapping at the pond's edge: four small waves a loop, each a swell
    of noise that brightens as it breaks, over the water's soft murmur, and
    three bubbles, small chirps that rise in pitch, as a bubble's does."""
    frames = 4 * RATE
    rng = random.Random("pond")
    white = [rng.uniform(-1, 1) for _ in range(2 * frames)]

    # The waves: when, how strong, how long they rise and fall, in s.
    swell = [0.0] * frames
    for start, level, rise, fall in [(0.10, 0.85, 0.50, 1.00), (1.15, 1.00, 0.55, 1.20),
                                     (2.20, 0.70, 0.45, 0.90), (3.05, 0.90, 0.40, 1.05)]:
        shape = []
        for i in range(round((rise + fall) * RATE)):
            t = i / RATE
            shape.append(level * (fade_in(t, rise) if t < rise else math.cos((t - rise) / fall * math.pi / 2) ** 2))
        add(swell, round(start * RATE), shape)

    murmur = scaled(high_pass(low_pass(white[:frames], 380, loop=True), 50, loop=True), 1.0)
    cutoff = [350 + 1500 * s for s in swell]
    slosh = scaled(high_pass(low_pass(white[frames:], cutoff, loop=True), 140, loop=True), 1.0)
    water = [m * (0.18 + 0.30 * s) + x * 0.9 * s ** 1.5 for m, x, s in zip(murmur, slosh, swell)]

    level = 0.8 * rms(water)
    for start, pitch in [(0.62, 780), (1.93, 1150), (3.48, 640)]:
        chirp = []
        for i in range(round(0.08 * RATE)):
            t = i / RATE
            # Its pitch rises by 32 % in 40 ms.
            phase = TAU * pitch * (t + 4 * t * t)
            envelope = fade_in(t, 0.0015) * math.exp(-t / 0.018) * math.cos(t / 0.08 * math.pi / 2) ** 2
            chirp.append(level * envelope * math.sin(phase))
        add(water, round(start * RATE), chirp)
    return water, 10 ** (-7 / 20) / peak(water)


def tune(beat, melody, bass, chords, voices, wet):
    """A loop of 8 s: the notes of `melody`, `bass` and `chords`, (beat, beats,
    MIDI notes) each, played with `voices`, a (partials, attack, damping,
    loudness) for each, the melody and the chords in a little reverb, `wet`;
    at −21 dBFS RMS, as loud as the other tune."""
    frames = 8 * RATE
    parts = []
    for notes, (partials, attack, damp, loudness) in zip((melody, bass, chords), voices):
        part = [0.0] * frames
        for at, beats, pitches in notes:
            for midi in pitches:
                add(part, round(at * beat * RATE), note(hz(midi), partials, attack, beats * beat, damp))
        parts.append(scaled(part, loudness))
    melody, bass, chords = parts
    out = mix(reverb(mix(melody, chords), wet), bass)
    # QOA, the music's format, starts its file with no history, so a loud
    # first sample decodes wrong, a click each time the loop comes round:
    # the loop starts where it crosses 0 nearest the downbeat instead, a
    # fraction of a millisecond away.
    ten = RATE // 100
    start = min((k for k in range(-ten, ten) if (out[k - 1] < 0) != (out[k] < 0)), key=abs)
    out = out[start:] + out[:start]
    return out, 10 ** (-21 / 20) / rms(out)


def north():
    """The north's tune, for the meadow: bright and light. C major
    pentatonic plucked as a soft marimba, at 120 BPM over C, Am, F and G,
    a soft round bass on each bar's first and third beats and a quiet pad
    of each chord, slightly detuned, under them."""
    beat = 0.5
    E5, G5, A5, C5, D5, C6 = 76, 79, 81, 72, 74, 84
    eighths = [(0, 2, E5), (2, 2, G5), (4, 1, A5), (5, 1, G5), (6, 2, E5),
               (8, 3, C5), (11, 1, D5), (12, 2, E5), (14, 2, C5),
               (16, 2, A5), (18, 1, G5), (19, 1, A5), (20, 2, C6), (22, 2, A5),
               (24, 3, G5), (27, 1, E5), (28, 4, D5)]
    melody = [(at / 2, length / 2, [midi]) for at, length, midi in eighths]
    bass = []
    for bar, (root, fifth) in enumerate([(48, 43), (45, 40), (41, 48), (43, 50)]):
        bass += [(4 * bar, 1.9, [root]), (4 * bar + 2, 1.9, [fifth])]
    pads = [(60, 64, 67), (57, 60, 64), (57, 60, 65), (55, 59, 62)]
    chords = [(4 * bar, 3.8, list(pad)) for bar, pad in enumerate(pads)]
    voices = [
        ([(1, 1.0, 0.55), (2, 0.12, 0.20), (4, 0.06, 0.05)], 0.003, 0.09, 1.0),
        ([(1, 1.0, 1.2), (2, 0.25, 0.5), (3, 0.06, 0.3)], 0.015, 0.06, 0.6),
        ([(1, 0.5, 100), (1.0017, 0.5, 100), (2, 0.08, 100)], 0.30, 0.12, 0.3),
    ]
    return tune(beat, melody, bass, chords, voices, wet=0.3)


def south():
    """The south's tune, by the water: calmer and darker. A minor at 90 BPM
    in 3/4 over Am, F, Dm and E, as a felt piano: a low bass on each bar's
    first beat, and the chord on its second, held, as a slow waltz's."""
    beat = 60 / 90
    A4, C5, E5, D5, F5, B4, Gs4 = 69, 72, 76, 74, 77, 71, 68
    melody = [(0, 1, [A4]), (1, 1, [C5]), (2, 1, [E5]),
              (3, 1.5, [D5]), (4.5, 0.5, [C5]), (5, 1, [A4]),
              (6, 1, [F5]), (7, 1, [E5]), (8, 1, [D5]),
              (9, 1.5, [E5]), (10.5, 0.5, [B4]), (11, 1, [Gs4])]
    roots = [45, 41, 38, 40]
    triads = [(52, 57, 60), (53, 57, 60), (53, 57, 62), (52, 56, 59)]
    bass = [(3 * bar, 2.9, [root]) for bar, root in enumerate(roots)]
    chords = [(3 * bar + 1, 1.9, list(triad)) for bar, triad in enumerate(triads)]
    piano = [(1, 1.0, 1.4), (2, 0.18, 0.5), (3, 0.05, 0.25)]
    voices = [
        (piano, 0.008, 0.25, 1.0),
        ([(1, 1.0, 2.0), (2, 0.35, 0.8), (3, 0.12, 0.4)], 0.02, 0.15, 0.55),
        (piano, 0.015, 0.25, 0.45),
    ]
    return tune(beat, melody, bass, chords, voices, wet=0.4)


SIDECAR = """// The pond's water, made by games/sandbox/make_sounds.py: a loop, which
// ends where it begins, not in silence.
{ "loop": true }
"""


def report(path, samples, size, looped):
    """A line on the WAV at `path`: its size, peak and loudness, and its seam,
    or for a sound that does not loop, its end."""
    full = [s / 32768 for s in samples]
    ten = RATE // 100
    if looped:
        jumps = [abs(b - a) for a, b in zip(samples, samples[1:])]
        edge = f"seam {abs(samples[0] - samples[-1])} (largest step within {max(jumps)})"
    else:
        edge = f"ends at {samples[-1]}, its last 2 ms within ±{max(abs(s) for s in samples[-RATE // 500:])}"
    print(f"{path}: {size} bytes, {len(samples) / RATE:.3f} s, peak {db(peak(full)):.1f} dBFS, "
          f"RMS {db(rms(full)):.1f} dBFS; {edge}; first and last 10 ms at "
          f"{db(rms(full[:ten])):.1f} and {db(rms(full[-ten:])):.1f} dBFS RMS")


# Each sound: its path, how it is made, whether it loops, and the loudest its
# peak may be, in dBFS.
SOUNDS = [
    ("audio/sfx/step.wav", step, False, -3),
    ("audio/ambience/pond.wav", pond, True, -6),
    ("audio/music/north.wav", north, True, -3),
    ("audio/music/south.wav", south, True, -3),
]


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    root = sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "..", "assets")
    for path, make, looped, most in SOUNDS:
        signal, gain = make()
        samples = to_i16(signal, gain)
        loudest = db(max(abs(s) for s in samples) / 32768)
        if loudest > most:
            sys.exit(f"{path}: its peak, {loudest:.1f} dBFS, is above {most} dBFS")
        # The cook warns of a sound that does not loop and ends above
        # 128, −48 dBFS, which clicks as it stops.
        if not looped and (samples[-1] != 0 or any(abs(s) > 128 for s in samples[-RATE // 500:])):
            sys.exit(f"{path}: it does not end at 0, as a sound that does not loop must")
        file = wav(samples)
        target = os.path.join(root, path)
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with open(target, "wb") as f:
            f.write(file)
        report(path, samples, len(file), looped)
    with open(os.path.join(root, "audio/ambience/pond.sound.jsonc"), "w") as f:
        f.write(SIDECAR)


if __name__ == "__main__":
    main()
