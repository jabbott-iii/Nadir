# Subsystem: `nadir-audio`

A software mixer that runs on the platform audio thread, plus a game-facing
API on the main thread. The OS device code is in `nadir-platform`; this
crate is pure Rust with no `unsafe`.

## Architecture

```
main thread                                   audio thread (platform)
───────────                                   ───────────────────────
Audio (handle) ──► CommandQueue ──try_lock──► Mixer::render(out, ch, rate)
   ▲                                             │ voices → buses → master → limiter
   └──────── ReturnQueue (Arc<SoundData> to drop) ◄┘
```

- **`CommandQueue`:** a `Mutex<VecDeque<Command>>` with capacity reserved up
  front, which **never grows** (M6.2, ADR-0046).
  - The audio thread uses `try_lock` and **never blocks**. If the lock is
    contended, it processes commands on the next callback (a few ms later).
    A lock that a panic poisoned is taken as it is, on both sides.
  - The main thread holds the lock only to push, and the audio thread
    only to take each command, before it carries it out. Past
    `AudioConfig::commands` (1,024), a command is dropped there and
    counted in `AudioStats::dropped`; changes to a voice and the
    listener's moves are dropped once it is half full, as the next frame
    sends them again. With the mixer gone, nothing is queued.
  - If profiling shows contention, replace it with a lock-free SPSC ring.
    That would be the only `unsafe` candidate here, so it would move into a
    tiny module in `platform`.
- **`ReturnQueue`:** when a voice finishes, its `Arc<SoundData>` goes back to
  the main thread to be dropped there. **The audio thread never frees
  memory.**
  - What the mixer is done with waits in a buffer of fixed capacity,
    `2 · (voices + 2) + commands`, then goes to the return queue, of the
    same capacity, when the audio thread can lock it. A command that
    brings a sound waits, with those after it, while the sounds the mixer
    holds and those waiting would pass that capacity, so a return always
    has room.
  - `Audio::update`, once a frame, swaps the return queue with a spare of
    the same capacity and drops what it held, outside the lock.
- **No allocation in `render`.**
  - The voice array (default 48 slots), the two music buffers and the
    scratch buffers are allocated at init. A command still queued when
    the mixer is dropped is dropped by the next `Audio::update`. The mix is made in parts of 512
    frames, so a callback of any size allocates nothing.
  - The audio thread never logs either: it counts, and `Audio::update`
    logs, once a second at most.
  - The dev allocation counter asserts this in a test
    (`crates/audio/tests/allocations.rs`).

## Game-facing API

Built in M6.2 as below, but taking `&Arc<SoundData>` and `&Arc<MusicData>`
where it takes handles here: M6.6 makes the `Sound` and `Music` assets,
and the handles (ADR-0046).

```rust
pub fn new(config: AudioConfig) -> Result<(Audio, Mixer), AudioError>;

impl Audio {
    pub fn play(&self, sound: &Handle<Sound>, params: PlayParams) -> VoiceId;
    pub fn play_at(&self, sound: &Handle<Sound>, pos: Vec2, params: PlayParams) -> VoiceId;
    pub fn stop(&self, v: VoiceId, fade_ms: u32);
    pub fn set_voice(&self, v: VoiceId, change: VoiceChange);     // volume, pitch, pan, position
    pub fn set_bus_volume(&self, bus: Bus, volume: f32, fade_ms: u32);
    pub fn set_listener(&self, pos: Vec2);
    pub fn play_music(&self, track: &Handle<Music>, crossfade_ms: u32);
    pub fn stop_music(&self, fade_ms: u32);
    pub fn update(&self);                                         // once a frame: drops returns, logs
    pub fn stats(&self) -> AudioStats;                            // active voices, underruns, callback time, rate, dropped
}

impl Mixer {                                                      // the audio thread's
    pub fn render(&mut self, out: &mut [f32], channels: usize, rate: u32);
    pub fn count_underrun(&self);
}

pub struct PlayParams { pub bus: Bus, pub volume: f32, pub pitch: f32, pub pan: f32,
                        pub looped: bool, pub priority: u8, pub max_instances: u8 }
pub enum Bus { Music, Sfx, Ui, Ambience }   // all feed Master
pub enum VoiceChange { Volume(f32), Pitch(f32), Pan(f32), Position(Vec2) }
```

- `PlayParams::default()`: `Sfx`, volume 1, pitch 1, pan 0, not looped,
  priority 128, `max_instances` 4 (0 counts as 1).
- Volumes are clamped to 0 … 16, pitch to 1/16 … 16, pan to −1 … 1;
  NaN gives 0 (pitch 1). A position that is not finite makes its voice
  silent; a listener that is not finite is ignored.
- `SoundData::new(channels, sample_rate, samples)` takes 1 or 2 channels,
  1 to 384,000 Hz, at most 2³¹ frames, and samples within −1 … 1. `MusicData::new(bytes)`
  takes a QOA file of 1 or 2 channels at those rates, and decodes every
  frame once, so a track cannot fail as it plays.

## Mixer details

- **Format:** interleaved `f32` stereo at the device rate (normally 48 kHz).
  Sounds are converted to the device rate **at load time**, on a worker, so
  most voices play without resampling.
- **Pitch:** a linear-interpolation resampler with a 32.32 fixed-point
  position. Good enough for SFX pitch variance (±20%).
