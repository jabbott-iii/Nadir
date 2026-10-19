//! What the tests share: an offscreen device, shaders, and small helpers.

use std::env;

use nadir_core::log::warn;
use nadir_core::{IRect, IVec2, LinearColor, UVec2};
use nadir_gfx::{
    BlendMode, BufferDesc, BufferId, BufferKind, BufferUsage, Device, IndexFormat, PipelineDesc,
    PipelineId, RenderTargetId, ShaderSource, TextureFormat, Topology, UniformBlockDesc,
    VertexAttribute, VertexFormat, VertexLayout, VertexStep,
};
use nadir_platform::{GpuSurface, HeadlessGl, PlatformError, create_headless_gl};

/// Names a part of the renderer, such as `llvmpipe`, that the context must
/// have. Unset or empty, any renderer will do.
pub(crate) const EXPECT_RENDERER: &str = "NADIR_EXPECT_GL_RENDERER";

/// An offscreen context, or `None` where `nadir-platform` cannot make one
/// yet (Windows and macOS, until M2.6 and M2.7) and no renderer is asked
/// for. Any other failure fails the test.
pub(crate) fn headless() -> Option<HeadlessGl> {
    let expected = env::var(EXPECT_RENDERER).unwrap_or_default();
    match create_headless_gl() {
        Ok(gl) => Some(gl),
        Err(PlatformError::Unsupported) if expected.is_empty() => {
            warn!("skipped: this system has no offscreen OpenGL context yet");
            None
        }
        Err(error) => panic!(
            "no offscreen OpenGL context: {error}. With libglvnd, Mesa can be chosen with \
             __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json"
        ),
    }
}

/// A device on an offscreen context.
pub(crate) struct Gpu {
    // The device goes first: it deletes its objects while the context is
    // there.
    pub(crate) device: Device,
    pub(crate) gl: HeadlessGl,
}

impl Gpu {
    pub(crate) fn new() -> Option<Self> {
        let gl = headless()?;
        let device = Device::new(GpuSurface::Gl(&gl)).expect("a device on an offscreen context");
        Some(Self { device, gl })
    }

    pub(crate) fn begin(&mut self) {
        self.device
            .begin_frame(GpuSurface::Gl(&self.gl))
            .expect("a frame to begin");
    }

    pub(crate) fn end(&mut self) {
        self.device
            .end_frame(GpuSurface::Gl(&self.gl))
            .expect("a frame to end");
    }

    pub(crate) fn target(
        &mut self,
        width: u32,
        height: u32,
        format: TextureFormat,
    ) -> RenderTargetId {
        self.device
            .create_render_target(UVec2::new(width, height), format)
            .expect("a render target")
    }

    pub(crate) fn vertices(&mut self, data: &[f32]) -> BufferId {
        let buffer = self
            .device
            .create_buffer(&BufferDesc {
                label: "vertices",
                kind: BufferKind::Vertex,
                usage: BufferUsage::Static,
                size: size_of_val(data),
            })
            .expect("a vertex buffer");
        self.device
            .write_buffer(buffer, 0, data)
            .expect("a vertex buffer write");
        buffer
    }

    pub(crate) fn indices_u16(&mut self, data: &[u16]) -> BufferId {
        self.indices(data.len() * 2, |device, buffer| {
            device
                .write_buffer(buffer, 0, data)
                .expect("an index buffer write");
        })
    }

    pub(crate) fn indices_u32(&mut self, data: &[u32]) -> BufferId {
        self.indices(data.len() * 4, |device, buffer| {
            device
                .write_buffer(buffer, 0, data)
                .expect("an index buffer write");
        })
    }

    fn indices(&mut self, size: usize, write: impl FnOnce(&mut Device, BufferId)) -> BufferId {
        let buffer = self
            .device
            .create_buffer(&BufferDesc {
                label: "indices",
                kind: BufferKind::Index,
                usage: BufferUsage::Static,
                size,
            })
            .expect("an index buffer");
        write(&mut self.device, buffer);
        buffer
    }

    /// Clears a target to `color`, in a frame of its own.
    pub(crate) fn clear(&mut self, target: RenderTargetId, color: [f32; 4]) {
        self.begin();
        let [r, g, b, a] = color;
        let _pass = self
            .device
            .begin_pass(target.into(), Some(LinearColor::new(r, g, b, a)))
            .expect("a pass that clears");
        self.end();
    }

