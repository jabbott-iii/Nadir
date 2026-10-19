//! The types of the API: resource ids, descriptions of resources and
//! pipelines, and what the device can do.

use nadir_core::{GenIndex, UVec2};

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// The number of vertex buffer slots a pipeline can have: slots 0 to 7.
pub const MAX_VERTEX_BUFFERS: u32 = 8;

/// The number of vertex attribute locations: 0 to 15.
pub const MAX_VERTEX_ATTRIBUTES: u32 = 16;

/// The number of uniform block bindings: 0 to 7.
pub const MAX_UNIFORM_BLOCKS: u32 = 8;

/// The number of texture slots a pipeline can have: 0 to 7.
pub const MAX_TEXTURE_SLOTS: u32 = 8;

/// The most bytes one uniform block can hold.
pub const MAX_UNIFORM_BYTES: usize = 256;

// ---------------------------------------------------------------------------
// Resource ids
// ---------------------------------------------------------------------------

/// A texture of a [`Device`](crate::Device). It stops resolving when the
/// texture is destroyed: using it then gives
/// [`GfxError::StaleHandle`](crate::GfxError::StaleHandle).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(pub(crate) GenIndex);

/// A buffer of a [`Device`](crate::Device).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BufferId(pub(crate) GenIndex);

/// A render target of a [`Device`](crate::Device): a texture that passes can
/// draw into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RenderTargetId(pub(crate) GenIndex);

/// A pipeline of a [`Device`](crate::Device): shaders, vertex layout and
/// blending, ready to draw with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PipelineId(pub(crate) GenIndex);

/// Any resource, for [`Device::destroy`](crate::Device::destroy).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceId {
    /// A texture.
    Texture(TextureId),
    /// A buffer.
    Buffer(BufferId),
    /// A render target.
    RenderTarget(RenderTargetId),
    /// A pipeline.
    Pipeline(PipelineId),
}

impl ResourceId {
    /// What kind of resource this is.
    pub const fn kind(self) -> ResourceKind {
        match self {
            Self::Texture(_) => ResourceKind::Texture,
            Self::Buffer(_) => ResourceKind::Buffer,
            Self::RenderTarget(_) => ResourceKind::RenderTarget,
            Self::Pipeline(_) => ResourceKind::Pipeline,
        }
    }
}

impl From<TextureId> for ResourceId {
    fn from(id: TextureId) -> Self {
        Self::Texture(id)
    }
}

impl From<BufferId> for ResourceId {
    fn from(id: BufferId) -> Self {
        Self::Buffer(id)
    }
}

impl From<RenderTargetId> for ResourceId {
    fn from(id: RenderTargetId) -> Self {
        Self::RenderTarget(id)
    }
}

impl From<PipelineId> for ResourceId {
    fn from(id: PipelineId) -> Self {
        Self::Pipeline(id)
    }
}

/// The kinds of resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    /// A texture.
    Texture,
    /// A buffer.
    Buffer,
    /// A render target.
    RenderTarget,
    /// A pipeline.
    Pipeline,
}

/// What a texture slot samples: a texture, or what a render target holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureRef {
    /// A texture.
    Texture(TextureId),
    /// A render target. A pass cannot sample the target it draws into.
    RenderTarget(RenderTargetId),
}

impl From<TextureId> for TextureRef {
    fn from(id: TextureId) -> Self {
        Self::Texture(id)
    }
}

impl From<RenderTargetId> for TextureRef {
    fn from(id: RenderTargetId) -> Self {
        Self::RenderTarget(id)
    }
}

/// What a pass draws into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PassTarget {
    /// The window, or nothing on a context without a default framebuffer:
    /// its format is [`TextureFormat::Rgba8Srgb`].
    Surface,
    /// A render target.
    RenderTarget(RenderTargetId),
}

impl From<RenderTargetId> for PassTarget {
    fn from(id: RenderTargetId) -> Self {
        Self::RenderTarget(id)
    }
}

// ---------------------------------------------------------------------------
// Textures
// ---------------------------------------------------------------------------

/// How the pixels of a texture or render target are stored.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    /// Red, green, blue and alpha, 8 bits each, used as they are.
    Rgba8,
    /// Red, green, blue and alpha, 8 bits each, with the colors sRGB-encoded:
    /// sampling decodes them to linear, and drawing encodes them (ADR-0013).
    Rgba8Srgb,
    /// One 8-bit channel. It samples as red, with green and blue 0 and alpha
    /// 1.
    R8,
}

