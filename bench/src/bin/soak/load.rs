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

//! What the soak plays: [`VOICES`] voices of sounds made here, each one
//! replaced by another at random as it ends, and two tracks of music that
//! crossfade, as the spec asks (`docs/subsystems/audio.md`, "Tests").

use std::f32::consts::TAU;
use std::f64::consts::TAU as TAU64;
use std::sync::Arc;
use std::time::Duration;

use nadir_audio::{
    Audio, Bus, Music, MusicData, PlayParams, Sound, SoundData, VoiceChange, VoiceId,
};
use nadir_codec::qoa;
use nadir_core::{Pcg32, Vec2};

use crate::error::SoakError;

/// The voices it keeps playing.
pub(crate) const VOICES: usize = 32;

/// How often the music crossfades to the other track.
pub(crate) const MUSIC_EVERY: Duration = Duration::from_secs(20);

/// A crossfade's length: `MusicArea::new`'s.
const CROSSFADE_MS: u32 = 1_500;

/// How far from the listener a voice at a position starts, along each
/// axis, in pixels: most within the mixer's 800 of silence.
const SPREAD: f32 = 700.0;

/// The fastest a voice that moves goes, in pixels a second.
const MAX_SPEED: f32 = 300.0;

/// The circle the listener goes round, as a camera would, and the seconds
/// of a lap.
const LISTENER_RADIUS: f32 = 200.0;
const LAP_SECS: f32 = 20.0;

/// The device's rate, at which most sounds are made.
const RATE: u32 = 48_000;

/// A sound the load plays.
struct Kind {
    name: &'static str,
    sound: Sound,
    /// Whether it loops until the load stops it.
    looped: bool,
}

/// One of the load's voices.
#[derive(Clone, Copy, Default)]
struct Slot {
    /// The voice, once the slot has started one.
    voice: Option<VoiceId>,
    /// Whether the voice loops.
    looped: bool,
    /// When the voice ends, for a sound that does not loop, or is to be
    /// stopped, for one that does: from the soak's start.
    due: Duration,
    /// Where a voice that moves is, and its velocity in pixels a second.
    moving: Option<(Vec2, Vec2)>,
}

/// The voices and the music, which [`Load::frame`] keeps going.
pub(crate) struct Load {
    rng: Pcg32,
    kinds: Vec<Kind>,
    tracks: [Music; 2],
    slots: [Slot; VOICES],
    music_every: Duration,
    /// The track that plays next, and when.
    next_track: usize,
    track_due: Duration,
    listener: Vec2,
    /// The voices started.
    played: u64,
}

impl Load {
    /// Makes the sounds and the tracks, from `seed`, with the music
    /// crossfading every `music_every`.
    pub(crate) fn new(seed: u64, music_every: Duration) -> Result<Self, SoakError> {
        let mut rng = Pcg32::new(seed, 0);
        let kinds = vec![
            Kind {
                name: "tick",
                sound: tick(&mut rng)?,
                looped: false,
            },
            Kind {
                name: "pluck",
                sound: pluck()?,
                looped: false,
            },
            Kind {
                name: "chime",
                sound: chime()?,
                looped: false,
            },
            Kind {
                name: "hum",
                sound: hum()?,
                looped: true,
            },
            Kind {
                name: "drone",
                sound: drone()?,
                looped: true,
            },
        ];
        Ok(Self {
            rng,
            kinds,
            tracks: [track([220.0, 277.0, 330.0])?, track([196.0, 247.0, 294.0])?],
            slots: [Slot::default(); VOICES],
            music_every,
            next_track: 0,
            track_due: Duration::ZERO,
            listener: Vec2::ZERO,
            played: 0,
        })
    }

