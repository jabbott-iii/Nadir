//! Textures loaded through `nadir-assets` onto a GPU, on an offscreen OpenGL
//! context from `Device::new_headless`.
//!
//! They need the `headless-test` feature and a working driver:
//!
//! ```text
//! LIBGL_ALWAYS_SOFTWARE=1 NADIR_EXPECT_GL_RENDERER=llvmpipe \
//!     cargo test -p nadir-render --features headless-test
//! ```
//!
//! Where libglvnd also has NVIDIA's EGL, add
//! `__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use nadir_assets::{AssetError, AssetServer, FinalizeCtx, LoadState, OnFailure};
use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
use nadir_core::UVec2;
use nadir_gfx::{
    BlendMode, BufferDesc, BufferKind, BufferUsage, Device, GfxError, PipelineDesc, Sampler,
    ShaderSource, TextureFormat, Topology, VertexAttribute, VertexFormat, VertexLayout, VertexStep,
};
use nadir_render::Texture;

/// Names a part of the renderer, such as `llvmpipe`, that the context must
/// have. Unset or empty, any renderer will do.
const EXPECT_RENDERER: &str = "NADIR_EXPECT_GL_RENDERER";

/// A device on an offscreen context, or `None` where this system has none
/// yet and no renderer is asked for. Any other failure fails the test.
fn device() -> Option<Device> {
    let expected = env::var(EXPECT_RENDERER).unwrap_or_default();
    match Device::new_headless() {
        Ok(device) => {
            let renderer = &device.caps().renderer;
            assert!(
                renderer.contains(&expected),
                "the renderer is `{renderer}`, not `{expected}`"
            );
            Some(device)
        }
        Err(GfxError::NoOffscreenContext) if expected.is_empty() => None,
        Err(error) => panic!(
            "no headless device: {error}. With libglvnd, Mesa can be chosen with \
             __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json"
        ),
    }
}

/// A fresh, empty asset root for one test, under `target/tmp`.
fn root(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("nadir-render-headless")
        .join(test);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("remove the previous run's directory");
    }
    fs::create_dir_all(dir.join("sprites")).expect("create the test directory");
    dir
}

/// Writes a QOI file of `width`×`height` texels, each `texel`.
fn write_qoi(path: &Path, width: u32, height: u32, color_space: ColorSpace, texel: [u8; 4]) {
    let header = Header {
        width,
        height,
        channels: Channels::Rgba,
        color_space,
    };
    let pixels = texel.repeat((width * height) as usize);
    fs::write(path, qoi::encode(&header, &pixels).expect("an image")).expect("write the file");
}

fn finish(server: &mut AssetServer, device: &mut Device) {
    server.finish_completed_loads(&mut FinalizeCtx::new().with(device));
}

#[test]
fn a_texture_goes_to_the_gpu_once() {
    let Some(mut device) = device() else { return };
    let root = root("loaded");
    write_qoi(
        &root.join("sprites/hero.qoi"),
        3,
        2,
        ColorSpace::Srgb,
        [10, 20, 30, 255],
    );
    write_qoi(
        &root.join("sprites/mask.qoi"),
        1,
        1,
        ColorSpace::Linear,
        [0, 0, 0, 0],
    );
    // Four corners, each its own color, so that the drawing shows the
    // texels and which way up they are.
    let corners = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
    ];
    let header = Header {
        width: 2,
        height: 2,
        channels: Channels::Rgba,
        color_space: ColorSpace::Srgb,
    };
    let bytes = qoi::encode(&header, corners.as_flattened()).expect("an image");
    fs::write(root.join("sprites/corners.qoi"), bytes).expect("write the file");
    let mut server = AssetServer::new(root, OnFailure::Placeholder);

    let hero = server.load::<Texture>("sprites/hero.qoi");
    assert_eq!(server.load::<Texture>("sprites/hero.qoi"), hero);
    let mask = server.load::<Texture>("sprites/mask.qoi");
    finish(&mut server, &mut device);
    assert_eq!(server.load::<Texture>("sprites/hero.qoi"), hero);

    let texture = server.get(hero).expect("a loaded texture");
    assert_eq!(server.state(hero), Some(LoadState::Loaded));
    assert_eq!(texture.size(), UVec2::new(3, 2));
    assert_eq!(texture.format(), TextureFormat::Rgba8Srgb);
    let mask = server.get(mask).expect("a loaded texture");
    assert_eq!(mask.format(), TextureFormat::Rgba8);
    assert_ne!(mask.id(), texture.id());
    let corners_handle = server.load::<Texture>("sprites/corners.qoi");
    finish(&mut server, &mut device);
    let loaded = *server.get(corners_handle).expect("a loaded texture");
    assert_eq!(draw(&mut device, &loaded), corners, "first row on top");
    assert_eq!(
        server.assets::<Texture>().map(|assets| assets.len()),
        Some(3)
    );
}

