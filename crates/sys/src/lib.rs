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

//! Hand-written raw FFI declarations for the Nadir engine: the types,
//! constants and functions of the system APIs that the engine calls.
//!
//! This crate declares and does nothing else: types, constants, `extern`
//! blocks, function types and symbol tables, each with the header it comes
//! from, and layout assertions. Its only code is what calling needs: the
//! tables' loaders, and `msg_send!` on macOS.
//!
//! # Where it fits
//!
//! Nadir keeps every raw OS declaration here. `nadir-platform` (windows,
//! events, contexts, paths) and `nadir-gfx` (the OpenGL backend) call them
//! and wrap them in safe APIs, which are the same on every system; games
//! reach those through `nadir_engine` and never use this crate. Depend on
//! it directly only to write OS code of your own on the same declarations,
//! and keep to what each call requires.
//!
//! # What is declared
//!
//! On every system:
//!
//! - [`gl`]: the types of OpenGL 3.3 core, the constants the engine passes,
//!   and a function type for each GL function it calls, such as
//!   [`PFNGLCLEARPROC`](gl::PFNGLCLEARPROC). GL is never linked: each
//!   function is looked up through the loader of the current context and
//!   cast to its type here. The module lists what every GL call requires.
//! - [`dynlib`]: the [`Library`](dynlib::Library) trait, through which a
//!   symbol table looks its functions up in a library that was loaded while
//!   the program runs, and [`MissingSymbols`](dynlib::MissingSymbols), the
//!   error that names every function the library lacks. The macro that
//!   makes the tables is private to this crate.
//!
//! On Linux, the module `linux`:
//!
//! - `linux::libc`: the dynamic loader, `dlopen`, `dlsym` and `dlerror`.
//!   There is no `dlclose`: a library stays loaded until the process exits,
//!   so the function pointers of a table never dangle.
//! - `linux::x11`: the Xlib types, constants and events the engine uses,
//!   and `Xlib`, the table of its functions in `libX11.so.6`.
//! - `linux::egl`: the EGL types and constants, `Egl`, the table of its
//!   functions of EGL 1.4 and earlier in `libEGL.so.1`, and the type of
//!   EGL 1.5's `eglGetPlatformDisplay`, which is looked up apart, after a
//!   version check, so that an older library still loads.
//! - `linux::evdev`: the kernel's key codes. An X server whose keyboard
//!   rules are `evdev`, as on every current Linux desktop, reports each one
//!   plus 8.
//!
//! On Windows, the module `windows`: `windows::types` (handles, `RECT`,
//! `GUID` and the like), and the functions and constants of `kernel32`,
//! `user32`, `gdi32`, `shell32` and `ole32` that a window, its messages,
//! the user's folders and a message box need. They are linked through the
//! Windows SDK's import libraries, and are all in Windows 10.
//!
//! On macOS, the module `macos`: `macos::objc`, the Objective-C runtime
//! and its `MessageArguments`, through which the `msg_send!` macro sends a
//! message; `macos::foundation` (`NSRect` and the other geometry, the
//! integers, strings and run-loop modes); `macos::appkit` (the constants
//! that AppKit's messages take); and `macos::hitoolbox` (the keyboard's
//! virtual key codes, `kVK_*`).
//!
//! A system's module exists only on that system. docs.rs documents this
//! crate for Linux, Windows and macOS: choose the platform there to see one.
//!
//! Not declared yet: WGL and NSOpenGL (roadmap tasks M2.6 and M2.7), the
//! audio APIs (ALSA, WASAPI and CoreAudio, M6.3 to M6.5), the gamepad APIs
//! (evdev's, XInput and GameController, M8.1 to M8.3), Metal (M10.1), and X
//! input methods for typed text (M7.2).
//!
//! # Examples
//!
//! Calling a GL function through the type declared for it, with a pointer
//! from the context's loader (`nadir-platform`'s `GlContext` gives one):
//!
//! ```
//! use core::ffi::{CStr, c_void};
//! use core::mem;
//!
//! use nadir_sys::gl::{GL_RENDERER, GLubyte, PFNGLGETSTRINGPROC};
//!
//! /// The name of the renderer, such as `llvmpipe (LLVM 20.1.2, 256 bits)`,
//! /// or `None` if the loader has no `glGetString` or the call fails.
//! ///
//! /// # Safety
//! ///
//! /// An OpenGL 3.3 core context is current on this thread, and `load`
//! /// returns its function of each name, or null.
//! unsafe fn renderer(load: impl Fn(&CStr) -> *const c_void) -> Option<String> {
//!     let pointer = load(c"glGetString");
//!     if pointer.is_null() {
//!         return None;
//!     }
//!     // SAFETY: the pointer is not null, and is `glGetString`, of this type.
//!     let get_string =
//!         unsafe { mem::transmute::<*const c_void, PFNGLGETSTRINGPROC>(pointer) };
//!     // SAFETY: the context is current, and `GL_RENDERER` is a name it
//!     // takes.
//!     let name: *const GLubyte = unsafe { get_string(GL_RENDERER) };
//!     if name.is_null() {
//!         return None;
//!     }
//!     // SAFETY: the string is not null, ends with a NUL, and lives as long
//!     // as the context, which outlives this call.
//!     let name = unsafe { CStr::from_ptr(name.cast()) };
//!     Some(name.to_string_lossy().into_owned())
//! }
//! ```
//!
//! On Linux, loading Xlib with a [`Library`](dynlib::Library) of your own,
//! as `nadir-platform` does. A library that lacks some of the table's
//! functions gives one error that names them all:
//!
//! ```no_run
//! # #[cfg(target_os = "linux")]
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use core::ffi::{CStr, c_void};
//!
//! use nadir_sys::dynlib::Library;
//! use nadir_sys::linux::libc::{RTLD_LOCAL, RTLD_NOW, dlopen, dlsym};
//! use nadir_sys::linux::x11::Xlib;
//!
//! /// A library from `dlopen`, which is never closed.
//! struct Loaded(*mut c_void);
//!
//! impl Library for Loaded {
//!     fn symbol(&self, name: &CStr) -> *const c_void {
//!         // SAFETY: the handle came from `dlopen` and is never closed, and
//!         // `name` ends with a NUL.
//!         unsafe { dlsym(self.0, name.as_ptr()) }
//!     }
//! }
//!
//! // SAFETY: the name ends with a NUL, and Xlib is sound to load.
//! let handle = unsafe { dlopen(c"libX11.so.6".as_ptr(), RTLD_NOW | RTLD_LOCAL) };
//! if handle.is_null() {
//!     return Err("could not load libX11.so.6".into());
//! }
//! // SAFETY: the library is Xlib, which the table was written for, and it
//! // stays loaded until the process exits.
//! let xlib = unsafe { Xlib::load(&Loaded(handle)) }?;
//! # let _ = xlib;
//! # Ok(())
//! # }
//! # #[cfg(not(target_os = "linux"))]
//! # fn main() {}
//! ```
//!
//! # Using the declarations
//!
//! - **Sources:** every item names the header it comes from and quotes its
//!   declaration there, so that it can be checked against the header. A
//!   constant keeps its header's name and value; a struct has all of its
//!   header's fields, in order.
//! - **Safety:** almost every function is an `unsafe fn`. The few declared
//!   `safe`, such as `dlerror` and `objc_autoreleasePoolPush`, have no
//!   precondition for any argument. Each module says what its functions
//!   require of a caller, besides each function's own documentation; that
//!   of [`gl`] covers every GL call.
//! - **Layouts:** each struct and union whose fields are declared has its
//!   size and alignment, and the offsets of the fields the engine reads,
//!   asserted at compile time, so a target with another layout fails the
//!   build. Opaque types, which only the system looks inside, have no
//!   fields and no assertions. The Linux numbers were measured by a C
//!   program on x86_64 Linux, and hold wherever `long` and pointers are 8
//!   bytes. The Windows and macOS numbers were worked out from Microsoft's
//!   and Apple's documented declarations, and have not been checked against
//!   the SDK headers yet.
//! - **Calling conventions:** `extern "system"` for Win32 and GL, `extern
//!   "C"` everywhere else. `msg_send!` casts `objc_msgSend` to the exact
//!   signature of each call, which arm64 requires, and on x86_64 sends a
//!   result of more than 16 bytes through `objc_msgSend_stret`.
//! - **Threads:** AppKit messages go to the main thread only; a Win32 window
//!   is used only from the thread that made it; a GL function needs its
//!   context current on the calling thread.
//! - **Allocation:** nothing here allocates, except a table's `load` when
//!   the library lacks functions: it collects their names.

// One of the three crates allowed to contain `unsafe` (AGENTS.md, rule 3). The
// workspace denies `unsafe_code` everywhere else.
#![allow(unsafe_code)]

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod dynlib;
pub mod gl;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;