impl TextureFormat {
    /// The bytes of one pixel.
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba8 | Self::Rgba8Srgb => 4,
            Self::R8 => 1,
        }
    }
}

/// A texture to create.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureDesc<'a> {
    /// A name for logs and errors.
    pub label: &'a str,
    /// The size in pixels. Each side is from 1 to [`Caps::max_texture_size`].
    pub size: UVec2,
    /// How the pixels are stored.
    pub format: TextureFormat,
}

// ---------------------------------------------------------------------------
// Buffers
// ---------------------------------------------------------------------------

/// What a buffer holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BufferKind {
    /// Vertex or instance data, for [`Pass::set_vertex_buffer`].
    ///
    /// [`Pass::set_vertex_buffer`]: crate::Pass::set_vertex_buffer
    Vertex,
    /// Indices, for [`Pass::set_index_buffer`]. The device keeps a copy of
    /// them, to check every indexed draw.
    ///
    /// [`Pass::set_index_buffer`]: crate::Pass::set_index_buffer
    Index,
}

/// How often a buffer is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BufferUsage {
    /// Written once, or rarely.
    Static,
    /// Written every frame. What is written is good only for the frame it is
    /// written in.
    Dynamic,
}

/// A buffer to create.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferDesc<'a> {
    /// A name for logs and errors.
    pub label: &'a str,
    /// What it holds.
    pub kind: BufferKind,
    /// How often it is written.
    pub usage: BufferUsage,
    /// Its size in bytes, at least 1. An index buffer starts as zeros; a
    /// vertex buffer starts with no particular contents.
    pub size: usize,
}

/// The type of each index of an index buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndexFormat {
    /// `u16` indices.
    U16,
    /// `u32` indices.
    U32,
}

impl IndexFormat {
    /// The bytes of one index.
    pub const fn size(self) -> usize {
        match self {
            Self::U16 => 2,
            Self::U32 => 4,
        }
    }
}

// ---------------------------------------------------------------------------
// Pipelines
// ---------------------------------------------------------------------------

/// A pipeline to create.
#[derive(Clone, Copy, Debug)]
pub struct PipelineDesc<'a> {
    /// A name for logs and errors.
    pub label: &'a str,
    /// The shaders.
    pub shader: ShaderSource<'a>,
    /// The vertex buffer slots, in slot order: at most
    /// [`MAX_VERTEX_BUFFERS`].
    pub vertex_layouts: &'a [VertexLayout<'a>],
    /// The uniform blocks of the shaders: at most [`MAX_UNIFORM_BLOCKS`].
    pub uniform_blocks: &'a [UniformBlockDesc<'a>],
    /// The names of the samplers of the shaders, in slot order: at most
    /// [`MAX_TEXTURE_SLOTS`].
    pub textures: &'a [&'a str],
    /// How the colors drawn mix with what is there.
    pub blend: BlendMode,
    /// What the vertices make.
    pub topology: Topology,
    /// The format of what the pipeline draws into. A pass of another format
    /// cannot use it.
    pub target_format: TextureFormat,
}

/// The shaders of a pipeline, for each backend. Each backend takes its own.
///
/// The device checks what a draw reads through the resources it binds, but
/// not what the shaders' own code does: an array index out of its bounds in
/// a shader is undefined behavior that no check covers.
#[derive(Clone, Copy, Debug)]
pub struct ShaderSource<'a> {
    /// The OpenGL vertex shader: GLSL whose first line is exactly
    /// `#version 330 core`, with no `#extension` directive.
    pub glsl_vs: &'a str,
    /// The OpenGL fragment shader, also GLSL 330 core.
    pub glsl_fs: &'a str,
    /// The Metal shaders, with `vs_main` and `fs_main` (M10). OpenGL does
    /// not read it.
    pub msl: &'a str,
}

/// One vertex buffer slot of a pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VertexLayout<'a> {
    /// The bytes from one vertex (or instance) to the next: a multiple of 4
    /// from 4 to 2048.
    pub stride: u32,
    /// Whether the slot moves on per vertex or per instance.
    pub step: VertexStep,
    /// The attributes in each vertex: at least one.
    pub attributes: &'a [VertexAttribute],
}