    /// One frame of the main thread, `now` from the soak's start and `dt`
    /// after the last: the listener moves, a voice that has ended or is
    /// due to stop is replaced, the voices that move do, and the music
    /// crossfades when it is due.
    pub(crate) fn frame(&mut self, audio: &Audio, now: Duration, dt: Duration) {
        self.listener = Vec2::from_angle(now.as_secs_f32() / LAP_SECS * TAU) * LISTENER_RADIUS;
        audio.set_listener(self.listener);
        if now >= self.track_due {
            audio.play_track(&self.tracks[self.next_track], CROSSFADE_MS);
            self.next_track = 1 - self.next_track;
            self.track_due += self.music_every;
        }
        let dt = dt.as_secs_f32();
        for index in 0..VOICES {
            let slot = &mut self.slots[index];
            if slot.voice.is_none() || now >= slot.due {
                self.start(index, audio, now);
            } else if let (Some(voice), Some((at, velocity))) = (slot.voice, &mut slot.moving) {
                *at += *velocity * dt;
                audio.set_voice(voice, VoiceChange::Position(*at));
            }
        }
    }

    /// Starts a random voice in slot `index`, after stopping its loop, if
    /// it had one.
    fn start(&mut self, index: usize, audio: &Audio, now: Duration) {
        let rng = &mut self.rng;
        let slot = &mut self.slots[index];
        if let (Some(voice), true) = (slot.voice, slot.looped) {
            audio.stop(voice, rng.range(50..501u32));
        }
        let kind = &self.kinds[rng.range(0..self.kinds.len() as u32) as usize];
        let params = PlayParams {
            bus: [Bus::Sfx, Bus::Ui, Bus::Ambience][rng.range(0..3u32) as usize],
            volume: rng.range(0.04..0.12),
            pitch: rng.range(0.5..2.0),
            pan: rng.range(-1.0..1.0),
            looped: kind.looped,
            priority: 128,
            max_instances: VOICES as u8,
        };
        // A third panned, a third at a position, and a third moving from
        // one.
        let placing = rng.range(0..3u32);
        let at = self.listener + Vec2::new(rng.range(-SPREAD..SPREAD), rng.range(-SPREAD..SPREAD));
        let velocity = Vec2::from_angle(rng.range(0.0..TAU)) * rng.range(0.0..MAX_SPEED);
        slot.voice = Some(if placing == 0 {
            audio.play_sound(&kind.sound, params)
        } else {
            audio.play_sound_at(&kind.sound, at, params)
        });
        slot.moving = (placing == 2).then_some((at, velocity));
        slot.looped = kind.looped;
        slot.due = now
            + if kind.looped {
                Duration::from_secs_f32(rng.range(2.0..8.0))
            } else {
                length(&kind.sound).div_f32(params.pitch)
            };
        self.played += 1;
    }

    /// Stops every voice, over 50 ms, and the music, over 100 ms.
    pub(crate) fn finish(&mut self, audio: &Audio) {
        for slot in &mut self.slots {
            if let Some(voice) = slot.voice.take() {
                audio.stop(voice, 50);
            }
        }
        audio.stop_music(100);
    }

    /// The voices started.
    pub(crate) fn played(&self) -> u64 {
        self.played
    }

    /// The sounds and tracks held elsewhere than here, as the mixer, or a
    /// command on its way to it, holds them, by name, with how many times:
    /// none, once the mixer has handed back all it played.
    pub(crate) fn held(&self) -> Vec<(&'static str, usize)> {
        let sounds = self
            .kinds
            .iter()
            .map(|kind| (kind.name, Arc::strong_count(kind.sound.data())));
        let tracks = ["track a", "track b"]
            .into_iter()
            .zip(&self.tracks)
            .map(|(name, track)| (name, Arc::strong_count(track.data())));
        sounds
            .chain(tracks)
            .filter_map(|(name, count)| (count > 1).then_some((name, count - 1)))
            .collect()
    }
}

/// How long a sound plays at its own pitch.
fn length(sound: &Sound) -> Duration {
    let data = sound.data();
    Duration::from_secs_f64(data.frames() as f64 / f64::from(data.sample_rate()))
}

/// `seconds` of sound at `rate`, of `channels` channels, each sample
/// `sample(t, channel)` at `t` seconds.
fn samples(
    channels: u8,
    rate: u32,
    seconds: f64,
    mut sample: impl FnMut(f64, usize) -> f64,
) -> Vec<f32> {
    let frames = (seconds * f64::from(rate)).round() as usize;
    let mut samples = Vec::with_capacity(frames * usize::from(channels));
    for frame in 0..frames {
        let t = frame as f64 / f64::from(rate);
        for channel in 0..usize::from(channels) {
            samples.push(sample(t, channel) as f32);
        }
    }
    samples
}

