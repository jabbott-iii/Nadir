//! EGL, which connects OpenGL to the window system (`libEGL.so.1`).
//!
//! Sources: `<EGL/egl.h>`, `<EGL/eglext.h>` and `<EGL/eglplatform.h>`, the
//! Khronos headers as libglvnd 1.7.0 installs them, and `<KHR/khrplatform.h>`
//! for the integer types. Each item names its header and quotes the
//! declaration there; those of `egl.h` and `eglext.h` also say which EGL
//! version or extension brings them.
//!
//! Only what `nadir-platform` uses is declared.
//!
//! # The table and EGL 1.5
//!
//! [`Egl`] holds functions of EGL 1.4 and earlier, which every
//! `libEGL.so.1` exports. `eglGetPlatformDisplay` is EGL 1.5: a library
//! that is older would fail the whole table, so it is looked up with
//! `eglGetProcAddress` instead, and called through
//! [`PFNEGLGETPLATFORMDISPLAYPROC`]. Whether the library is EGL 1.5 shows in
//! `eglQueryString(EGL_NO_DISPLAY, EGL_VERSION)`, which an older library
//! answers with null and `EGL_BAD_DISPLAY`; that `eglGetProcAddress` finds
//! the function proves nothing by itself. The attributes of EGL 1.5 that are
//! declared here work on an older EGL only with the extension each one
//! names.
//!
//! # Displays
//!
//! The same native display gives the same `EGLDisplay`, for the whole
//! process, and its initialization is not counted: one `eglTerminate` ends it
//! for every user of the handle. A display handle is never freed.
//!
//! # Constants
//!
//! A constant keeps its header's name and value. One that the header gives a
//! type (`EGL_CAST(EGLDisplay,0)`) has that type. One that the header leaves
//! as a bare number (`#define EGL_NONE 0x3038`) has the type of the
//! parameter or list it is used in: attribute names and values are
//! `EGLint`s, platforms and APIs `EGLenum`s.
//!
//! # Calling EGL
//!
//! What every function in [`Egl`] requires of its caller:
//!
//! - A `dpy` argument came from `eglGetDisplay` or `eglGetPlatformDisplay`,
//!   and has been initialized and not terminated. The exceptions are
//!   `eglInitialize`, `eglTerminate`, `eglQueryString` with `EGL_NO_DISPLAY`,
//!   and the `eglMakeCurrent` that releases the current context (with
//!   `EGL_NO_SURFACE`, `EGL_NO_SURFACE` and `EGL_NO_CONTEXT`).
//! - A native display is an open Xlib `Display *`, or `EGL_DEFAULT_DISPLAY`,
//!   and stays open while its `EGLDisplay` is in use. A native window is an
//!   existing window of that same X display. EGL makes native handles that
//!   are not valid undefined behavior (EGL 1.5, section 3.1.2).
//! - A config, surface or context argument belongs to that display, or is
//!   the `EGL_NO_*` value that the function allows.
//! - A pointer argument is valid for what the function reads or writes
//!   through it, and an attribute list ends with `EGL_NONE`.
//! - A context is current on at most one thread at a time.
//!
//! A handle that is stale or of the wrong display is not a memory error in
//! Mesa or libglvnd: the function fails and sets an error, which
//! `eglGetError` returns. A function that returns `EGLBoolean` returns
//! [`EGL_FALSE`] on failure.

// `__eglMustCastToProperFunctionPointerType` keeps the name its header gives
// it, so that it can be compared with the header.
#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_void};

use crate::dynlib::dynlib;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A truth value: 1 or [`EGL_FALSE`]. `EGL/egl.h` (EGL 1.0): `typedef
/// unsigned int EGLBoolean;`
pub type EGLBoolean = u32;

/// A 32-bit integer: an attribute, a value, an error code.
/// `EGL/eglplatform.h`: `typedef khronos_int32_t EGLint;`, and
/// `KHR/khrplatform.h`: `typedef int32_t khronos_int32_t;`
pub type EGLint = i32;

/// A number from a list of choices, such as a platform or a client API.
/// `EGL/egl.h` (EGL 1.2): `typedef unsigned int EGLenum;`
pub type EGLenum = u32;

/// An attribute value the size of a pointer, for the EGL 1.5 functions.
/// `EGL/egl.h` (EGL 1.5): `typedef intptr_t EGLAttrib;`
pub type EGLAttrib = isize;