/// When a vertex buffer slot moves on to its next record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VertexStep {
    /// Once per vertex.
    Vertex,
    /// Once per instance. Shaders read instance data only through such
    /// attributes, never through `gl_InstanceID`, because a draw of
    /// instances `n..m` starts the buffer at instance `n`.
    Instance,
}

/// One attribute of a vertex.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VertexAttribute {
    /// The shader's `layout(location = N)`: below
    /// [`MAX_VERTEX_ATTRIBUTES`], and used once in a pipeline.
    pub location: u32,
    /// What it holds.
    pub format: VertexFormat,
    /// Its bytes from the start of the vertex: a multiple of 4, with the
    /// attribute inside the stride.
    pub offset: u32,
}

/// What a vertex attribute holds.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VertexFormat {
    /// One `f32`: a `float`.
    Float32,
    /// Two: a `vec2`.
    Float32x2,
    /// Three: a `vec3`.
    Float32x3,
    /// Four: a `vec4`.
    Float32x4,
}

impl VertexFormat {
    /// The number of `f32`s.
    pub const fn components(self) -> u32 {
        match self {
            Self::Float32 => 1,
            Self::Float32x2 => 2,
            Self::Float32x3 => 3,
            Self::Float32x4 => 4,
        }
    }

    /// The bytes it takes.
    pub const fn size(self) -> u32 {
        self.components() * 4
    }
}

/// A uniform block of a pipeline's shaders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UniformBlockDesc<'a> {
    /// The block's name in the shaders.
    pub name: &'a str,
    /// The binding that [`Pass::set_uniforms`] fills it from: below
    /// [`MAX_UNIFORM_BLOCKS`], and used once in a pipeline.
    ///
    /// [`Pass::set_uniforms`]: crate::Pass::set_uniforms
    pub binding: u32,
    /// The block's size in bytes, laid out by std140 rules: a multiple of
    /// 16, from 16 to [`MAX_UNIFORM_BYTES`]. It must be the size the shader
    /// compiler gives the block.
    pub size: u32,
}

/// How a draw's colors mix with what the target holds. Blending happens in
/// linear light, on sRGB targets too (ADR-0013).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlendMode {
    /// The color drawn replaces what is there, alpha included.
    Opaque,
    /// Straight alpha: `src * a + dst * (1 - a)`, and alpha
    /// `a + dst_a * (1 - a)`.
    Alpha,
    /// Premultiplied alpha: `src + dst * (1 - a)`, for color and alpha.
    PremultipliedAlpha,
    /// Light added: `src * a + dst`, with alpha left as it is.
    Additive,
    /// Darkening: `src * dst`, with alpha left as it is.
    Multiply,
}

/// What the vertices of a draw make.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Topology {
    /// Each three vertices make a triangle.
    Triangles,
    /// Each vertex after the second makes a triangle with the two before it.
    TriangleStrip,
    /// Each two vertices make a line.
    Lines,
}

/// How a texture slot reads its texture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sampler {
    /// The nearest texel, with coordinates outside 0 to 1 clamped to the
    /// edge. The default, for pixel art.
    #[default]
    NearestClamp,
    /// The nearest texel, with the texture repeated.
    NearestRepeat,
    /// The four nearest texels, mixed, clamped to the edge.
    LinearClamp,
    /// The four nearest texels, mixed, with the texture repeated.
    LinearRepeat,
}

/// A stage of building a pipeline's program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    /// Compiling the vertex shader.
    Vertex,
    /// Compiling the fragment shader.
    Fragment,
    /// Linking the two.
    Link,
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// What a device runs on, and its limits.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caps {
    /// The backend.
    pub backend: BackendKind,
    /// Who made the driver, as it says (`GL_VENDOR`).
    pub vendor: String,
    /// The renderer, as the driver names it (`GL_RENDERER`), such as
    /// `llvmpipe (LLVM 20.1.2, 256 bits)`.
    pub renderer: String,
    /// The version, as the driver gives it (`GL_VERSION`).
    pub version: String,
    /// The widest and tallest texture or render target, in pixels.
    pub max_texture_size: u32,
}

/// The kinds of backend.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// OpenGL 3.3, core profile.
    OpenGl,
}

impl BackendKind {
    /// The backend's name, for logs: `"OpenGL"`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::OpenGl => "OpenGL",
        }
    }
}
