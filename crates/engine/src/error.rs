//! The error type of the `nadir` facade.

use std::fmt;

use nadir_platform::PlatformError;

/// Errors that end [`App::run`](crate::App::run).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The window could not be opened.
    Platform(PlatformError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(error) => write!(f, "could not open the window: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(error) => Some(error),
        }
    }
}

impl From<PlatformError> for Error {
    fn from(error: PlatformError) -> Self {
        Self::Platform(error)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn a_platform_error_says_what_failed_and_why() {
        let error = Error::from(PlatformError::Unsupported);
        assert_eq!(
            error.to_string(),
            "could not open the window: this operating system has no backend yet"
        );
        let source = error.source().map(ToString::to_string);
        assert_eq!(
            source.as_deref(),
            Some("this operating system has no backend yet")
        );
    }
}