- **Pan:** constant-power for a mono sound: `l = cos(θ)`, `r = sin(θ)`,
  with `θ = (pan + 1) · π/4`. A stereo sound is balanced:
  `√2 · (cos θ, sin θ)` per channel, capped at 1, so the centre and the
  near side stay at unity and no gain lifts a stereo sound past full scale
  (ADR-0046). Music is not panned: a mono track plays on both sides as it
  is.
- **Positional sound** (2D top-down):
  - pan comes from the horizontal offset to the listener, clamped by
    `pan_width`
  - attenuation is linear between `min_dist` and `max_dist`, then silent
  - positions update each frame from the facade (entity transforms). A
    voice's gains move to a change of position, volume or pan over 16 ms,
    about a tick, whatever the callback's length, to avoid zipper noise
    (ADR-0046: one callback can be 64 frames)
- **Voice limits:**
  - global 48
  - per-sound `max_instances` (default 4)
  - when full, steal a voice fading out already, whatever its priority, or
    else the lowest-priority, then oldest, voice of a priority no higher
    than the new one's; at a sound's `max_instances`, only among that
    sound's voices. If there is none, the new voice does not play.
  - The stolen voice fades out over 5 ms in its slot while the new one
    plays from its start: for those 5 ms the slot sounds both, and is not
    stolen from.
- **Fades:** per-sample linear ramps on volume changes, so there are no
  clicks. Every start and stop, and every change of a bus's volume, has at
  least a 2 ms ramp; a stop of a voice already fading out keeps the
  sooner end. A sound that ends on its own plays to its last sample, with
  no ramp: one would silence sounds shorter than two ramps, so a sound's
  data should end at 0 (M6.6's cook can see to it).
- **Master limiter:** a cheap soft clip (`x / (1 + |x|)`-shaped curve above
  −1 dBFS). It's never hit in normal mixes; it's there to protect speakers
  on stacking. Its output stays below 1 for any input, and NaN gives 0.
- **Music:**
  - the QOA track is loaded fully into memory compressed (3.2 bits per
    sample: about 307 kbps for 48 kHz stereo, so roughly 6.9 MB for a
    3-minute track)
  - it's decoded incrementally on the audio thread, one QOA frame at a time,
    into a small per-voice buffer
  - there is no file I/O on the audio thread
  - crossfading uses two music voices; tracks loop
  - a track still sounding in either voice, fading out or not, fades back
    in from where it is when it is played again, rather than starting
    over; a new track takes a free voice, or the quieter one after a 2 ms
    fade, as the buffer holds one track at a time
  - `stop_music` fades every track out within its fade

## Sound assets

| Asset | Source | Cooked | Runtime representation |
| :- | :- | :- | :- |
| `Sound` (SFX) | WAV PCM 16/24-bit or float, mono/stereo | PCM16 (`.kpcm`) or QOA, chosen per asset in cook config | `Arc<SoundData>`: `f32` interleaved at device rate |
| `Music` | WAV | QOA (`.qoa`) | `Arc<[u8]>` compressed + per-voice streaming decoder state |

## Device events

- When the platform reopens a device (default-device change on Windows,
  ALSA recovery), the mixer keeps all state. Only the output rate may
  change; voices then resample.
- Underruns are counted and exposed in `AudioStats`, and logged once per
  second at most.

## Tests

- **Offline rendering:** feed commands and render N frames into a buffer,
  then compare with expected sums (`crates/audio/tests/mixer.rs`, M6.2).
  - constant-power pan law: power at every pan, and each pan's angle;
    stereo balanced at unity and never above
  - fades are ramp-shaped: in over 2 ms, out over the stop's fade, a bus's
    over its own, a crossfade's
  - voice stealing order: priority, then age; one fading out first; a
    refusal; a sound's `max_instances`
  - bus volumes multiply
  - a looped sound has no click at the loop point (no discontinuity above a
    threshold): a cosine looped at pitch 1.37, against the cosine itself
  - positional pan and attenuation, and a move smoothed over 16 ms with
    callbacks of 16, 100 and 4,096 frames
  - music: the samples QOA decodes, across frames and the loop, mono and
    stereo; resampled from 44.1 kHz, against the interpolation computed in
    the test; a crossfade back to a track still fading out
- **No allocation, no free:** `crates/audio/tests/allocations.rs` counts
  every `render` from a new mixer's first, through steals, refusals,
  music frames, loops and crossfades, a change of rate and a main thread
  that does not call `update` for a while.
- **Property:** random sequences of every call, with NaN, infinities,
  `f32::MAX` and `u32::MAX` among their values: the output stays finite and
  below 1, and every sound comes back once all is stopped
  (`crates/audio/tests/property.rs`).
- **QOA:** encode/decode round-trip SNR ≥ the threshold the spec's reference
  results suggest. Set it at implementation time from measured values and
  record it in the test. M6.1 measured seven signals and asserts each, from
  13 dB for plucks of noise to 71 dB for a tone
  (`crates/codec/tests/qoa.rs`).
- **Resampler:** a sine at 44.1 kHz resampled to 48 kHz keeps its frequency
  (zero-crossing count) and amplitude (±0.5 dB), and stays within linear
  interpolation's error of the sine itself.
- **Bench:** `mixer_32_voices` (`cargo xtask bench mixer`), a callback of
  512 frames with 32 voices and a track.
- **Soak** (`xtask soak --audio`, manual or nightly on hardware): 10 minutes
  of 32 random voices plus music, with zero underruns on min-spec.
