//! OpenGL contexts through EGL: the window's, on X11, and offscreen ones, on
//! Mesa's surfaceless platform.
//!
//! - **One context a thread.** A thread has at most one context of ours,
//!   which holds the thread's [`Claim`]. Under libglvnd the GL functions
//!   are stubs that call into whichever context is current on the thread,
//!   so with two contexts on one thread, one would draw with the other's
//!   objects.
//! - **The API is set per thread.** `eglBindAPI` applies to the calling
//!   thread only, and a new thread starts with OpenGL ES, so it is called
//!   before every `eglCreateContext`.
//! - **Displays.** EGL gives the same display for the same native display,
//!   to the whole process, and does not count initializations: one
//!   `eglTerminate` ends it for everyone. The surfaceless display is
//!   therefore initialized by every headless context and never terminated,
//!   so it lives until the process exits. The X11 display belongs to one
//!   window, and is terminated before its X connection closes: a new
//!   connection can get the same address, and EGL would hand back the old
//!   display for it.
//! - **Teardown.** A context is released before it is destroyed, also on
//!   the way out: a thread that ends with its context current leaks it.

use core::cell::OnceCell;
use core::ffi::{CStr, c_char, c_void};
use core::ptr::{self, NonNull};

use nadir_core::log::{debug, info, warn};
use nadir_sys::dynlib::Library as _;
use nadir_sys::gl::{
    GL_CONTEXT_CORE_PROFILE_BIT, GL_CONTEXT_PROFILE_MASK, GL_MAJOR_VERSION, GL_MINOR_VERSION,
    GL_RENDERER, GL_VERSION, GLenum, GLint, PFNGLGETINTEGERVPROC, PFNGLGETSTRINGPROC,
};
use nadir_sys::linux::egl::{
    EGL_BAD_ALLOC, EGL_BAD_ATTRIBUTE, EGL_BAD_CONFIG, EGL_BAD_DISPLAY, EGL_BAD_MATCH,
    EGL_BAD_NATIVE_WINDOW, EGL_BAD_PARAMETER, EGL_BAD_SURFACE, EGL_BLUE_SIZE, EGL_CONTEXT_LOST,
    EGL_CONTEXT_MAJOR_VERSION, EGL_CONTEXT_MINOR_VERSION, EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT,
    EGL_CONTEXT_OPENGL_PROFILE_MASK, EGL_DEFAULT_DISPLAY, EGL_EXTENSIONS, EGL_FALSE,
    EGL_GL_COLORSPACE, EGL_GL_COLORSPACE_SRGB, EGL_GREEN_SIZE, EGL_HEIGHT, EGL_MAX_SWAP_INTERVAL,
    EGL_MIN_SWAP_INTERVAL, EGL_NATIVE_VISUAL_ID, EGL_NO_CONTEXT, EGL_NO_DISPLAY, EGL_NO_SURFACE,
    EGL_NONE, EGL_NOT_INITIALIZED, EGL_OPENGL_API, EGL_OPENGL_BIT, EGL_PBUFFER_BIT,
    EGL_PLATFORM_SURFACELESS_MESA, EGL_PLATFORM_X11_KHR, EGL_RED_SIZE, EGL_RENDERABLE_TYPE,
    EGL_SUCCESS, EGL_SURFACE_TYPE, EGL_VERSION, EGL_WIDTH, EGL_WINDOW_BIT, EGLConfig, EGLContext,
    EGLDisplay, EGLNativeWindowType, EGLSurface, EGLint, Egl, PFNEGLGETPLATFORMDISPLAYPROC,
};
use nadir_sys::linux::x11::{Display, VisualID, Window};

use super::library::Library;
use crate::PlatformError;
use crate::gpu::{Claim, GlContextId};

/// The file name of EGL, for the loader.
const LIBEGL: &CStr = c"libEGL.so.1";

/// The file name of EGL, for messages.
const LIBEGL_NAME: &str = "libEGL.so.1";

/// libglvnd's dispatch library for OpenGL, where a GL function can be found
/// that EGL does not give.
const LIBOPENGL: &CStr = c"libOpenGL.so.0";

/// The OpenGL version we ask for, with the core profile.
const GL_REQUIRED: (GLint, GLint) = (3, 3);

// ---------------------------------------------------------------------------
// The library
// ---------------------------------------------------------------------------

/// EGL as loaded, and what the library says of itself.
///
/// The `SAFETY` comments below rely on `egl` having been loaded from libEGL,
/// which is never unloaded.
struct Client {
    egl: Egl,
    /// `eglGetPlatformDisplay`, if the library is EGL 1.5 or later.
    get_platform_display: Option<PFNEGLGETPLATFORMDISPLAYPROC>,
    /// The client extensions, separated by spaces. Empty if the library
    /// cannot say.
    extensions: String,
    /// Whether `eglGetProcAddress` gives the core GL functions too, and not
    /// only those of extensions.
    all_proc_addresses: bool,
}

impl Client {
    /// Loads EGL.
    fn load() -> Result<Self, PlatformError> {
        // SAFETY: libEGL is a system library that is sound to load into any
        // process.
        let library =
            unsafe { Library::open(LIBEGL) }.map_err(|reason| PlatformError::LibraryNotFound {
                library: LIBEGL_NAME,
                reason,
            })?;
        // SAFETY: the library is libEGL, which defines these functions as its
        // headers declare them, and a `Library` is never unloaded.
        let egl =
            unsafe { Egl::load(&library) }.map_err(|missing| PlatformError::MissingSymbols {
                library: LIBEGL_NAME,
                symbols: missing.names,
            })?;
        // An EGL older than 1.5 answers both with null.
        let version = query_client(&egl, EGL_VERSION)
            .as_deref()
            .and_then(parse_version);
        let extensions = query_client(&egl, EGL_EXTENSIONS).unwrap_or_default();
        let is_1_5 = version.is_some_and(|version| version >= (1, 5));
        let get_platform_display = if is_1_5 {
            // SAFETY: the name is a C string.
            let found = unsafe { (egl.eglGetProcAddress)(c"eglGetPlatformDisplay".as_ptr()) };
            found.map(|function| {
                // SAFETY: the library is EGL 1.5, so this is its
                // `eglGetPlatformDisplay`, whose type this is. The pointer is
                // only converted here.
                unsafe {
                    core::mem::transmute::<unsafe extern "C" fn(), PFNEGLGETPLATFORMDISPLAYPROC>(
                        function,
                    )
                }
            })
        } else {
            None
        };
        let all_proc_addresses =
            is_1_5 || has_extension(&extensions, "EGL_KHR_client_get_all_proc_addresses");
        Ok(Self {
            egl,
            get_platform_display,
            extensions,
            all_proc_addresses,
        })
    }

