//! The device and its passes: the API that the backends implement.

use core::cell::Cell;
use core::marker::PhantomData;
use core::ops::Range;

use nadir_core::{IRect, LinearColor};
use nadir_platform::{GlContextId, GpuSurface};

use crate::GfxError;
use crate::gl::{DrawVertices, GlDevice, GlFns, PassState};
use crate::pod::{Pod, bytes_of};
use crate::types::{
    BufferDesc, BufferId, Caps, IndexFormat, PassTarget, PipelineDesc, PipelineId, RenderTargetId,
    ResourceId, Sampler, TextureDesc, TextureFormat, TextureId, TextureRef,
};
use nadir_core::UVec2;

thread_local! {
    /// The context of this thread's device, if it has one.
    static DEVICE: Cell<Option<GlContextId>> = const { Cell::new(None) };
}

/// This thread's one claim to a device. Two devices on one context would
/// each change GL state that the other has recorded.
struct DeviceClaim {
    context: GlContextId,
}

impl DeviceClaim {
    fn take(context: GlContextId) -> Result<Self, GfxError> {
        // A thread has one context at a time, and its ids are never reused:
        // a device of an earlier context, gone now, does not count.
        if DEVICE.with(Cell::get) == Some(context) {
            return Err(GfxError::DeviceExists);
        }
        DEVICE.with(|device| device.set(Some(context)));
        Ok(Self { context })
    }
}

impl Drop for DeviceClaim {
    fn drop(&mut self) {
        // The key has no destructor, so it can always be reached.
        let _ = DEVICE.try_with(|device| {
            if device.get() == Some(self.context) {
                device.set(None);
            }
        });
    }
}

/// The GPU, as `nadir-gfx` draws with it: OpenGL 3.3, core profile.
///
/// A device holds no borrow of the surface it was made for. Each call that
/// uses the GPU first checks that the surface's context still exists, and
/// fails with [`GfxError::ContextLost`] once it is gone (ADR-0016). A context
/// has at most one device (a thread, at most one whose context exists), and
/// the device stays on that thread:
///
/// ```compile_fail
/// fn needs_send<T: Send>() {}
/// needs_send::<nadir_gfx::Device>();
/// ```
///
/// ```compile_fail
/// fn needs_sync<T: Sync>() {}
/// needs_sync::<nadir_gfx::Device>();
/// ```
///
/// A frame is [`Device::begin_frame`], passes, then [`Device::end_frame`].
/// Resources can be made, changed and destroyed at any time outside a pass.
pub struct Device {
    // Dropped before the claim: the objects are deleted while the device
    // still has it.
    gl: GlDevice,
    _claim: DeviceClaim,
    /// Keeps the device on its thread.
    _thread: PhantomData<*const ()>,
}

impl Device {
    /// Makes the device for `surface`, whose context must be current on this
    /// thread, and logs what it runs on.
    ///
    /// # Errors
    ///
    /// - [`GfxError::UnsupportedSurface`] for a surface that is not OpenGL.
    /// - [`GfxError::MissingFunction`] if the context lacks a function, as
    ///   the stand-in context of Windows and macOS does for now.
    /// - [`GfxError::NoContext`] if the surface is not a current context of
    ///   `nadir-platform`'s.
    /// - [`GfxError::DeviceExists`] if the context has a device.
    /// - [`GfxError::GlTooOld`] if the context is not 3.3 core.
    pub fn new(surface: GpuSurface<'_>) -> Result<Self, GfxError> {
        let context = match surface {
            GpuSurface::Gl(context) => context,
            _ => return Err(GfxError::UnsupportedSurface),
        };
        let fns = GlFns::load(context)?;
        let id = context
            .context_id()
            .filter(|id| id.is_current())
            .ok_or(GfxError::NoContext)?;
        let claim = DeviceClaim::take(id)?;
        Ok(Self {
            gl: GlDevice::new(fns, id)?,
            _claim: claim,
            _thread: PhantomData,
        })
    }

