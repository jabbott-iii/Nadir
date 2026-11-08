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

//! A voice of a sound in memory: its resampler, gains and fade.

use std::sync::Arc;

use nadir_core::Vec2;

use crate::dsp::{Ramp, attenuation, pan_gains};
use crate::{Bus, MAX_PITCH, MAX_VOLUME, MIN_PITCH, PlayParams, SoundData, VoiceChange, VoiceId};

/// One in 2³²: a step of the resampler's position is a 32.32 fixed-point
/// number of source frames.
pub(crate) const ONE: u64 = 1 << 32;

/// The longest step: 64 source frames for each frame out.
pub(crate) const MAX_STEP: u64 = 64 << 32;

/// The step through a source at `source_rate` that plays it at `pitch` at
/// `rate`, in 32.32 source frames a frame.
pub(crate) fn step(source_rate: u32, rate: u32, pitch: f32) -> u64 {
    let step = f64::from(source_rate) / f64::from(rate) * f64::from(pitch) * ONE as f64;
    (step.round() as u64).clamp(1, MAX_STEP)
}

/// The sample between `a` and `b` that is `t` of the way, `t` from 0 to 1:
/// `a` itself at 0, as at a step of exactly 1.
pub(crate) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

/// The fraction of a frame in a 32.32 position.
pub(crate) fn fraction(position: u64) -> f32 {
    (position & (ONE - 1)) as f32 * (1.0 / ONE as f32)
}

/// A volume as the mixer takes it: from 0 to [`MAX_VOLUME`], and 0 if it is
/// not a number.
pub(crate) fn volume(volume: f32) -> f32 {
    if volume.is_nan() {
        0.0
    } else {
        volume.clamp(0.0, MAX_VOLUME)
    }
}

/// A pitch as the mixer takes it: from [`MIN_PITCH`] to [`MAX_PITCH`], and
/// 1 if it is 0 or less or not a number.
pub(crate) fn pitch(pitch: f32) -> f32 {
    if pitch > 0.0 {
        pitch.clamp(MIN_PITCH, MAX_PITCH)
    } else {
        1.0
    }
}

/// A pan as the mixer takes it: from −1 to 1, and 0 if it is not a number.
pub(crate) fn pan(pan: f32) -> f32 {
    if pan.is_nan() {
        0.0
    } else {
        pan.clamp(-1.0, 1.0)
    }
}

/// Where the listener is, and how far sounds carry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Space {
    pub(crate) listener: Vec2,
    pub(crate) pan_width: f32,
    pub(crate) min_dist: f32,
    pub(crate) max_dist: f32,
}

/// How a call to [`Voice::render`] ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rendered {
    /// It goes on.
    Playing,
    /// It reached its end, or its fade reached 0, after mixing this many
    /// frames, and is silent from there.
    Ended(usize),
}

/// A voice of a sound.
#[derive(Debug)]
pub(crate) struct Voice {
    pub(crate) id: VoiceId,
    pub(crate) sound: Arc<SoundData>,
    pub(crate) bus: Bus,
    pub(crate) priority: u8,
    /// Its place in the order voices started in: the lowest is the oldest.
    pub(crate) order: u64,
    looped: bool,
    volume: f32,
    pitch: f32,
    pan: f32,
    /// Where it is in the world, if it was played at a position.
    position: Option<Vec2>,
    /// Where it is in the sound, in 32.32 frames.
    cursor: u64,
    /// Its left and right gains, moving to the targets its volume, pan and
    /// position give; `None` before it first mixes, when they start there.
    gains: Option<[Ramp; 2]>,
    /// The fade it is in: in at the start, out at a stop.
    pub(crate) fade: Ramp,
}

impl Voice {
    /// A voice that fades in over `fade_in` samples.
    pub(crate) fn new(
        id: VoiceId,
        sound: Arc<SoundData>,
        params: &PlayParams,
        position: Option<Vec2>,
        order: u64,
        fade_in: u32,
    ) -> Self {
        let mut fade = Ramp::at(0.0);
        fade.to(1.0, fade_in);
        Self {
            id,
            sound,
            bus: params.bus,
            priority: params.priority,
            order,
            looped: params.looped,
            volume: volume(params.volume),
            pitch: pitch(params.pitch),
            pan: pan(params.pan),
            position,
            cursor: 0,
            gains: None,
            fade,
        }
    }

    pub(crate) fn change(&mut self, change: VoiceChange) {
        match change {
            VoiceChange::Volume(value) => self.volume = volume(value),
            VoiceChange::Pitch(value) => self.pitch = pitch(value),
            VoiceChange::Pan(value) => self.pan = pan(value),
            VoiceChange::Position(value) => self.position = Some(value),
        }
    }

    /// The gains it moves to: its volume, pan and attenuation. At a
    /// position that is not finite, it is silent.
    fn target_gains(&self, space: &Space) -> [f32; 2] {
        let (pan, gain) = match self.position {
            None => (self.pan, self.volume),
            Some(position) if !(position.x.is_finite() && position.y.is_finite()) => (0.0, 0.0),
            Some(position) => {
                let offset = position - space.listener;
                let pan = (offset.x / space.pan_width).clamp(-1.0, 1.0);
                let distance = offset.length();
                // A distance too great for `f32` is infinite: silent.
                let gain = attenuation(distance, space.min_dist, space.max_dist);
                (if pan.is_nan() { 0.0 } else { pan }, self.volume * gain)
            }
        };
        let [left, right] = pan_gains(pan, self.sound.channels());
        [left * gain, right * gain]
    }

    /// Mixes the next `out.len() / 2` frames into `out`, stereo, at
    /// `rate`. A change of its gains' targets moves them there over
    /// `smooth` samples.
    pub(crate) fn render(
        &mut self,
        out: &mut [f32],
        rate: u32,
        space: &Space,
        smooth: u32,
    ) -> Rendered {
        let target = self.target_gains(space);
        let gains = self
            .gains
            .get_or_insert_with(|| [Ramp::at(target[0]), Ramp::at(target[1])]);
        for (gain, target) in gains.iter_mut().zip(target) {
            if gain.target() != target {
                gain.to(target, smooth);
            }
        }
        let step = step(self.sound.sample_rate(), rate, self.pitch);
        let sound = &*self.sound;
        let samples = sound.samples();
        let length = sound.frames() as u64;
        let channels = usize::from(sound.channels());
        for (k, frame) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let mut index = self.cursor >> 32;
            if index >= length {
                if !self.looped {
                    return Rendered::Ended(k);
                }
                // A loop shorter than a step can pass its end more than
                // once in one.
                self.cursor %= length << 32;
                index = self.cursor >> 32;
            }
            let t = fraction(self.cursor);
            let next = if index + 1 < length {
                Some(index + 1)
            } else if self.looped {
                Some(0)
            } else {
                None
            };
            let fade = self.fade.next();
            let left = gains[0].next() * fade;
            let right = gains[1].next() * fade;
            let at = index as usize * channels;
            let sample = |channel: usize| {
                let a = samples[at + channel];
                let b = next.map_or(0.0, |next| samples[next as usize * channels + channel]);
                lerp(a, b, t)
            };
            if channels == 1 {
                let x = sample(0);
                frame[0] += x * left;
                frame[1] += x * right;
            } else {
                frame[0] += sample(0) * left;
                frame[1] += sample(1) * right;
            }
            self.cursor += step;
            if self.fade.target() == 0.0 && self.fade.remaining() == 0 {
                return Rendered::Ended(k + 1);
            }
        }
        Rendered::Playing
    }
}