    /// The error of the last EGL call on this thread, as the error of
    /// `call`. Call it right after the call that failed: every EGL call sets
    /// the error anew.
    fn failure(&self, call: &'static str) -> PlatformError {
        // SAFETY: `eglGetError` takes nothing.
        let code = unsafe { (self.egl.eglGetError)() };
        PlatformError::GpuCallFailed {
            call,
            error: error_text(code),
        }
    }

    /// Returns the EGL display of an X server.
    ///
    /// # Safety
    ///
    /// `native` is open, and stays open while the EGL display is in use.
    unsafe fn x11_display(&self, native: NonNull<Display>) -> Result<EGLDisplay, PlatformError> {
        let (display, call) = match self.get_platform_display {
            Some(get_platform_display) => {
                let platforms = ["EGL_KHR_platform_x11", "EGL_EXT_platform_x11"];
                if !platforms
                    .iter()
                    .any(|platform| has_extension(&self.extensions, platform))
                {
                    return Err(PlatformError::GpuCallFailed {
                        call: "eglGetPlatformDisplay",
                        error: "the EGL library has no X11 platform".to_string(),
                    });
                }
                // SAFETY: the native display of the X11 platform is an Xlib
                // `Display *`, which the caller keeps open, and the attribute
                // list may be null.
                let display = unsafe {
                    get_platform_display(EGL_PLATFORM_X11_KHR, native.as_ptr().cast(), ptr::null())
                };
                (display, "eglGetPlatformDisplay")
            }
            None => {
                // An EGL older than 1.5 has only this, which guesses the
                // platform from the display.
                // SAFETY: the native display is an open Xlib `Display *`,
                // which the caller keeps open.
                let display = unsafe { (self.egl.eglGetDisplay)(native.as_ptr().cast()) };
                (display, "eglGetDisplay")
            }
        };
        if display == EGL_NO_DISPLAY {
            return Err(match self.failure(call) {
                // No driver took the display, and none said why.
                PlatformError::GpuCallFailed { call, error }
                    if error == error_text(EGL_SUCCESS) =>
                {
                    PlatformError::GpuCallFailed {
                        call,
                        error: "no EGL driver can draw on this X display".to_string(),
                    }
                }
                other => other,
            });
        }
        Ok(display)
    }

    /// Returns the display of Mesa's surfaceless platform, which needs no
    /// display server.
    fn surfaceless_display(&self) -> Result<EGLDisplay, PlatformError> {
        let get_platform_display = self
            .get_platform_display
            .filter(|_| has_extension(&self.extensions, "EGL_MESA_platform_surfaceless"));
        let Some(get_platform_display) = get_platform_display else {
            return Err(PlatformError::GpuCallFailed {
                call: "eglGetPlatformDisplay",
                error: "the EGL library has no surfaceless platform \
                        (EGL_MESA_platform_surfaceless)"
                    .to_string(),
            });
        };
        // SAFETY: the surfaceless platform takes `EGL_DEFAULT_DISPLAY` as its
        // native display, and the attribute list may be null.
        let display = unsafe {
            get_platform_display(
                EGL_PLATFORM_SURFACELESS_MESA,
                EGL_DEFAULT_DISPLAY,
                ptr::null(),
            )
        };
        if display == EGL_NO_DISPLAY {
            return Err(self.failure("eglGetPlatformDisplay"));
        }
        Ok(display)
    }

    /// Initializes a display, or finds it initialized, and reads its version
    /// and extensions.
    fn initialize(&self, handle: EGLDisplay) -> Result<DisplayInfo, PlatformError> {
        let (mut major, mut minor) = (0, 0);
        // SAFETY: the display came from `eglGetPlatformDisplay` or
        // `eglGetDisplay`, and the version goes to two live `EGLint`s.
        let initialized =
            unsafe { (self.egl.eglInitialize)(handle, &raw mut major, &raw mut minor) };
        if initialized == EGL_FALSE {
            return Err(self.failure("eglInitialize"));
        }
        // SAFETY: the display is initialized.
        let extensions = unsafe { (self.egl.eglQueryString)(handle, EGL_EXTENSIONS) };
        let extensions = if extensions.is_null() {
            String::new()
        } else {
            // SAFETY: EGL returned a NUL-terminated string, which lives until
            // the display is terminated. It is copied here.
            let extensions = unsafe { CStr::from_ptr(extensions) };
            extensions.to_string_lossy().into_owned()
        };
        Ok(DisplayInfo {
            handle,
            version: (major, minor),
            extensions,
        })
    }

    /// Returns the first config, in EGL's order, that draws OpenGL to
    /// surfaces of `surface_type`, has exactly 8 bits each of red, green and
    /// blue, and that `accept` takes.
    fn choose_config(
        &self,
        display: EGLDisplay,
        surface_type: EGLint,
        accept: impl Fn(&ConfigRecord) -> bool,
    ) -> Result<(EGLConfig, ConfigRecord), PlatformError> {
        // EGL_ALPHA_SIZE stays out: asking for alpha would make EGL put
        // configs with alpha first, whose windows a compositor blends with
        // what is behind them.
        let attributes = [
            EGL_SURFACE_TYPE,
            surface_type,
            EGL_RENDERABLE_TYPE,
            EGL_OPENGL_BIT,
            EGL_RED_SIZE,
            8,
            EGL_GREEN_SIZE,
            8,
            EGL_BLUE_SIZE,
            8,
            EGL_NONE,
        ];
        let mut count = 0;
        // SAFETY: the display is initialized, the list ends with `EGL_NONE`,
        // and with no array EGL only writes the count, to a live `EGLint`.
        let chosen = unsafe {
            (self.egl.eglChooseConfig)(
                display,
                attributes.as_ptr(),
                ptr::null_mut(),
                0,
                &raw mut count,
            )
        };
        if chosen == EGL_FALSE {
            return Err(self.failure("eglChooseConfig"));
        }
        let mut configs: Vec<EGLConfig> =
            vec![ptr::null_mut(); usize::try_from(count).unwrap_or(0)];
        // The length came from an `EGLint`.
        let room = EGLint::try_from(configs.len()).unwrap_or(0);
        // SAFETY: as above, and the array has room for `room` configs.
        let chosen = unsafe {
            (self.egl.eglChooseConfig)(
                display,
                attributes.as_ptr(),
                configs.as_mut_ptr(),
                room,
                &raw mut count,
            )
        };
        if chosen == EGL_FALSE {
            return Err(self.failure("eglChooseConfig"));
        }
        configs.truncate(usize::try_from(count).unwrap_or(0));
        let mut records = Vec::with_capacity(configs.len());
        for &config in &configs {
            records.push(ConfigRecord {
                red: self.attribute(display, config, EGL_RED_SIZE)?,
                green: self.attribute(display, config, EGL_GREEN_SIZE)?,
                blue: self.attribute(display, config, EGL_BLUE_SIZE)?,
                visual: self.attribute(display, config, EGL_NATIVE_VISUAL_ID)?,
            });
        }
        let index = pick(&records, accept).ok_or(PlatformError::NoGlConfig)?;
        Ok((configs[index], records[index]))
    }

