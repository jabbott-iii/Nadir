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

//! The OS layer of the Nadir engine: a window and its events, OpenGL
//! contexts, audio output, the user's folders, an error dialog and a
//! precise sleep, with one safe API on Linux, Windows and macOS.
//!
//! # Where it fits
//!
//! This crate wraps the raw declarations of [`nadir_sys`], and is where the
//! rest of Nadir meets the operating system, through one API that is the
//! same on every system. `nadir-gfx` draws through its [`GlContext`],
//! `nadir-input` reads its [`Event`]s, and `nadir_engine` opens the window
//! with it and runs the main loop. A game reaches it through `nadir_engine`, which re-exports
//! [`Event`] as `nadir::Event` and, with its own `dev` feature, the
//! allocation counter. Depend on this crate directly for a window and its
//! events without the rest of the engine.
//!
//! # The window
//!
//! - [`Platform::new`] opens the window that a [`WindowConfig`] describes:
//!   its title, its size and smallest size in logical pixels, whether it can
//!   be resized, whether it starts fullscreen, and vsync.
//! - [`Platform::pump_events`] appends the [`Event`]s that arrived since the
//!   last call, and does not wait for any. Call it once a frame.
//! - [`Platform::window_size`] is the inside of the window in physical
//!   pixels, and [`Platform::scale_factor`] the physical pixels a logical
//!   one covers.
//! - [`Platform::set_fullscreen`], [`Platform::set_title`] and
//!   [`Platform::set_cursor_visible`] change the window.
//! - [`Platform::set_modal_frame_callback`] sets what runs while the system
//!   holds the event loop, and `pump_events` does not return: while Windows
//!   drags or resizes the window, and while macOS resizes it. The callback
//!   gets a [`ModalFrame`] with the size and the scale factor, so that the
//!   game can run frames meanwhile. Linux never calls it.
//!
//! # Events
//!
//! An [`Event`] is something that happened to the window or to the keyboard
//! and mouse, in the order the system reported it:
//!
//! - One [`Event::ScaleFactorChanged`] comes before any other event. On
//!   Windows and macOS another comes each time the scale factor changes.
//! - [`Event::CloseRequested`] leaves the window open: the game decides.
//! - [`Event::Key`] names the physical key, a [`nadir_core::Key`], whatever
//!   the keyboard layout, and says whether a press is a repeat.
//! - [`Event::Text`] is a character typed, after the key press that typed
//!   it: one a character, a composed one included, never a control
//!   character, and none for a shortcut. On Linux it comes from an X input
//!   method, on Windows from `WM_CHAR`, and on macOS from a key press's
//!   `characters`.
//! - [`Event::MouseMoved`] is in physical pixels from the top-left corner of
//!   the inside of the window, with y growing downward, on every system.
//!   [`Event::MouseWheel`] is in lines.
//! - Right after [`Event::Focused`]`(false)`, every key and button that is
//!   down gets a release, so nothing stays held while the window cannot see
//!   it go up.
//!
//! # Drawing
//!
//! - [`Platform::gpu_surface`] gives what the GPU draws the window with: for
//!   now always [`GpuSurface::Gl`], an OpenGL context current on the thread
//!   that opened the window.
//! - [`GlContext`] looks GL functions up ([`GlContext::get_proc_address`]),
//!   shows a frame ([`GlContext::swap_buffers`]) and sets the swap interval.
//!   The functions it returns are called through the types of
//!   [`nadir_sys::gl`], which is `unsafe`; `nadir-gfx` does that, behind a
//!   safe API.
//! - [`create_headless_gl`] makes a [`HeadlessGl`], an OpenGL 3.3 core
//!   context that draws only offscreen, into render targets, with no window
//!   and no display server: for tests and golden images.
//! - A thread has at most one OpenGL context, the window's or a headless
//!   one, and it stays current there until it is dropped. A second fails
//!   with [`PlatformError::GlContextExists`]. Each context this crate makes
//!   has a [`GlContextId`] ([`GlContext::context_id`]), which says whether
//!   the context still exists, so that a user can keep the id in place of a
//!   borrow. The stand-in of Windows and macOS (below) has none.
//!
//! # Audio
//!
//! - [`open_audio`] opens the default output device, as an
//!   [`AudioOutputConfig`] asks (48 kHz stereo, 30 ms of latency by
//!   default), and starts an audio thread that feeds it until the
//!   [`AudioStream`] it returns is dropped. [`AudioStream::latency`] is the
//!   device's buffer, or `None` while there is no device.
//! - The thread calls an [`AudioRender`], given to `open_audio`, a period
//!   at a time: [`AudioRender::render`] fills it, and
//!   [`AudioRender::underrun`] hears of each time the device ran dry. It
//!   must not block, allocate, free or log, as `nadir-audio`'s mixer does
//!   not; a panic in it ends the stream.
//! - A device that stops working, as one unplugged does, or that is not
//!   there at the start, is logged, and the thread renders on in time while
//!   it tries to open it again once a second, and logs when it has. It logs
//!   nothing else.
//! - It needs no window: open it before [`Platform::new`], or without one.
//!
//! # The game's files, errors and timing
//!
//! - [`user_dirs`] works out the folders for the game's settings, saves and
//!   logs, as a [`UserDirs`]. It creates none of them. It rejects a studio
//!   or game name that some system would refuse as a folder name, on every
//!   system, so that a name that works on one works on all.
//! - [`show_error_dialog`] tells the player about an error that stops the
//!   game: on stderr, and also in a message box on Windows and an alert on
//!   macOS.
//! - [`precise_sleep`] blocks until an [`Instant`](std::time::Instant), for
//!   a frame limiter. It sleeps, then spins through the last millisecond,
//!   and never returns early.
//!
//! # Examples
//!
//! Open a window and run until it is closed:
//!
//! ```no_run
//! use std::time::{Duration, Instant};
//!
//! use nadir_core::log::{debug, info};
//! use nadir_core::{Key, UVec2};
//! use nadir_platform::{Event, Platform, WindowConfig, precise_sleep};
//!
//! let mut platform = Platform::new(&WindowConfig {
//!     title: "My game".to_string(),
//!     size: UVec2::new(640, 360),
//!     min_size: UVec2::ZERO,
//!     resizable: true,
//!     fullscreen: false,
//!     vsync: true,
//! })?;
//! // Frames to run while Windows drags the window, or macOS resizes it.
//! platform.set_modal_frame_callback(Box::new(|frame| {
//!     debug!("a modal frame at {}x{}", frame.size.x, frame.size.y);
//! }));
//! let mut events = Vec::new();
//! let mut fullscreen = false;
//! let mut next_frame = Instant::now();
//! 'run: loop {
//!     events.clear();
//!     platform.pump_events(&mut events);
//!     for &event in &events {
//!         match event {
//!             Event::CloseRequested => break 'run,
//!             Event::Key { key: Key::F11, pressed: true, repeat: false } => {
//!                 fullscreen = !fullscreen;
//!                 platform.set_fullscreen(fullscreen);
//!             }
//!             Event::Resized(size) => info!("now {}x{} physical pixels", size.x, size.y),
//!             _ => {}
//!         }
//!     }
//!     next_frame += Duration::from_micros(16_667);
//!     precise_sleep(next_frame);
//! }
//! # Ok::<(), nadir_platform::PlatformError>(())
//! ```
//!
//! Draw offscreen, and ask the driver for its name through the GL function
//! types of `nadir-sys`, which a program that does this depends on too:
//!
//! ```no_run
//! use core::ffi::{CStr, c_void};
//! use core::mem;
//!
//! use nadir_platform::{GlContext, create_headless_gl};
//! use nadir_sys::gl::{GL_RENDERER, PFNGLGETSTRINGPROC};
//!
//! let gl = create_headless_gl()?;
//! let pointer = gl.get_proc_address(c"glGetString");
//! assert!(!pointer.is_null());
//! // SAFETY: the pointer is not null, and is `glGetString`, of this type.
//! let get_string =
//!     unsafe { mem::transmute::<*const c_void, PFNGLGETSTRINGPROC>(pointer) };
//! // SAFETY: the context is current on this thread, and takes this name.
//! let name = unsafe { get_string(GL_RENDERER) };
//! assert!(!name.is_null());
//! // SAFETY: the string is not null, ends with a NUL, and lives as long as
//! // the context.
//! let renderer = unsafe { CStr::from_ptr(name.cast()) };
//! nadir_core::log::info!("drawing with {}", renderer.to_string_lossy());
//! # Ok::<(), nadir_platform::PlatformError>(())
//! ```
//!
//! # Errors
//!
//! Every call that can fail returns a [`PlatformError`], whose message says
//! what went wrong and, where the player can fix it, how: which library or
//! function is missing, or that a Wayland session needs XWayland turned on.
//! The calls that change the window, such as [`Platform::set_title`],
//! return nothing, and log what fails. [`PlatformError`] is
//! `#[non_exhaustive]`: new kinds of error can join it.
//!
//! # Threads
//!
//! A [`Platform`] stays on the thread that opened it, and so does its OpenGL
//! context. On macOS that must be the main thread: on any other,
//! [`Platform::new`] fails with [`PlatformError::NotMainThread`]. A
//! [`HeadlessGl`] is neither `Send` nor `Sync`.
//!
//! [`open_audio`] starts a thread of its own, `nadir-audio`, which alone
//! calls the [`AudioRender`] and the system's audio functions. The
//! [`AudioStream`] it returns is `Send` and `Sync` on every system, and can
//! be opened and dropped on any thread.
//!
//! # Counting allocations (feature `dev`)
//!
//! With the `dev` feature, the global allocator of every binary that links
//! this crate counts heap allocations, for each thread and for the whole
//! process, so that tests can show that a steady frame allocates nothing.
//! `thread_alloc_counts` and `process_alloc_counts` read the counts, as an
//! `AllocCounts`, and `AllocCounts::since` takes the difference of two
//! readings. No other crate in the binary may declare a global allocator.
//! Turn `dev` on only from a `[features]` table or a `[dev-dependencies]`
//! entry, so that no release build carries it. The counter sees only what
//! goes through Rust's allocator: C libraries that call `malloc`, such as
//! Mesa and Xlib, are not counted.
//!
//! ```
//! # #[cfg(feature = "dev")]
//! # {
//! use std::hint::black_box;
//!
//! use nadir_platform::thread_alloc_counts;
//!
//! let mut numbers: Vec<u32> = Vec::with_capacity(64);
//! let start = thread_alloc_counts();
//! for number in 0..64 {
//!     numbers.push(black_box(number));
//! }
//! // The pushes fit the capacity, so nothing was allocated.
//! assert_eq!(thread_alloc_counts().since(start).allocations, 0);
//! # }
//! ```
//!
//! The `headless-test` feature is for the workspace's own tests, which need
//! an EGL driver with OpenGL 3.3 on Linux.
//!
//! # Platforms
//!
//! - **Linux**, on X11: everything above. A Wayland session runs the window
//!   through XWayland. Xlib (`libX11.so.6`), EGL (`libEGL.so.1`) and ALSA
//!   (`libasound.so.2`) are loaded while the program runs, not linked, so
//!   that a machine without them gets [`PlatformError::LibraryNotFound`] or
//!   [`PlatformError::MissingSymbols`]. The window's context and
//!   [`create_headless_gl`] are OpenGL 3.3 core through EGL; the headless
//!   one needs `EGL_MESA_platform_surfaceless`, which Mesa and NVIDIA have.
//!   The scale factor is read once, from `Xft.dpi`, when the window opens.
//!   Typed text comes from the X input method that `XMODIFIERS` names, or
//!   else Xlib's own, which composes from the locale's Compose file.
//!   Audio goes to ALSA's `default` device, which PipeWire or PulseAudio
//!   usually provides.
//! - **Windows** 10 and later: a Win32 window, aware of each monitor's DPI,
//!   with borderless fullscreen. It builds and passes its tests in CI, and
//!   still awaits its checks by hand (roadmap task M1.5). It has no OpenGL
//!   context yet (M2.6): [`Platform::gpu_surface`] is a stand-in with no
//!   functions, whose swaps fail with [`PlatformError::Unsupported`], and
//!   [`create_headless_gl`] returns that error. [`WindowConfig::vsync`] is
//!   ignored there for now, and [`open_audio`] returns
//!   [`PlatformError::Unsupported`] until M6.4.
//! - **macOS** 12 and later: an AppKit window, whose fullscreen is the
//!   system's own, in a Space of its own. It builds and passes its tests in
//!   CI, and still awaits its checks by hand (M1.6). It has no OpenGL context
//!   yet (M2.7): the same stand-in as on Windows, and `vsync` is ignored.
//!   Nor has it audio output yet (M6.5).
//! - On any other system, [`Platform::new`], [`user_dirs`],
//!   [`create_headless_gl`] and [`open_audio`] return
//!   [`PlatformError::Unsupported`].
//!
//! Not built yet on any system: gamepads (M8.1 to M8.3), Metal (M10.1),
//! showing a text being composed, noticing that the window was minimized,
//! holding the pointer inside the window, and native Wayland. macOS has no
//! input method for typed text yet.

