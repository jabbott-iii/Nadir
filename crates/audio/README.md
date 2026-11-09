# nadir-audio

Audio for the Nadir engine: a software mixer of voices, buses and music,
which runs on the platform's audio thread, the game's handle on it, and the
sound and music assets it plays, loaded by
[nadir-assets](https://crates.io/crates/nadir-assets).

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Depend on it directly to use it on its own, as below;
the engine's facade, [nadir_engine](https://crates.io/crates/nadir_engine),
plays a game's sound through it from roadmap task M6.6.

## Status

Version 0.3: the API will change before 1.0.

Built:

- `nadir_audio::new`, which makes the two sides: `Audio`, on the main
  thread, whose every call queues a command, and `Mixer`, for the audio
  thread, whose `render` carries the commands out and mixes. The audio
  thread never waits for the main thread, and never allocates, frees or
  logs: `Audio::update`, once a frame, drops what the mixer is done with.
- `Sound` and `Music`, nadir-assets assets: a sound loads from the cook's
  `.kpcm` or `.qoa` files, decoded on a worker; one at 8,000 to 384,000 Hz
  that ends in quiet, as a sound that does not loop does, is converted to
  `LOAD_RATE`, the output's 48 kHz, and any other keeps its rate, for the
  mixer to resample as it plays. A track loads from a `.qoa` file, kept
  compressed and decoded a frame at a time as it plays. With the server's
  placeholders on, a failed sound plays as silence, and a failed track
  fades the music out.
- `Audio::play` and `play_at` play the sound under a handle, looked up in
  the `AssetServer`, on one of four buses, with a volume, pitch, pan, loop,
  priority and most instances at once; at a position, a sound is panned
  and attenuated from the listener. `play_music` crossfades from one track
  to another, looped. `play_sound`, `play_sound_at` and `play_track` play a
  `Sound` or `Music` as it is, such as a sound made in code.
- 48 voices by default (`AudioConfig::voices`), voices stolen by priority, a
  ramp of 2 ms at least at every start, stop and change of a bus's volume,
  and a soft limiter above −1 dBFS. 32 voices and a track take about
  88 µs of a 10.7 ms callback.

## Usage

```toml
[dependencies]
nadir-audio = "0.3"
nadir-assets = "0.3"
nadir-core = "0.3"
# For the examples: files written as the cook writes them, and the output.
nadir-codec = "0.3"
nadir-platform = "0.3"
```

`nadir-assets` loads the sounds; `nadir-core` has the `Vec2` of a position.
Load a sound and a track, play the sound to the listener's right and the
track behind it, and mix 10 ms, as the audio thread would:

```rust
use std::collections::HashMap;

use nadir_assets::{AssetError, AssetPath, AssetServer, FinalizeCtx, OnFailure, Source};
use nadir_audio::{AudioConfig, Music, PlayParams, Sound};
use nadir_codec::{kpcm, qoa};
use nadir_core::Vec2;

/// The game's files, in memory: a game reads them from the pack.
struct Memory(HashMap<&'static str, Vec<u8>>);

impl Source for Memory {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
        self.0.get(path.as_str()).cloned().ok_or(AssetError::NotFound)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A 440 Hz step, 16-bit, and a second of a quiet track, in QOA.
    let wave = |frames: usize, level: f32| -> Vec<i16> {
        (0..frames)
            .map(|i| ((i as f32 / 48_000.0 * 440.0 * std::f32::consts::TAU).sin() * level) as i16)
            .collect()
    };
    let step = kpcm::Header { channels: 1, sample_rate: 48_000, frames: 4_800 };
    let theme = qoa::Header { channels: 1, sample_rate: 48_000, samples: 48_000 };
    let files = HashMap::from([
        ("audio/sfx/step.kpcm", kpcm::encode(&step, &wave(4_800, 16_000.0))?),
        ("audio/music/theme.qoa", qoa::encode(&theme, &wave(48_000, 2_000.0))?),
    ]);
    let mut assets = AssetServer::with_source(Memory(files), OnFailure::Nothing);
    let step = assets.load::<Sound>("audio/sfx/step.kpcm");
    let theme = assets.load::<Music>("audio/music/theme.qoa");
    // A game finishes loads once a frame; this waits for them.
    assets.finish_all_loads(&mut FinalizeCtx::new());

    let (audio, mut mixer) = nadir_audio::new(AudioConfig::default())?;
    audio.set_listener(Vec2::new(0.0, 0.0));
    audio.play_at(&assets, step, Vec2::new(200.0, 0.0), PlayParams::default());
    audio.play_music(&assets, theme, 500);

    // On the audio thread: 10 ms of stereo at 48 kHz.
    let mut out = vec![0.0; 480 * 2];
    mixer.render(&mut out, 2, 48_000);
    let power = |channel: usize| -> f32 { out.iter().skip(channel).step_by(2).map(|x| x * x).sum() };
    assert!(power(1) > power(0), "the step is to the right");

    // Once a frame, on the main thread.
    audio.update();
    Ok(())
}
```

A handle with nothing under it, a sound still loading, failed with no
placeholder, or unloaded, plays nothing, and `AudioStats::unplayed` counts
it: load what must play at once with `AssetServer::load_now`. A sound that
fails to load is logged with its path, and `AssetServer::error` says why.

To hear it, give the `Mixer` to the platform's audio output, which calls it
on the audio thread:

```rust,no_run
use nadir_audio::{AudioConfig, Mixer};
use nadir_platform::{AudioOutputConfig, AudioRender, open_audio};

/// The mixer, as the output's render.
struct Output(Mixer);

impl AudioRender for Output {
    fn render(&mut self, out: &mut [f32], channels: u16, sample_rate: u32) {
        self.0.render(out, usize::from(channels), sample_rate);
    }

    fn underrun(&mut self) {
        self.0.count_underrun();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (audio, mixer) = nadir_audio::new(AudioConfig::default())?;
    // 48 kHz stereo, the rate sounds load at, unless the config says
    // otherwise.
    let _stream = open_audio(&AudioOutputConfig::default(), Box::new(Output(mixer)))?;
    // ... play sounds through `audio`, and call `audio.update()` once a
    // frame, while the stream lives.
    audio.update();
    Ok(())
}
```

## Platforms

The mixer is the same on every system: it renders into a buffer, at any
rate and channel count the device asks for. Hearing it takes an output,
such as nadir-platform's `open_audio`; see
[nadir-platform](https://crates.io/crates/nadir-platform) for the systems
it plays on.

## Documentation

- API reference: https://docs.rs/nadir-audio
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/audio.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