    /// What the device runs on, and its limits.
    pub fn caps(&self) -> &Caps {
        self.gl.caps()
    }

    /// Makes a texture, filled from `data` if there is some: rows from the
    /// top, tightly packed, [`TextureFormat::bytes_per_pixel`] bytes a
    /// pixel. Without data its texels are undefined. Its first row is `v =
    /// 0`.
    ///
    /// # Errors
    ///
    /// [`GfxError::TooLarge`] for a size of 0 or over
    /// [`Caps::max_texture_size`]; [`GfxError::DataSize`] for data that is
    /// not exactly the texture's.
    pub fn create_texture(
        &mut self,
        desc: &TextureDesc<'_>,
        data: Option<&[u8]>,
    ) -> Result<TextureId, GfxError> {
        self.gl.create_texture(desc, data)
    }

    /// Replaces the texels of `region`, in pixels from the first row, with
    /// `data`: the region's rows, tightly packed. An empty region does
    /// nothing.
    ///
    /// # Errors
    ///
    /// [`GfxError::StaleHandle`], [`GfxError::OutOfBounds`] for a region
    /// outside the texture, [`GfxError::DataSize`].
    pub fn update_texture(
        &mut self,
        id: TextureId,
        region: IRect,
        data: &[u8],
    ) -> Result<(), GfxError> {
        self.gl.update_texture(id, region, data)
    }

    /// Makes a buffer.
    ///
    /// # Errors
    ///
    /// [`GfxError::InvalidArgument`] for a size of 0, and
    /// [`GfxError::OutOfMemory`].
    pub fn create_buffer(&mut self, desc: &BufferDesc<'_>) -> Result<BufferId, GfxError> {
        self.gl.create_buffer(desc)
    }

    /// Writes `data` into a buffer, from element `offset_elems` (counted in
    /// `T`s). Writing nothing, inside the buffer, does nothing.
    ///
    /// # Errors
    ///
    /// [`GfxError::StaleHandle`], and [`GfxError::OutOfBounds`] for a write
    /// past the end.
    pub fn write_buffer<T: Pod>(
        &mut self,
        id: BufferId,
        offset_elems: usize,
        data: &[T],
    ) -> Result<(), GfxError> {
        self.gl
            .write_buffer(id, offset_elems, size_of::<T>(), bytes_of(data))
    }

    /// Makes a render target of `size` pixels. Its contents are undefined
    /// until a pass clears it.
    ///
    /// On OpenGL a render target is stored from its bottom row: sampled, its
    /// `v = 0` is the bottom of what was drawn (`y = -1` in clip space),
    /// unlike a texture, whose `v = 0` is its first row of data.
    ///
    /// # Errors
    ///
    /// [`GfxError::TooLarge`], [`GfxError::FramebufferIncomplete`].
    pub fn create_render_target(
        &mut self,
        size: UVec2,
        format: TextureFormat,
    ) -> Result<RenderTargetId, GfxError> {
        self.gl.create_render_target(size, format)
    }

    /// Makes a pipeline: compiles and links its shaders, and checks them
    /// against the description.
    ///
    /// Draws are then checked against what the device binds: vertex and
    /// instance records, indices, and uniform blocks of their full size. The
    /// shaders' own code is trusted: one that indexes an array out of its
    /// bounds is undefined behavior that no check covers.
    ///
    /// # Errors
    ///
    /// [`GfxError::Shader`] with the driver's log if a shader does not
    /// compile or the program does not link, and [`GfxError::Interface`]
    /// if the description breaks a limit or does not match the shaders.
    pub fn create_pipeline(&mut self, desc: &PipelineDesc<'_>) -> Result<PipelineId, GfxError> {
        self.gl.create_pipeline(desc)
    }

    /// Destroys a resource. Its id, and every copy of it, then gives
    /// [`GfxError::StaleHandle`]. Destroying it again only logs a warning.
    pub fn destroy(&mut self, resource: impl Into<ResourceId>) {
        let resource = resource.into();
        self.gl.destroy(resource);
    }