    pub(crate) fn read(&mut self, target: RenderTargetId) -> Vec<[u8; 4]> {
        let pixels = self
            .device
            .read_pixels(target)
            .expect("the target's pixels");
        pixels.as_chunks::<4>().0.to_vec()
    }

    /// Draws `quad` (a triangle strip, two floats a corner) in `color` into
    /// `target`, with the [`color_pipeline`] `pipeline`, in a frame of its
    /// own.
    pub(crate) fn fill(
        &mut self,
        target: RenderTargetId,
        pipeline: PipelineId,
        quad: BufferId,
        color: [f32; 4],
    ) {
        self.begin();
        {
            let mut pass = self.device.begin_pass(target.into(), None).expect("a pass");
            pass.set_pipeline(pipeline).expect("the pipeline");
            pass.set_vertex_buffer(0, quad, 0).expect("the quad");
            pass.set_uniforms(0, &color).expect("the color");
            pass.draw(0..4, 0..1).expect("a draw");
        }
        self.end();
    }
}

/// The four corners of a rectangle of clip space, as a triangle strip.
pub(crate) fn strip(x0: f32, y0: f32, x1: f32, y1: f32) -> [f32; 8] {
    [x0, y0, x1, y0, x0, y1, x1, y1]
}

/// The whole of clip space, as a triangle strip.
pub(crate) const FULL: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];

pub(crate) fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> IRect {
    IRect::new(IVec2::new(x0, y0), IVec2::new(x1, y1))
}

/// Whether two pixels are within `tolerance` in every channel.
pub(crate) fn near(found: [u8; 4], expected: [u8; 4], tolerance: u8) -> bool {
    found
        .iter()
        .zip(expected)
        .all(|(&found, expected)| found.abs_diff(expected) <= tolerance)
}

// ---------------------------------------------------------------------------
// Shaders
// ---------------------------------------------------------------------------

/// Positions in clip space, one `vec2` a vertex.
pub(crate) const POSITION_VS: &str = "#version 330 core
layout(location = 0) in vec2 position;
void main() {
    gl_Position = vec4(position, 0.0, 1.0);
}
";

/// One color for everything, from uniform block `Color` (binding 0).
pub(crate) const COLOR_FS: &str = "#version 330 core
layout(std140) uniform Color {
    vec4 color;
};
out vec4 out_color;
void main() {
    out_color = color;
}
";

pub(crate) fn shaders<'a>(vs: &'a str, fs: &'a str) -> ShaderSource<'a> {
    ShaderSource {
        glsl_vs: vs,
        glsl_fs: fs,
        msl: "",
    }
}

pub(crate) const POSITION: [VertexAttribute; 1] = [VertexAttribute {
    location: 0,
    format: VertexFormat::Float32x2,
    offset: 0,
}];

pub(crate) const POSITIONS: [VertexLayout<'static>; 1] = [VertexLayout {
    stride: 8,
    step: VertexStep::Vertex,
    attributes: &POSITION,
}];

pub(crate) const COLOR_BLOCK: [UniformBlockDesc<'static>; 1] = [UniformBlockDesc {
    name: "Color",
    binding: 0,
    size: 16,
}];

/// Draws positions in one color from binding 0.
pub(crate) fn color_pipeline(gpu: &mut Gpu, format: TextureFormat, blend: BlendMode) -> PipelineId {
    gpu.device
        .create_pipeline(&PipelineDesc {
            label: "color",
            shader: shaders(POSITION_VS, COLOR_FS),
            vertex_layouts: &POSITIONS,
            uniform_blocks: &COLOR_BLOCK,
            textures: &[],
            blend,
            topology: Topology::TriangleStrip,
            target_format: format,
        })
        .expect("a pipeline")
}

/// Positions and texture coordinates.
pub(crate) const TEXTURED_VS: &str = "#version 330 core
layout(location = 0) in vec2 position;
layout(location = 1) in vec2 uv;
out vec2 v_uv;
void main() {
    v_uv = uv;
    gl_Position = vec4(position, 0.0, 1.0);
}
";

/// The texture of slot 0.
pub(crate) const TEXTURED_FS: &str = "#version 330 core
uniform sampler2D tex;
in vec2 v_uv;
out vec4 out_color;
void main() {
    out_color = texture(tex, v_uv);
}
";