/// A display: EGL's connection to the window system, and to a driver.
/// `EGL/egl.h` (EGL 1.0): `typedef void *EGLDisplay;`
pub type EGLDisplay = *mut c_void;

/// A frame-buffer configuration.
/// `EGL/egl.h` (EGL 1.0): `typedef void *EGLConfig;`
pub type EGLConfig = *mut c_void;

/// Something to draw to: a window or a pbuffer. `EGL/egl.h` (EGL 1.0):
/// `typedef void *EGLSurface;`
pub type EGLSurface = *mut c_void;

/// A rendering context of a client API.
/// `EGL/egl.h` (EGL 1.0): `typedef void *EGLContext;`
pub type EGLContext = *mut c_void;

/// The window system's display, for `eglGetDisplay`: on X11, an Xlib
/// `Display *`. `EGL/eglplatform.h`: `typedef void *EGLNativeDisplayType;`
/// (on Unix; with `USE_X11` it is `Display *`, the same to the ABI)
pub type EGLNativeDisplayType = *mut c_void;

/// The window system's window, for `eglCreateWindowSurface`: on X11, a
/// `Window`. `EGL/eglplatform.h`: `typedef khronos_uintptr_t
/// EGLNativeWindowType;` (on Unix; with `USE_X11` it is `Window`, an
/// `unsigned long`, the same to the ABI)
pub type EGLNativeWindowType = usize;

// An X window id is passed as one: the two have one size.
const _: () = assert!(size_of::<EGLNativeWindowType>() == size_of::<super::x11::Window>());

/// What `eglGetProcAddress` returns: a function, to be cast to its real
/// type, or null. `EGL/egl.h` (EGL 1.0): `typedef void
/// (*__eglMustCastToProperFunctionPointerType)(void);`
pub type __eglMustCastToProperFunctionPointerType = Option<unsafe extern "C" fn()>;

/// `eglGetPlatformDisplay`, as `eglGetProcAddress` finds it (EGL 1.5). It
/// returns the display of `platform` for `native_display`, or `EGL_NO_DISPLAY`.
/// `attrib_list` may be null, or a list of `EGLAttrib`s that ends with
/// `EGL_NONE`. As with `eglGetDisplay`, the same arguments give the same
/// handle, for the whole process, and the handle is never freed.
/// `EGL/egl.h` (EGL 1.5): `typedef EGLDisplay (EGLAPIENTRYP
/// PFNEGLGETPLATFORMDISPLAYPROC) (EGLenum platform, void *native_display, const
/// EGLAttrib *attrib_list);`
///
/// # Safety
///
/// As for the functions of [`Egl`]. Also: `eglQueryString(EGL_NO_DISPLAY,
/// EGL_VERSION)` gave 1.5 or later, the pointer came from
/// `eglGetProcAddress("eglGetPlatformDisplay")` of that library,
/// `native_display` is what the platform takes, and `attrib_list` is null or
/// ends with `EGL_NONE`.
pub type PFNEGLGETPLATFORMDISPLAYPROC = unsafe extern "C" fn(
    platform: EGLenum,
    native_display: *mut c_void,
    attrib_list: *const EGLAttrib,
) -> EGLDisplay;

// ---------------------------------------------------------------------------
// Results and errors
// ---------------------------------------------------------------------------

/// The false result of a function that returns an `EGLBoolean`.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_FALSE 0`
pub const EGL_FALSE: EGLBoolean = 0;

/// The last call succeeded.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_SUCCESS 0x3000`
pub const EGL_SUCCESS: EGLint = 0x3000;

/// EGL is not initialized for the display, or `eglInitialize` could not
/// initialize it: there is no driver for it, for example.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_NOT_INITIALIZED 0x3001`
pub const EGL_NOT_INITIALIZED: EGLint = 0x3001;

/// EGL could not allocate what the call needed.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_BAD_ALLOC 0x3003`
pub const EGL_BAD_ALLOC: EGLint = 0x3003;

/// An attribute or its value is not valid, or not supported by this EGL.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_BAD_ATTRIBUTE 0x3004`
pub const EGL_BAD_ATTRIBUTE: EGLint = 0x3004;

/// The config is not valid.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_BAD_CONFIG 0x3005`
pub const EGL_BAD_CONFIG: EGLint = 0x3005;

/// The display is not valid.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_BAD_DISPLAY 0x3008`
pub const EGL_BAD_DISPLAY: EGLint = 0x3008;

