// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Audio for the Nadir engine: a software mixer of voices, buses and music,
//! which runs on the platform's audio thread, the game's handle on it, and
//! the sound and music assets it plays.
//!
//! [`new`] makes the two sides. [`Audio`] stays on the main thread: each of
//! its calls queues a command. [`Mixer`] goes to the audio thread, which
//! calls [`Mixer::render`] for each buffer the device asks for; it carries
//! out the commands, then mixes. The audio thread never waits for the main
//! thread, and never allocates, frees or logs: what the mixer is done with
//! goes back to the main thread, which drops it in [`Audio::update`].
//! The design is in
//! [`docs/subsystems/audio.md`](https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/audio.md).
//!
//! # Contents
//!
//! - **Sounds**, [`Sound`]: samples of 1 or 2 channels, in memory. As a
//!   nadir-assets asset, a sound loads from the cook's `.kpcm` or `.qoa`
//!   files, on a worker; one at 8,000 to 384,000 Hz that ends in quiet, as
//!   a sound that does not loop does, is converted to [`LOAD_RATE`], the
//!   output's 48 kHz.
//!   [`Audio::play`] plays the sound under a handle, looked up in the
//!   `AssetServer`, on a [`Bus`], as its [`PlayParams`] say;
//!   [`Audio::play_at`] plays one at a position in the world, which pans it
//!   and attenuates it from where the listener is
//!   ([`Audio::set_listener`]). [`Audio::set_voice`] changes a voice as it
//!   plays, and [`Audio::stop`] fades it out. [`Sound::new`] makes a sound
//!   in code, of a [`SoundData`], which [`Audio::play_sound`] and
//!   [`Audio::play_sound_at`] play as it is.
//! - **Music**, [`Music`]: a QOA file of 1 or 2 channels, kept compressed
//!   and decoded a frame at a time as it plays, looped. As an asset, it
//!   loads from the cook's `.qoa` files. [`Audio::play_music`] crossfades
//!   from one track to another; [`Audio::play_track`] plays a [`Music`] as
//!   it is.
//! - **Buses**: music, sound effects, the interface and ambience, each with
//!   a volume ([`Audio::set_bus_volume`]), all feeding the master.
//!
//! A handle with nothing under it, a sound still loading, failed with no
//! placeholder, or unloaded, plays nothing ([`AudioStats::unplayed`]
//! counts it). With the server's placeholders on, a failed sound plays as
//! silence, and a failed track fades the music out.
//!
//! # What the mixer does
//!
//! - It mixes in stereo `f32` at the device's rate, resampling a voice by
//!   linear interpolation, with a 32.32 fixed-point position, where its
//!   rate or pitch asks.
//! - Panning follows the constant-power law for a mono sound, and a
//!   balance law, never above unity, for a stereo one.
//! - A sound at a position is panned by how far it is to the side of the
//!   listener, up to [`AudioConfig::pan_width`], and its volume falls
//!   linearly from [`AudioConfig::min_dist`] to silence at
//!   [`AudioConfig::max_dist`]. Changes of volume, pan or position reach
//!   the voice over 16 ms, so that nothing clicks or zips.
//! - Every start, stop and change of a bus's volume ramps over 2 ms at
//!   least. A sound that ends on its own plays to its last sample.
//! - [`AudioConfig::voices`] voices play at once, and [`PlayParams`]'s
//!   `max_instances` of the same sound. When the voices are full, a new
//!   voice takes the place of one fading out already, or else of the one of
//!   the lowest priority no higher than its own, the oldest of those; at a
//!   sound's `max_instances`, of the same choice among that sound's voices
//!   that are not fading out. The stolen voice fades out over 5 ms. If
//!   there is none to take, the new voice does not play.
//! - A soft limiter guards the master above −1 dBFS.
//!
//! # Example
//!
//! Load a sound as the cook writes it, from files in memory, play it, mix
//! 10 ms of it, and let the main thread drop what the mixer is done with:
//!
//! ```
//! use std::collections::HashMap;
//! use std::sync::Arc;
//!
//! use nadir_assets::{AssetError, AssetPath, AssetServer, FinalizeCtx, OnFailure, Source};
//! use nadir_audio::{AudioConfig, PlayParams, Sound};
//! use nadir_codec::kpcm;
//!
//! /// The game's files, in memory.
//! struct Memory(HashMap<&'static str, Vec<u8>>);
//!
//! impl Source for Memory {
//!     fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
//!         self.0.get(path.as_str()).cloned().ok_or(AssetError::NotFound)
//!     }
//! }
//!
//! // 10 ms of a 440 Hz tone at 48 kHz, as a sound file.
//! let tone: Vec<i16> = (0..480)
//!     .map(|i| ((i as f32 / 48_000.0 * 440.0 * std::f32::consts::TAU).sin() * 16_384.0) as i16)
//!     .collect();
//! let header = kpcm::Header { channels: 1, sample_rate: 48_000, frames: 480 };
//! let files = HashMap::from([("audio/sfx/tone.kpcm", kpcm::encode(&header, &tone)?)]);
//! let mut assets = AssetServer::with_source(Memory(files), OnFailure::Nothing);
//! let tone = assets.load_now::<Sound>("audio/sfx/tone.kpcm", &mut FinalizeCtx::new())?;
//!
//! let (audio, mut mixer) = nadir_audio::new(AudioConfig::default())?;
//! audio.play(&assets, tone, PlayParams::default());
//!
//! // On the audio thread: 10 ms of stereo at 48 kHz.
//! let mut out = vec![0.0; 480 * 2];
//! mixer.render(&mut out, 2, 48_000);
//! assert!(out.iter().any(|&sample| sample != 0.0));
//!
//! // The tone has ended; the next callback hands it back, and only the
//! // server holds it.
//! mixer.render(&mut out, 2, 48_000);
//! audio.update();
//! let sound = assets.get(tone).expect("loaded");
//! assert_eq!(Arc::strong_count(sound.data()), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod asset;
mod audio;
mod data;
mod dsp;
mod error;
mod mixer;
mod music;
mod params;
mod shared;
mod voice;

#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use asset::{LOAD_RATE, Music, Sound};
pub use audio::Audio;
pub use data::{MAX_SAMPLE_RATE, MusicData, SoundData};
pub use error::AudioError;
pub use mixer::Mixer;
pub use params::{
    AudioConfig, AudioStats, Bus, MAX_PITCH, MAX_VOLUME, MIN_PITCH, PlayParams, VoiceChange,
    VoiceId,
};

/// Makes the mixer: the main thread's [`Audio`], and the [`Mixer`] for the
/// audio thread.
///
/// # Errors
///
/// [`AudioError::Config`] for a field of `config` out of range.
pub fn new(config: AudioConfig) -> Result<(Audio, Mixer), AudioError> {
    config.validate()?;
    // Room for every sound and track the slots can hold, two to a slot, and
    // for a full queue's worth of sounds turned away.
    let returns = 2 * (config.voices + 2) + config.commands;
    let shared = Arc::new(shared::Shared::new(config.commands, returns));
    let mixer = Mixer::new(Arc::clone(&shared), &config, returns);
    Ok((Audio::new(shared, returns), mixer))
}

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
