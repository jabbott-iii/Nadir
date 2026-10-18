//! What the GPU draws with: the window's OpenGL context, and contexts that
//! draw offscreen.

use core::ffi::{CStr, c_void};
use core::fmt;
use core::marker::PhantomData;

use nadir_core::UVec2;

use crate::PlatformError;
#[cfg(target_os = "linux")]
use crate::linux::Headless;
#[cfg(not(target_os = "linux"))]
use crate::no_gl::NoHeadless as Headless;

/// What the GPU draws the window with, as [`Platform::gpu_surface`] gives
/// it: for now always [`GpuSurface::Gl`]. Metal joins it on macOS later.
///
/// [`Platform::gpu_surface`]: crate::Platform::gpu_surface
#[non_exhaustive]
#[derive(Clone, Copy)]
pub enum GpuSurface<'a> {
    /// An OpenGL context, current on this thread. On Windows and macOS, for
    /// now, a stand-in with no functions: see [`Platform::gpu_surface`].
    ///
    /// [`Platform::gpu_surface`]: crate::Platform::gpu_surface
    Gl(&'a dyn GlContext),
}

impl fmt::Debug for GpuSurface<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gl(_) => f.write_str("GpuSurface::Gl(..)"),
        }
    }
}

/// An OpenGL context, and the way to its functions.
///
/// A thread has at most one: the window's on the thread that opened the
/// window, or one from [`create_headless_gl`]. It is current on that thread
/// for as long as it exists, and cannot leave it.
///
/// # Safety
///
/// An implementation promises that whenever [`GlContext::get_proc_address`]
/// returns a pointer that is not null:
///
/// - The pointer is the function of that name, and stays valid until the
///   process exits. It can be called through the type that
///   [`nadir_sys::gl`] declares for the name, on the thread that made the
///   context, while the value exists.
/// - The context is OpenGL 3.3 or later with the core profile. It is current
///   on the thread that made the value, and no other context is made current
///   there while the value exists. The value is neither `Send` nor `Sync`.
///
/// Calling any method at any time, on that thread, is sound.
///
/// A pointer that is not null does not prove that the driver has the
/// function: under libglvnd, every name that starts with `gl` gives one.
/// Check the version first, and call nothing that 3.3 core lacks without an
/// extension check.
pub unsafe trait GlContext {
    /// Returns the address of the GL function called `name`, such as
    /// `glClear`, or null if there is none.
    fn get_proc_address(&self, name: &CStr) -> *const c_void;

    /// Shows what was drawn to the default framebuffer, and gives a new back
    /// buffer to draw the next frame into. With a swap interval above 0 it
    /// may wait for the display.
    ///
    /// # Errors
    ///
    /// Fails if the system cannot show the frame, for instance because the
    /// window is gone or the GPU was reset. The context is then of no more
    /// use.
    fn swap_buffers(&self) -> Result<(), PlatformError>;

    /// Sets how many refreshes of the display a swap waits for: 0 to swap at
    /// once, 1 for vsync. Returns `true` only if the driver accepted exactly
    /// this interval, and `false`, changing nothing, if it cannot do it.
    ///
    /// `true` promises no pacing: llvmpipe takes an interval of 1 and does
    /// not wait at all, and under XWayland a swap can wait less than a
    /// refresh. Keep a frame limiter.
    fn set_swap_interval(&self, interval: i32) -> bool;

    /// The size of the default framebuffer in physical pixels, which is the
    /// inside of the window. Zero if there is none to draw to: draw to a
    /// render target instead.
    fn drawable_size(&self) -> UVec2;
}

/// An OpenGL 3.3 core context that draws only offscreen, into render
/// targets, for tests and golden images. Make one with
/// [`create_headless_gl`].
///
/// It has no default framebuffer: [`GlContext::drawable_size`] is zero,
/// [`GlContext::swap_buffers`] does nothing, and
/// [`GlContext::set_swap_interval`] returns `false`.
///
/// It is current on the thread that made it until it is dropped, and stays
/// on that thread:
///
/// ```compile_fail
/// fn needs_send<T: Send>() {}
/// needs_send::<nadir_platform::HeadlessGl>();
/// ```
///
/// ```compile_fail
/// fn needs_sync<T: Sync>() {}
/// needs_sync::<nadir_platform::HeadlessGl>();
/// ```
pub struct HeadlessGl {
    inner: Headless,
    /// Keeps the value on its thread on every system, also where `inner`
    /// alone would let it move.
    _thread: PhantomData<*const ()>,
}

impl fmt::Debug for HeadlessGl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HeadlessGl { .. }")
    }
}

// SAFETY: `Headless` keeps the promises of the trait: it makes a 3.3 core
// context current on this thread, takes the thread's one claim to a context,
// and looks functions up for it. The marker keeps the value on its thread.
unsafe impl GlContext for HeadlessGl {
    fn get_proc_address(&self, name: &CStr) -> *const c_void {
        self.inner.get_proc_address(name)
    }

    fn swap_buffers(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn set_swap_interval(&self, _interval: i32) -> bool {
        false
    }

    fn drawable_size(&self) -> UVec2 {
        UVec2::ZERO
    }
}

/// Makes an OpenGL 3.3 core context that draws offscreen, with no window and
/// no display server, and makes it current on this thread.
///
/// ```no_run
/// use nadir_platform::{GpuSurface, create_headless_gl};
///
/// let gl = create_headless_gl()?;
/// let surface = GpuSurface::Gl(&gl);
/// # let _ = surface;
/// # Ok::<(), nadir_platform::PlatformError>(())
/// ```
///
/// On Linux it needs `libEGL.so.1` and a driver with Mesa's surfaceless
/// platform (`EGL_MESA_platform_surfaceless`), which Mesa and NVIDIA have.
/// The EGL display it uses stays initialized until the process exits.
///
/// # Errors
///
/// - [`PlatformError::GlContextExists`] if this thread already has an
///   OpenGL context.
/// - [`PlatformError::Unsupported`] on every system but Linux, for now.
/// - On Linux, if EGL is missing, or the driver cannot make a 3.3 core
///   context.
pub fn create_headless_gl() -> Result<HeadlessGl, PlatformError> {
    Ok(HeadlessGl {
        inner: Headless::new()?,
        _thread: PhantomData,
    })
}