    /// Reads an attribute of a config.
    fn attribute(
        &self,
        display: EGLDisplay,
        config: EGLConfig,
        attribute: EGLint,
    ) -> Result<EGLint, PlatformError> {
        let mut value = 0;
        // SAFETY: the display is initialized, the config is one of its own,
        // and the value goes to a live `EGLint`.
        let read =
            unsafe { (self.egl.eglGetConfigAttrib)(display, config, attribute, &raw mut value) };
        if read == EGL_FALSE {
            return Err(self.failure("eglGetConfigAttrib"));
        }
        Ok(value)
    }

    /// Makes an OpenGL 3.3 core context of a config.
    fn create_context(
        &self,
        display: &DisplayInfo,
        config: EGLConfig,
    ) -> Result<EGLContext, PlatformError> {
        // Before EGL 1.5, only `EGL_KHR_create_context` can ask for a version
        // of desktop OpenGL.
        if display.version < (1, 5) && !has_extension(&display.extensions, "EGL_KHR_create_context")
        {
            return Err(PlatformError::GlTooOld { found: None });
        }
        // SAFETY: `eglBindAPI` takes a value, and sets the API of this thread.
        let bound = unsafe { (self.egl.eglBindAPI)(EGL_OPENGL_API) };
        if bound == EGL_FALSE {
            // The display has no desktop OpenGL.
            return Err(PlatformError::GlTooOld { found: None });
        }
        let attributes = [
            EGL_CONTEXT_MAJOR_VERSION,
            GL_REQUIRED.0,
            EGL_CONTEXT_MINOR_VERSION,
            GL_REQUIRED.1,
            EGL_CONTEXT_OPENGL_PROFILE_MASK,
            EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT,
            EGL_NONE,
        ];
        // SAFETY: the display is initialized, the config is its own, there is
        // no context to share objects with, and the list ends with
        // `EGL_NONE`.
        let context = unsafe {
            (self.egl.eglCreateContext)(display.handle, config, EGL_NO_CONTEXT, attributes.as_ptr())
        };
        if context == EGL_NO_CONTEXT {
            // SAFETY: `eglGetError` takes nothing.
            let code = unsafe { (self.egl.eglGetError)() };
            return Err(match code {
                // The driver has no such version or profile.
                EGL_BAD_MATCH | EGL_BAD_ATTRIBUTE => PlatformError::GlTooOld { found: None },
                _ => PlatformError::GpuCallFailed {
                    call: "eglCreateContext",
                    error: error_text(code),
                },
            });
        }
        Ok(context)
    }

    /// Releases the context current on this thread, if any. Logs a failure,
    /// which leaves nothing to do.
    fn release_current(&self, display: EGLDisplay) {
        // SAFETY: the display is initialized. With no surfaces and no
        // context, this releases whatever is current on this thread, which
        // is ours by the claim.
        let released = unsafe {
            (self.egl.eglMakeCurrent)(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT)
        };
        if released == EGL_FALSE {
            warn!(
                "could not release the OpenGL context: {}",
                self.failure("eglMakeCurrent")
            );
        }
    }

    /// Destroys a surface. Logs a failure, which leaves nothing to do.
    fn destroy_surface(&self, display: EGLDisplay, surface: EGLSurface) {
        // SAFETY: the display is initialized, and the surface is one of its
        // own, current nowhere and not used after this.
        let destroyed = unsafe { (self.egl.eglDestroySurface)(display, surface) };
        if destroyed == EGL_FALSE {
            warn!(
                "could not destroy an EGL surface: {}",
                self.failure("eglDestroySurface")
            );
        }
    }

    /// Destroys a context. Logs a failure, which leaves nothing to do.
    fn destroy_context(&self, display: EGLDisplay, context: EGLContext) {
        // SAFETY: the display is initialized, and the context is one of its
        // own, current nowhere and not used after this.
        let destroyed = unsafe { (self.egl.eglDestroyContext)(display, context) };
        if destroyed == EGL_FALSE {
            warn!(
                "could not destroy an OpenGL context: {}",
                self.failure("eglDestroyContext")
            );
        }
    }
}

/// Asks the library about itself, which EGL 1.5 can answer with no display.
/// Returns `None` if it cannot.
fn query_client(egl: &Egl, name: EGLint) -> Option<String> {
    // SAFETY: with `EGL_NO_DISPLAY`, EGL 1.5 answers for the library and
    // needs nothing initialized, and an older EGL returns null.
    let text = unsafe { (egl.eglQueryString)(EGL_NO_DISPLAY, name) };
    if text.is_null() {
        return None;
    }
    // SAFETY: EGL returned a NUL-terminated string that it owns, and that
    // lives as long as the library, which is never unloaded.
    let text = unsafe { CStr::from_ptr(text) };
    Some(text.to_string_lossy().into_owned())
}

/// An initialized display, with its version and extensions.
struct DisplayInfo {
    handle: EGLDisplay,
    version: (EGLint, EGLint),
    /// Separated by spaces.
    extensions: String,
}

/// What [`Client::choose_config`] reads of a config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ConfigRecord {
    red: EGLint,
    green: EGLint,
    blue: EGLint,
    /// The id of the X visual of the config, or 0 if it has none.
    visual: EGLint,
}

/// Returns the index of the first record with exactly 8 bits each of red,
/// green and blue that `accept` takes. EGL's order is kept: it puts the
/// configs it would choose first.
fn pick(records: &[ConfigRecord], accept: impl Fn(&ConfigRecord) -> bool) -> Option<usize> {
    records
        .iter()
        .position(|record| (record.red, record.green, record.blue) == (8, 8, 8) && accept(record))
}

/// Looks GL functions up for a context of ours.
struct GlLookup {
    /// Whether to ask `eglGetProcAddress` first.
    from_egl: bool,
    /// libOpenGL, opened the first time it is needed, if it can be.
    libopengl: OnceCell<Option<Library>>,
}

impl GlLookup {
    fn new(client: &Client) -> Self {
        Self {
            from_egl: client.all_proc_addresses,
            libopengl: OnceCell::new(),
        }
    }

