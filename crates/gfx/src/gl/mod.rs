//! The OpenGL 3.3 core backend.
//!
//! The rules it keeps, and that its `unsafe` code relies on:
//!
//! - Every call into GL goes through a [`Gl`] token, which is made only
//!   while the device's context is current on this thread.
//! - Every draw first passes [`check_draw`], and the GL state it reads (the
//!   vertex array's attribute pointers and element buffer, the uniform
//!   ranges) is what the checked pass state says. A state change is recorded
//!   only once its call succeeded, and before each draw, in every build, a
//!   `glGetError` makes sure that no call failed unseen. If one did, the
//!   draw is refused and the recorded state forgotten.
//! - The device keeps `GL_UNPACK_ALIGNMENT` at 1 and binds no pixel buffers,
//!   so that an upload reads exactly its tightly packed pixels.

mod cache;
mod calls;
mod fns;

use core::ops::Range;
use std::ffi::CString;

use nadir_core::log::{info, warn};
use nadir_core::{GenArena, IRect, IVec2, LinearColor, UVec2};
use nadir_platform::{GlContext, GlContextId, GpuSurface};
use nadir_sys::gl::{
    GL_ARRAY_BUFFER, GL_CLAMP_TO_EDGE, GL_COLOR_BUFFER_BIT, GL_DYNAMIC_DRAW,
    GL_ELEMENT_ARRAY_BUFFER, GL_FRAGMENT_SHADER, GL_FRAMEBUFFER_COMPLETE, GL_FRAMEBUFFER_SRGB,
    GL_INVALID_INDEX, GL_LINEAR, GL_LINES, GL_NEAREST, GL_OUT_OF_MEMORY, GL_R8, GL_RED, GL_REPEAT,
    GL_RGBA, GL_RGBA8, GL_SRGB8_ALPHA8, GL_STATIC_DRAW, GL_TEXTURE_MAG_FILTER,
    GL_TEXTURE_MAX_LEVEL, GL_TEXTURE_MIN_FILTER, GL_TEXTURE_WRAP_S, GL_TEXTURE_WRAP_T,
    GL_TRIANGLE_STRIP, GL_TRIANGLES, GL_UNIFORM_BUFFER, GL_UNSIGNED_INT, GL_UNSIGNED_SHORT,
    GL_VERTEX_SHADER, GLenum, GLint, GLuint,
};

use self::cache::Cache;
use self::calls::{Gl, Integer, Status, Text};
pub(crate) use self::fns::GlFns;
use crate::GfxError;
use crate::types::{
    BackendKind, BlendMode, BufferDesc, BufferId, BufferKind, BufferUsage, Caps, IndexFormat,
    MAX_TEXTURE_SLOTS, MAX_UNIFORM_BLOCKS, MAX_UNIFORM_BYTES, MAX_VERTEX_BUFFERS, PassTarget,
    PipelineDesc, PipelineId, RenderTargetId, ResourceId, ResourceKind, Sampler, ShaderStage,
    TextureDesc, TextureFormat, TextureId, TextureRef, Topology, VertexStep,
};
use crate::validate::{
    BoundIndices, BoundVertices, DrawKind, IndexShadow, NO_LOG, SlotLayout, check_draw,
    check_pipeline_desc, check_region, check_texture_size, flip_rows, intersect, rect_to_gl,
    ring_stride, write_range,
};

/// The size of the uniform ring, which holds the uniform data of a frame.
const RING_SIZE: usize = 64 * 1024;

const SLOTS: usize = MAX_VERTEX_BUFFERS as usize;
const BLOCKS: usize = MAX_UNIFORM_BLOCKS as usize;
const UNITS: usize = MAX_TEXTURE_SLOTS as usize;

/// The four samplers, in the order of [`Sampler`]'s variants.
const SAMPLERS: [Sampler; 4] = [
    Sampler::NearestClamp,
    Sampler::NearestRepeat,
    Sampler::LinearClamp,
    Sampler::LinearRepeat,
];

struct Texture {
    name: GLuint,
    size: UVec2,
    format: TextureFormat,
}

struct Buffer {
    name: GLuint,
    size: usize,
    kind: BufferKind,
    /// The device's copy of an index buffer.
    shadow: Option<IndexShadow>,
}

struct Target {
    framebuffer: GLuint,
    texture: GLuint,
    size: UVec2,
    format: TextureFormat,
}

/// An attribute, as the vertex array needs it.
#[derive(Clone, Copy)]
struct Attribute {
    location: u32,
    components: u32,
    offset: u32,
}

/// A uniform block that the program uses.
#[derive(Clone, Copy)]
struct Block {
    binding: u32,
    size: usize,
}

struct Pipeline {
    program: GLuint,
    vertex_array: GLuint,
    /// The vertex buffer slots, in slot order, and the attributes of each.
    layouts: Vec<SlotLayout>,
    attributes: Vec<Vec<Attribute>>,
    blocks: Vec<Block>,
    /// The number of texture slots.
    textures: usize,
    blend: BlendMode,
    mode: GLenum,
    target_format: TextureFormat,
    /// What each slot of the vertex array points at, and from which byte:
    /// state of the vertex array, which outlives frames. `None` is unknown.
    slot_bound: [Option<(BufferId, usize)>; SLOTS],
    /// The element buffer of the vertex array.
    index_bound: Option<BufferId>,
}

impl Pipeline {
    /// Forgets what the vertex array points at, so that the next draw sets
    /// it all again.
    fn forget_bindings(&mut self) {
        self.slot_bound = [None; SLOTS];
        self.index_bound = None;
    }
}

/// The uniform ring: one buffer that the uniform data of a frame is written
/// into, block after block.
struct Ring {
    buffer: GLuint,
    /// The bytes from one block's data to the next.
    stride: usize,
    /// Where the next block goes.
    cursor: usize,
}

/// What a pass draws into.
#[derive(Clone, Copy)]
struct PassTargetInfo {
    framebuffer: GLuint,
    size: UVec2,
    format: TextureFormat,
    render_target: Option<RenderTargetId>,
}

/// The uniform data of one binding in a pass.
#[derive(Clone, Copy)]
struct Uniform {
    bytes: [u8; MAX_UNIFORM_BYTES],
    /// The bytes set; 0 if none were.
    len: usize,
    /// Set since it was last written to the ring.
    dirty: bool,
}