#[test]
fn a_missing_texture_shows_the_placeholder() {
    let Some(mut device) = device() else { return };
    let mut server = AssetServer::new(root("missing"), OnFailure::Placeholder);
    let missing = server.load::<Texture>("sprites/missing.qoi");
    finish(&mut server, &mut device);
    assert_eq!(server.state(missing), Some(LoadState::Failed));
    assert_eq!(server.error(missing), Some(&AssetError::NotFound));
    let placeholder = *server.get(missing).expect("the placeholder");
    assert_eq!(placeholder.size(), UVec2::new(8, 8));

    // Drawn texel for texel, it is a checker of magenta and black.
    let texels = draw(&mut device, &placeholder);
    let magenta = [255, 0, 255, 255];
    let black = [0, 0, 0, 255];
    for (i, texel) in texels.iter().enumerate() {
        let (x, y) = (i % 8, i / 8);
        let expected = if (x + y) % 2 == 0 { magenta } else { black };
        assert_eq!(*texel, expected, "texel ({x}, {y})");
    }
}

#[test]
fn without_placeholders_a_missing_texture_is_nothing() {
    let Some(mut device) = device() else { return };
    let mut server = AssetServer::new(root("nothing"), OnFailure::Nothing);
    let missing = server.load::<Texture>("sprites/missing.qoi");
    finish(&mut server, &mut device);
    assert_eq!(server.state(missing), Some(LoadState::Failed));
    assert_eq!(server.get(missing), None);
}

const VS: &str = "#version 330 core
layout(location = 0) in vec2 position;
layout(location = 1) in vec2 uv;
out vec2 v_uv;
void main() {
    v_uv = uv;
    gl_Position = vec4(position, 0.0, 1.0);
}
";

const FS: &str = "#version 330 core
uniform sampler2D tex;
in vec2 v_uv;
out vec4 out_color;
void main() {
    out_color = texture(tex, v_uv);
}
";

/// Draws `texture` over a render target of its size, nearest-sampled, and
/// reads the target back from the top row.
fn draw(device: &mut Device, texture: &Texture) -> Vec<[u8; 4]> {
    let attributes = [
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
    let pipeline = device
        .create_pipeline(&PipelineDesc {
            label: "textured",
            shader: ShaderSource {
                glsl_vs: VS,
                glsl_fs: FS,
                msl: "",
            },
            vertex_layouts: &[VertexLayout {
                stride: 16,
                step: VertexStep::Vertex,
                attributes: &attributes,
            }],
            uniform_blocks: &[],
            textures: &["tex"],
            blend: BlendMode::Opaque,
            topology: Topology::TriangleStrip,
            target_format: TextureFormat::Rgba8Srgb,
        })
        .expect("a pipeline");
    // The top of the target is clip-space y = 1, and the texture's first
    // row is v = 0.
    let corners: [f32; 16] = [
        -1.0, -1.0, 0.0, 1.0, //
        1.0, -1.0, 1.0, 1.0, //
        -1.0, 1.0, 0.0, 0.0, //
        1.0, 1.0, 1.0, 0.0,
    ];
    let quad = device
        .create_buffer(&BufferDesc {
            label: "quad",
            kind: BufferKind::Vertex,
            usage: BufferUsage::Static,
            size: size_of_val(&corners),
        })
        .expect("a vertex buffer");
    device.write_buffer(quad, 0, &corners).expect("the corners");
    let target = device
        .create_render_target(texture.size(), TextureFormat::Rgba8Srgb)
        .expect("a render target");

    device.begin_offscreen_frame().expect("a frame");
    {
        let mut pass = device.begin_pass(target.into(), None).expect("a pass");
        pass.set_pipeline(pipeline).expect("the pipeline");
        pass.set_vertex_buffer(0, quad, 0).expect("the corners");
        pass.set_texture(0, texture.id(), Sampler::NearestClamp)
            .expect("the texture");
        pass.draw(0..4, 0..1).expect("a draw");
    }
    device.end_offscreen_frame().expect("the end of the frame");
    let pixels = device.read_pixels(target).expect("the target's pixels");
    pixels.as_chunks::<4>().0.to_vec()
}