/// The arguments do not go together: for example, a context that the driver
/// cannot make, or a window whose visual does not match the config.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_BAD_MATCH 0x3009`
pub const EGL_BAD_MATCH: EGLint = 0x3009;

/// The window of the window system is not valid.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_BAD_NATIVE_WINDOW 0x300B`
pub const EGL_BAD_NATIVE_WINDOW: EGLint = 0x300B;

/// An argument is not valid, such as an unknown platform or API.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_BAD_PARAMETER 0x300C`
pub const EGL_BAD_PARAMETER: EGLint = 0x300C;

/// The surface is not valid.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_BAD_SURFACE 0x300D`
pub const EGL_BAD_SURFACE: EGLint = 0x300D;

/// A power-management event lost the context, which must be made again.
/// `EGL/egl.h` (EGL 1.1): `#define EGL_CONTEXT_LOST 0x300E`
pub const EGL_CONTEXT_LOST: EGLint = 0x300E;

// ---------------------------------------------------------------------------
// Displays, contexts and surfaces
// ---------------------------------------------------------------------------

/// The default display of the window system, for `eglGetDisplay`, and the
/// display the surfaceless platform takes.
/// `EGL/egl.h` (EGL 1.4): `#define EGL_DEFAULT_DISPLAY
/// EGL_CAST(EGLNativeDisplayType,0)`
pub const EGL_DEFAULT_DISPLAY: EGLNativeDisplayType = core::ptr::null_mut();

/// No display: what `eglGetDisplay` returns on failure.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_NO_DISPLAY EGL_CAST(EGLDisplay,0)`
pub const EGL_NO_DISPLAY: EGLDisplay = core::ptr::null_mut();

/// No context: the share context to pass when there is none, and what
/// `eglCreateContext` returns on failure.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_NO_CONTEXT EGL_CAST(EGLContext,0)`
pub const EGL_NO_CONTEXT: EGLContext = core::ptr::null_mut();

/// No surface: what the surface functions return on failure.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_NO_SURFACE EGL_CAST(EGLSurface,0)`
pub const EGL_NO_SURFACE: EGLSurface = core::ptr::null_mut();

// ---------------------------------------------------------------------------
// Platforms
// ---------------------------------------------------------------------------

/// Displays of an X server; the native display is an Xlib `Display *`.
/// `EGL/eglext.h` (EGL_KHR_platform_x11): `#define EGL_PLATFORM_X11_KHR 0x31D5`
pub const EGL_PLATFORM_X11_KHR: EGLenum = 0x31D5;

/// Mesa's platform without a window system, for rendering offscreen; the native
/// display must be `EGL_DEFAULT_DISPLAY`.
/// `EGL/eglext.h` (EGL_MESA_platform_surfaceless): `#define
/// EGL_PLATFORM_SURFACELESS_MESA 0x31DD`
pub const EGL_PLATFORM_SURFACELESS_MESA: EGLenum = 0x31DD;

// ---------------------------------------------------------------------------
// Attribute lists
// ---------------------------------------------------------------------------

/// The end of an attribute list.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_NONE 0x3038`
pub const EGL_NONE: EGLint = 0x3038;

/// A config attribute: the bits of red in a pixel.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_RED_SIZE 0x3024`
pub const EGL_RED_SIZE: EGLint = 0x3024;

/// A config attribute: the bits of green.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_GREEN_SIZE 0x3023`
pub const EGL_GREEN_SIZE: EGLint = 0x3023;

/// A config attribute: the bits of blue.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_BLUE_SIZE 0x3022`
pub const EGL_BLUE_SIZE: EGLint = 0x3022;

/// A config attribute: the kinds of surface the config can make, as
/// `EGL_*_BIT`s.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_SURFACE_TYPE 0x3033`
pub const EGL_SURFACE_TYPE: EGLint = 0x3033;

/// A value of `EGL_SURFACE_TYPE`: window surfaces.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_WINDOW_BIT 0x0004`
pub const EGL_WINDOW_BIT: EGLint = 0x0004;

/// A value of `EGL_SURFACE_TYPE`: pbuffer surfaces, which are offscreen.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_PBUFFER_BIT 0x0001`
pub const EGL_PBUFFER_BIT: EGLint = 0x0001;

/// A config attribute: the client APIs that can draw with it, as `EGL_*_BIT`s.
/// `EGL/egl.h` (EGL 1.2): `#define EGL_RENDERABLE_TYPE 0x3040`
pub const EGL_RENDERABLE_TYPE: EGLint = 0x3040;