/// What a pass has set. Its setters only record; draws apply it.
pub(crate) struct PassState {
    target: PassTargetInfo,
    pipeline: Option<PipelineId>,
    vertex: [Option<(BufferId, usize)>; SLOTS],
    index: Option<(BufferId, IndexFormat)>,
    textures: [Option<(TextureRef, Sampler)>; UNITS],
    uniforms: [Uniform; BLOCKS],
    viewport: IRect,
    scissor: Option<IRect>,
}

impl PassState {
    fn new(target: PassTargetInfo) -> Self {
        Self {
            target,
            pipeline: None,
            vertex: [None; SLOTS],
            index: None,
            textures: [None; UNITS],
            uniforms: [Uniform {
                bytes: [0; MAX_UNIFORM_BYTES],
                len: 0,
                dirty: false,
            }; BLOCKS],
            viewport: full(target.size),
            scissor: None,
        }
    }

    /// Whether the pass has no pixels to draw: a window of size 0, or a
    /// context with no default framebuffer.
    fn is_empty(&self) -> bool {
        self.target.size.x == 0 || self.target.size.y == 0
    }

    pub(crate) fn set_viewport(&mut self, viewport: IRect) {
        self.viewport = viewport;
    }

    pub(crate) fn set_scissor(&mut self, scissor: Option<IRect>) {
        self.scissor = scissor;
    }
}

/// The whole of a target of `size`.
fn full(size: UVec2) -> IRect {
    let side = |side: u32| i32::try_from(side).unwrap_or(i32::MAX);
    IRect::new(IVec2::ZERO, IVec2::new(side(size.x), side(size.y)))
}

/// The open frame.
struct Frame {
    /// The size of the surface's default framebuffer.
    drawable: UVec2,
}

/// The OpenGL device.
pub(crate) struct GlDevice {
    fns: GlFns,
    status: Status,
    context: GlContextId,
    caps: Caps,
    cache: Cache,
    textures: GenArena<Texture>,
    buffers: GenArena<Buffer>,
    targets: GenArena<Target>,
    pipelines: GenArena<Pipeline>,
    /// The sampler objects, in the order of [`SAMPLERS`]; 0 until made.
    samplers: [GLuint; 4],
    ring: Ring,
    frame: Option<Frame>,
}

/// The internal and upload formats of a texture format.
fn gl_formats(format: TextureFormat) -> (GLint, GLenum) {
    match format {
        TextureFormat::Rgba8 => (GL_RGBA8, GL_RGBA),
        TextureFormat::Rgba8Srgb => (GL_SRGB8_ALPHA8, GL_RGBA),
        TextureFormat::R8 => (GL_R8, GL_RED),
    }
}

/// The filter and wrap of a sampler.
fn sampler_parameters(sampler: Sampler) -> (GLint, GLint) {
    match sampler {
        Sampler::NearestClamp => (GL_NEAREST, GL_CLAMP_TO_EDGE),
        Sampler::NearestRepeat => (GL_NEAREST, GL_REPEAT),
        Sampler::LinearClamp => (GL_LINEAR, GL_CLAMP_TO_EDGE),
        Sampler::LinearRepeat => (GL_LINEAR, GL_REPEAT),
    }
}

fn gl_mode(topology: Topology) -> GLenum {
    match topology {
        Topology::Triangles => GL_TRIANGLES,
        Topology::TriangleStrip => GL_TRIANGLE_STRIP,
        Topology::Lines => GL_LINES,
    }
}

fn gl_index_type(format: IndexFormat) -> GLenum {
    match format {
        IndexFormat::U16 => GL_UNSIGNED_SHORT,
        IndexFormat::U32 => GL_UNSIGNED_INT,
    }
}

/// The surface's context, if the surface is OpenGL.
fn gl_context(surface: GpuSurface<'_>) -> Result<&dyn GlContext, GfxError> {
    match surface {
        GpuSurface::Gl(context) => Ok(context),
        _ => Err(GfxError::UnsupportedSurface),
    }
}

impl GlDevice {
    /// Makes the device for the current context `context`, whose functions
    /// are `fns`.
    pub(crate) fn new(fns: GlFns, context: GlContextId) -> Result<Self, GfxError> {
        let mut device = Self {
            fns,
            status: Status::new(),
            context,
            caps: Caps {
                backend: BackendKind::OpenGl,
                vendor: String::new(),
                renderer: String::new(),
                version: String::new(),
                max_texture_size: 0,
            },
            cache: Cache::default(),
            textures: GenArena::new(),
            buffers: GenArena::new(),
            targets: GenArena::new(),
            pipelines: GenArena::new(),
            samplers: [0; 4],
            ring: Ring {
                buffer: 0,
                stride: 0,
                cursor: 0,
            },
            frame: None,
        };
        let gl = Gl::new(&device.fns, &device.status, device.context)?;
        // Nothing else may be called before the version is known to be 3.3.
        let major = gl.get_integer(Integer::MajorVersion)?;
        let minor = gl.get_integer(Integer::MinorVersion)?;
        let profile = gl.get_integer(Integer::ProfileMask)?;
        device.caps.version = gl.get_string(Text::Version)?;
        if (major, minor) < (3, 3) || profile & nadir_sys::gl::GL_CONTEXT_CORE_PROFILE_BIT == 0 {
            return Err(GfxError::GlTooOld {
                version: device.caps.version.clone(),
            });
        }
        // The version queries are the only calls so far, and they worked: an
        // error left is from before the device, and not its concern, unless
        // the context is out of memory.
        if gl.drain_errors() == Some(GL_OUT_OF_MEMORY) {
            return Err(GfxError::OutOfMemory("the context"));
        }
        device.caps.vendor = gl.get_string(Text::Vendor)?;
        device.caps.renderer = gl.get_string(Text::Renderer)?;
        device.caps.max_texture_size =
            u32::try_from(gl.get_integer(Integer::MaxTextureSize)?).unwrap_or(0);
        let alignment = gl.get_integer(Integer::UniformBufferOffsetAlignment)?;
        let alignment = usize::try_from(alignment)
            .ok()
            .filter(|&alignment| alignment > 0)
            .ok_or(GfxError::InvalidArgument(
                "the driver's uniform buffer offset alignment",
            ))?;
        gl.unpack_tightly()?;
        gl.enable(GL_FRAMEBUFFER_SRGB)?;
        for (name, sampler) in device.samplers.iter_mut().zip(SAMPLERS) {
            *name = gl.gen_sampler()?;
            let (filter, wrap) = sampler_parameters(sampler);
            gl.sampler_parameter(*name, GL_TEXTURE_MIN_FILTER, filter)?;
            gl.sampler_parameter(*name, GL_TEXTURE_MAG_FILTER, filter)?;
            gl.sampler_parameter(*name, GL_TEXTURE_WRAP_S, wrap)?;
            gl.sampler_parameter(*name, GL_TEXTURE_WRAP_T, wrap)?;
        }
        device.ring.stride = ring_stride(alignment);
        if device.ring.stride * BLOCKS > RING_SIZE {
            return Err(GfxError::InvalidArgument(
                "the driver's uniform buffer offset alignment",
            ));
        }
        device.ring.buffer = gl.gen_buffer()?;
        device.cache.bind_uniform_buffer(gl, device.ring.buffer)?;
        gl.buffer_data(
            GL_UNIFORM_BUFFER,
            RING_SIZE,
            None,
            GL_DYNAMIC_DRAW,
            "the uniform ring",
        )?;
        info!(
            "graphics: OpenGL {} on {} ({})",
            device.caps.version, device.caps.renderer, device.caps.vendor
        );
        Ok(device)
    }