    /// Starts a frame on `surface`, which must be the one the device was
    /// made for.
    ///
    /// # Errors
    ///
    /// [`GfxError::WrongSurface`], [`GfxError::FrameOrder`] inside a frame,
    /// [`GfxError::ContextLost`].
    pub fn begin_frame(&mut self, surface: GpuSurface<'_>) -> Result<(), GfxError> {
        self.gl.begin_frame(surface)
    }

    /// Starts a pass that draws into `target`, cleared to `clear` if it is
    /// given. Dropping the pass ends it.
    ///
    /// A pass starts with nothing set: its pipeline, buffers, textures and
    /// uniforms are set again in each pass. A pass on a surface with no
    /// pixels (a minimized window, or an offscreen context) draws nothing.
    ///
    /// # Errors
    ///
    /// [`GfxError::FrameOrder`] outside a frame, [`GfxError::StaleHandle`].
    pub fn begin_pass(
        &mut self,
        target: PassTarget,
        clear: Option<LinearColor>,
    ) -> Result<Pass<'_>, GfxError> {
        let state = self.gl.begin_pass(target, clear)?;
        Ok(Pass {
            device: &mut self.gl,
            state,
        })
    }

    /// Ends the frame, and shows it: swaps the surface's buffers.
    ///
    /// # Errors
    ///
    /// [`GfxError::FrameOrder`] outside a frame, [`GfxError::WrongSurface`],
    /// and [`GfxError::Present`] if the frame cannot be shown.
    pub fn end_frame(&mut self, surface: GpuSurface<'_>) -> Result<(), GfxError> {
        self.gl.end_frame(surface)
    }

    /// Reads a render target back: RGBA8, from the top row, 4 bytes a pixel.
    /// An sRGB target gives its encoded bytes, and an R8 target gives each
    /// pixel as `(r, 0, 0, 255)`. It waits for the GPU, so it is for tests.
    ///
    /// # Errors
    ///
    /// [`GfxError::StaleHandle`].
    pub fn read_pixels(&mut self, target: RenderTargetId) -> Result<Vec<u8>, GfxError> {
        self.gl.read_pixels(target)
    }

    /// The backend, for the tests of its internals.
    #[cfg(all(test, feature = "headless-test"))]
    pub(crate) fn gl(&self) -> &GlDevice {
        &self.gl
    }
}

/// A pass: draws into one target. It borrows the device, so nothing else
/// can change the device until it is dropped:
///
/// ```compile_fail
/// # fn test(device: &mut nadir_gfx::Device) -> Result<(), nadir_gfx::GfxError> {
/// let mut pass = device.begin_pass(nadir_gfx::PassTarget::Surface, None)?;
/// let _ = device.caps();
/// pass.set_scissor(None);
/// # Ok(()) }
/// ```
///
/// Setters record what the draws use, and fail at once on an id that is
/// stale or an argument out of range, leaving that setting unset. Draws
/// check every read the device sets up, and refuse one outside a buffer.
pub struct Pass<'d> {
    device: &'d mut GlDevice,
    state: PassState,
}

