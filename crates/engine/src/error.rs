//! The error type of the `nadir` facade.

use std::fmt;

use nadir_platform::PlatformError;
use nadir_render::RenderError;

/// Errors that end [`App::run`](crate::App::run).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The window could not be opened.
    Platform(PlatformError),
    /// The renderer could not start, or a frame could not be drawn or
    /// shown.
    Render(RenderError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(error) => write!(f, "could not open the window: {error}"),
            Self::Render(error) => write!(f, "could not draw: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(error) => Some(error),
            Self::Render(error) => Some(error),
        }
    }
}

impl From<RenderError> for Error {
    fn from(error: RenderError) -> Self {
        Self::Render(error)
    }
}

impl From<nadir_gfx::GfxError> for Error {
    fn from(error: nadir_gfx::GfxError) -> Self {
        Self::Render(RenderError::Gfx(error))
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

    #[test]
    fn a_render_error_says_it_could_not_draw() {
        let error = Error::from(nadir_gfx::GfxError::ContextLost);
        assert!(error.to_string().starts_with("could not draw: "), "{error}");
        assert!(error.source().is_some());
    }
}
