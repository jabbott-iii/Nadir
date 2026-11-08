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
//! which runs on the platform's audio thread, and the game's handle on it.
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
//! - **Sounds**, [`SoundData`]: `f32` samples of 1 or 2 channels, in
//!   memory, at a rate of their own. [`Audio::play`] plays one on a
//!   [`Bus`], as its [`PlayParams`] say; [`Audio::play_at`] plays one at a
//!   position in the world, which pans it and attenuates it from where the
//!   listener is ([`Audio::set_listener`]). [`Audio::set_voice`] changes a
//!   voice as it plays, and [`Audio::stop`] fades it out.
//! - **Music**, [`MusicData`]: a QOA file of 1 or 2 channels, kept
//!   compressed and decoded a frame at a time as it plays, looped.
//!   [`Audio::play_music`] crossfades from one track to another.
//! - **Buses**: music, sound effects, the interface and ambience, each with
//!   a volume ([`Audio::set_bus_volume`]), all feeding the master.
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
//! Play a tone, mix 10 ms of it, and let the main thread drop it:
//!
//! ```
//! use std::sync::Arc;
//!
//! use nadir_audio::{AudioConfig, PlayParams, SoundData};
//!
//! let (audio, mut mixer) = nadir_audio::new(AudioConfig::default())?;
//! let tone: Vec<f32> = (0..480)
//!     .map(|i| (i as f32 / 48_000.0 * 440.0 * std::f32::consts::TAU).sin() * 0.5)
//!     .collect();
//! let tone = Arc::new(SoundData::new(1, 48_000, tone)?);
//! audio.play(&tone, PlayParams::default());
//!
//! // On the audio thread: 10 ms of stereo at 48 kHz.
//! let mut out = vec![0.0; 480 * 2];
//! mixer.render(&mut out, 2, 48_000);
//! assert!(out.iter().any(|&sample| sample != 0.0));
//!
//! // The tone has ended; the next callback hands it back.
//! mixer.render(&mut out, 2, 48_000);
//! audio.update();
//! assert_eq!(Arc::strong_count(&tone), 1);
//! # Ok::<(), nadir_audio::AudioError>(())
//! ```

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