    /// Returns the GL function called `name`, or null.
    fn get(&self, egl: &Egl, name: &CStr) -> *const c_void {
        if self.from_egl {
            // SAFETY: the name is a C string.
            let found = unsafe { (egl.eglGetProcAddress)(name.as_ptr()) };
            if let Some(function) = found {
                return function as *const c_void;
            }
        }
        let libopengl = self.libopengl.get_or_init(|| {
            // SAFETY: libOpenGL is libglvnd's dispatch library, a system
            // library that is sound to load into any process. Its functions
            // call into the context current on the thread.
            unsafe { Library::open(LIBOPENGL) }.ok()
        });
        libopengl
            .as_ref()
            .map_or(ptr::null(), |library| library.symbol(name))
    }
}

/// What a new context says it is.
struct GlStrings {
    version: String,
    renderer: String,
}

/// Checks that the context current on this thread is OpenGL 3.3 or later
/// with the core profile, and returns its version and renderer.
fn check_context(lookup: impl Fn(&CStr) -> *const c_void) -> Result<GlStrings, PlatformError> {
    let get_string = lookup(c"glGetString");
    let get_integer = lookup(c"glGetIntegerv");
    for (call, function) in [("glGetString", get_string), ("glGetIntegerv", get_integer)] {
        if function.is_null() {
            return Err(PlatformError::GpuCallFailed {
                call,
                error: "the graphics driver does not have it".to_string(),
            });
        }
    }
    // SAFETY: the pointer is `glGetString`, whose type this is. It is only
    // converted here.
    let get_string =
        unsafe { core::mem::transmute::<*const c_void, PFNGLGETSTRINGPROC>(get_string) };
    // SAFETY: the pointer is `glGetIntegerv`, whose type this is. It is only
    // converted here.
    let get_integer =
        unsafe { core::mem::transmute::<*const c_void, PFNGLGETINTEGERVPROC>(get_integer) };
    let string = |name: GLenum| {
        // SAFETY: a context is current on this thread, and every version of
        // GL has `glGetString`.
        let text = unsafe { get_string(name) };
        if text.is_null() {
            return None;
        }
        // SAFETY: GL returned a NUL-terminated string, which lives as long as
        // the context. It is copied here.
        let text = unsafe { CStr::from_ptr(text.cast::<c_char>()) };
        Some(text.to_string_lossy().into_owned())
    };
    let integer = |name: GLenum| {
        // A version that does not know the name leaves the value alone.
        let mut value = 0;
        // SAFETY: a context is current on this thread, every version of GL
        // has `glGetIntegerv`, and these names give one integer each.
        unsafe { get_integer(name, &raw mut value) };
        value
    };
    let Some(version) = string(GL_VERSION) else {
        return Err(PlatformError::GpuCallFailed {
            call: "glGetString",
            error: "it gave no GL_VERSION".to_string(),
        });
    };
    let renderer = string(GL_RENDERER).unwrap_or_default();
    let (major, minor) = (integer(GL_MAJOR_VERSION), integer(GL_MINOR_VERSION));
    if !is_3_3_core(major, minor, integer(GL_CONTEXT_PROFILE_MASK)) {
        return Err(PlatformError::GlTooOld {
            found: Some(version),
        });
    }
    Ok(GlStrings { version, renderer })
}

/// Whether a context of this version and profile mask is what we ask for.
fn is_3_3_core(major: GLint, minor: GLint, profile_mask: GLint) -> bool {
    (major, minor) >= GL_REQUIRED && profile_mask & GL_CONTEXT_CORE_PROFILE_BIT != 0
}

/// Whether a swap interval is one that the config allows.
fn interval_supported(interval: i32, (min, max): (EGLint, EGLint)) -> bool {
    interval >= 0 && (min..=max).contains(&interval)
}

// ---------------------------------------------------------------------------
// Headless
// ---------------------------------------------------------------------------

/// An offscreen OpenGL context: see [`HeadlessGl`](crate::HeadlessGl).
pub(crate) struct Headless {
    client: Client,
    display: EGLDisplay,
    /// The 1 by 1 pbuffer that the context is current with, or
    /// `EGL_NO_SURFACE` if it needs none.
    pbuffer: EGLSurface,
    context: EGLContext,
    lookup: GlLookup,
    // Declared last, so that the claim is given up after the context is
    // gone.
    claim: Claim,
}

impl Headless {
    /// Makes an offscreen 3.3 core context and makes it current on this
    /// thread.
    pub(crate) fn new() -> Result<Self, PlatformError> {
        let claim = Claim::take()?;
        let client = Client::load()?;
        let handle = client.surfaceless_display()?;
        // Never terminated: see the module documentation.
        let display = client.initialize(handle)?;
        let (config, _) = client.choose_config(handle, EGL_PBUFFER_BIT, |_| true)?;
        let context = client.create_context(&display, config)?;
        let lookup = GlLookup::new(&client);
        // From here on, dropping `headless` destroys the context.
        let mut headless = Self {
            client,
            display: handle,
            pbuffer: EGL_NO_SURFACE,
            context,
            lookup,
            claim,
        };
        if !has_extension(&display.extensions, "EGL_KHR_surfaceless_context") {
            let attributes = [EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE];
            // SAFETY: the display is initialized, the config is its own, and
            // the list ends with `EGL_NONE`.
            let pbuffer = unsafe {
                (headless.client.egl.eglCreatePbufferSurface)(handle, config, attributes.as_ptr())
            };
            if pbuffer == EGL_NO_SURFACE {
                return Err(headless.client.failure("eglCreatePbufferSurface"));
            }
            headless.pbuffer = pbuffer;
        }
        // SAFETY: the display is initialized, and the surface (or
        // `EGL_NO_SURFACE`, which the display allows) and the context are its
        // own. Nothing of ours is current on this thread: the claim.
        let current = unsafe {
            (headless.client.egl.eglMakeCurrent)(
                handle,
                headless.pbuffer,
                headless.pbuffer,
                headless.context,
            )
        };
        if current == EGL_FALSE {
            return Err(headless.client.failure("eglMakeCurrent"));
        }
        let strings = check_context(|name| headless.get_proc_address(name))?;
        debug!(
            "made an offscreen OpenGL context: {} on {}",
            strings.version, strings.renderer
        );
        Ok(headless)
    }

    /// Returns the GL function called `name`, or null.
    pub(crate) fn get_proc_address(&self, name: &CStr) -> *const c_void {
        self.lookup.get(&self.client.egl, name)
    }

    /// The id of the context, current while it exists.
    pub(crate) fn id(&self) -> GlContextId {
        self.claim.id()
    }
}

impl Drop for Headless {
    fn drop(&mut self) {
        self.client.release_current(self.display);
        if self.pbuffer != EGL_NO_SURFACE {
            self.client.destroy_surface(self.display, self.pbuffer);
        }
        self.client.destroy_context(self.display, self.context);
        // The display stays initialized: see the module documentation.
    }
}

// ---------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------

