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

//! A music voice: a QOA track decoded a frame at a time, as it plays, into
//! a buffer of its own.

use std::sync::Arc;

use nadir_codec::qoa::{self, FRAME_LEN};

use crate::MusicData;
use crate::dsp::Ramp;
use crate::voice::{ONE, Rendered, fraction, lerp, step};

/// The first frame's offset in a QOA file, after its 8-byte header.
const FIRST_FRAME: usize = 8;

/// A track playing, looped.
#[derive(Debug)]
pub(crate) struct Track {
    pub(crate) music: Arc<MusicData>,
    /// The offset of the frame after the one in the buffer.
    next: usize,
    /// The frames the buffer holds after its carry: the frame's samples a
    /// channel.
    len: usize,
    /// Where it is in the buffer, in 32.32 frames.
    cursor: u64,
    pub(crate) fade: Ramp,
    /// Whether a frame failed to decode, which ended it.
    pub(crate) failed: bool,
}

impl Track {
    /// The track from its start, decoded into `buffer`, fading in over
    /// `fade_in` samples; the track back if its first frame does not decode
    /// (which [`MusicData::new`] has ruled out).
    pub(crate) fn start(
        music: Arc<MusicData>,
        buffer: &mut [i16],
        fade_in: u32,
    ) -> Result<Self, Arc<MusicData>> {
        let mut fade = Ramp::at(0.0);
        fade.to(1.0, fade_in);
        let channels = usize::from(music.header().channels);
        buffer[..channels].fill(0);
        let mut track = Self {
            music,
            next: FIRST_FRAME,
            len: 0,
            // Past the carry, which nothing comes before.
            cursor: ONE,
            fade,
            failed: false,
        };
        match track.decode(buffer) {
            Some(()) => Ok(track),
            None => Err(track.music),
        }
    }

    /// Decodes the next frame into `buffer` after its carry, or the first
    /// frame after the last.
    fn decode(&mut self, buffer: &mut [i16]) -> Option<()> {
        let music = &*self.music;
        let channels = usize::from(music.header().channels);
        if self.next >= music.bytes().len() {
            self.next = FIRST_FRAME;
        }
        let frame = qoa::decode_frame(
            music.bytes(),
            music.header(),
            self.next,
            &mut buffer[channels..],
        )
        .ok()?;
        self.next = frame.next;
        self.len = frame.samples;
        Some(())
    }

    /// Mixes the next `out.len() / 2` frames into `out`, stereo, at
    /// `rate`, from `buffer`, which it decodes into as it goes. Music is
    /// not panned: a mono track goes to both sides as it is.
    pub(crate) fn render(&mut self, out: &mut [f32], rate: u32, buffer: &mut [i16]) -> Rendered {
        let header = *self.music.header();
        let channels = usize::from(header.channels);
        let step = step(header.sample_rate, rate, 1.0);
        let scale = 1.0 / 32768.0;
        for (k, frame) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            while (self.cursor >> 32) as usize >= self.len {
                // The last frame becomes the carry, for the sample between
                // it and the next frame's first.
                let last = self.len * channels;
                buffer.copy_within(last..last + channels, 0);
                self.cursor -= (self.len as u64) << 32;
                if self.decode(buffer).is_none() {
                    self.failed = true;
                    return Rendered::Ended(k);
                }
            }
            let index = (self.cursor >> 32) as usize;
            let t = fraction(self.cursor);
            let sample = |channel: usize| {
                let a = f32::from(buffer[index * channels + channel]);
                let b = f32::from(buffer[(index + 1) * channels + channel]);
                lerp(a, b, t) * scale
            };
            let fade = self.fade.next();
            if channels == 1 {
                let x = sample(0) * fade;
                frame[0] += x;
                frame[1] += x;
            } else {
                frame[0] += sample(0) * fade;
                frame[1] += sample(1) * fade;
            }
            self.cursor += step;
            if self.fade.target() == 0.0 && self.fade.remaining() == 0 {
                return Rendered::Ended(k + 1);
            }
        }
        Rendered::Playing
    }
}

/// The length of a music voice's buffer: a carried frame and a frame of
/// QOA, stereo.
pub(crate) const BUFFER_LEN: usize = (1 + FRAME_LEN) * 2;
