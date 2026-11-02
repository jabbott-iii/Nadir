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

//! The backend of an operating system that has no backend yet.
//!
//! It cannot be created, so [`Backend`] has no values and none of its other
//! methods can be reached. It exists so that the crate builds everywhere.

use nadir_core::UVec2;

use crate::{Event, GpuSurface, ModalFrame, PlatformError, UserDirs, WindowConfig};

/// The user's folders are unknown without a backend.
pub(crate) fn user_dirs(_studio: &str, _game: &str) -> Result<UserDirs, PlatformError> {
    Err(PlatformError::Unsupported)
}

pub(crate) enum Backend {}

impl Backend {
    pub(crate) fn new(_config: &WindowConfig) -> Result<Self, PlatformError> {
        Err(PlatformError::Unsupported)
    }

    pub(crate) fn pump_events(&mut self, _out: &mut Vec<Event>) {
        match *self {}
    }

    pub(crate) fn window_size(&self) -> UVec2 {
        match *self {}
    }

    pub(crate) fn scale_factor(&self) -> f32 {
        match *self {}
    }

    pub(crate) fn set_fullscreen(&mut self, _on: bool) {
        match *self {}
    }

    pub(crate) fn set_title(&mut self, _title: &str) {
        match *self {}
    }

    pub(crate) fn set_cursor_visible(&mut self, _visible: bool) {
        match *self {}
    }

    pub(crate) fn set_modal_frame_callback(&mut self, _callback: Box<dyn FnMut(ModalFrame)>) {
        match *self {}
    }

    pub(crate) fn gpu_surface(&self) -> GpuSurface<'_> {
        match *self {}
    }
}