    pub(crate) fn caps(&self) -> &Caps {
        &self.caps
    }

    /// How the device's calls are checked and counted.
    #[cfg(all(test, feature = "headless-test"))]
    pub(crate) fn status(&self) -> &Status {
        &self.status
    }

    /// Calls `glEnable`, which an unknown `cap` makes fail.
    #[cfg(all(test, feature = "headless-test"))]
    pub(crate) fn enable(&self, cap: GLenum) -> Result<(), GfxError> {
        Gl::new(&self.fns, &self.status, self.context)?.enable(cap)
    }

    // -- Resources --------------------------------------------------------------

    pub(crate) fn create_texture(
        &mut self,
        desc: &TextureDesc<'_>,
        data: Option<&[u8]>,
    ) -> Result<TextureId, GfxError> {
        let gl = Gl::new(&self.fns, &self.status, self.context)?;
        check_texture_size(desc.size, self.caps.max_texture_size)?;
        let name = gl.gen_texture()?;
        let (internal, format) = gl_formats(desc.format);
        let made = (|| {
            self.cache.bind_texture_to_change(gl, name)?;
            gl.tex_parameter(GL_TEXTURE_MAX_LEVEL, 0)?;
            gl.tex_image_2d(
                internal,
                [desc.size.x, desc.size.y],
                format,
                desc.format.bytes_per_pixel(),
                data,
            )
        })();
        if let Err(error) = made {
            delete(gl.delete_texture(name), desc.label);
            self.cache.invalidate();
            return Err(error);
        }
        Ok(TextureId(self.textures.insert(Texture {
            name,
            size: desc.size,
            format: desc.format,
        })))
    }

    pub(crate) fn update_texture(
        &mut self,
        id: TextureId,
        region: IRect,
        data: &[u8],
    ) -> Result<(), GfxError> {
        let texture = self
            .textures
            .get(id.0)
            .ok_or(GfxError::StaleHandle(ResourceKind::Texture))?;
        let [x, y, width, height] = check_region(region, texture.size)?;
        let bytes_per_pixel = texture.format.bytes_per_pixel();
        let expected = width as usize * height as usize * bytes_per_pixel;
        if data.len() != expected {
            return Err(GfxError::DataSize {
                expected,
                found: data.len(),
            });
        }
        let gl = Gl::new(&self.fns, &self.status, self.context)?;
        if expected == 0 {
            return Ok(());
        }
        let (_, format) = gl_formats(texture.format);
        self.cache.bind_texture_to_change(gl, texture.name)?;
        gl.tex_sub_image_2d([x, y, width, height], format, bytes_per_pixel, data)
    }

    pub(crate) fn create_buffer(&mut self, desc: &BufferDesc<'_>) -> Result<BufferId, GfxError> {
        if desc.size == 0 || isize::try_from(desc.size).is_err() {
            return Err(GfxError::InvalidArgument(
                "a buffer size, which must be at least 1",
            ));
        }
        let gl = Gl::new(&self.fns, &self.status, self.context)?;
        let shadow = match desc.kind {
            BufferKind::Vertex => None,
            BufferKind::Index => Some(IndexShadow::new(desc.size)?),
        };
        let usage = match desc.usage {
            BufferUsage::Static => GL_STATIC_DRAW,
            BufferUsage::Dynamic => GL_DYNAMIC_DRAW,
        };
        let name = gl.gen_buffer()?;
        let made = (|| {
            // Every upload, of index data too, goes through GL_ARRAY_BUFFER.
            // GL_ELEMENT_ARRAY_BUFFER is state of the bound vertex array,
            // whose record of its element buffer would then be wrong.
            self.cache.bind_array_buffer(gl, name)?;
            // An index buffer starts as its copy, zeros, so that the two
            // agree from the start.
            let data = shadow.as_ref().map(IndexShadow::bytes);
            gl.buffer_data(GL_ARRAY_BUFFER, desc.size, data, usage, "a buffer")
        })();
        if let Err(error) = made {
            delete(gl.delete_buffer(name), desc.label);
            self.cache.invalidate();
            return Err(error);
        }
        Ok(BufferId(self.buffers.insert(Buffer {
            name,
            size: desc.size,
            kind: desc.kind,
            shadow,
        })))
    }

