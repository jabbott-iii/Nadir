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

//! The error type for the soak.

use std::fmt;
use std::io;

use nadir_audio::AudioError;
use nadir_codec::CodecError;
use nadir_platform::PlatformError;

/// Why the soak could not run, as opposed to what a soak that ran found.
#[derive(Debug)]
pub(crate) enum SoakError {
    /// The command line was not understood.
    Usage(String),
    /// A sound of the load could not be made.
    Sound(AudioError),
    /// A track of the load could not be encoded.
    Track(CodecError),
    /// The audio output did not open.
    Open(PlatformError),
    /// The audio output opened, but no device did within its 2 s.
    NoDevice,
    /// Writing to standard output failed.
    Output(io::Error),
}

impl fmt::Display for SoakError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => f.write_str(message),
            Self::Sound(error) => write!(f, "cannot make the load's sounds: {error}"),
            Self::Track(error) => write!(f, "cannot encode the load's music: {error}"),
            Self::Open(error) => write!(f, "cannot open the audio output: {error}"),
            Self::NoDevice => f.write_str(
                "no audio device opened within 2 s, and a soak needs one \
                 (on Linux, NADIR_ALSA_DEVICE names an ALSA device to use)",
            ),
            Self::Output(error) => write!(f, "cannot write the report: {error}"),
        }
    }
}

impl std::error::Error for SoakError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sound(error) => Some(error),
            Self::Track(error) => Some(error),
            Self::Open(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::Usage(_) | Self::NoDevice => None,
        }
    }
}

impl From<AudioError> for SoakError {
    fn from(error: AudioError) -> Self {
        Self::Sound(error)
    }
}

impl From<CodecError> for SoakError {
    fn from(error: CodecError) -> Self {
        Self::Track(error)
    }
}

impl From<PlatformError> for SoakError {
    fn from(error: PlatformError) -> Self {
        Self::Open(error)
    }
}

impl From<io::Error> for SoakError {
    fn from(error: io::Error) -> Self {
        Self::Output(error)
    }
}