/// A value of `EGL_RENDERABLE_TYPE`: desktop OpenGL.
/// `EGL/egl.h` (EGL 1.4): `#define EGL_OPENGL_BIT 0x0008`
pub const EGL_OPENGL_BIT: EGLint = 0x0008;

/// A config attribute to read with `eglGetConfigAttrib` (`eglChooseConfig`
/// ignores it): the id of the X visual of the config, for the window, or 0 if
/// it has none.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_NATIVE_VISUAL_ID 0x302E`
pub const EGL_NATIVE_VISUAL_ID: EGLint = 0x302E;

/// A config attribute: the smallest swap interval of a surface of the config.
/// `eglSwapInterval` raises a smaller one to it, without an error.
/// `EGL/egl.h` (EGL 1.1): `#define EGL_MIN_SWAP_INTERVAL 0x303B`
pub const EGL_MIN_SWAP_INTERVAL: EGLint = 0x303B;

/// A config attribute: the largest swap interval of a surface of the config.
/// `eglSwapInterval` lowers a larger one to it, without an error.
/// `EGL/egl.h` (EGL 1.1): `#define EGL_MAX_SWAP_INTERVAL 0x303C`
pub const EGL_MAX_SWAP_INTERVAL: EGLint = 0x303C;

/// A context attribute: the major version of the API. The same number is
/// `EGL_CONTEXT_MAJOR_VERSION_KHR` of `EGL_KHR_create_context`, which an EGL
/// older than 1.5 needs for a desktop OpenGL context; it is also
/// `EGL_CONTEXT_CLIENT_VERSION` of EGL 1.3, which applies only to OpenGL ES.
/// `EGL/egl.h` (EGL 1.5): `#define EGL_CONTEXT_MAJOR_VERSION 0x3098`
pub const EGL_CONTEXT_MAJOR_VERSION: EGLint = 0x3098;

/// A context attribute: the minor version of the API. The same number is
/// `EGL_CONTEXT_MINOR_VERSION_KHR` of `EGL_KHR_create_context`, which an EGL
/// older than 1.5 needs for it.
/// `EGL/egl.h` (EGL 1.5): `#define EGL_CONTEXT_MINOR_VERSION 0x30FB`
pub const EGL_CONTEXT_MINOR_VERSION: EGLint = 0x30FB;

/// A context attribute: the OpenGL profile, as an
/// `EGL_CONTEXT_OPENGL_*_PROFILE_BIT`. The same number is
/// `EGL_CONTEXT_OPENGL_PROFILE_MASK_KHR` of `EGL_KHR_create_context`, which an
/// EGL older than 1.5 needs for it.
/// `EGL/egl.h` (EGL 1.5): `#define EGL_CONTEXT_OPENGL_PROFILE_MASK 0x30FD`
pub const EGL_CONTEXT_OPENGL_PROFILE_MASK: EGLint = 0x30FD;

/// A value of `EGL_CONTEXT_OPENGL_PROFILE_MASK`: the core profile. The same
/// number is `EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT_KHR` of
/// `EGL_KHR_create_context`.
/// `EGL/egl.h` (EGL 1.5): `#define EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT
/// 0x00000001`
pub const EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT: EGLint = 0x0000_0001;

/// A window-surface attribute: how the surface's pixels encode color. The same
/// number is `EGL_GL_COLORSPACE_KHR` of `EGL_KHR_gl_colorspace`, which an EGL
/// older than 1.5 needs for it.
/// `EGL/egl.h` (EGL 1.5): `#define EGL_GL_COLORSPACE 0x309D`
pub const EGL_GL_COLORSPACE: EGLint = 0x309D;

/// A value of `EGL_GL_COLORSPACE`: sRGB, so that `GL_FRAMEBUFFER_SRGB` takes
/// effect on the default framebuffer. The same number is
/// `EGL_GL_COLORSPACE_SRGB_KHR` of `EGL_KHR_gl_colorspace`.
/// `EGL/egl.h` (EGL 1.5): `#define EGL_GL_COLORSPACE_SRGB 0x3089`
pub const EGL_GL_COLORSPACE_SRGB: EGLint = 0x3089;

/// A pbuffer attribute: the width in pixels.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_WIDTH 0x3057`
pub const EGL_WIDTH: EGLint = 0x3057;