    pub(crate) fn write_buffer(
        &mut self,
        id: BufferId,
        offset_elems: usize,
        element_size: usize,
        bytes: &[u8],
    ) -> Result<(), GfxError> {
        let buffer = self
            .buffers
            .get_mut(id.0)
            .ok_or(GfxError::StaleHandle(ResourceKind::Buffer))?;
        let offset = write_range(buffer.size, offset_elems, element_size, bytes.len())?;
        let gl = Gl::new(&self.fns, &self.status, self.context)?;
        if bytes.is_empty() {
            return Ok(());
        }
        self.cache.bind_array_buffer(gl, buffer.name)?;
        match &mut buffer.shadow {
            None => gl.buffer_sub_data(GL_ARRAY_BUFFER, offset, bytes),
            Some(shadow) => {
                // The copy must say what the GPU holds, so this write is
                // checked in every build, and copied only once it worked.
                gl.buffer_sub_data_checked(GL_ARRAY_BUFFER, offset, bytes)?;
                shadow.write(offset, bytes);
                Ok(())
            }
        }
    }

    pub(crate) fn create_render_target(
        &mut self,
        size: UVec2,
        format: TextureFormat,
    ) -> Result<RenderTargetId, GfxError> {
        let gl = Gl::new(&self.fns, &self.status, self.context)?;
        check_texture_size(size, self.caps.max_texture_size)?;
        let (internal, upload) = gl_formats(format);
        let texture = gl.gen_texture()?;
        let mut framebuffer = 0;
        let made = (|| {
            self.cache.bind_texture_to_change(gl, texture)?;
            gl.tex_parameter(GL_TEXTURE_MAX_LEVEL, 0)?;
            gl.tex_image_2d(
                internal,
                [size.x, size.y],
                upload,
                format.bytes_per_pixel(),
                None,
            )?;
            framebuffer = gl.gen_framebuffer()?;
            self.cache.bind_framebuffer(gl, framebuffer)?;
            gl.framebuffer_texture(texture)?;
            let status = gl.framebuffer_status()?;
            if status == GL_FRAMEBUFFER_COMPLETE {
                Ok(())
            } else {
                Err(GfxError::FramebufferIncomplete { status })
            }
        })();
        if let Err(error) = made {
            if framebuffer != 0 {
                delete(gl.delete_framebuffer(framebuffer), "a render target");
            }
            delete(gl.delete_texture(texture), "a render target");
            self.cache.invalidate();
            return Err(error);
        }
        Ok(RenderTargetId(self.targets.insert(Target {
            framebuffer,
            texture,
            size,
            format,
        })))
    }

    pub(crate) fn create_pipeline(
        &mut self,
        desc: &PipelineDesc<'_>,
    ) -> Result<PipelineId, GfxError> {
        let interface = |reason: String| GfxError::Interface {
            label: desc.label.to_string(),
            reason,
        };
        check_pipeline_desc(desc).map_err(interface)?;
        // The names go to GL as C strings. The check above refused NULs.
        let c_string =
            |name: &str| CString::new(name).map_err(|_| interface(format!("`{name}` has a NUL")));
        let block_names = desc
            .uniform_blocks
            .iter()
            .map(|block| c_string(block.name))
            .collect::<Result<Vec<_>, _>>()?;
        let sampler_names = desc
            .textures
            .iter()
            .map(|name| c_string(name))
            .collect::<Result<Vec<_>, _>>()?;
        let gl = Gl::new(&self.fns, &self.status, self.context)?;
        let vertex = compile(
            gl,
            GL_VERTEX_SHADER,
            desc.shader.glsl_vs,
            desc.label,
            ShaderStage::Vertex,
        )?;
        let fragment = match compile(
            gl,
            GL_FRAGMENT_SHADER,
            desc.shader.glsl_fs,
            desc.label,
            ShaderStage::Fragment,
        ) {
            Ok(fragment) => fragment,
            Err(error) => {
                delete(gl.delete_shader(vertex), desc.label);
                return Err(error);
            }
        };
        let program = link(gl, vertex, fragment, desc.label);
        delete(gl.delete_shader(vertex), desc.label);
        delete(gl.delete_shader(fragment), desc.label);
        let program = program?;
        let mut vertex_array = 0;
        let made = (|| {
            // Uniform blocks are found by name: their indices are the
            // driver's own order.
            let mut blocks = Vec::with_capacity(desc.uniform_blocks.len());
            let mut found: Vec<(GLuint, &str)> = Vec::with_capacity(desc.uniform_blocks.len());
            for (block, name) in desc.uniform_blocks.iter().zip(&block_names) {
                let index = gl.uniform_block_index(program, name)?;
                if index == GL_INVALID_INDEX {
                    warn!(
                        "pipeline `{}`: its shaders do not use uniform block `{}`",
                        desc.label, block.name
                    );
                    continue;
                }
                // Two names can find one block (`B` and `B[0]`), which would
                // leave another block without a binding.
                if let Some((_, other)) = found.iter().find(|(seen, _)| *seen == index) {
                    return Err(interface(format!(
                        "`{other}` and `{}` name the same uniform block",
                        block.name
                    )));
                }
                found.push((index, block.name));
                let size = gl.uniform_block_size(program, index)?;
                if usize::try_from(size).ok() != Some(block.size as usize) {
                    return Err(interface(format!(
                        "uniform block `{}` is {size} bytes in the shaders, and {} in the \
                         description",
                        block.name, block.size
                    )));
                }
                gl.uniform_block_binding(program, index, block.binding)?;
                blocks.push(Block {
                    binding: block.binding,
                    size: block.size as usize,
                });
            }
            // A block the description leaves out would read binding 0. The
            // blocks found are distinct, so this counts them exactly.
            let active = gl.active_uniform_blocks(program)?;
            if usize::try_from(active).is_ok_and(|active| active > blocks.len()) {
                return Err(interface(format!(
                    "the shaders use {active} uniform blocks, and the description names {} of \
                     them",
                    blocks.len()
                )));
            }
            // Each sampler is set to its slot's unit, once.
            self.cache.use_program(gl, program)?;
            for ((slot, name), label) in sampler_names.iter().enumerate().zip(desc.textures) {
                let location = gl.uniform_location(program, name)?;
                if location == -1 {
                    warn!(
                        "pipeline `{}`: its shaders do not use sampler `{label}`",
                        desc.label
                    );
                    continue;
                }
                gl.uniform_1i(location, GLint::try_from(slot).unwrap_or(0))?;
            }
            vertex_array = gl.gen_vertex_array()?;
            self.cache.bind_vertex_array(gl, vertex_array)?;
            let mut layouts = Vec::with_capacity(desc.vertex_layouts.len());
            let mut attributes = Vec::with_capacity(desc.vertex_layouts.len());
            for layout in desc.vertex_layouts {
                let mut slot = Vec::with_capacity(layout.attributes.len());
                let mut extent = 0;
                for attribute in layout.attributes {
                    gl.enable_vertex_attrib_array(attribute.location)?;
                    if layout.step == VertexStep::Instance {
                        gl.vertex_attrib_divisor(attribute.location, 1)?;
                    }
                    extent = extent.max(attribute.offset + attribute.format.size());
                    slot.push(Attribute {
                        location: attribute.location,
                        components: attribute.format.components(),
                        offset: attribute.offset,
                    });
                }
                layouts.push(SlotLayout {
                    stride: layout.stride,
                    step: layout.step,
                    extent,
                });
                attributes.push(slot);
            }
            Ok((blocks, layouts, attributes))
        })();
        let (blocks, layouts, attributes) = match made {
            Ok(made) => made,
            Err(error) => {
                if vertex_array != 0 {
                    delete(gl.delete_vertex_array(vertex_array), desc.label);
                }
                delete(gl.delete_program(program), desc.label);
                self.cache.invalidate();
                return Err(error);
            }
        };
        Ok(PipelineId(self.pipelines.insert(Pipeline {
            program,
            vertex_array,
            layouts,
            attributes,
            blocks,
            textures: desc.textures.len(),
            blend: desc.blend,
            mode: gl_mode(desc.topology),
            target_format: desc.target_format,
            slot_bound: [None; SLOTS],
            index_bound: None,
        })))
    }