/// A sound that plays once, its last 2 ms faded to silence, so that it
/// does not click as it ends.
fn once(channels: u8, rate: u32, mut samples: Vec<f32>) -> Result<Sound, SoakError> {
    let width = usize::from(channels);
    let frames = samples.len() / width;
    let start = frames.saturating_sub((rate / 500) as usize);
    let fade = frames - start;
    for (index, frame) in samples[start * width..].chunks_mut(width).enumerate() {
        let gain = (fade - 1 - index) as f32 / (fade.max(2) - 1) as f32;
        frame.iter_mut().for_each(|sample| *sample *= gain);
    }
    Ok(Sound::new(SoundData::new(channels, rate, samples)?))
}

/// A sound that loops: whole cycles of each of its tones, so that its end
/// meets its start.
fn looped(channels: u8, rate: u32, samples: Vec<f32>) -> Result<Sound, SoakError> {
    Ok(Sound::new(SoundData::new(channels, rate, samples)?))
}

/// `frequency` hertz at `t` seconds.
fn sine(frequency: f64, t: f64) -> f64 {
    (TAU64 * frequency * t).sin()
}

/// 40 ms of noise that dies away, mono, at 48 kHz: a footstep.
fn tick(rng: &mut Pcg32) -> Result<Sound, SoakError> {
    let samples = samples(1, RATE, 0.04, |t, _| {
        f64::from(rng.range(-1.0..1.0f32)) * (-t / 0.008).exp()
    });
    once(1, RATE, samples)
}

/// 400 ms of a plucked string, mono, at 44.1 kHz, which the mixer
/// resamples.
fn pluck() -> Result<Sound, SoakError> {
    let samples = samples(1, 44_100, 0.4, |t, _| {
        0.8 * sine(330.0, t) * (-t / 0.08).exp()
    });
    once(1, 44_100, samples)
}

/// 1.5 s of a chime, stereo, at 48 kHz.
fn chime() -> Result<Sound, SoakError> {
    let samples = samples(2, RATE, 1.5, |t, channel| {
        let detune = 1.5 * channel as f64;
        let chord = sine(523.0 + detune, t) + sine(784.0 + detune, t) + sine(1047.0 + detune, t);
        chord / 3.0 * (-t / 0.4).exp()
    });
    once(2, RATE, samples)
}

/// A 2 s loop of a hum, mono, at 22.05 kHz, which the mixer resamples.
fn hum() -> Result<Sound, SoakError> {
    let samples = samples(1, 22_050, 2.0, |t, _| {
        0.4 * sine(110.0, t) + 0.2 * sine(165.0, t)
    });
    looped(1, 22_050, samples)
}

/// A 3 s loop of a drone, stereo, at 44.1 kHz.
fn drone() -> Result<Sound, SoakError> {
    let samples = samples(2, 44_100, 3.0, |t, channel| {
        let base = 73.0 + channel as f64;
        0.3 * sine(base, t) + 0.15 * sine(2.0 * base, t)
    });
    looped(2, 44_100, samples)
}

/// 8 s of a chord of `chord`'s three tones, swelling twice, stereo, at
/// 48 kHz, as QOA: a track, which loops as music does.
fn track(chord: [f64; 3]) -> Result<Music, SoakError> {
    let samples = samples(2, RATE, 8.0, |t, channel| {
        let detune = 0.5 * channel as f64;
        let swell = 0.6 + 0.4 * (TAU64 * 0.25 * t).cos();
        let tones: f64 = chord.iter().map(|&tone| sine(tone + detune, t)).sum();
        0.12 * swell * tones
    });
    let pcm: Vec<i16> = samples
        .iter()
        .map(|&sample| (sample * f32::from(i16::MAX)) as i16)
        .collect();
    let header = qoa::Header {
        channels: 2,
        sample_rate: RATE,
        samples: (pcm.len() / 2) as u32,
    };
    let file = qoa::encode(&header, &pcm)?;
    Ok(Music::new(MusicData::new(file)?))
}

#[cfg(test)]
mod tests {
    use nadir_audio::AudioConfig;

