//! The error type for `nadir-core`.

use std::fmt;

/// Errors produced by `nadir-core`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreError {
    /// A hex color string was not `RRGGBB` or `RRGGBBAA`, with an optional
    /// leading `#`.
    InvalidHexColor,
    /// A command-line option that takes a value was given without one.
    MissingOptionValue {
        /// The option, as the caller named it (for example `--secs`).
        option: String,
    },
    /// The value of a command-line option could not be parsed.
    InvalidOptionValue {
        /// The option, as the caller named it (for example `--secs`).
        option: String,
        /// The value that was given.
        value: String,
    },
    /// A command-line argument that nothing asked for: an unknown option, or
    /// a known one given twice or with a value it does not take.
    UnexpectedArgument {
        /// The argument as typed.
        argument: String,
    },
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHexColor => f.write_str(
                "invalid hex color: expected `RRGGBB` or `RRGGBBAA`, optionally prefixed with `#`",
            ),
            Self::MissingOptionValue { option } => write!(f, "option `{option}` needs a value"),
            Self::InvalidOptionValue { option, value } => {
                write!(f, "invalid value `{value}` for option `{option}`")
            }
            Self::UnexpectedArgument { argument } => write!(f, "unexpected argument `{argument}`"),
        }
    }
}

impl std::error::Error for CoreError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_expected_format() {
        let message = CoreError::InvalidHexColor.to_string();
        assert!(message.contains("RRGGBB"), "{message}");
    }

    #[test]
    fn display_names_the_offending_argument() {
        let missing = CoreError::MissingOptionValue {
            option: "--secs".into(),
        };
        assert_eq!(missing.to_string(), "option `--secs` needs a value");
        let invalid = CoreError::InvalidOptionValue {
            option: "--secs".into(),
            value: "soon".into(),
        };
        assert_eq!(
            invalid.to_string(),
            "invalid value `soon` for option `--secs`"
        );
        let unexpected = CoreError::UnexpectedArgument {
            argument: "--frobnicate".into(),
        };
        assert_eq!(unexpected.to_string(), "unexpected argument `--frobnicate`");
    }
}