    /// Destroys a resource. A stale id does nothing but a warning.
    pub(crate) fn destroy(&mut self, resource: ResourceId) {
        let names: [Option<(GLuint, DeleteName)>; 2] = match resource {
            ResourceId::Texture(id) => match self.textures.remove(id.0) {
                Some(texture) => [
                    Some((texture.name, |gl, name| gl.delete_texture(name))),
                    None,
                ],
                None => return stale(resource),
            },
            ResourceId::Buffer(id) => match self.buffers.remove(id.0) {
                Some(buffer) => [Some((buffer.name, |gl, name| gl.delete_buffer(name))), None],
                None => return stale(resource),
            },
            ResourceId::RenderTarget(id) => match self.targets.remove(id.0) {
                Some(target) => [
                    Some((target.framebuffer, |gl, name| gl.delete_framebuffer(name))),
                    Some((target.texture, |gl, name| gl.delete_texture(name))),
                ],
                None => return stale(resource),
            },
            ResourceId::Pipeline(id) => match self.pipelines.remove(id.0) {
                Some(pipeline) => [
                    Some((pipeline.program, |gl, name| gl.delete_program(name))),
                    Some((pipeline.vertex_array, |gl, name| {
                        gl.delete_vertex_array(name)
                    })),
                ],
                None => return stale(resource),
            },
        };
        // Once the context is gone, so are its objects. A lost device whose
        // context is still current deletes them.
        if let Some(gl) = Gl::for_teardown(&self.fns, &self.status, self.context) {
            for (name, delete_one) in names.into_iter().flatten() {
                delete(delete_one(gl, name), "a destroyed resource");
            }
        }
        // Deleting a bound object unbinds it, and its name comes back.
        self.cache.invalidate();
    }

    // -- Frames and passes ------------------------------------------------------

    pub(crate) fn begin_frame(&mut self, surface: GpuSurface<'_>) -> Result<(), GfxError> {
        let context = gl_context(surface)?;
        if context.context_id() != Some(self.context) {
            return Err(GfxError::WrongSurface);
        }
        let drawable = context.drawable_size();
        let gl = Gl::new(&self.fns, &self.status, self.context)?;
        if self.frame.is_some() {
            return Err(GfxError::FrameOrder("begin_frame came inside a frame"));
        }
        self.cache.invalidate();
        gl.unpack_tightly()?;
        gl.enable(GL_FRAMEBUFFER_SRGB)?;
        // New storage for the ring: the GPU may still read last frame's.
        self.cache.bind_uniform_buffer(gl, self.ring.buffer)?;
        gl.buffer_data(
            GL_UNIFORM_BUFFER,
            RING_SIZE,
            None,
            GL_DYNAMIC_DRAW,
            "the uniform ring",
        )?;
        self.ring.cursor = 0;
        self.frame = Some(Frame { drawable });
        Ok(())
    }

    pub(crate) fn end_frame(&mut self, surface: GpuSurface<'_>) -> Result<(), GfxError> {
        let context = gl_context(surface)?;
        if context.context_id() != Some(self.context) {
            return Err(GfxError::WrongSurface);
        }
        let gl = Gl::new(&self.fns, &self.status, self.context)?;
        if self.frame.take().is_none() {
            return Err(GfxError::FrameOrder("end_frame came outside a frame"));
        }
        // Where errors are not checked after every call, this is where they
        // are seen. Out of memory loses the device, and shows nothing.
        match gl.drain_errors() {
            None => {}
            Some(GL_OUT_OF_MEMORY) => return Err(GfxError::OutOfMemory("a frame")),
            Some(code) => warn!(
                "OpenGL reported {} in this frame, up to {}",
                crate::error::gl_error_name(code),
                self.status.last_call()
            ),
        }
        context.swap_buffers().map_err(GfxError::Present)
    }