impl Pass<'_> {
    /// Sets the part of the target that draws map to, in pixels from the
    /// top left. It starts as the whole target. An empty viewport makes
    /// draws draw nothing.
    pub fn set_viewport(&mut self, viewport: IRect) {
        self.state.set_viewport(viewport);
    }

    /// Limits draws to a part of the target, in pixels from the top left, or
    /// lifts the limit with `None`, as a pass starts. The part outside the
    /// target is ignored, and an empty one makes draws draw nothing.
    pub fn set_scissor(&mut self, scissor: Option<IRect>) {
        self.state.set_scissor(scissor);
    }

    /// Sets the pipeline of the next draws.
    ///
    /// # Errors
    ///
    /// [`GfxError::StaleHandle`], and [`GfxError::FormatMismatch`] if the
    /// pipeline draws to another format than the pass's target.
    pub fn set_pipeline(&mut self, pipeline: PipelineId) -> Result<(), GfxError> {
        self.device.set_pipeline(&mut self.state, pipeline)
    }

    /// Sets the buffer of vertex slot `slot`, from byte `offset_bytes`.
    ///
    /// # Errors
    ///
    /// [`GfxError::StaleHandle`], and [`GfxError::InvalidArgument`] for a
    /// slot from [`MAX_VERTEX_BUFFERS`](crate::MAX_VERTEX_BUFFERS) on.
    pub fn set_vertex_buffer(
        &mut self,
        slot: u32,
        buffer: BufferId,
        offset_bytes: usize,
    ) -> Result<(), GfxError> {
        self.device
            .set_vertex_buffer(&mut self.state, slot, buffer, offset_bytes)
    }

    /// Sets the index buffer of the next indexed draws.
    ///
    /// # Errors
    ///
    /// [`GfxError::StaleHandle`].
    pub fn set_index_buffer(
        &mut self,
        buffer: BufferId,
        format: IndexFormat,
    ) -> Result<(), GfxError> {
        self.device
            .set_index_buffer(&mut self.state, buffer, format)
    }

    /// Sets what texture slot `slot` samples, and how.
    ///
    /// # Errors
    ///
    /// [`GfxError::StaleHandle`], [`GfxError::FeedbackLoop`] for the pass's
    /// own target, and [`GfxError::InvalidArgument`] for a slot from
    /// [`MAX_TEXTURE_SLOTS`](crate::MAX_TEXTURE_SLOTS) on.
    pub fn set_texture(
        &mut self,
        slot: u32,
        texture: impl Into<TextureRef>,
        sampler: Sampler,
    ) -> Result<(), GfxError> {
        let texture = texture.into();
        self.device
            .set_texture(&mut self.state, slot, texture, sampler)
    }

    /// Sets the data of uniform block binding `block`: the block's bytes,
    /// laid out by std140 rules, exactly the size the pipeline gives it.
    ///
    /// # Errors
    ///
    /// [`GfxError::InvalidArgument`] for a binding from
    /// [`MAX_UNIFORM_BLOCKS`](crate::MAX_UNIFORM_BLOCKS) on, or more than
    /// [`MAX_UNIFORM_BYTES`](crate::MAX_UNIFORM_BYTES).
    pub fn set_uniforms<T: Pod>(&mut self, block: u32, data: &[T]) -> Result<(), GfxError> {
        GlDevice::set_uniforms(&mut self.state, block, bytes_of(data))
    }

    /// Draws vertices `vertices` of the vertex buffers, for instances
    /// `instances`. An empty range draws nothing.
    ///
    /// # Errors
    ///
    /// [`GfxError::NoPipeline`], [`GfxError::Unbound`] for a buffer,
    /// texture or uniform block the pipeline needs and the pass did not set,
    /// [`GfxError::DataSize`] for uniforms of another size than the
    /// pipeline's, [`GfxError::OutOfBounds`] for a draw that would read
    /// past a buffer.
    pub fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) -> Result<(), GfxError> {
        self.device
            .draw(&mut self.state, DrawVertices::Arrays(vertices), instances)
    }

    /// Draws the indices `indices` of the index buffer, each plus
    /// `base_vertex`, for instances `instances`.
    ///
    /// # Errors
    ///
    /// As [`Pass::draw`], and for an index buffer that is not set, or an
    /// index that names a vertex past a buffer.
    pub fn draw_indexed(
        &mut self,
        indices: Range<u32>,
        base_vertex: i32,
        instances: Range<u32>,
    ) -> Result<(), GfxError> {
        let vertices = DrawVertices::Indexed {
            range: indices,
            base_vertex,
        };
        self.device.draw(&mut self.state, vertices, instances)
    }
}
