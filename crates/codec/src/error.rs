//! The error type for `nadir-codec`.

use std::fmt;

use crate::json;

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
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json { line, column, kind } => {
                write!(f, "JSON error at line {line}, column {column}: {kind}")
            }
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
    }
}