    pub(crate) fn begin_pass(
        &mut self,
        target: PassTarget,
        clear: Option<LinearColor>,
    ) -> Result<PassState, GfxError> {
        let gl = Gl::new(&self.fns, &self.status, self.context)?;
        let Some(frame) = &self.frame else {
            return Err(GfxError::FrameOrder("a pass came outside a frame"));
        };
        let info = match target {
            PassTarget::Surface => PassTargetInfo {
                framebuffer: 0,
                size: frame.drawable,
                format: TextureFormat::Rgba8Srgb,
                render_target: None,
            },
            PassTarget::RenderTarget(id) => {
                let target = self
                    .targets
                    .get(id.0)
                    .ok_or(GfxError::StaleHandle(ResourceKind::RenderTarget))?;
                PassTargetInfo {
                    framebuffer: target.framebuffer,
                    size: target.size,
                    format: target.format,
                    render_target: Some(id),
                }
            }
        };
        let state = PassState::new(info);
        if state.is_empty() {
            // Nothing to draw into: the pass makes no GL call at all.
            return Ok(state);
        }
        self.cache.bind_framebuffer(gl, info.framebuffer)?;
        self.cache
            .viewport(gl, rect_to_gl(full(info.size), info.size.y))?;
        self.cache.scissor(gl, None)?;
        if let Some(color) = clear {
            gl.clear_color([color.r, color.g, color.b, color.a])?;
            gl.clear(GL_COLOR_BUFFER_BIT)?;
        }
        Ok(state)
    }

    pub(crate) fn set_pipeline(
        &self,
        state: &mut PassState,
        id: PipelineId,
    ) -> Result<(), GfxError> {
        state.pipeline = None;
        let pipeline = self
            .pipelines
            .get(id.0)
            .ok_or(GfxError::StaleHandle(ResourceKind::Pipeline))?;
        if pipeline.target_format != state.target.format {
            return Err(GfxError::FormatMismatch {
                pipeline: pipeline.target_format,
                target: state.target.format,
            });
        }
        state.pipeline = Some(id);
        Ok(())
    }

    pub(crate) fn set_vertex_buffer(
        &self,
        state: &mut PassState,
        slot: u32,
        id: BufferId,
        offset: usize,
    ) -> Result<(), GfxError> {
        let bound = state
            .vertex
            .get_mut(slot as usize)
            .ok_or(GfxError::InvalidArgument("a vertex buffer slot"))?;
        *bound = None;
        if !self.buffers.contains(id.0) {
            return Err(GfxError::StaleHandle(ResourceKind::Buffer));
        }
        *bound = Some((id, offset));
        Ok(())
    }

    pub(crate) fn set_index_buffer(
        &self,
        state: &mut PassState,
        id: BufferId,
        format: IndexFormat,
    ) -> Result<(), GfxError> {
        state.index = None;
        if !self.buffers.contains(id.0) {
            return Err(GfxError::StaleHandle(ResourceKind::Buffer));
        }
        state.index = Some((id, format));
        Ok(())
    }

    pub(crate) fn set_texture(
        &self,
        state: &mut PassState,
        slot: u32,
        texture: TextureRef,
        sampler: Sampler,
    ) -> Result<(), GfxError> {
        let bound = state
            .textures
            .get_mut(slot as usize)
            .ok_or(GfxError::InvalidArgument("a texture slot"))?;
        *bound = None;
        match texture {
            TextureRef::Texture(id) if !self.textures.contains(id.0) => {
                return Err(GfxError::StaleHandle(ResourceKind::Texture));
            }
            TextureRef::RenderTarget(id) if !self.targets.contains(id.0) => {
                return Err(GfxError::StaleHandle(ResourceKind::RenderTarget));
            }
            TextureRef::RenderTarget(id) if state.target.render_target == Some(id) => {
                return Err(GfxError::FeedbackLoop);
            }
            _ => {}
        }
        *bound = Some((texture, sampler));
        Ok(())
    }

    pub(crate) fn set_uniforms(
        state: &mut PassState,
        block: u32,
        bytes: &[u8],
    ) -> Result<(), GfxError> {
        let uniform = state
            .uniforms
            .get_mut(block as usize)
            .ok_or(GfxError::InvalidArgument("a uniform block binding"))?;
        let Some(target) = uniform.bytes.get_mut(..bytes.len()) else {
            // A failed setter leaves the binding unset.
            uniform.len = 0;
            uniform.dirty = false;
            return Err(GfxError::InvalidArgument(
                "uniform data larger than 256 bytes",
            ));
        };
        target.copy_from_slice(bytes);
        uniform.len = bytes.len();
        uniform.dirty = true;
        Ok(())
    }