/// A pbuffer attribute: the height in pixels.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_HEIGHT 0x3056`
pub const EGL_HEIGHT: EGLint = 0x3056;

// ---------------------------------------------------------------------------
// Strings and APIs
// ---------------------------------------------------------------------------

/// The EGL version of the display, as `"<major>.<minor> <vendor text>"`. With
/// `EGL_NO_DISPLAY`, that of the client library (EGL 1.5); an older library
/// returns null and sets `EGL_BAD_DISPLAY`.
/// `EGL/egl.h` (EGL 1.0): `#define EGL_VERSION 0x3054`
pub const EGL_VERSION: EGLint = 0x3054;

/// The extensions, separated by spaces: of the display, or with
/// `EGL_NO_DISPLAY` those of the client library (EGL 1.5, or
/// `EGL_EXT_client_extensions`; an older library returns null and sets
/// `EGL_BAD_DISPLAY`).
/// `EGL/egl.h` (EGL 1.0): `#define EGL_EXTENSIONS 0x3055`
pub const EGL_EXTENSIONS: EGLint = 0x3055;

/// Desktop OpenGL, for `eglBindAPI`.
/// `EGL/egl.h` (EGL 1.4): `#define EGL_OPENGL_API 0x30A2`
pub const EGL_OPENGL_API: EGLenum = 0x30A2;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

