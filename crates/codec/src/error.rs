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

//! The error type for `nadir-codec`.

use std::fmt;

#[cfg(feature = "png")]
use crate::png;
use crate::{
    aseprite, atlas, bmfont, inflate, json, kbj, klz, kpak, ldtk, level, prefab, qoi, wav,
};

/// Errors produced by `nadir-codec`.
///
/// Non-exhaustive: which variants exist depends on the crate's features
/// (`Png` comes with `png`), and Cargo turns a feature on for every crate
/// in a build that wants it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodecError {
    /// JSON or JSONC text is malformed or exceeds a limit.
    Json {
        /// The line of the problem, starting at 1.
        line: u32,
        /// The column of the problem, starting at 1 and counted in
        /// characters, not bytes.
        column: u32,
        /// What is wrong.
        kind: json::ErrorKind,
    },
    /// A QOI file is malformed or exceeds a limit.
    Qoi {
        /// The byte offset of the problem in the file.
        offset: usize,
        /// What is wrong.
        kind: qoi::ErrorKind,
    },
    /// An image cannot be encoded as QOI.
    QoiEncode {
        /// Why not.
        kind: qoi::EncodeErrorKind,
    },
    /// A DEFLATE or zlib stream is malformed or exceeds a limit.
    Inflate {
        /// The byte offset of the problem in the stream.
        offset: usize,
        /// What is wrong.
        kind: inflate::ErrorKind,
    },
    /// A KLZ stream is malformed, does not hold the length given, or
    /// exceeds a limit.
    Klz {
        /// The byte offset of the problem in the stream.
        offset: usize,
        /// What is wrong.
        kind: klz::ErrorKind,
    },
    /// A pack file is malformed, damaged or exceeds a limit
    /// ([`kpak`](crate::kpak)).
    Kpak {
        /// The byte offset of the problem in the pack file.
        offset: u64,
        /// What is wrong.
        kind: kpak::ErrorKind,
    },
    /// A KBJ file is malformed or exceeds a limit.
    Kbj {
        /// The byte offset of the problem in the file.
        offset: usize,
        /// What is wrong.
        kind: kbj::ErrorKind,
    },
    /// A BMFont file is malformed, misses what the font asset needs, or
    /// exceeds a limit.
    BmFont {
        /// The line of the problem, starting at 1.
        line: u32,
        /// The column of the problem, starting at 1 and counted in
        /// characters.
        column: u32,
        /// What is wrong.
        kind: bmfont::ErrorKind,
    },
    /// A KBJ file is not an atlas file the game can read
    /// ([`atlas::parse`](crate::atlas::parse)).
    Atlas {
        /// Where the problem is, such as `sprites[2].frames[0].w`; empty
        /// for the file as a whole.
        at: String,
        /// What is wrong.
        kind: atlas::ErrorKind,
    },
    /// A JSON file is not an Aseprite export the cook takes
    /// ([`aseprite::parse`](crate::aseprite::parse)).
    Aseprite {
        /// Where the problem is, such as `frames[3].frame.w`; empty for the
        /// file as a whole.
        at: String,
        /// What is wrong.
        kind: aseprite::ErrorKind,
    },
    /// A JSON file is not an LDtk project or level file the cook takes
    /// ([`ldtk::parse_project`](crate::ldtk::parse_project),
    /// [`ldtk::parse_level`](crate::ldtk::parse_level)).
    Ldtk {
        /// Where the problem is, such as `levels[0].layerInstances[2].gridTiles[5].t`;
        /// empty for the file as a whole.
        at: String,
        /// What is wrong.
        kind: ldtk::ErrorKind,
    },
    /// A KBJ file is not a level file the game can read
    /// ([`level::parse`](crate::level::parse)).
    Level {
        /// Where the problem is, such as `layers[0].cells[130]`; empty for
        /// the file as a whole.
        at: String,
        /// What is wrong.
        kind: level::ErrorKind,
    },
    /// A value is not a prefab file
    /// ([`prefab::parse`](crate::prefab::parse)).
    Prefab {
        /// Where the problem is, such as `components.Health`; empty for the
        /// file as a whole.
        at: String,
        /// What is wrong.
        kind: prefab::ErrorKind,
    },
    /// A value cannot be encoded as KBJ.
    KbjEncode {
        /// Why not.
        kind: kbj::EncodeErrorKind,
    },
    /// A WAV file is malformed, outside the supported subset, or exceeds a
    /// limit.
    Wav {
        /// The byte offset of the problem in the file.
        offset: usize,
        /// What is wrong.
        kind: wav::ErrorKind,
    },
    /// A PNG file is malformed, outside the supported subset, or exceeds a
    /// limit.
    #[cfg(feature = "png")]
    Png {
        /// The byte offset of the problem in the file.
        offset: usize,
        /// What is wrong.
        kind: png::ErrorKind,
    },
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json { line, column, kind } => {
                write!(f, "JSON error at line {line}, column {column}: {kind}")
            }
            Self::Qoi { offset, kind } => write!(f, "QOI error at byte {offset}: {kind}"),
            Self::QoiEncode { kind } => write!(f, "cannot encode as QOI: {kind}"),
            Self::Inflate { offset, kind } => write!(f, "inflate error at byte {offset}: {kind}"),
            Self::Klz { offset, kind } => write!(f, "KLZ error at byte {offset}: {kind}"),
            Self::Kpak { offset, kind } => write!(f, "pack error at byte {offset}: {kind}"),
            Self::Kbj { offset, kind } => write!(f, "KBJ error at byte {offset}: {kind}"),
            Self::KbjEncode { kind } => write!(f, "cannot encode as KBJ: {kind}"),
            Self::Atlas { at, kind } if at.is_empty() => {
                write!(f, "atlas error in the file: {kind}")
            }
            Self::Atlas { at, kind } => write!(f, "atlas error at `{at}`: {kind}"),
            Self::Aseprite { at, kind } if at.is_empty() => {
                write!(f, "Aseprite export error in the file: {kind}")
            }
            Self::Aseprite { at, kind } => write!(f, "Aseprite export error at `{at}`: {kind}"),
            Self::Ldtk { at, kind } if at.is_empty() => {
                write!(f, "LDtk error in the file: {kind}")
            }
            Self::Ldtk { at, kind } => write!(f, "LDtk error at `{at}`: {kind}"),
            Self::Level { at, kind } if at.is_empty() => {
                write!(f, "level file error in the file: {kind}")
            }
            Self::Level { at, kind } => write!(f, "level file error at `{at}`: {kind}"),
            Self::Prefab { at, kind } if at.is_empty() => {
                write!(f, "prefab error in the file: {kind}")
            }
            Self::Prefab { at, kind } => write!(f, "prefab error at `{at}`: {kind}"),
            Self::Wav { offset, kind } => write!(f, "WAV error at byte {offset}: {kind}"),
            Self::BmFont { line, column, kind } => {
                write!(f, "BMFont error at line {line}, column {column}: {kind}")
            }
            #[cfg(feature = "png")]
            Self::Png { offset, kind } => write!(f, "PNG error at byte {offset}: {kind}"),
        }
    }
}

impl std::error::Error for CodecError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_format_the_position_and_the_problem() {
        let error = CodecError::Json {
            line: 3,
            column: 14,
            kind: json::ErrorKind::LeadingZero,
        };
        assert_eq!(
            error.to_string(),
            "JSON error at line 3, column 14: numbers cannot have leading zeros"
        );
        let error = CodecError::Qoi {
            offset: 12,
            kind: qoi::ErrorKind::BadChannels { found: 5 },
        };
        assert_eq!(
            error.to_string(),
            "QOI error at byte 12: the channel count is 5, not 3 or 4"
        );
        let error = CodecError::QoiEncode {
            kind: qoi::EncodeErrorKind::AlphaInRgb { pixel: 3 },
        };
        assert_eq!(
            error.to_string(),
            "cannot encode as QOI: pixel 3 of an RGB image has an alpha other than 255"
        );
    }
}