/// The OpenGL context of an X11 window, and its EGL display.
///
/// It is made in two steps, because the window needs the visual of the
/// config: [`WindowGl::open`] chooses the config, and
/// [`WindowGl::make_current`] makes the surface and the context.
pub(super) struct WindowGl {
    client: Client,
    display: DisplayInfo,
    config: EGLConfig,
    /// The X visual of the config.
    visual: VisualID,
    /// The smallest and largest swap intervals of the config.
    swap_range: (EGLint, EGLint),
    /// `EGL_NO_SURFACE` until `make_current`, and after `release`.
    surface: EGLSurface,
    /// `EGL_NO_CONTEXT` until `make_current`, and after `release`.
    context: EGLContext,
    lookup: GlLookup,
    // Declared last, so that the claim is given up after the context is
    // gone.
    claim: Claim,
}

impl WindowGl {
    /// Takes this thread's claim, initializes the EGL display of
    /// `x_display`, and chooses the first OpenGL config, with 8-bit red,
    /// green and blue, whose X visual `accept_visual` takes.
    ///
    /// # Safety
    ///
    /// `x_display` is open, and stays open until this value is dropped,
    /// which terminates the EGL display.
    pub(super) unsafe fn open(
        x_display: NonNull<Display>,
        accept_visual: impl Fn(VisualID) -> bool,
    ) -> Result<Self, PlatformError> {
        let claim = Claim::take()?;
        let client = Client::load()?;
        // SAFETY: the caller keeps the display open while this value exists,
        // and the EGL display is not used after it.
        let handle = unsafe { client.x11_display(x_display) }?;
        let display = client.initialize(handle)?;
        // From here on, dropping `gl` terminates the display.
        let mut gl = Self {
            lookup: GlLookup::new(&client),
            client,
            display,
            config: ptr::null_mut(),
            visual: 0,
            swap_range: (0, 0),
            surface: EGL_NO_SURFACE,
            context: EGL_NO_CONTEXT,
            claim,
        };
        let (config, record) = gl.client.choose_config(handle, EGL_WINDOW_BIT, |record| {
            VisualID::try_from(record.visual)
                .is_ok_and(|visual| visual != 0 && accept_visual(visual))
        })?;
        gl.config = config;
        // `choose_config` took only configs with a visual id that converts.
        gl.visual = VisualID::try_from(record.visual).unwrap_or(0);
        gl.swap_range = (
            gl.client.attribute(handle, config, EGL_MIN_SWAP_INTERVAL)?,
            gl.client.attribute(handle, config, EGL_MAX_SWAP_INTERVAL)?,
        );
        Ok(gl)
    }

    /// The X visual that the window must have.
    pub(super) fn visual(&self) -> VisualID {
        self.visual
    }

    /// Makes the window's surface and context, makes the context current on
    /// this thread, checks it, and sets the swap interval to 1 with `vsync`
    /// and to 0 without.
    ///
    /// # Safety
    ///
    /// `window` is a window of the X display, of the visual that
    /// [`WindowGl::visual`] gives, and it is destroyed only after
    /// [`WindowGl::release`], or after this value is dropped.
    pub(super) unsafe fn make_current(
        &mut self,
        window: Window,
        vsync: bool,
    ) -> Result<(), PlatformError> {
        let handle = self.display.handle;
        // An X window id is the native window of the X11 platform.
        let native: EGLNativeWindowType = window as EGLNativeWindowType;
        // ADR-0013: blend in linear space, which needs an sRGB framebuffer.
        let srgb_list = [EGL_GL_COLORSPACE, EGL_GL_COLORSPACE_SRGB, EGL_NONE];
        let mut colorspace = "linear";
        if self.display.version >= (1, 5)
            || has_extension(&self.display.extensions, "EGL_KHR_gl_colorspace")
        {
            // SAFETY: the display is initialized, the config is its own, the
            // window is one of its X display's and of the config's visual
            // (the caller), and the list ends with `EGL_NONE`.
            let surface = unsafe {
                (self.client.egl.eglCreateWindowSurface)(
                    handle,
                    self.config,
                    native,
                    srgb_list.as_ptr(),
                )
            };
            if surface == EGL_NO_SURFACE {
                warn!(
                    "could not make an sRGB window surface, so it is linear: {}",
                    self.client.failure("eglCreateWindowSurface")
                );
            } else {
                self.surface = surface;
                colorspace = "sRGB";
            }
        }
        if self.surface == EGL_NO_SURFACE {
            // SAFETY: as above, with no attributes.
            let surface = unsafe {
                (self.client.egl.eglCreateWindowSurface)(handle, self.config, native, ptr::null())
            };
            if surface == EGL_NO_SURFACE {
                return Err(self.client.failure("eglCreateWindowSurface"));
            }
            self.surface = surface;
        }
        self.context = self.client.create_context(&self.display, self.config)?;
        // SAFETY: the display is initialized, and the surface and the context
        // are its own. Nothing of ours is current on this thread: the claim.
        let current = unsafe {
            (self.client.egl.eglMakeCurrent)(handle, self.surface, self.surface, self.context)
        };
        if current == EGL_FALSE {
            return Err(self.client.failure("eglMakeCurrent"));
        }
        let strings = check_context(|name| self.get_proc_address(name))?;
        let interval = i32::from(vsync);
        if !self.set_swap_interval(interval) {
            warn!(
                "the driver cannot set the swap interval to {interval}, only from {} to {}",
                self.swap_range.0, self.swap_range.1
            );
        }
        info!(
            "OpenGL {} on {}: X visual {:#x}, {colorspace} window surface, swap interval {} to {}",
            strings.version, strings.renderer, self.visual, self.swap_range.0, self.swap_range.1
        );
        Ok(())
    }

    /// Releases the context, and destroys it and the window's surface, so
    /// that the window can be destroyed. Does nothing the second time.
    pub(super) fn release(&mut self) {
        let handle = self.display.handle;
        if self.surface == EGL_NO_SURFACE && self.context == EGL_NO_CONTEXT {
            return;
        }
        self.client.release_current(handle);
        if self.surface != EGL_NO_SURFACE {
            self.client.destroy_surface(handle, self.surface);
            self.surface = EGL_NO_SURFACE;
        }
        if self.context != EGL_NO_CONTEXT {
            self.client.destroy_context(handle, self.context);
            self.context = EGL_NO_CONTEXT;
        }
    }

    /// Returns the GL function called `name`, or null.
    pub(super) fn get_proc_address(&self, name: &CStr) -> *const c_void {
        self.lookup.get(&self.client.egl, name)
    }

    /// The id of the context. It is current from `make_current` until this
    /// value is dropped; before `make_current`, nothing reads it.
    pub(super) fn id(&self) -> GlContextId {
        self.claim.id()
    }