// One of the three crates allowed to contain `unsafe` (AGENTS.md, rule 3). The
// workspace denies `unsafe_code` everywhere else.
#![allow(unsafe_code)]

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

#[cfg(feature = "dev")]
mod alloc_counter;
mod audio;
mod dialog;
mod error;
mod event;
mod gpu;
mod paths;
mod sleep;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
mod text;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "linux"))]
mod no_audio;
#[cfg(not(target_os = "linux"))]
mod no_gl;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

use nadir_core::UVec2;

#[cfg(feature = "dev")]
pub use alloc_counter::{AllocCounts, process_alloc_counts, thread_alloc_counts};
pub use audio::{AudioOutputConfig, AudioRender, AudioStream, open_audio};
pub use dialog::show_error_dialog;
pub use error::PlatformError;
pub use event::Event;
pub use gpu::{GlContext, GlContextId, GpuSurface, HeadlessGl, create_headless_gl};
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
    /// On Linux, it first sets the C library's character locale
    /// (`LC_CTYPE`) from the environment, if it is still "C", which a Rust
    /// program never changes: X's input methods type in it, and in "C" they
    /// compose Latin-1 characters only. The locale is the whole process's,
    /// and changing it races with another thread's C code that reads it
    /// meanwhile, so open the window before starting threads that call such
    /// code. A program that has set a locale of its own keeps it.
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