pub(crate) const POSITION_UV: [VertexAttribute; 2] = [
    VertexAttribute {
        location: 0,
        format: VertexFormat::Float32x2,
        offset: 0,
    },
    VertexAttribute {
        location: 1,
        format: VertexFormat::Float32x2,
        offset: 8,
    },
];

pub(crate) const POSITIONS_UVS: [VertexLayout<'static>; 1] = [VertexLayout {
    stride: 16,
    step: VertexStep::Vertex,
    attributes: &POSITION_UV,
}];

/// Draws a texture from slot 0, with positions and texture coordinates.
pub(crate) fn textured_pipeline(gpu: &mut Gpu, format: TextureFormat) -> PipelineId {
    gpu.device
        .create_pipeline(&PipelineDesc {
            label: "textured",
            shader: shaders(TEXTURED_VS, TEXTURED_FS),
            vertex_layouts: &POSITIONS_UVS,
            uniform_blocks: &[],
            textures: &["tex"],
            blend: BlendMode::Opaque,
            topology: Topology::TriangleStrip,
            target_format: format,
        })
        .expect("a pipeline")
}

/// A quad over all of clip space, with texture coordinates from `u0` to
/// `u1` across and from `v0` (bottom) to `v1` (top).
pub(crate) fn textured_quad(gpu: &mut Gpu, [u0, u1]: [f32; 2], [v0, v1]: [f32; 2]) -> BufferId {
    gpu.vertices(&[
        -1.0, -1.0, u0, v0, //
        1.0, -1.0, u1, v0, //
        -1.0, 1.0, u0, v1, //
        1.0, 1.0, u1, v1,
    ])
}

/// Each instance a rectangle of clip space (`rect`) in its own color.
pub(crate) const INSTANCED_VS: &str = "#version 330 core
layout(location = 0) in vec2 corner;
layout(location = 1) in vec4 rect;
layout(location = 2) in vec4 color;
out vec4 v_color;
void main() {
    v_color = color;
    gl_Position = vec4(mix(rect.xy, rect.zw, corner), 0.0, 1.0);
}
";

pub(crate) const INSTANCED_FS: &str = "#version 330 core
in vec4 v_color;
out vec4 out_color;
void main() {
    out_color = v_color;
}
";

pub(crate) const CORNER: [VertexAttribute; 1] = [VertexAttribute {
    location: 0,
    format: VertexFormat::Float32x2,
    offset: 0,
}];

pub(crate) const INSTANCE: [VertexAttribute; 2] = [
    VertexAttribute {
        location: 1,
        format: VertexFormat::Float32x4,
        offset: 0,
    },
    VertexAttribute {
        location: 2,
        format: VertexFormat::Float32x4,
        offset: 16,
    },
];

pub(crate) const INSTANCED: [VertexLayout<'static>; 2] = [
    VertexLayout {
        stride: 8,
        step: VertexStep::Vertex,
        attributes: &CORNER,
    },
    VertexLayout {
        stride: 32,
        step: VertexStep::Instance,
        attributes: &INSTANCE,
    },
];

pub(crate) fn instanced_pipeline(gpu: &mut Gpu) -> PipelineId {
    gpu.device
        .create_pipeline(&PipelineDesc {
            label: "instanced",
            shader: shaders(INSTANCED_VS, INSTANCED_FS),
            vertex_layouts: &INSTANCED,
            uniform_blocks: &[],
            textures: &[],
            blend: BlendMode::Opaque,
            topology: Topology::TriangleStrip,
            target_format: TextureFormat::Rgba8,
        })
        .expect("a pipeline")
}

/// A unit square's corners, from 0 to 1, as a triangle strip.
pub(crate) const UNIT: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];

/// Whether an index format is used, for tests that try both.
pub(crate) const INDEX_FORMATS: [IndexFormat; 2] = [IndexFormat::U16, IndexFormat::U32];

/// The value nearest to `linear` of an 8-bit sRGB channel.
pub(crate) fn srgb8(linear: f32) -> u8 {
    (nadir_core::color::linear_to_srgb(linear) * 255.0).round() as u8
}

/// The linear value of an 8-bit sRGB channel.
pub(crate) fn linear(srgb: u8) -> f32 {
    nadir_core::color::srgb_to_linear(f32::from(srgb) / 255.0)
}
