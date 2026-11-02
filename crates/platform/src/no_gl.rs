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

//! Stand-ins for the OpenGL contexts that this system cannot make yet.

use core::ffi::{CStr, c_void};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use core::ptr;

#[cfg(any(target_os = "windows", target_os = "macos"))]
use nadir_core::UVec2;

#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::GlContext;
use crate::PlatformError;
use crate::gpu::GlContextId;

/// The OpenGL context of a window that has none yet. It has no functions,
/// and cannot show anything.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) struct NoGl;

/// The one stand-in, which every window shares.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) static NO_GL: NoGl = NoGl;

// SAFETY: `get_proc_address` never returns a pointer, so the trait promises
// nothing else.
#[cfg(any(target_os = "windows", target_os = "macos"))]
unsafe impl GlContext for NoGl {
    fn get_proc_address(&self, _name: &CStr) -> *const c_void {
        ptr::null()
    }

    fn swap_buffers(&self) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }

    fn set_swap_interval(&self, _interval: i32) -> bool {
        false
    }

    fn drawable_size(&self) -> UVec2 {
        UVec2::ZERO
    }
}

/// An offscreen context on a system that cannot make one yet. It has no
/// values.
pub(crate) enum NoHeadless {}

impl NoHeadless {
    pub(crate) fn new() -> Result<Self, PlatformError> {
        Err(PlatformError::Unsupported)
    }

    pub(crate) fn get_proc_address(&self, _name: &CStr) -> *const c_void {
        match *self {}
    }

    pub(crate) fn id(&self) -> GlContextId {
        match *self {}
    }
}

#[cfg(test)]
mod tests {
    use crate::{PlatformError, create_headless_gl};

    #[test]
    fn there_is_no_headless_context_yet() {
        assert_eq!(create_headless_gl().err(), Some(PlatformError::Unsupported));
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn the_window_surface_has_no_functions_and_shows_nothing() {
        use nadir_core::UVec2;

        use super::NO_GL;
        use crate::GlContext;

        assert!(NO_GL.get_proc_address(c"glClear").is_null());
        assert_eq!(NO_GL.swap_buffers(), Err(PlatformError::Unsupported));
        assert!(!NO_GL.set_swap_interval(1));
        assert_eq!(NO_GL.drawable_size(), UVec2::ZERO);
    }
}
