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
  front.
  - The audio thread uses `try_lock` and **never blocks**. If the lock is
    contended, it processes commands on the next callback (a few ms later).
  - The main thread holds the lock only to push.
  - If profiling shows contention, replace it with a lock-free SPSC ring.
    That would be the only `unsafe` candidate here, so it would move into a
    tiny module in `platform`.
- **`ReturnQueue`:** when a voice finishes, its `Arc<SoundData>` goes back to
  the main thread to be dropped there. **The audio thread never frees
  memory.**
- **No allocation in `render`.**
  - The voice array (default 48 slots) and scratch buffers are allocated at
    init, sized for the largest callback the device reports, ×2.
  - The dev allocation counter asserts this in a test.

## Game-facing API

```rust
impl Audio {
    pub fn play(&self, sound: &Handle<Sound>, params: PlayParams) -> VoiceId;
    pub fn play_at(&self, sound: &Handle<Sound>, pos: Vec2, params: PlayParams) -> VoiceId;
    pub fn stop(&self, v: VoiceId, fade_ms: u32);
    pub fn set_voice(&self, v: VoiceId, change: VoiceChange);     // volume, pitch, pan, position
    pub fn set_bus_volume(&self, bus: Bus, volume: f32, fade_ms: u32);
    pub fn set_listener(&self, pos: Vec2);
    pub fn play_music(&self, track: &Handle<Music>, crossfade_ms: u32);
    pub fn stats(&self) -> AudioStats;                            // active voices, underruns, callback time
}

pub struct PlayParams { pub bus: Bus, pub volume: f32, pub pitch: f32, pub pan: f32,
                        pub looped: bool, pub priority: u8, pub max_instances: u8 }
pub enum Bus { Music, Sfx, Ui, Ambience }   // all feed Master
```

## Mixer details

- **Format:** interleaved `f32` stereo at the device rate (normally 48 kHz).
  Sounds are converted to the device rate **at load time**, on a worker, so
  most voices play without resampling.
- **Pitch:** a linear-interpolation resampler with a 32.32 fixed-point
  position. Good enough for SFX pitch variance (±20%).
- **Pan:** constant-power: `l = cos(θ)`, `r = sin(θ)`, with
  `θ = (pan + 1) · π/4`.
- **Positional sound** (2D top-down):
  - pan comes from the horizontal offset to the listener, clamped by
    `pan_width`
  - attenuation is linear between `min_dist` and `max_dist`, then silent
  - positions update each frame from the facade (entity transforms) and are
    smoothed per callback to avoid zipper noise
- **Voice limits:**
  - global 48
  - per-sound `max_instances` (default 4)
  - when full, steal the lowest-priority, then oldest, voice, with a 5 ms
    fade
- **Fades:** per-sample linear ramps on volume changes, so there are no
  clicks. Every start and stop has at least a 2 ms ramp.
- **Master limiter:** a cheap soft clip (`x / (1 + |x|)`-shaped curve above
  −1 dBFS). It's never hit in normal mixes; it's there to protect speakers
  on stacking.
- **Music:**
  - the QOA track is loaded fully into memory compressed (3.2 bits per
    sample: about 307 kbps for 48 kHz stereo, so roughly 6.9 MB for a
    3-minute track)
  - it's decoded incrementally on the audio thread, one QOA frame at a time,
    into a small per-voice buffer
  - there is no file I/O on the audio thread
  - crossfading uses two music voices

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
  then compare with expected sums.
  - constant-power pan law
  - fades are ramp-shaped
  - voice stealing order
  - bus volumes multiply
  - a looped sound has no click at the loop point (no discontinuity above a
    threshold)
- **QOA:** encode/decode round-trip SNR ≥ the threshold the spec's reference
  results suggest. Set it at implementation time from measured values and
  record it in the test.
- **Resampler:** a sine at 44.1 kHz resampled to 48 kHz keeps its frequency
  (zero-crossing count) and amplitude (±0.5 dB).
- **Soak** (`xtask soak --audio`, manual or nightly on hardware): 10 minutes
  of 32 random voices plus music, with zero underruns on min-spec.
