//! The error type for `nadir-codec`.

use std::fmt;

#[cfg(feature = "png")]
use crate::png;
use crate::{inflate, json, qoi};

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