    /// Checks a draw, sets what it needs, and draws.
    pub(crate) fn draw(
        &mut self,
        state: &mut PassState,
        vertices: DrawVertices,
        instances: Range<u32>,
    ) -> Result<(), GfxError> {
        let gl = Gl::new(&self.fns, &self.status, self.context)?;
        let empty_range = match &vertices {
            DrawVertices::Arrays(range) | DrawVertices::Indexed { range, .. } => range.is_empty(),
        };
        if empty_range || instances.is_empty() {
            return Ok(());
        }
        let pipeline_id = state.pipeline.ok_or(GfxError::NoPipeline)?;
        let pipeline = self
            .pipelines
            .get_mut(pipeline_id.0)
            .ok_or(GfxError::StaleHandle(ResourceKind::Pipeline))?;

        // Everything the draw reads must be set and in bounds.
        let mut bound = [None; SLOTS];
        for (slot, binding) in state.vertex.iter().enumerate().take(pipeline.layouts.len()) {
            let Some((id, offset)) = *binding else {
                continue;
            };
            let buffer = self
                .buffers
                .get(id.0)
                .ok_or(GfxError::StaleHandle(ResourceKind::Buffer))?;
            if buffer.kind != BufferKind::Vertex {
                return Err(GfxError::Unbound {
                    what: "vertex buffer (an index buffer is set)",
                    slot: slot as u32,
                });
            }
            bound[slot] = Some(BoundVertices {
                size: buffer.size,
                offset,
            });
        }
        let (kind, index) = match &vertices {
            DrawVertices::Arrays(range) => (DrawKind::Arrays(range.clone()), None),
            DrawVertices::Indexed { range, base_vertex } => {
                let (id, format) = state.index.ok_or(GfxError::Unbound {
                    what: "index buffer",
                    slot: 0,
                })?;
                let buffer = self
                    .buffers
                    .get(id.0)
                    .ok_or(GfxError::StaleHandle(ResourceKind::Buffer))?;
                let shadow = buffer.shadow.as_ref().ok_or(GfxError::Unbound {
                    what: "index buffer (a vertex buffer is set)",
                    slot: 0,
                })?;
                let kind = DrawKind::Indexed {
                    indices: BoundIndices { shadow, format },
                    range: range.clone(),
                    base_vertex: *base_vertex,
                };
                (kind, Some((id, buffer.name, format)))
            }
        };
        check_draw(
            &pipeline.layouts,
            &bound[..pipeline.layouts.len()],
            &kind,
            &instances,
        )?;
        for block in &pipeline.blocks {
            let uniform = &state.uniforms[block.binding as usize];
            if uniform.len == 0 {
                return Err(GfxError::Unbound {
                    what: "uniform block",
                    slot: block.binding,
                });
            }
            if uniform.len != block.size {
                return Err(GfxError::DataSize {
                    expected: block.size,
                    found: uniform.len,
                });
            }
        }
        let mut units = [(0, 0); UNITS];
        for (slot, unit) in units.iter_mut().enumerate().take(pipeline.textures) {
            let (texture, sampler) = state.textures[slot].ok_or(GfxError::Unbound {
                what: "texture",
                slot: slot as u32,
            })?;
            let name = match texture {
                TextureRef::Texture(id) => self.textures.get(id.0).map(|texture| texture.name),
                TextureRef::RenderTarget(id) => self.targets.get(id.0).map(|target| target.texture),
            };
            let name = name.ok_or(GfxError::StaleHandle(match texture {
                TextureRef::Texture(_) => ResourceKind::Texture,
                TextureRef::RenderTarget(_) => ResourceKind::RenderTarget,
            }))?;
            *unit = (name, self.samplers[sampler as usize]);
        }

        // A pass with no pixels, or a draw outside them, draws nothing.
        let size = state.target.size;
        let viewport = rect_to_gl(state.viewport, size.y);
        let scissor = state.scissor.map(|scissor| intersect(scissor, size));
        if state.is_empty()
            || viewport[2] == 0
            || viewport[3] == 0
            || scissor.is_some_and(IRect::is_empty)
        {
            return Ok(());
        }

        let applied = apply(
            gl,
            &mut self.cache,
            &self.buffers,
            &mut self.ring,
            pipeline,
            state,
            &units,
            viewport,
            scissor.map(|scissor| rect_to_gl(scissor, size.y)),
            index,
            &instances,
        );
        let drawn = applied.and_then(|()| {
            // Every state call worked, so GL holds what was checked: in every
            // build, no call may have failed unseen. (No error is dropped
            // between the state calls and here: a call that allocates fails
            // on an error left before it.)
            gl.check_pending("a draw")?;
            let count = |range: &Range<u32>| i32::try_from(range.end - range.start).unwrap_or(0);
            let instance_count = count(&instances);
            match &vertices {
                DrawVertices::Arrays(range) => {
                    let first = i32::try_from(range.start).unwrap_or(0);
                    // SAFETY: `check_draw` found that, for these vertices
                    // and instances, every slot of the pipeline reads inside
                    // its buffer, and `apply` pointed the vertex array at
                    // those buffers and offsets (instance slots moved on by
                    // the first instance). Every uniform block of the
                    // program has a range of its size bound: the pipeline
                    // describes each, once. GL reported no failure of those
                    // calls. What the shaders' own code indexes is theirs to
                    // keep in bounds (see `Device::create_pipeline`).
                    unsafe {
                        gl.draw_arrays_instanced(pipeline.mode, first, count(range), instance_count)
                    }
                }
                DrawVertices::Indexed { range, base_vertex } => {
                    let Some((_, _, format)) = index else {
                        return Err(GfxError::Unbound {
                            what: "index buffer",
                            slot: 0,
                        });
                    };
                    let offset = range.start as usize * format.size();
                    // SAFETY: as above, and the element buffer of the vertex
                    // array is the index buffer, whose indices `range` are in
                    // it, and each plus `base_vertex` names a vertex that
                    // every per-vertex slot reads inside its buffer.
                    unsafe {
                        gl.draw_elements_instanced_base_vertex(
                            pipeline.mode,
                            count(range),
                            gl_index_type(format),
                            offset,
                            instance_count,
                            *base_vertex,
                        )
                    }
                }
            }
        });
        if drawn.is_err() {
            // What GL holds is no longer known.
            pipeline.forget_bindings();
            self.cache.invalidate();
            for uniform in &mut state.uniforms {
                uniform.dirty = uniform.len > 0;
            }
        }
        drawn
    }

    pub(crate) fn read_pixels(&mut self, id: RenderTargetId) -> Result<Vec<u8>, GfxError> {
        let target = self
            .targets
            .get(id.0)
            .ok_or(GfxError::StaleHandle(ResourceKind::RenderTarget))?;
        let gl = Gl::new(&self.fns, &self.status, self.context)?;
        let row = target.size.x as usize * 4;
        let mut pixels = vec![0; row * target.size.y as usize];
        self.cache.bind_framebuffer(gl, target.framebuffer)?;
        gl.read_pixels([target.size.x, target.size.y], &mut pixels)?;
        gl.check_pending("a readback")?;
        // GL's rows start at the bottom.
        flip_rows(&mut pixels, row);
        Ok(pixels)
    }
}

/// The vertices of a draw.
pub(crate) enum DrawVertices {
    Arrays(Range<u32>),
    Indexed { range: Range<u32>, base_vertex: i32 },
}

