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

//! The error type of the `nadir` facade.

use std::fmt;

use nadir_platform::PlatformError;
use nadir_render::RenderError;

use crate::LevelError;

/// Errors that end [`App::run`](crate::App::run).
///
/// It is `#[non_exhaustive]` since roadmap M9.1, which added
/// [`Error::Level`] (ADR-0067).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Error {
    /// The window could not be opened.
    Platform(PlatformError),
    /// The renderer could not start, or a frame could not be drawn or
    /// shown.
    Render(RenderError),
    /// The level of the game state `Loading` could not be loaded, or,
    /// without the `dev` feature, a texture of it failed to load.
    Level {
        /// The level's name.
        name: String,
        /// Why, boxed: it is much larger than the other variants.
        error: Box<LevelError>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(error) => write!(f, "could not open the window: {error}"),
            Self::Render(error) => write!(f, "could not draw: {error}"),
            Self::Level { name, error } => write!(f, "could not load the level `{name}`: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::Level { error, .. } => Some(error.as_ref()),
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
    fn a_level_error_names_the_level_and_says_why() {
        let error = Error::Level {
            name: "cave".to_string(),
            error: Box::new(LevelError::Load {
                path: "levels/cave.klvl".to_string(),
                error: nadir_assets::AssetError::NotFound,
            }),
        };
        assert_eq!(
            error.to_string(),
            "could not load the level `cave`: `levels/cave.klvl` could not be loaded: no such file"
        );
        assert!(error.source().is_some());
    }

    #[test]
    fn a_render_error_says_it_could_not_draw() {
        let error = Error::from(nadir_gfx::GfxError::ContextLost);
        assert!(error.to_string().starts_with("could not draw: "), "{error}");
        assert!(error.source().is_some());
    }
}
