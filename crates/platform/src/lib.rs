//! OS behavior for the Nadir engine: windows, the event loop, input devices,
//! GL/Metal surfaces, audio device output, paths and dynamic loading.
//!
//! So far there is a window with its events and the user's folders
//! ([`user_dirs`]) on Linux (X11, and Wayland sessions through XWayland),
//! Windows and macOS, and [`precise_sleep`] and [`show_error_dialog`]. On
//! any other system [`Platform::new`], [`user_dirs`] and
//! [`create_headless_gl`] return [`PlatformError::Unsupported`].
//!
//! On Linux the window draws with an OpenGL 3.3 core context
//! ([`Platform::gpu_surface`]), and [`create_headless_gl`] makes one that
//! draws offscreen. Windows and macOS have no OpenGL context yet: their
//! surface has no functions, and [`create_headless_gl`] returns
//! [`PlatformError::Unsupported`].

// One of the three crates allowed to contain `unsafe` (AGENTS.md, rule 3). The
// workspace denies `unsafe_code` everywhere else.
#![allow(unsafe_code)]

mod dialog;
mod error;
mod event;
mod gpu;
mod paths;
mod sleep;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "linux"))]
mod no_gl;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

use nadir_core::UVec2;

pub use dialog::show_error_dialog;
pub use error::PlatformError;
pub use event::Event;
pub use gpu::{GlContext, GpuSurface, HeadlessGl, create_headless_gl};
pub use paths::{UserDirs, user_dirs};
pub use sleep::precise_sleep;

#[cfg(target_os = "linux")]
use linux::Backend;
#[cfg(target_os = "macos")]
use macos::Backend;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
use unsupported::Backend;
#[cfg(target_os = "windows")]
use windows::Backend;

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
    /// Whether a swap waits for the display's refresh, so that frames do not
    /// tear: a swap interval of 1, or else 0. If the driver cannot do it, the
    /// window opens anyway, and a warning is logged. See
    /// [`GlContext::set_swap_interval`]. Windows and macOS ignore it for now.
    pub vsync: bool,
}

/// What a modal frame callback is told: see
/// [`Platform::set_modal_frame_callback`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModalFrame {
    /// The size of the inside of the window, in physical pixels.
    pub size: UVec2,
    /// The window's scale factor, as [`Platform::scale_factor`] gives it.
    pub scale_factor: f32,
}

/// The game's window and its connection to the operating system.
///
/// It stays on the thread that created it. On Linux it holds that thread's
/// OpenGL context, so the thread cannot have another (see [`GlContext`]).
pub struct Platform {
    backend: Backend,
}

impl Platform {
    /// Opens the window.
    ///
    /// # Errors
    ///
    /// Fails if this system has no backend, if there is no display server
    /// to connect to, or if a system library is missing or too old. On
    /// macOS it fails on any thread but the main one. On Linux it also fails
    /// if the graphics driver cannot make an OpenGL 3.3 core context for the
    /// window, or if this thread already has an OpenGL context.
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

    /// Sets what to call while the operating system runs an event loop of
    /// its own, and [`Platform::pump_events`] therefore does not return:
    /// while the user drags or resizes the window on Windows, or resizes it
    /// on macOS. It is called every 10 to 16 ms on Windows, and on macOS
    /// once for each step of the resize, so that the game can run frames.
    /// It replaces any callback set before.
    ///
    /// The events of that time come from the next pump, after the loop.
    /// On Linux there is no such loop, and the callback is never called.
    pub fn set_modal_frame_callback(&mut self, callback: Box<dyn FnMut(ModalFrame)>) {
        self.backend.set_modal_frame_callback(callback);
    }

    /// What the GPU draws the window with. On Linux it is the window's
    /// OpenGL context, current on this thread. On Windows and macOS, for
    /// now, it is a context with no functions, whose swaps fail with
    /// [`PlatformError::Unsupported`].
    pub fn gpu_surface(&self) -> GpuSurface<'_> {
        self.backend.gpu_surface()
    }
}
