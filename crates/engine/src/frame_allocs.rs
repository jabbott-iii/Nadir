//! [`FrameAllocs`]: what each frame allocated, in development builds.

use nadir_platform::AllocCounts;

/// What the main thread allocated in the last frame: a resource of
/// development builds (feature `dev`).
///
/// A frame counts from the end of the one before it to its own end: the
/// wait for it, its events, its ticks, the frame systems, the drawing and
/// the asset loading. A frame system reads the frame before its own. A
/// steady frame should allocate nothing.
///
/// Only Rust's global allocator is counted, not what C libraries such as
/// the GPU driver allocate themselves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameAllocs(pub AllocCounts);
