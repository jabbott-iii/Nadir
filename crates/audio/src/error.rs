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

//! The error type for `nadir-audio`.

use std::fmt;

use nadir_codec::CodecError;

/// Why the mixer, a sound or a music track could not be made.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AudioError {
    /// A field of the [`AudioConfig`](crate::AudioConfig) is out of range.
    Config {
        /// The field.
        field: &'static str,
        /// What it must be.
        expected: &'static str,
    },
    /// A sound has a number of channels the mixer does not play: it plays
    /// 1 or 2.
    Channels {
        /// The channels it has.
        found: u32,
    },
    /// A sound's sample rate is 0, or above
    /// [`MAX_SAMPLE_RATE`](crate::MAX_SAMPLE_RATE).
    SampleRate {
        /// Its rate.
        found: u32,
    },
    /// A sound has more frames than the mixer can count: 2³¹ at most, over
    /// 12 hours at 48 kHz.
    TooLong,
    /// A sound has no samples.
    NoSamples,
    /// A sound's samples are not a whole number of frames.
    PartialFrame {
        /// The samples it has.
        samples: usize,
        /// Its channels.
        channels: u8,
    },
    /// A sound's sample is outside −1 … 1, or not a number.
    OutOfRange {
        /// The sample's index, interleaved.
        index: usize,
    },
    /// The codec refused a music track's file.
    Decode(CodecError),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config { field, expected } => {
                write!(f, "the audio config's `{field}` must be {expected}")
            }
            Self::Channels { found } => {
                write!(f, "a sound of {found} channels: the mixer plays 1 or 2")
            }
            Self::SampleRate { found } => write!(
                f,
                "a sound at {found} Hz: the mixer plays rates of 1 to {} Hz",
                crate::MAX_SAMPLE_RATE
            ),
            Self::TooLong => f.write_str("a sound of more than 2^31 frames"),
            Self::NoSamples => f.write_str("a sound has no samples"),
            Self::PartialFrame { samples, channels } => write!(
                f,
                "{samples} samples are not a whole number of frames of {channels} channels"
            ),
            Self::OutOfRange { index } => {
                write!(f, "sample {index} is outside -1 to 1, or not a number")
            }
            Self::Decode(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for AudioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode(error) => Some(error),
            _ => None,
        }
    }
}

impl From<CodecError> for AudioError {
    fn from(error: CodecError) -> Self {
        Self::Decode(error)
    }
}
