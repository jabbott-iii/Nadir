//! The error type for `nadir-render`.

use std::fmt;

use nadir_gfx::GfxError;

/// Errors produced by `nadir-render`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// The GPU refused a call.
    Gfx(GfxError),
    /// The renderer's configuration cannot work.
    BadConfig(&'static str),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gfx(error) => write!(f, "the GPU refused: {error}"),
            Self::BadConfig(reason) => write!(f, "the render configuration cannot work: {reason}"),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Gfx(error) => Some(error),
            Self::BadConfig(_) => None,
        }
    }
}

impl From<GfxError> for RenderError {
    fn from(error: GfxError) -> Self {
        Self::Gfx(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_say_what_went_wrong() {
        assert_eq!(
            RenderError::BadConfig("the virtual resolution is empty").to_string(),
            "the render configuration cannot work: the virtual resolution is empty"
        );
        let error = RenderError::from(GfxError::NoPipeline);
        assert!(error.to_string().starts_with("the GPU refused: "));
        assert!(std::error::Error::source(&error).is_some());
    }
}