/// Sets what a checked draw needs: the program, blending, the vertex array
/// and its pointers, the uniforms, the textures, the viewport and scissor.
#[expect(
    clippy::too_many_arguments,
    reason = "the parts of the device a draw sets, borrowed apart"
)]
fn apply(
    gl: Gl<'_>,
    cache: &mut Cache,
    buffers: &GenArena<Buffer>,
    ring: &mut Ring,
    pipeline: &mut Pipeline,
    state: &mut PassState,
    units: &[(GLuint, GLuint); UNITS],
    viewport: [i32; 4],
    scissor: Option<[i32; 4]>,
    index: Option<(BufferId, GLuint, IndexFormat)>,
    instances: &Range<u32>,
) -> Result<(), GfxError> {
    cache.use_program(gl, pipeline.program)?;
    cache.blend(gl, pipeline.blend)?;
    cache.bind_vertex_array(gl, pipeline.vertex_array)?;
    for (slot, (layout, attributes)) in pipeline
        .layouts
        .iter()
        .zip(&pipeline.attributes)
        .enumerate()
    {
        let Some((id, offset)) = state.vertex[slot] else {
            continue;
        };
        // An instance slot starts at the first instance: GL 3.3 has no base
        // instance.
        let offset = match layout.step {
            VertexStep::Vertex => offset,
            VertexStep::Instance => offset + instances.start as usize * layout.stride as usize,
        };
        if pipeline.slot_bound[slot] == Some((id, offset)) {
            continue;
        }
        let name = buffers
            .get(id.0)
            .map(|buffer| buffer.name)
            .ok_or(GfxError::StaleHandle(ResourceKind::Buffer))?;
        pipeline.slot_bound[slot] = None;
        cache.bind_array_buffer(gl, name)?;
        for attribute in attributes {
            gl.vertex_attrib_pointer(
                attribute.location,
                attribute.components,
                layout.stride,
                offset + attribute.offset as usize,
            )?;
        }
        pipeline.slot_bound[slot] = Some((id, offset));
    }
    if let Some((id, name, _)) = index
        && pipeline.index_bound != Some(id)
    {
        pipeline.index_bound = None;
        // The element buffer is state of the vertex array, which is bound.
        gl.bind_buffer(GL_ELEMENT_ARRAY_BUFFER, name)?;
        pipeline.index_bound = Some(id);
    }
    // Every block the draw uses goes into the ring after the last time the
    // ring got new storage.
    let dirty = pipeline
        .blocks
        .iter()
        .filter(|block| state.uniforms[block.binding as usize].dirty)
        .count();
    if ring.cursor + dirty * ring.stride > RING_SIZE {
        cache.bind_uniform_buffer(gl, ring.buffer)?;
        gl.buffer_data(
            GL_UNIFORM_BUFFER,
            RING_SIZE,
            None,
            GL_DYNAMIC_DRAW,
            "the uniform ring",
        )?;
        ring.cursor = 0;
        for uniform in &mut state.uniforms {
            uniform.dirty = uniform.len > 0;
        }
    }
    for block in &pipeline.blocks {
        let uniform = &mut state.uniforms[block.binding as usize];
        if !uniform.dirty {
            continue;
        }
        cache.bind_uniform_buffer(gl, ring.buffer)?;
        gl.buffer_sub_data(GL_UNIFORM_BUFFER, ring.cursor, &uniform.bytes[..block.size])?;
        cache.bind_uniform_range(gl, block.binding, ring.buffer, ring.cursor, block.size)?;
        ring.cursor += ring.stride;
        uniform.dirty = false;
    }
    for (unit, &(texture, sampler)) in units.iter().enumerate().take(pipeline.textures) {
        let unit = unit as u32;
        cache.bind_texture(gl, unit, texture)?;
        cache.bind_sampler(gl, unit, sampler)?;
    }
    cache.viewport(gl, viewport)?;
    cache.scissor(gl, scissor)
}

/// Compiles one shader stage.
fn compile(
    gl: Gl<'_>,
    stage: GLenum,
    source: &str,
    label: &str,
    which: ShaderStage,
) -> Result<GLuint, GfxError> {
    let shader = gl.create_shader(stage)?;
    let compiled = (|| {
        gl.shader_source(shader, source)?;
        gl.compile_shader(shader)?;
        if gl.shader_compiled(shader)? {
            Ok(())
        } else {
            Err(GfxError::Shader {
                label: label.to_string(),
                stage: which,
                log: gl.shader_info_log(shader)?,
            })
        }
    })();
    match compiled {
        Ok(()) => Ok(shader),
        Err(error) => {
            delete(gl.delete_shader(shader), label);
            Err(error)
        }
    }
}

/// Links two compiled stages into a program.
fn link(gl: Gl<'_>, vertex: GLuint, fragment: GLuint, label: &str) -> Result<GLuint, GfxError> {
    let program = gl.create_program()?;
    let linked = (|| {
        gl.attach_shader(program, vertex)?;
        gl.attach_shader(program, fragment)?;
        gl.link_program(program)?;
        let log = gl.program_info_log(program)?;
        if gl.program_linked(program)? {
            if log != NO_LOG {
                warn!("pipeline `{label}` linked, and the driver said: {log}");
            }
            Ok(())
        } else {
            Err(GfxError::Shader {
                label: label.to_string(),
                stage: ShaderStage::Link,
                log,
            })
        }
    })();
    match linked {
        Ok(()) => Ok(program),
        Err(error) => {
            delete(gl.delete_program(program), label);
            Err(error)
        }
    }
}

/// Deletes one GL object by name.
type DeleteName = fn(Gl<'_>, GLuint) -> Result<(), GfxError>;

/// Reports a deletion that failed: there is nothing else to do about one.
fn delete(result: Result<(), GfxError>, what: &str) {
    if let Err(error) = result {
        warn!("could not delete the GL objects of {what}: {error}");
    }
}

/// Warns that a resource destroyed already was destroyed again.
fn stale(resource: ResourceId) {
    warn!("{:?} was destroyed already", resource.kind());
}

impl Drop for GlDevice {
    fn drop(&mut self) {
        // Once the context is gone, so are its objects. Lost or not, a
        // current context can delete them.
        let Some(gl) = Gl::for_teardown(&self.fns, &self.status, self.context) else {
            return;
        };
        for (_, texture) in self.textures.iter() {
            delete(gl.delete_texture(texture.name), "a texture");
        }
        for (_, buffer) in self.buffers.iter() {
            delete(gl.delete_buffer(buffer.name), "a buffer");
        }
        for (_, target) in self.targets.iter() {
            delete(gl.delete_framebuffer(target.framebuffer), "a render target");
            delete(gl.delete_texture(target.texture), "a render target");
        }
        for (_, pipeline) in self.pipelines.iter() {
            delete(gl.delete_program(pipeline.program), "a pipeline");
            delete(gl.delete_vertex_array(pipeline.vertex_array), "a pipeline");
        }
        for &sampler in self.samplers.iter().filter(|&&sampler| sampler != 0) {
            delete(gl.delete_sampler(sampler), "a sampler");
        }
        if self.ring.buffer != 0 {
            delete(gl.delete_buffer(self.ring.buffer), "the uniform ring");
        }
    }
}
