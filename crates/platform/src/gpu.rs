//! What the GPU draws with: the window's OpenGL context, and contexts that
//! draw offscreen.

use core::cell::Cell;
use core::ffi::{CStr, c_void};
use core::fmt;
use core::marker::PhantomData;
use core::num::NonZeroU64;
use core::sync::atomic::{AtomicU64, Ordering};

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
/// If [`GlContext::context_id`] returns `Some(id)`, it also promises that
/// every pointer from `get_proc_address` that is not null stays callable, on
/// the thread that made the value, for as long as [`GlContextId::is_current`]
/// is `true` for `id` there. So a user that keeps the id, and not a borrow,
/// can still tell when the context is gone (ADR-0016).
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

    /// The id of this context, if it is one of `nadir-platform`'s. The
    /// default is `None`, which promises nothing.
    fn context_id(&self) -> Option<GlContextId> {
        None
    }
}

/// Names one of `nadir-platform`'s OpenGL contexts. No two contexts of a
/// process get the same id.
///
/// The id outlives the context: [`GlContextId::is_current`] says whether
/// the context still exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlContextId(NonZeroU64);

impl GlContextId {
    /// Whether the context with this id still exists, on this thread. While
    /// it does, it is current here, and it is the thread's only context.
    pub fn is_current(self) -> bool {
        CURRENT
            .try_with(|current| current.get() == Some(self))
            .unwrap_or(false)
    }
}

thread_local! {
    /// The id of this thread's OpenGL context, if it has one.
    static CURRENT: Cell<Option<GlContextId>> = const { Cell::new(None) };
}

/// The id of the next context. A `u64` that grows by one per context is
/// never used up.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// This thread's one claim to an OpenGL context, which every context of ours
/// holds: there can be only one at a time. Dropping it gives the claim up,
/// and its id is then no longer current.
// Only Linux makes contexts so far; Windows (M2.6) and macOS (M2.7) will.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct Claim {
    id: GlContextId,
    /// The claim is this thread's, so it stays on it.
    _thread: PhantomData<*const ()>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl Claim {
    /// Takes this thread's claim, with a new id, if no context of ours has
    /// it.
    pub(crate) fn take() -> Result<Self, PlatformError> {
        if CURRENT.with(Cell::get).is_some() {
            return Err(PlatformError::GlContextExists);
        }
        let next = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        // The counter starts at 1, and would take centuries to wrap.
        let id = GlContextId(NonZeroU64::new(next).unwrap_or(NonZeroU64::MAX));
        CURRENT.with(|current| current.set(Some(id)));
        Ok(Self {
            id,
            _thread: PhantomData,
        })
    }

    /// The id of the context that holds the claim.
    pub(crate) fn id(&self) -> GlContextId {
        self.id
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        // The claim is the thread's while it exists, so the id is still
        // there. The key has no destructor, so it can always be reached.
        let _ = CURRENT.try_with(|current| current.set(None));
    }
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

    fn context_id(&self) -> Option<GlContextId> {
        Some(self.inner.id())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_claims_one_context_at_a_time() {
        let claim = Claim::take().unwrap();
        assert_eq!(Claim::take().err(), Some(PlatformError::GlContextExists));
        // Another thread has a claim of its own.
        std::thread::spawn(|| assert!(Claim::take().is_ok()))
            .join()
            .unwrap();
        drop(claim);
        assert!(Claim::take().is_ok());
    }

    #[test]
    fn an_id_is_current_only_while_its_claim_exists_on_its_thread() {
        let claim = Claim::take().unwrap();
        let id = claim.id();
        assert!(id.is_current());
        std::thread::spawn(move || assert!(!id.is_current()))
            .join()
            .unwrap();
        drop(claim);
        assert!(!id.is_current());
        // The next context gets another id, and the old one stays gone.
        let next = Claim::take().unwrap();
        assert_ne!(next.id(), id);
        assert!(next.id().is_current());
        assert!(!id.is_current());
    }
}