    use super::*;

    #[test]
    fn the_sounds_are_of_the_rates_and_channels_the_mixer_must_handle() {
        let load = Load::new(1, MUSIC_EVERY).unwrap();
        let shapes: Vec<_> = load
            .kinds
            .iter()
            .map(|kind| {
                let data = kind.sound.data();
                (kind.name, data.channels(), data.sample_rate(), kind.looped)
            })
            .collect();
        assert_eq!(
            shapes,
            [
                ("tick", 1, 48_000, false),
                ("pluck", 1, 44_100, false),
                ("chime", 2, 48_000, false),
                ("hum", 1, 22_050, true),
                ("drone", 2, 44_100, true),
            ]
        );
        for kind in &load.kinds {
            let samples = kind.sound.data().samples();
            let channels = usize::from(kind.sound.data().channels());
            assert!(
                samples.iter().all(|sample| sample.abs() <= 1.0),
                "{}",
                kind.name
            );
            if kind.looped {
                // The end meets the start: the step across the seam is no
                // larger than the steps within, but for rounding. (Its
                // tones all rise through 0 there, where they are steepest.)
                let step = |a: f32, b: f32| (a - b).abs();
                let seam = step(samples[samples.len() - channels], samples[0]);
                let largest = samples
                    .windows(channels + 1)
                    .map(|pair| step(pair[0], pair[channels]))
                    .fold(0.0, f32::max);
                assert!(seam <= largest * 1.001, "{}: {seam} > {largest}", kind.name);
            } else {
                let last = &samples[samples.len() - channels..];
                assert!(
                    last.iter().all(|&sample| sample == 0.0),
                    "{} ends silent",
                    kind.name
                );
            }
        }
        for track in &load.tracks {
            assert_eq!(track.data().header().channels, 2);
            assert_eq!(track.data().header().samples, 8 * RATE);
        }
        assert_eq!(load.held(), []);
    }

    #[test]
    fn what_is_held_is_counted_past_the_load_s_own() {
        let load = Load::new(1, MUSIC_EVERY).unwrap();
        let hum = load.kinds[3].sound.clone();
        let tracks = [load.tracks[1].clone(), load.tracks[1].clone()];
        assert_eq!(load.held(), [("hum", 1), ("track b", 2)]);
        drop((hum, tracks));
        assert_eq!(load.held(), []);
    }

    #[test]
    fn the_voices_that_move_do_and_the_others_keep_their_place() {
        let (audio, _mixer) = nadir_audio::new(AudioConfig::default()).unwrap();
        let mut load = Load::new(3, MUSIC_EVERY).unwrap();
        let dt = Duration::from_millis(16);
        load.frame(&audio, Duration::ZERO, dt);
        let before = load.slots;
        load.frame(&audio, dt, dt);
        let mut moved = 0;
        for (was, is) in before.iter().zip(&load.slots) {
            if was.voice != is.voice {
                continue;
            }
            match (was.moving, is.moving) {
                (Some((from, velocity)), Some((to, _))) => {
                    let step = from + velocity * dt.as_secs_f32();
                    assert!((to - step).length() < 1e-3, "{from:?} to {to:?}");
                    moved += usize::from(velocity.length() > 1.0);
                }
                (None, None) => {}
                other => panic!("a voice began or stopped moving: {other:?}"),
            }
        }
        // About a third of the 32 move.
        assert!((4..=20).contains(&moved), "{moved} moved");
    }

    #[test]
    fn the_same_seed_makes_the_same_load() {
        let (audio, _mixer) = nadir_audio::new(AudioConfig::default()).unwrap();
        let mut a = Load::new(7, MUSIC_EVERY).unwrap();
        let mut b = Load::new(7, MUSIC_EVERY).unwrap();
        for frame in 0..600u32 {
            let now = Duration::from_millis(u64::from(frame) * 1000 / 60);
            a.frame(&audio, now, Duration::from_millis(16));
            b.frame(&audio, now, Duration::from_millis(16));
        }
        assert_eq!(a.played, b.played);
        let dues = |load: &Load| load.slots.map(|slot| (slot.due, slot.looped));
        assert_eq!(dues(&a), dues(&b));
    }
}
