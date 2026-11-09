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

//! The types the game hands the mixer: buses, how a voice plays and what
//! may change, and the mixer's configuration and statistics.

use std::time::Duration;

use nadir_core::Vec2;

use crate::AudioError;

/// A bus: a group of voices with a volume of its own. All four feed the
/// master, which the limiter guards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bus {
    /// Music tracks, and any sound played on it.
    Music,
    /// Sound effects.
    Sfx,
    /// The interface's sounds.
    Ui,
    /// Ambient loops.
    Ambience,
}

impl Bus {
    /// Every bus, in the order of their indices.
    pub const ALL: [Self; 4] = [Self::Music, Self::Sfx, Self::Ui, Self::Ambience];

    /// The bus's place in [`Bus::ALL`].
    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

/// How a voice plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayParams {
    /// The bus it plays on.
    pub bus: Bus,
    /// Its volume: 1 as the sound is. Below 0, or not a number, counts as
    /// 0; above [`MAX_VOLUME`], as [`MAX_VOLUME`].
    pub volume: f32,
    /// How fast it plays: 1 at its own pitch, 2 an octave higher and half
    /// as long, 1.2 about 3 semitones higher and a sixth shorter. Clamped to [`MIN_PITCH`] … [`MAX_PITCH`];
    /// 0 or less, or not a number, counts as 1.
    pub pitch: f32,
    /// Where it sits between the speakers, from −1 (left) to 1 (right);
    /// clamped, and 0 if not a number. A voice played at a position takes
    /// its pan from there instead.
    pub pan: f32,
    /// Whether it loops until it is stopped.
    pub looped: bool,
    /// How much it matters when voices run short: a voice that is playing,
    /// not fading out, is stolen only for one of the same priority or
    /// higher.
    pub priority: u8,
    /// How many voices of the same sound may play at once; 0 counts as 1.
    pub max_instances: u8,
}

impl Default for PlayParams {
    /// On [`Bus::Sfx`], at volume 1, pitch 1 and pan 0, not looped, at
    /// priority 128, and at most 4 at once.
    fn default() -> Self {
        Self {
            bus: Bus::Sfx,
            volume: 1.0,
            pitch: 1.0,
            pan: 0.0,
            looped: false,
            priority: 128,
            max_instances: 4,
        }
    }
}

/// The highest volume a voice or a bus takes.
pub const MAX_VOLUME: f32 = 16.0;

/// The lowest pitch a voice takes.
pub const MIN_PITCH: f32 = 1.0 / 16.0;

/// The highest pitch a voice takes.
pub const MAX_PITCH: f32 = 16.0;

/// A change to a voice that is playing, from
/// [`Audio::set_voice`](crate::Audio::set_voice).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VoiceChange {
    /// Its volume, as [`PlayParams::volume`]. The voice reaches it over
    /// 16 ms, however long the device's callbacks.
    Volume(f32),
    /// Its pitch, as [`PlayParams::pitch`].
    Pitch(f32),
    /// Its pan, as [`PlayParams::pan`], reached over 16 ms. A voice at a
    /// position keeps the pan its position gives it.
    Pan(f32),
    /// Its position in the world, which gives it its pan and attenuation
    /// from then on, reached over 16 ms. A position that is not finite
    /// makes it silent.
    Position(Vec2),
}

/// A voice, as [`Audio::play`](crate::Audio::play) named it. A voice's id
/// is never used again, so one that names a voice that has ended, or was
/// never started, changes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VoiceId(pub(crate) u64);

/// How the mixer is set up. [`AudioConfig::default`] suits a game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioConfig {
    /// The voices that may play at once: 48. A voice stolen for another
    /// fades out over 5 ms in the same slot, so for that long a slot sounds
    /// two.
    pub voices: usize,
    /// The commands the queue holds: 1,024. It never grows: past this, a
    /// command is dropped on the main thread, and counted in
    /// [`AudioStats::dropped`]. Changes to a voice, and the listener's
    /// moves, are dropped once it is half full, to keep room for the rest.
    pub commands: usize,
    /// How far to the side of the listener, in pixels, a sound is panned
    /// fully to one speaker: 480.
    pub pan_width: f32,
    /// The distance, in pixels, up to which a sound plays at full volume:
    /// 64.
    pub min_dist: f32,
    /// The distance, in pixels, from which a sound is silent: 800. Its
    /// volume falls linearly from [`AudioConfig::min_dist`] to here.
    pub max_dist: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            voices: 48,
            commands: 1024,
            pan_width: 480.0,
            min_dist: 64.0,
            max_dist: 800.0,
        }
    }
}

impl AudioConfig {
    /// Checks each field.
    pub(crate) fn validate(&self) -> Result<(), AudioError> {
        let fail = |field, expected| Err(AudioError::Config { field, expected });
        if self.voices == 0 || self.voices > 1024 {
            return fail("voices", "from 1 to 1024");
        }
        if self.commands < 2 || self.commands > 1 << 20 {
            return fail("commands", "from 2 to 1048576");
        }
        if !(self.pan_width.is_finite() && self.pan_width > 0.0) {
            return fail("pan_width", "finite and above 0");
        }
        if !(self.min_dist.is_finite() && self.min_dist >= 0.0) {
            return fail("min_dist", "finite and at least 0");
        }
        if !(self.max_dist.is_finite() && self.max_dist > self.min_dist) {
            return fail("max_dist", "finite and above min_dist");
        }
        Ok(())
    }
}

/// What the mixer has been doing, from [`Audio::stats`](crate::Audio::stats).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioStats {
    /// The voices sounding at the end of the last callback, those fading
    /// out included, and music not.
    pub active_voices: u32,
    /// The underruns the device has reported.
    pub underruns: u64,
    /// How long the last callback took to mix.
    pub callback_time: Duration,
    /// The device's rate, in frames a second, at the last callback: 0
    /// before the first. Sounds load at [`LOAD_RATE`](crate::LOAD_RATE),
    /// the output's: at another, the mixer resamples them.
    pub rate: u32,
    /// The commands dropped on the main thread: a change to a voice or a
    /// move of the listener once the queue is half full, and anything once
    /// it is full. It fills when more commands come between two callbacks
    /// than it holds, or while the mixer waits for
    /// [`Audio::update`](crate::Audio::update) to take back what it is
    /// done with.
    pub dropped: u64,
    /// The plays of handles with nothing under them, which played nothing:
    /// a sound or a track still loading, failed with no placeholder, or
    /// unloaded ([`Audio::play`](crate::Audio::play)).
    pub unplayed: u64,
}