dynlib! {
    /// The functions we call in `libEGL.so.1`, all of EGL 1.4 or earlier.
    ///
    /// The module documentation lists what every one of them requires.
    pub struct Egl: extern "C" {
        /// Returns the EGL display for a display of the window system, or
        /// `EGL_NO_DISPLAY`. It does not initialize it. The same display gives
        /// the same handle, for the whole process, and the handle is never
        /// freed (see the module documentation).
        /// `EGL/egl.h` (EGL 1.0): `EGLDisplay eglGetDisplay
        /// (EGLNativeDisplayType display_id);`
        fn eglGetDisplay(display_id: EGLNativeDisplayType) -> EGLDisplay;

        /// Initializes a display, and writes its EGL version to `major` and
        /// `minor` when they are not null. Initializing it again does nothing
        /// more.
        /// `EGL/egl.h` (EGL 1.0): `EGLBoolean eglInitialize (EGLDisplay dpy,
        /// EGLint *major, EGLint *minor);`
        fn eglInitialize(dpy: EGLDisplay, major: *mut EGLint, minor: *mut EGLint) -> EGLBoolean;

        /// Releases what EGL holds for a display, for every user of its handle:
        /// initialization is not counted. Contexts and surfaces that are
        /// current are destroyed only once they are made not current, but their
        /// handles are invalid at once, and drawing with them is undefined:
        /// only the `eglMakeCurrent` that releases them should follow.
        /// Terminating a display that is not initialized does nothing.
        /// `EGL/egl.h` (EGL 1.0): `EGLBoolean eglTerminate (EGLDisplay dpy);`
        fn eglTerminate(dpy: EGLDisplay) -> EGLBoolean;

        /// Returns a string about the display: `EGL_VERSION` or
        /// `EGL_EXTENSIONS`, for example. It belongs to EGL; do not free it.
        /// With `EGL_NO_DISPLAY` it answers for the client library (EGL 1.5, or
        /// `EGL_EXT_client_extensions` for the extensions). Returns null on
        /// failure.
        /// `EGL/egl.h` (EGL 1.0): `const char *eglQueryString (EGLDisplay dpy,
        /// EGLint name);`
        fn eglQueryString(dpy: EGLDisplay, name: EGLint) -> *const c_char;

        /// Makes `api` the client API of this thread, for the contexts it makes
        /// next.
        /// `EGL/egl.h` (EGL 1.2): `EGLBoolean eglBindAPI (EGLenum api);`
        fn eglBindAPI(api: EGLenum) -> EGLBoolean;

        /// Writes up to `config_size` configs that match `attrib_list`, best
        /// first, to `configs`, and their number to `num_config`.
        /// `EGL/egl.h` (EGL 1.0): `EGLBoolean eglChooseConfig (EGLDisplay dpy,
        /// const EGLint *attrib_list, EGLConfig *configs, EGLint config_size,
        /// EGLint *num_config);`
        fn eglChooseConfig(
            dpy: EGLDisplay,
            attrib_list: *const EGLint,
            configs: *mut EGLConfig,
            config_size: EGLint,
            num_config: *mut EGLint,
        ) -> EGLBoolean;

        /// Writes the value of one attribute of a config to `value`.
        /// `EGL/egl.h` (EGL 1.0): `EGLBoolean eglGetConfigAttrib (EGLDisplay
        /// dpy, EGLConfig config, EGLint attribute, EGLint *value);`
        fn eglGetConfigAttrib(
            dpy: EGLDisplay,
            config: EGLConfig,
            attribute: EGLint,
            value: *mut EGLint,
        ) -> EGLBoolean;

        /// Makes a surface that draws into a window of the window system.
        /// Returns `EGL_NO_SURFACE` on failure. The surface is ours: destroy it
        /// with `eglDestroySurface`.
        /// `EGL/egl.h` (EGL 1.0): `EGLSurface eglCreateWindowSurface
        /// (EGLDisplay dpy, EGLConfig config, EGLNativeWindowType win, const
        /// EGLint *attrib_list);`
        fn eglCreateWindowSurface(
            dpy: EGLDisplay,
            config: EGLConfig,
            win: EGLNativeWindowType,
            attrib_list: *const EGLint,
        ) -> EGLSurface;

        /// Makes an offscreen surface of the size `attrib_list` gives. Returns
        /// `EGL_NO_SURFACE` on failure. The surface is ours: destroy it with
        /// `eglDestroySurface`.
        /// `EGL/egl.h` (EGL 1.0): `EGLSurface eglCreatePbufferSurface
        /// (EGLDisplay dpy, EGLConfig config, const EGLint *attrib_list);`
        fn eglCreatePbufferSurface(
            dpy: EGLDisplay,
            config: EGLConfig,
            attrib_list: *const EGLint,
        ) -> EGLSurface;

        /// Destroys a surface, once it is not current on any thread.
        /// `EGL/egl.h` (EGL 1.0): `EGLBoolean eglDestroySurface (EGLDisplay
        /// dpy, EGLSurface surface);`
        fn eglDestroySurface(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean;

        /// Makes a rendering context of the API bound to this thread, sharing
        /// objects with `share_context` unless that is `EGL_NO_CONTEXT`.
        /// Returns `EGL_NO_CONTEXT` on failure. The context is ours: destroy it
        /// with `eglDestroyContext`.
        /// `EGL/egl.h` (EGL 1.0): `EGLContext eglCreateContext (EGLDisplay dpy,
        /// EGLConfig config, EGLContext share_context, const EGLint *attrib_list);`
        fn eglCreateContext(
            dpy: EGLDisplay,
            config: EGLConfig,
            share_context: EGLContext,
            attrib_list: *const EGLint,
        ) -> EGLContext;

        /// Destroys a context, once it is not current on any thread.
        /// `EGL/egl.h` (EGL 1.0): `EGLBoolean eglDestroyContext (EGLDisplay
        /// dpy, EGLContext ctx);`
        fn eglDestroyContext(dpy: EGLDisplay, ctx: EGLContext) -> EGLBoolean;

        /// Makes `ctx` the current context of this thread, drawing to `draw`
        /// and reading from `read`. A context is current on one thread at a
        /// time.
        /// `EGL/egl.h` (EGL 1.0): `EGLBoolean eglMakeCurrent (EGLDisplay dpy,
        /// EGLSurface draw, EGLSurface read, EGLContext ctx);`
        fn eglMakeCurrent(
            dpy: EGLDisplay,
            draw: EGLSurface,
            read: EGLSurface,
            ctx: EGLContext,
        ) -> EGLBoolean;

        /// Shows what was drawn to a window surface, after the swap interval.
        /// `EGL/egl.h` (EGL 1.0): `EGLBoolean eglSwapBuffers (EGLDisplay dpy,
        /// EGLSurface surface);`
        fn eglSwapBuffers(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean;

        /// Sets how many vertical retraces each swap waits for, for the surface
        /// of the context current on this thread, from its next
        /// `eglSwapBuffers`: 1 is vsync, 0 none. A value outside the config's
        /// `EGL_MIN_SWAP_INTERVAL` to `EGL_MAX_SWAP_INTERVAL` is brought inside
        /// it, without an error. Needs a current context.
        /// `EGL/egl.h` (EGL 1.1): `EGLBoolean eglSwapInterval (EGLDisplay dpy,
        /// EGLint interval);`
        fn eglSwapInterval(dpy: EGLDisplay, interval: EGLint) -> EGLBoolean;

        /// Returns the error of the last EGL call on this thread, and resets it
        /// to `EGL_SUCCESS`.
        /// `EGL/egl.h` (EGL 1.0): `EGLint eglGetError (void);`
        fn eglGetError() -> EGLint;

        /// Returns the function called `procname`, of EGL or of a client API
        /// such as OpenGL, or null. A non-null result does not mean that the
        /// function is supported: for one of EGL, check the version or the
        /// extension string; for one of GL, the version or extensions of the
        /// context. A function that is not an extension's can be looked up only
        /// on EGL 1.5, or with `EGL_KHR_get_all_proc_addresses` or
        /// `EGL_KHR_client_get_all_proc_addresses`; EGL 1.4 does not allow it,
        /// and the platform falls back to `dlsym`.
        /// `EGL/egl.h` (EGL 1.0): `__eglMustCastToProperFunctionPointerType
        /// eglGetProcAddress (const char *procname);`
        fn eglGetProcAddress(procname: *const c_char) -> __eglMustCastToProperFunctionPointerType;
    }
}

#[cfg(test)]
mod tests {
    use core::ffi::{CStr, c_void};

    use super::*;
    use crate::dynlib::Library;
    use crate::linux::libc::{RTLD_LOCAL, RTLD_NOW, dlopen, dlsym};

    /// A library opened with `dlopen`.
    struct Opened(*mut c_void);

    impl Library for Opened {
        fn symbol(&self, name: &CStr) -> *const c_void {
            // SAFETY: the handle came from `dlopen` and is never closed, and
            // the name is a C string.
            unsafe { dlsym(self.0, name.as_ptr()) }
        }
    }

    #[test]
    fn the_installed_egl_has_every_function_of_the_table() {
        // SAFETY: the name is a C string, and libEGL is sound to load into
        // any process.
        let handle = unsafe { dlopen(c"libEGL.so.1".as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        if handle.is_null() {
            // This machine has no EGL, so there is nothing to compare with.
            return;
        }
        // SAFETY: the library is libEGL, which defines these functions as its
        // headers declare them, and it is never unloaded.
        let table = unsafe { Egl::load(&Opened(handle)) };
        let egl = match table {
            Ok(egl) => egl,
            Err(missing) => panic!("libEGL.so.1: {missing}"),
        };
        // The EGL 1.5 function, where the library exports it, is what
        // `eglGetProcAddress` finds, and then the library says it is 1.5
        // when asked without a display. Neither needs a display or a
        // driver.
        // SAFETY: the handle came from `dlopen`, and the name is a C string.
        let exported = unsafe { dlsym(handle, c"eglGetPlatformDisplay".as_ptr()) };
        if exported.is_null() {
            // An older library, which the platform allows for.
            return;
        }
        // SAFETY: the name is a C string.
        let found = unsafe { (egl.eglGetProcAddress)(c"eglGetPlatformDisplay".as_ptr()) };
        let found: Option<PFNEGLGETPLATFORMDISPLAYPROC> = found.map(|function| {
            // SAFETY: the function is eglGetPlatformDisplay, whose type this
            // is. The pointer is only converted, not called.
            unsafe {
                core::mem::transmute::<unsafe extern "C" fn(), PFNEGLGETPLATFORMDISPLAYPROC>(
                    function,
                )
            }
        });
        assert_eq!(
            found.map(|function| function as *const c_void),
            Some(exported.cast_const())
        );
        // SAFETY: `EGL_NO_DISPLAY` with `EGL_VERSION` asks the client library,
        // which on EGL 1.5 needs nothing initialized.
        let version = unsafe { (egl.eglQueryString)(EGL_NO_DISPLAY, EGL_VERSION) };
        assert!(!version.is_null());
        // SAFETY: EGL returned a NUL-terminated string that it owns, and
        // that lives as long as the library.
        let version = unsafe { CStr::from_ptr(version) }.to_string_lossy();
        let number = version.split(' ').next().unwrap_or_default();
        let mut parts = number.split('.').map(str::parse::<u32>);
        let (Some(Ok(major)), Some(Ok(minor))) = (parts.next(), parts.next()) else {
            panic!("not an EGL version: {version}");
        };
        assert!((major, minor) >= (1, 5), "{version}");
        // SAFETY: `eglGetError` takes nothing.
        assert_eq!(unsafe { (egl.eglGetError)() }, EGL_SUCCESS);
    }
}
