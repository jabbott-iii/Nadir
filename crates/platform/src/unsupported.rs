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
