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

//! What the mixer plays: sounds, decoded into memory, and music tracks,
//! kept as QOA and decoded as they play.

use nadir_codec::qoa::{self, FRAME_LEN};
use nadir_core::hash::fnv1a64;

use crate::AudioError;

/// The highest sample rate a sound or a track may have, in frames a
/// second.
pub const MAX_SAMPLE_RATE: u32 = 384_000;

/// The most frames a sound may have: 2³¹, over 12 hours at 48 kHz, so
/// that a voice's 32.32 position, a step past its end, still fits in 64
/// bits.
const MAX_FRAMES: usize = 1 << 31;

/// Checks a sound's or a track's rate.
pub(crate) fn check_rate(rate: u32) -> Result<(), AudioError> {
    if rate == 0 || rate > MAX_SAMPLE_RATE {
        Err(AudioError::SampleRate { found: rate })
    } else {
        Ok(())
    }
}

/// A sound, decoded: `f32` samples, interleaved, of 1 or 2 channels, at a
/// sample rate of its own. The mixer plays it at any rate, resampling it
/// if the device's differs; a [`Sound`](crate::Sound) that loads converts
/// to [`LOAD_RATE`](crate::LOAD_RATE), the output's, so that most play
/// without.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundData {
    channels: u8,
    sample_rate: u32,
    samples: Box<[f32]>,
}

impl SoundData {
    /// A sound of `channels` channels at `sample_rate` frames a second.
    /// The samples are interleaved, from −1 to 1.
    ///
    /// # Errors
    ///
    /// - [`AudioError::Channels`] unless `channels` is 1 or 2;
    /// - [`AudioError::SampleRate`] for a rate of 0 or above
    ///   [`MAX_SAMPLE_RATE`];
    /// - [`AudioError::NoSamples`] for no samples;
    /// - [`AudioError::PartialFrame`] if the samples are not a whole number
    ///   of frames;
    /// - [`AudioError::TooLong`] for more than 2³¹ frames;
    /// - [`AudioError::OutOfRange`] for a sample outside −1 … 1, or not a
    ///   number, which could carry the mix past what the limiter bounds.
    pub fn new(channels: u8, sample_rate: u32, samples: Vec<f32>) -> Result<Self, AudioError> {
        if !(1..=2).contains(&channels) {
            return Err(AudioError::Channels {
                found: channels.into(),
            });
        }
        check_rate(sample_rate)?;
        if samples.is_empty() {
            return Err(AudioError::NoSamples);
        }
        if !samples.len().is_multiple_of(usize::from(channels)) {
            return Err(AudioError::PartialFrame {
                samples: samples.len(),
                channels,
            });
        }
        if samples.len() / usize::from(channels) > MAX_FRAMES {
            return Err(AudioError::TooLong);
        }
        if let Some(index) = samples
            .iter()
            .position(|sample| !(-1.0..=1.0).contains(sample))
        {
            return Err(AudioError::OutOfRange { index });
        }
        Ok(Self {
            channels,
            sample_rate,
            samples: samples.into_boxed_slice(),
        })
    }

    /// Its channels: 1 or 2.
    pub fn channels(&self) -> u8 {
        self.channels
    }

    /// Its frames a second.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Its samples, interleaved.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Its length in frames: at least 1.
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }
}

/// A music track: a QOA file of 1 or 2 channels, kept compressed, which
/// the mixer decodes a frame at a time as it plays (about 6.9 MB for three
/// minutes of 48 kHz stereo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MusicData {
    bytes: Box<[u8]>,
    header: qoa::Header,
    /// The FNV-1a hash of the bytes: the mixer takes two tracks of the same
    /// bytes, loaded apart, as the same track.
    pub(crate) key: u64,
}

impl MusicData {
    /// A track from the bytes of a QOA file. Every frame is decoded once
    /// here, so that the mixer, which decodes them again as it plays, never
    /// meets one it cannot.
    ///
    /// # Errors
    ///
    /// - [`AudioError::Decode`] for a file the QOA decoder refuses
    ///   (`nadir_codec::qoa`);
    /// - [`AudioError::Channels`] for a file of more than 2 channels;
    /// - [`AudioError::SampleRate`] for a rate above [`MAX_SAMPLE_RATE`].
    pub fn new(bytes: Vec<u8>) -> Result<Self, AudioError> {
        let header = qoa::header(&bytes)?;
        if header.channels > 2 {
            return Err(AudioError::Channels {
                found: header.channels.into(),
            });
        }
        check_rate(header.sample_rate)?;
        let mut frame = vec![0; FRAME_LEN * usize::from(header.channels)];
        let mut at = 8;
        while at < bytes.len() {
            at = qoa::decode_frame(&bytes, &header, at, &mut frame)?.next;
        }
        Ok(Self {
            key: fnv1a64(&bytes),
            bytes: bytes.into_boxed_slice(),
            header,
        })
    }

    /// Whether `other` is the same track: the same bytes, wherever they
    /// were loaded from.
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.key == other.key && self.bytes.len() == other.bytes.len()
    }

    /// The file's header: its channels, sample rate and length.
    pub fn header(&self) -> &qoa::Header {
        &self.header
    }

    /// The file.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
