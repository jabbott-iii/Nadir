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
   └── ReturnQueue (Arc<SoundData>, Arc<MusicData> to drop) ◄┘
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
  - The mixer never logs either: it counts, and `Audio::update` logs, once
    a second at most. The platform's audio thread logs only when the device
    is lost and when it opens again (ADR-0047).
  - The dev allocation counter asserts this in a test
    (`crates/audio/tests/allocations.rs`).

## Game-facing API

Built in M6.2 (ADR-0046), with the handles of M6.9 (ADR-0050): `play`,
`play_at` and `play_music` look a handle up in the `AssetServer` they are
given, on the main thread, as the renderer looks up its textures; the
calls that take the asset itself play a sound or track as it is, such as
one made in code.

```rust
pub fn new(config: AudioConfig) -> Result<(Audio, Mixer), AudioError>;

impl Audio {
    pub fn play(&self, assets: &AssetServer, sound: Handle<Sound>, params: PlayParams) -> VoiceId;
    pub fn play_at(&self, assets: &AssetServer, sound: Handle<Sound>, pos: Vec2, params: PlayParams) -> VoiceId;
    pub fn play_sound(&self, sound: &Sound, params: PlayParams) -> VoiceId;
    pub fn play_sound_at(&self, sound: &Sound, pos: Vec2, params: PlayParams) -> VoiceId;
    pub fn stop(&self, v: VoiceId, fade_ms: u32);
    pub fn set_voice(&self, v: VoiceId, change: VoiceChange);     // volume, pitch, pan, position
    pub fn set_bus_volume(&self, bus: Bus, volume: f32, fade_ms: u32);
    pub fn set_listener(&self, pos: Vec2);
    pub fn play_music(&self, assets: &AssetServer, track: Handle<Music>, crossfade_ms: u32);
    pub fn play_track(&self, track: &Music, crossfade_ms: u32);
    pub fn stop_music(&self, fade_ms: u32);
    pub fn update(&self);                                         // once a frame: drops returns, logs
    pub fn stats(&self) -> AudioStats;                            // active voices, underruns, callback time, rate, dropped, unplayed
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
  frame once, so a track cannot fail as it plays. `Sound::new` and
  `Music::new` make assets of them, as they are.
- **A handle with nothing under it**, a sound or track still loading,
  failed with no placeholder, or unloaded, plays nothing: `play` and
  `play_at` return a `VoiceId` that names no voice, `play_music` leaves
  the music as it is, and `AudioStats::unplayed` counts each. A one-off
  play of a sound still loading is lost: load what must sound at once
  with `load_now`. A handle names an asset of its own server: another
  server's may name another sound. Unloading a scope does not stop the
  voices that play its sounds: the mixer holds them until they end.

## Mixer details

- **Format:** interleaved `f32` stereo at the device rate (normally 48 kHz).
  The cook resamples sounds and music to 48 kHz unless their sidecars say
  otherwise, with a windowed sinc (M6.8, ADR-0049); a sound at another
  rate, from 8,000 to 384,000 Hz, that ends in quiet (its last 2 ms within
  −48 dBFS, as the cook asks of a sound that does not loop) is converted
  to `LOAD_RATE`, 48 kHz, the output's default, **at load time**, on a
  worker, by the same sinc, as a sound that does not loop (M6.9,
  ADR-0050), so most voices play without resampling. A sound that ends
  louder may be a loop, which that would break at its seam: it keeps its
  rate, as does a sound at another rate than those, and the mixer
  resamples it as it plays, wrapping its seam; so it does for a sound
  played on an output at another rate than 48 kHz, and for music, which
  streams.
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
  - positions come from the facade (entity transforms), when they move. A
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
  data should end at 0. The cook does not change it, as a fade would break
  a loop's seam, but warns of a sound that does not loop and ends above
  −48 dBFS (M6.8).
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
| `Sound` (SFX) | WAV PCM 16/24-bit or float, mono/stereo, below `audio/` | The sound file (`.kpcm`, PCM16) or QOA, by its sidecar, or by its folder (QOA below `audio/music/`), at 48 kHz unless the sidecar gives a rate (M6.8) | `Arc<SoundData>`: `f32` interleaved, at 48 kHz (`LOAD_RATE`) if it loaded from 8,000 to 384,000 Hz, else at its own rate; 4 bytes a sample, about 66 MiB for three minutes of stereo |
| `Music` | WAV, below `audio/` (`audio/music/` by default) | QOA (`.qoa`), looped | `Arc<MusicData>`: the QOA file, compressed, about 6.9 MB for three minutes of 48 kHz stereo, decoded a frame at a time as it plays |

- **Loading** (`nadir_assets::Asset`, M6.9, ADR-0050): on a worker, a
  QOA's header is checked first (1 or 2 channels, 1 to 384,000 Hz), a
  sound's samples are decoded within half the server's limits, so that
  their `f32` copy is held to them, then converted; a track's file is held
  to the limits before it is copied, and every frame is decoded once.
  Converted samples past full scale are clipped, with a warning naming the
  file. Converting takes about 0.4 s for 10 s of 44.1 kHz mono optimized,
  8 to 14 times that unoptimized, and up to about a minute for a file at
  the limits. The finalize needs no resource, so `load_now` works with an
  empty `FinalizeCtx`, on the caller's thread, which then decodes and
  converts. QOA's error rings into the silence after a sound, so a QOA
  sound converts only if it fades out over a few milliseconds more.
- **Placeholders** (`OnFailure::Placeholder`, the facade's `dev`): a failed
  sound is a frame of silence, mono, at 48 kHz, which plays as silence; a
  failed track, a QOA frame of silence, which `play_music` fades to.
- **The same track:** two tracks of the same file's bytes, loaded apart, as
  a level unloads its scope and loads its music again, are the same track:
  `MusicData` keeps a hash of the bytes, and `play_music` of one while the
  other plays goes on. A sound's `max_instances` counts the voices of one
  load (one `Arc`).

## Device events

- When the platform reopens a device (default-device change on Windows,
  ALSA recovery), the mixer keeps all state. Only the output rate may
  change; voices then resample.
- While there is no device, lost or not yet there, `nadir_platform::open_audio`'s
  thread goes on calling the mixer in time, a period at a time, and tries
  the device every second (M6.3, ADR-0047): voices end, music moves on,
  and sounds come back to the main thread as they would with one.
- Underruns are counted and exposed in `AudioStats`, and logged once per
  second at most. The platform's `AudioRender::underrun` reports each, on
  the audio thread, and the facade's wrapper of the mixer passes it to
  `Mixer::count_underrun` (M6.6).

## The game's sound: the facade

Built in M6.6 (ADR-0051). nadir-audio knows nothing of the ECS
(ADR-0014): the facade, `nadir`, opens the output, defines the components
and plays them, and re-exports the crate as `nadir::audio`.

- **Opening:** `App::new` makes the mixer of `AudioConfig::default()`,
  inserts `Audio` and `AudioState` as resources, and sets each bus to
  `Config::volumes`. `App::run` opens the default output, 48 kHz stereo,
  with the mixer, once the window and the GPU device are open, and closes
  it as soon as the loop ends, before an error is shown (2 s at most).
  Where it does not open, on Windows and macOS until M6.4 and M6.5, or on
  Linux without `libasound.so.2`, a warning says why and the game runs
  silent. A missing device is no error: the output plays on without one
  and opens it when it comes (M6.3).
- **What is silent:** `App::update`'s first call drops a mixer the app
  still holds. So tests, benchmarks, `--headless` replays,
  `App::offscreen` and the sandbox's `--capture` open no device, and
  `Audio` does nothing there; only `AudioStats::unplayed` still counts.
  `App::take_mixer` takes the mixer before then, for a test or a tool
  that renders it itself, as the facade's tests do; `App::run` then opens
  no output, and logs why.
- **Each frame, `nadir::sound`**, the engine's, first in frame `Extract`,
  after the game's camera:
  1. puts the listener where the first entity with an `AudioListener` and
     a `Transform` is drawn, or else at the camera,
     `RenderFrame::camera.position`;
  2. starts each `AudioEmitter` whose entity has a `Transform`, once its
     sound has loaded and it is within `max_dist` (800 px) of the
     listener, and sends its position; stops it, over 100 ms, more than
     64 px past that, or when its entity is despawned or loses the emitter
     or the `Transform`; and starts it again back in range;
  3. sends the track of the `MusicArea` the listener is in, a box, its
     entity's position ± `half`, which crossfades over `crossfade_ms`;
  4. calls `Audio::update`.

  Positions are where the entities are drawn, between ticks, as sprites
  are: `PrevTransform.lerp(Transform, alpha)`, or the `Transform`. The
  listener and each emitter that plays are placed when they are not where
  the queue last took them, so a frame where nothing moves sends nothing,
  and a move the half-full queue dropped is sent again the next frame; so
  is a play, a stop, a change or a track it dropped. Once its table and its
  marker's storage hold the emitters, it allocates nothing; it writes no
  reflected component and spawns nothing, so world hashes are as they
  were.
- **The components**, not reflected: a game gives them in code, not in
  prefabs or levels.
  - `AudioListener`: where the game hears from, in place of the camera.
  - `AudioEmitter { sound, params }`: `new(sound, params)`, or
    `looped(sound)`, for a sound of the place, looped on `Bus::Ambience`
    at priority 192, with no limit to its voices, so that the sound's
    limit does not make its emitters take one another's voices; when all
    the mixer's voices play, a play of the same priority or higher still
    takes the oldest. A new `sound`, `bus`, `looped`,
    `priority` or `max_instances` restarts it; a new `volume` or `pitch`
    reaches the voice as it plays; its `pan` is not used. A voice stolen
    for another stays silent until the emitter starts again: a priority
    protects it only from plays of a lower one.
  - `MusicArea { music, half, crossfade_ms }`: `new(music, half)`
    crossfades over 1,500 ms. Where areas overlap, the one whose track
    plays stays while the listener is in it, and otherwise the first that
    holds it plays. Outside every area the music plays on; coming back to
    the area whose track plays, it goes on as it is (ADR-0046).
    `AudioState::music_area()` names the area whose track was sent last.
  - A sound or track still loading waits; one that failed, or that the
    asset server does not hold, is warned of once, with the entity and
    its path, and not tried again until its component changes. With
    `dev`'s placeholders, a failed sound or track plays its placeholder,
    silence, instead, and the asset server logs the failure.
- **Volumes:** `Config::volumes` holds each bus's percent, 0 to 100, 100
  by default, more counting as 100. A percent is heard as its square,
  `Volumes::gain(bus)`, so 50 is −12 dB. A game changes one while it runs
  with `audio.set_bus_volume(bus, volumes.gain(bus), fade)`; M9.3's
  settings file will fill them. There is no master volume.
- **A sound played once**, as something happens, a footstep say, plays
  only if it has loaded: load it first with `load_now` (M6.9).
- **Not built yet:** a level's own `music` and `ambience`, which
  `load_level` does not play, as the cook writes neither (the roadmap's
  backlog, ADR-0051); the components in prefabs and levels; an emitter
  that comes back on its own when its voice is stolen; output on Windows
  and macOS (M6.4, M6.5); the settings file (M9.3).

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
  - each played from a handle, of a sound or track written as the cook
    writes it and loaded through an `AssetServer` in memory (M6.9)
- **Assets** (`crates/audio/tests/assets.rs`, M6.9): `.kpcm` and `.qoa`
  sounds and music loaded on the server's workers play as the same samples
  made in code; a sound at another rate that ends in quiet converts as
  `pcm::resample` does, from `.kpcm` and from QOA, one at 48 kHz or below
  8,000 Hz keeps its samples, and seamless loops that end loud keep their
  rate and step across their seams no more than elsewhere; clipping is
  clamped and told; what the mixer cannot play is refused, QOA's channels
  from its header; the limits at 4 bytes a sample, and a track's at its
  file; placeholders, one per failure; a handle with nothing under it
  plays nothing, is counted, and names no voice; the same track loaded
  again, playing or waiting to start, goes on, and the second load comes
  back; a reloaded sound plays its new samples from its next play.
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
- **Resampler:** a sine at 48 kHz played at 44.1 kHz keeps its frequency
  (zero-crossing count) and amplitude (±0.5 dB), and stays within linear
  interpolation's error of the sine itself: the mixer resamples a loaded
  sound only when the output's rate is not its own (M6.9).
- **The facade** (`crates/engine/src/sound.rs`, M6.6): with the mixer
  taken and rendered offline at 48 kHz stereo, and sounds and tracks the
  test writes as the cook does into an asset server in memory, so that
  `dev` changes nothing: `MixerOutput` renders the mixer and counts its
  underruns; the first `update` drops a mixer not taken, and `Audio` then
  queues nothing; the listener at the camera, at an `AudioListener`, and
  between ticks; an emitter waits for its sound, follows its entity,
  takes a new volume as it plays, restarts for a new sound, stops when it
  goes or is far and plays again when near; five looped emitters of one
  sound all sound; the crossfade into another area, by its level at 0, ½
  and 1 of its length; the music plays on outside every area, and where
  areas overlap changes only when the listener leaves the one playing; a
  play, a move and a track that a full queue dropped are sent again, and
  a frame where nothing moves sends nothing; the area playing plays on
  when its music changes to a track that did not load, and is named no
  more once despawned; a sound played to its end is given back the frame
  after; the configured volumes scale what is heard; a sound or track
  that did not load is warned of once, until it changes, and an emitter
  warned of is not warned of again for losing its `Transform`; a looped
  emitter's parameters. With `dev`, a steady frame of
  40 emitters moving in and out of range, a listener and two areas
  allocates nothing (`docs/04-quality.md`).
- **The sandbox** (M6.6): its four sounds load from the loose files and
  from the pack; the player's footsteps, one at the first tick it moves
  and then every 12 ticks, panned to its side of a listener beside its
  path, and none once it stops; in `stress-sprites`, with the camera as
  the listener, the steps pan to the side of the view the player is on;
  the capture's tour crosses from the north's music to the south's,
  heard.
- **By ear** (Linux, headphones; the user's check, as no test plays
  sound): `cargo xtask cook`, then `cargo dev`. The north's tune fades in
  over 1.5 s, and the pond laps from the right. Walking east along the
  path, the steps thud twice a cycle, none when stopped or against a
  wall, and the pond moves from the right through the centre to the left,
  fading with distance. Standing for 10 s, no loop clicks. Walking south
  through the gap at column 50 (y 576), the music crossfades to the
  south's over 1.5 s, and back. The report still says 0 allocations, with
  no warning of underruns, and closing quits within 2 s. Then `cargo dev
  --scene stress-sprites`: walking left and right, the steps pan to the
  person's side; and `cargo run -p sandbox`, the pack, the same.
- **Bench:** `mixer_32_voices` (`cargo xtask bench mixer`), a callback of
  512 frames with 32 voices and a track.
- **Soak** (`cargo xtask soak --audio [--minutes N] [--silent]`, M6.7,
  ADR-0052; manual or nightly on hardware): 10 minutes, by default, of 32
  random voices plus music through the default device, with zero
  underruns and no late callbacks on min-spec. `docs/04-quality.md`
  ("Soak") has the method. It shows the device's buffer, not the latency
  from trigger to speaker, which the M6 exit criteria ask to be measured
  apart.
