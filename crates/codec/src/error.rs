//! The error type for `nadir-codec`.

use std::fmt;

use crate::{json, qoi};

/// Errors produced by `nadir-codec`.
#[derive(Clone, Debug, PartialEq, Eq)]
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
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json { line, column, kind } => {
                write!(f, "JSON error at line {line}, column {column}: {kind}")
            }
            Self::Qoi { offset, kind } => write!(f, "QOI error at byte {offset}: {kind}"),
            Self::QoiEncode { kind } => write!(f, "cannot encode as QOI: {kind}"),
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