    /// Shows the back buffer.
    pub(super) fn swap_buffers(&self) -> Result<(), PlatformError> {
        // SAFETY: the display is initialized, and the surface is its own. If
        // it was released, the surface is `EGL_NO_SURFACE`, and EGL fails.
        let swapped =
            unsafe { (self.client.egl.eglSwapBuffers)(self.display.handle, self.surface) };
        if swapped == EGL_FALSE {
            return Err(self.client.failure("eglSwapBuffers"));
        }
        Ok(())
    }

    /// Sets the swap interval, if the config allows it: see
    /// [`GlContext::set_swap_interval`](crate::GlContext::set_swap_interval).
    pub(super) fn set_swap_interval(&self, interval: i32) -> bool {
        // EGL would take any interval, and quietly use the nearest one it
        // allows.
        if !interval_supported(interval, self.swap_range) {
            return false;
        }
        // SAFETY: the display is initialized, and the interval applies to the
        // surface of the context current on this thread, if any.
        let set = unsafe { (self.client.egl.eglSwapInterval)(self.display.handle, interval) };
        set != EGL_FALSE
    }
}

impl Drop for WindowGl {
    fn drop(&mut self) {
        self.release();
        // SAFETY: the display is initialized, its X display is still open
        // (the caller of `open`), and nothing uses it after this.
        let terminated = unsafe { (self.client.egl.eglTerminate)(self.display.handle) };
        if terminated == EGL_FALSE {
            warn!(
                "could not terminate the EGL display: {}",
                self.client.failure("eglTerminate")
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// Reads the `<major>.<minor>` at the start of an EGL version string, such
/// as `1.5 libglvnd`.
fn parse_version(text: &str) -> Option<(EGLint, EGLint)> {
    let number = text.split_ascii_whitespace().next()?;
    let (major, minor) = number.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// Whether `name` is one of the extensions in `list`, which are separated by
/// spaces. A name must match whole: `EGL_KHR_create_context` is not in
/// `EGL_KHR_create_context_no_error`.
fn has_extension(list: &str, name: &str) -> bool {
    list.split_ascii_whitespace()
        .any(|extension| extension == name)
}

/// Names an EGL error code, for a message.
fn error_text(code: EGLint) -> String {
    let name = match code {
        EGL_SUCCESS => "no error code",
        EGL_NOT_INITIALIZED => "EGL_NOT_INITIALIZED",
        EGL_BAD_ALLOC => "EGL_BAD_ALLOC",
        EGL_BAD_ATTRIBUTE => "EGL_BAD_ATTRIBUTE",
        EGL_BAD_CONFIG => "EGL_BAD_CONFIG",
        EGL_BAD_DISPLAY => "EGL_BAD_DISPLAY",
        EGL_BAD_MATCH => "EGL_BAD_MATCH",
        EGL_BAD_NATIVE_WINDOW => "EGL_BAD_NATIVE_WINDOW",
        EGL_BAD_PARAMETER => "EGL_BAD_PARAMETER",
        EGL_BAD_SURFACE => "EGL_BAD_SURFACE",
        EGL_CONTEXT_LOST => "EGL_CONTEXT_LOST",
        _ => return format!("EGL error {code:#06x}"),
    };
    name.to_string()
}

#[cfg(test)]
pub(super) mod gl_test {
    use core::ffi::{CStr, c_char, c_void};

    use nadir_sys::gl::{
        GL_COLOR_ATTACHMENT0, GL_COLOR_BUFFER_BIT, GL_CONTEXT_CORE_PROFILE_BIT,
        GL_CONTEXT_PROFILE_MASK, GL_FRAMEBUFFER, GL_FRAMEBUFFER_COMPLETE, GL_MAJOR_VERSION,
        GL_MINOR_VERSION, GL_NO_ERROR, GL_RENDERER, GL_RGBA, GL_RGBA8, GL_TEXTURE_2D,
        GL_UNSIGNED_BYTE, PFNGLBINDFRAMEBUFFERPROC, PFNGLBINDTEXTUREPROC,
        PFNGLCHECKFRAMEBUFFERSTATUSPROC, PFNGLCLEARCOLORPROC, PFNGLCLEARPROC,
        PFNGLDELETEFRAMEBUFFERSPROC, PFNGLDELETETEXTURESPROC, PFNGLFRAMEBUFFERTEXTURE2DPROC,
        PFNGLGENFRAMEBUFFERSPROC, PFNGLGENTEXTURESPROC, PFNGLGETERRORPROC, PFNGLGETINTEGERVPROC,
        PFNGLGETSTRINGPROC, PFNGLREADPIXELSPROC, PFNGLTEXIMAGE2DPROC,
    };

    use crate::GlContext;

    /// Looks a GL function up and converts it to its type.
    ///
    /// # Safety
    ///
    /// `T` is the type that `nadir_sys::gl` declares for `name`.
    unsafe fn function<T: Copy>(gl: &dyn GlContext, name: &CStr) -> T {
        let address = gl.get_proc_address(name);
        assert!(!address.is_null(), "no {name:?}");
        assert_eq!(size_of::<T>(), size_of::<*const c_void>());
        // SAFETY: `T` is the type of the function (the caller), a function
        // pointer, which has the size of an address.
        unsafe { core::mem::transmute_copy::<*const c_void, T>(&address) }
    }

    /// Checks that the context of `gl` is OpenGL 3.3 or later with the core
    /// profile, and that it draws: it clears a 4 by 4 render target to a
    /// color and reads every pixel back. Returns `GL_RENDERER`.
    pub(in crate::linux) fn check_context_and_draw(gl: &dyn GlContext) -> String {
        // SAFETY: each type below is the one declared for its name.
        let get_string: PFNGLGETSTRINGPROC = unsafe { function(gl, c"glGetString") };
        // SAFETY: as above.
        let get_integer: PFNGLGETINTEGERVPROC = unsafe { function(gl, c"glGetIntegerv") };
        // SAFETY: as above.
        let get_error: PFNGLGETERRORPROC = unsafe { function(gl, c"glGetError") };
        // SAFETY: as above.
        let gen_textures: PFNGLGENTEXTURESPROC = unsafe { function(gl, c"glGenTextures") };
        // SAFETY: as above.
        let bind_texture: PFNGLBINDTEXTUREPROC = unsafe { function(gl, c"glBindTexture") };
        // SAFETY: as above.
        let tex_image: PFNGLTEXIMAGE2DPROC = unsafe { function(gl, c"glTexImage2D") };
        // SAFETY: as above.
        let delete_textures: PFNGLDELETETEXTURESPROC = unsafe { function(gl, c"glDeleteTextures") };
        // SAFETY: as above.
        let gen_framebuffers: PFNGLGENFRAMEBUFFERSPROC =
            unsafe { function(gl, c"glGenFramebuffers") };
        // SAFETY: as above.
        let bind_framebuffer: PFNGLBINDFRAMEBUFFERPROC =
            unsafe { function(gl, c"glBindFramebuffer") };
        // SAFETY: as above.
        let framebuffer_texture: PFNGLFRAMEBUFFERTEXTURE2DPROC =
            unsafe { function(gl, c"glFramebufferTexture2D") };
        // SAFETY: as above.
        let check_framebuffer: PFNGLCHECKFRAMEBUFFERSTATUSPROC =
            unsafe { function(gl, c"glCheckFramebufferStatus") };
        // SAFETY: as above.
        let delete_framebuffers: PFNGLDELETEFRAMEBUFFERSPROC =
            unsafe { function(gl, c"glDeleteFramebuffers") };
        // SAFETY: as above.
        let clear_color: PFNGLCLEARCOLORPROC = unsafe { function(gl, c"glClearColor") };
        // SAFETY: as above.
        let clear: PFNGLCLEARPROC = unsafe { function(gl, c"glClear") };
        // SAFETY: as above.
        let read_pixels: PFNGLREADPIXELSPROC = unsafe { function(gl, c"glReadPixels") };

        // In what follows, the context of `gl` is current on this thread,
        // and it is OpenGL 3.3 or later, which has every one of these
        // functions.
        let integer = |name| {
            let mut value = 0;
            // SAFETY: the context is current, and the name gives one integer.
            unsafe { get_integer(name, &raw mut value) };
            value
        };
        let version = (integer(GL_MAJOR_VERSION), integer(GL_MINOR_VERSION));
        assert!(version >= (3, 3), "OpenGL {version:?}");
        assert_ne!(
            integer(GL_CONTEXT_PROFILE_MASK) & GL_CONTEXT_CORE_PROFILE_BIT,
            0
        );
        // SAFETY: the context is current.
        let renderer = unsafe { get_string(GL_RENDERER) };
        assert!(!renderer.is_null());
        // SAFETY: GL returned a NUL-terminated string, copied here.
        let renderer = unsafe { CStr::from_ptr(renderer.cast::<c_char>()) };
        let renderer = renderer.to_string_lossy().into_owned();

        let (mut texture, mut framebuffer) = (0, 0);
        // SAFETY: the context is current, and the name goes to a live `GLuint`.
        unsafe { gen_textures(1, &raw mut texture) };
        // SAFETY: the context is current, and the texture is a new name.
        unsafe { bind_texture(GL_TEXTURE_2D, texture) };
        // SAFETY: the context is current, and with no pixels GL reads nothing.
        unsafe {
            tex_image(
                GL_TEXTURE_2D,
                0,
                GL_RGBA8,
                4,
                4,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                core::ptr::null(),
            )
        };
        // SAFETY: the context is current, and the name goes to a live `GLuint`.
        unsafe { gen_framebuffers(1, &raw mut framebuffer) };
        // SAFETY: the context is current, and the framebuffer is a new name.
        unsafe { bind_framebuffer(GL_FRAMEBUFFER, framebuffer) };
        // SAFETY: the context is current, and the texture is bound and has a
        // level 0.
        unsafe {
            framebuffer_texture(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                texture,
                0,
            )
        };
        // SAFETY: the context is current.
        let status = unsafe { check_framebuffer(GL_FRAMEBUFFER) };
        assert_eq!(status, GL_FRAMEBUFFER_COMPLETE);
        // 0.2, 0.4 and 0.6 are 51, 102 and 153 of 255, exactly.
        // SAFETY: the context is current.
        unsafe { clear_color(0.2, 0.4, 0.6, 1.0) };
        // SAFETY: the context is current.
        unsafe { clear(GL_COLOR_BUFFER_BIT) };
        let mut pixels = [0u8; 4 * 4 * 4];
        // SAFETY: the context is current, and the buffer holds 4 rows of 4
        // RGBA pixels of 4 bytes, rows of 16 bytes, which the default pack
        // alignment of 4 needs no padding for.
        unsafe {
            read_pixels(
                0,
                0,
                4,
                4,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                pixels.as_mut_ptr().cast(),
            )
        };
        // SAFETY: the context is current, and 0 is the default framebuffer.
        unsafe { bind_framebuffer(GL_FRAMEBUFFER, 0) };
        // SAFETY: the context is current, and the name is a live `GLuint`.
        unsafe { delete_framebuffers(1, &raw const framebuffer) };
        // SAFETY: the context is current, and the name is a live `GLuint`.
        unsafe { delete_textures(1, &raw const texture) };
        // SAFETY: the context is current.
        let error = unsafe { get_error() };
        assert_eq!(error, GL_NO_ERROR);
        for pixel in pixels.as_chunks::<4>().0 {
            assert_eq!(*pixel, [51, 102, 153, 255], "on {renderer}");
        }
        renderer
    }
}

#[cfg(all(test, feature = "headless-test"))]
mod headless_tests {
    use std::env;
    use std::panic::{self, AssertUnwindSafe};
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use nadir_core::UVec2;
    use nadir_core::log::{self, info};

    use super::gl_test::check_context_and_draw;
    use crate::{GlContext, PlatformError, create_headless_gl};

    /// Names a part of the renderer, such as `llvmpipe`, that the context
    /// must have. Unset or empty, any renderer will do.
    const EXPECT_RENDERER: &str = "NADIR_EXPECT_GL_RENDERER";

    #[test]
    fn a_headless_context_is_opengl_3_3_core_and_draws() {
        let gl = create_headless_gl().unwrap();
        let renderer = check_context_and_draw(&gl);
        info!("the headless OpenGL renderer is {renderer}");
        let expected = env::var(EXPECT_RENDERER).unwrap_or_default();
        assert!(
            renderer.contains(&expected),
            "the renderer is `{renderer}`, and {EXPECT_RENDERER} asks for `{expected}`. With \
             libglvnd, Mesa can be chosen with \
             __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json"
        );
    }

    #[test]
    fn a_headless_context_has_no_default_framebuffer() {
        let gl = create_headless_gl().unwrap();
        assert_eq!(gl.swap_buffers(), Ok(()));
        assert!(!gl.set_swap_interval(0));
        assert!(!gl.set_swap_interval(1));
        assert_eq!(gl.drawable_size(), UVec2::ZERO);
    }

    #[test]
    fn a_headless_context_finds_gl_functions_only() {
        let gl = create_headless_gl().unwrap();
        assert!(!gl.get_proc_address(c"glGetString").is_null());
        // libglvnd gives a stub for any name that starts with `gl`, so the
        // name that is missing is not one.
        assert!(gl.get_proc_address(c"nadirNoSuchFunction").is_null());
    }

    #[test]
    fn a_headless_context_has_an_id_that_is_current_while_it_exists() {
        let gl = create_headless_gl().unwrap();
        let id = gl.context_id().unwrap();
        assert!(id.is_current());
        drop(gl);
        assert!(!id.is_current());
        let next = create_headless_gl().unwrap();
        assert!(
            next.context_id()
                .is_some_and(|next| next != id && next.is_current())
        );
    }

    #[test]
    fn a_thread_has_one_headless_context_at_a_time() {
        let first = create_headless_gl().unwrap();
        assert_eq!(
            create_headless_gl().err(),
            Some(PlatformError::GlContextExists)
        );
        drop(first);
        let second = create_headless_gl().unwrap();
        check_context_and_draw(&second);
    }

    /// Whether the log shows that a context could not be released or
    /// destroyed, which happens to the other contexts if a drop ends the
    /// display they share.
    fn teardown_failed() -> Vec<String> {
        log::tail(log::RING_CAPACITY)
            .into_iter()
            .filter(|record| {
                record.contains("could not release the OpenGL context")
                    || record.contains("could not destroy an OpenGL context")
            })
            .collect()
    }

    #[test]
    fn dropping_a_headless_context_leaves_the_others_on_the_display() {
        // The second context is made and dropped while the first exists, and
        // nothing initializes the display again before the first goes.
        let (made, dropped) = (Barrier::new(2), Barrier::new(2));
        thread::scope(|scope| {
            scope.spawn(|| {
                let first = create_headless_gl();
                made.wait();
                dropped.wait();
                let first = first.unwrap();
                check_context_and_draw(&first);
            });
            scope.spawn(|| {
                made.wait();
                let second = create_headless_gl();
                let made_second = second.is_ok();
                drop(second);
                dropped.wait();
                assert!(made_second);
            });
        });
        assert_eq!(teardown_failed(), Vec::<String>::new());
    }

    #[test]
    fn headless_contexts_draw_on_several_threads_at_once() {
        const THREADS: usize = 4;
        const ROUNDS: usize = 3;
        // In each round, every thread makes a context before any draws with
        // its own and drops it. A drop that ended the shared display would
        // make the others' creations or teardowns fail.
        let barrier = Barrier::new(THREADS);
        let failures = AtomicUsize::new(0);
        thread::scope(|scope| {
            for _ in 0..THREADS {
                scope.spawn(|| {
                    for _ in 0..ROUNDS {
                        let gl = create_headless_gl();
                        barrier.wait();
                        // A failure must not leave the other threads waiting
                        // at the barrier, so it is counted, not raised.
                        let drew = panic::catch_unwind(AssertUnwindSafe(|| {
                            check_context_and_draw(&gl.unwrap());
                        }));
                        if drew.is_err() {
                            failures.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        assert_eq!(failures.into_inner(), 0);
        assert_eq!(teardown_failed(), Vec::<String>::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_read_from_the_start_of_the_string() {
        assert_eq!(parse_version("1.5 libglvnd"), Some((1, 5)));
        assert_eq!(parse_version("1.5"), Some((1, 5)));
        assert_eq!(parse_version("1.4 (DRI2)"), Some((1, 4)));
        assert_eq!(parse_version("10.12 Vendor 3.4"), Some((10, 12)));
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("abc"), None);
        assert_eq!(parse_version("1."), None);
        assert_eq!(parse_version("1"), None);
    }

    #[test]
    fn an_extension_matches_only_whole() {
        let list = "EGL_KHR_create_context_no_error EGL_KHR_create_context EGL_KHR_gl_colorspace";
        assert!(has_extension(list, "EGL_KHR_create_context"));
        assert!(has_extension(list, "EGL_KHR_create_context_no_error"));
        assert!(has_extension(list, "EGL_KHR_gl_colorspace"));
        assert!(!has_extension(
            "EGL_KHR_create_context_no_error",
            "EGL_KHR_create_context"
        ));
        assert!(!has_extension("", "EGL_KHR_create_context"));
        assert!(!has_extension(list, ""));
    }

    /// A config with these bits of color and this visual.
    fn record(bits: EGLint, visual: EGLint) -> ConfigRecord {
        ConfigRecord {
            red: bits,
            green: bits,
            blue: bits,
            visual,
        }
    }

    #[test]
    fn the_first_8_bit_config_that_is_accepted_is_picked() {
        // As Mesa orders them: 10-bit configs come first.
        let records = [
            record(10, 0x40),
            record(10, 0x41),
            record(8, 0),
            record(8, 0x70),
            record(8, 0x40),
            record(8, 0x41),
        ];
        let window = |record: &ConfigRecord| record.visual == 0x40 || record.visual == 0x41;
        assert_eq!(pick(&records, window), Some(4));
        assert_eq!(pick(&records, |_| true), Some(2));
        assert_eq!(pick(&records, |record| record.visual == 0x41), Some(5));
        assert_eq!(pick(&records, |_| false), None);
        assert_eq!(pick(&[], |_| true), None);
        // Red, green and blue must each be 8.
        let uneven = ConfigRecord {
            red: 8,
            green: 8,
            blue: 10,
            visual: 0x40,
        };
        assert_eq!(pick(&[uneven], |_| true), None);
    }

    #[test]
    fn an_interval_must_be_in_the_range_of_the_config() {
        assert!(!interval_supported(0, (1, 1)));
        assert!(interval_supported(1, (1, 1)));
        assert!(!interval_supported(-1, (0, 1000)));
        assert!(interval_supported(0, (0, 1000)));
        assert!(interval_supported(2, (0, 1000)));
        assert!(!interval_supported(2, (0, 1)));
        assert!(!interval_supported(1, (0, 0)));
        // A range that starts below 0 still allows no negative interval.
        assert!(!interval_supported(-1, (-1, 1)));
    }

    #[test]
    fn only_3_3_or_later_with_the_core_profile_is_taken() {
        let core = GL_CONTEXT_CORE_PROFILE_BIT;
        let compatibility = 0x2;
        assert!(is_3_3_core(3, 3, core));
        assert!(is_3_3_core(4, 6, core));
        assert!(is_3_3_core(4, 0, core));
        assert!(!is_3_3_core(3, 2, core));
        assert!(!is_3_3_core(3, 3, compatibility));
        assert!(!is_3_3_core(4, 6, 0));
        assert!(!is_3_3_core(0, 0, 0));
    }

    #[test]
    fn an_egl_error_is_named_by_its_code() {
        assert_eq!(error_text(0x3009), "EGL_BAD_MATCH");
        assert_eq!(error_text(0x3001), "EGL_NOT_INITIALIZED");
        assert_eq!(error_text(0x300E), "EGL_CONTEXT_LOST");
        // EGL_BAD_CURRENT_SURFACE, which is not declared.
        assert_eq!(error_text(0x3007), "EGL error 0x3007");
        assert_eq!(error_text(0x3000), "no error code");
    }
}
