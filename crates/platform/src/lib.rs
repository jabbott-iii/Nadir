//! OS behavior for the Nadir engine: windows, the event loop, input devices,
//! GL/Metal surfaces, audio device output, paths and dynamic loading.
//!
//! So far there is a window with its events, on Linux (X11, and Wayland
//! sessions through XWayland), and [`precise_sleep`] on every system. On
//! other systems [`Platform::new`] returns [`PlatformError::Unsupported`].

// One of the three crates allowed to contain `unsafe` (AGENTS.md, rule 3). The
// workspace denies `unsafe_code` everywhere else.
#![allow(unsafe_code)]

mod error;
mod event;
mod sleep;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(target_os = "linux"))]
mod unsupported;

use nadir_core::UVec2;

pub use error::PlatformError;
pub use event::Event;
pub use sleep::precise_sleep;

#[cfg(target_os = "linux")]
use linux::Backend;
#[cfg(not(target_os = "linux"))]
use unsupported::Backend;

/// What window to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowConfig {
    /// The text in the title bar.
    pub title: String,
    /// The size of the inside of the window, in logical pixels. The window
    /// gets this many physical pixels times the scale factor.
    pub size: UVec2,
    /// The smallest size the user can give the window, in logical pixels. A
    /// zero means no limit in that direction.
    pub min_size: UVec2,
    /// Whether the user can resize the window.
    pub resizable: bool,
    /// Whether the window starts fullscreen. Fullscreen is borderless: the
    /// window covers the monitor, and the video mode does not change.
    pub fullscreen: bool,
}

/// The game's window and its connection to the operating system.
///
/// It stays on the thread that created it.
pub struct Platform {
    backend: Backend,
}

impl Platform {
    /// Opens the window.
    ///
    /// # Errors
    ///
    /// Fails if this system has no backend, if there is no display server
    /// to connect to, or if a system library is missing or too old.
    pub fn new(config: &WindowConfig) -> Result<Self, PlatformError> {
        Ok(Self {
            backend: Backend::new(config)?,
        })
    }

    /// Appends the events that arrived since the last call to `out`. Does
    /// not wait for any.
    pub fn pump_events(&mut self, out: &mut Vec<Event>) {
        self.backend.pump_events(out);
    }

    /// The size of the inside of the window, in physical pixels.
    pub fn window_size(&self) -> UVec2 {
        self.backend.window_size()
    }

    /// How many physical pixels one logical pixel covers: 1.0 on a standard
    /// display, 2.0 on one of twice the density.
    pub fn scale_factor(&self) -> f32 {
        self.backend.scale_factor()
    }

    /// Enters or leaves borderless fullscreen.
    pub fn set_fullscreen(&mut self, on: bool) {
        self.backend.set_fullscreen(on);
    }

    /// Changes the text in the title bar.
    pub fn set_title(&mut self, title: &str) {
        self.backend.set_title(title);
    }

    /// Shows or hides the pointer while it is over the window.
    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.backend.set_cursor_visible(visible);
    }
}
