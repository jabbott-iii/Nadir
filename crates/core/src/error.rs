//! The error type for `nadir-core`.

use std::fmt;

/// Errors produced by `nadir-core`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreError {
    /// A hex color string was not `RRGGBB` or `RRGGBBAA`, with an optional
    /// leading `#`.
    InvalidHexColor,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHexColor => f.write_str(
                "invalid hex color: expected `RRGGBB` or `RRGGBBAA`, optionally prefixed with `#`",
            ),
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
}
