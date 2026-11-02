# nadir-gfx

A small render hardware interface (RHI) over OpenGL 3.3 core: textures,
buffers, render targets, pipelines, and passes whose draws are checked
before they reach the GPU.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Games do not use it directly: the engine,
[nadir_engine](https://crates.io/crates/nadir_engine), makes the device for
its window, and [nadir-render](https://crates.io/crates/nadir-render) draws
sprites and tiles with it. Depend on it to write a renderer of your own.

## Status

Version 0.1: the API will change before 1.0.

Built:

- one backend, OpenGL 3.3 core, which draws on Linux today;
- 2D textures (`Rgba8`, `Rgba8Srgb`, `R8`), vertex and index buffers, color
  render targets and pipelines, each named by a generational id;
- passes with viewport and scissor, instanced and indexed draws, uniform
  blocks of up to 256 bytes, five blend modes, and four samplers;
- checks on every draw: a vertex, instance or index read outside its
  buffer, a uniform block or texture not set, or a resource that was
  destroyed is an error, not a draw;
- an offscreen device for tests and tools, and readback of render targets.

Not yet:

- the OpenGL contexts of Windows (roadmap M2.6) and macOS (M2.7);
- the Metal backend (M10.2);
- GPU timer queries (M7.3).

## Usage

```toml
[dependencies]
nadir-gfx = "0.1"
nadir-core = "0.1"
```

`nadir-core` has the math and color types the API takes. A window comes
from [nadir-platform](https://crates.io/crates/nadir-platform), whose
`Platform::gpu_surface` is what `Device::new` takes.

This draws a triangle into a render target on an offscreen device, and
reads it back:

```rust,no_run
use nadir_core::{LinearColor, UVec2};
use nadir_gfx::{
    BlendMode, BufferDesc, BufferKind, BufferUsage, Device, GfxError, PipelineDesc,
    ShaderSource, TextureFormat, Topology, UniformBlockDesc, VertexAttribute, VertexFormat,
    VertexLayout, VertexStep,
};

const VERTEX: &str = "#version 330 core
layout(location = 0) in vec2 position;
void main() {
    gl_Position = vec4(position, 0.0, 1.0);
}
";

const FRAGMENT: &str = "#version 330 core
layout(std140) uniform Color {
    vec4 color;
};
out vec4 out_color;
void main() {
    out_color = color;
}
";

fn main() -> Result<(), GfxError> {
    let mut device = Device::new_headless()?;
    let target = device.create_render_target(UVec2::new(64, 64), TextureFormat::Rgba8Srgb)?;

    // Three corners in clip space, two floats each.
    let triangle: [f32; 6] = [-0.5, -0.5, 0.5, -0.5, 0.0, 0.5];
    let vertices = device.create_buffer(&BufferDesc {
        label: "triangle",
        kind: BufferKind::Vertex,
        usage: BufferUsage::Static,
        size: size_of_val(&triangle),
    })?;
    device.write_buffer(vertices, 0, &triangle)?;

    let pipeline = device.create_pipeline(&PipelineDesc {
        label: "flat",
        shader: ShaderSource {
            glsl_vs: VERTEX,
            glsl_fs: FRAGMENT,
            msl: "",
        },
        vertex_layouts: &[VertexLayout {
            stride: 8,
            step: VertexStep::Vertex,
            attributes: &[VertexAttribute {
                location: 0,
                format: VertexFormat::Float32x2,
                offset: 0,
            }],
        }],
        uniform_blocks: &[UniformBlockDesc {
            name: "Color",
            binding: 0,
            size: 16,
        }],
        textures: &[],
        blend: BlendMode::Opaque,
        topology: Topology::Triangles,
        target_format: TextureFormat::Rgba8Srgb,
    })?;

    device.begin_offscreen_frame()?;
    {
        let mut pass = device.begin_pass(target.into(), Some(LinearColor::BLACK))?;
        pass.set_pipeline(pipeline)?;
        pass.set_vertex_buffer(0, vertices, 0)?;
        pass.set_uniforms(0, &[1.0f32, 0.5, 0.0, 1.0])?;
        pass.draw(0..3, 0..1)?;
    } // Dropping the pass ends it.
    device.end_offscreen_frame()?;

    // RGBA8, from the top row.
    let pixels = device.read_pixels(target)?;
    assert_eq!(pixels.len(), 64 * 64 * 4);
    Ok(())
}
```

## Features

`headless-test` builds the crate's own GPU tests, for the workspace's CI.
A user does not need it.

## Platforms

The code is the same on every system, and builds on Linux, Windows and
macOS. What it can draw on depends on the OpenGL context that
`nadir-platform` makes:

- **Linux (X11):** a window's context and an offscreen one (EGL). Everything
  works.
- **Windows and macOS:** no OpenGL context yet. `Device::new` on a window's
  surface fails with `GfxError::MissingFunction`, and `Device::new_headless`
  with `GfxError::NoOffscreenContext`.

## Documentation

- API reference: https://docs.rs/nadir-gfx
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/gfx.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
