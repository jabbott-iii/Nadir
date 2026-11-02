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
