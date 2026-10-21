//! What the tests share: a headless device, asset roots and images.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use nadir_assets::{AssetServer, FinalizeCtx, Handle, OnFailure};
use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
use nadir_core::UVec2;
use nadir_gfx::{Device, GfxError, RenderTargetId, TextureFormat};
use nadir_render::{FrameStats, Output, RenderConfig, RenderFrame, Renderer, Texture};

/// Names a part of the renderer, such as `llvmpipe`, that the context must
/// have. Unset or empty, any renderer will do.
pub(crate) const EXPECT_RENDERER: &str = "NADIR_EXPECT_GL_RENDERER";

/// A device on an offscreen context, or `None` where this system has none
/// yet and no renderer is asked for. Any other failure fails the test.
pub(crate) fn device() -> Option<Device> {
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
pub(crate) fn root(test: &str) -> PathBuf {
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
pub(crate) fn write_qoi(
    path: &Path,
    width: u32,
    height: u32,
    color_space: ColorSpace,
    texel: [u8; 4],
) {
    let header = Header {
        width,
        height,
        channels: Channels::Rgba,
        color_space,
    };
    let pixels = texel.repeat((width * height) as usize);
    fs::write(path, qoi::encode(&header, &pixels).expect("an image")).expect("write the file");
}

pub(crate) fn finish(server: &mut AssetServer, device: &mut Device) {
    server.finish_completed_loads(&mut FinalizeCtx::new().with(device));
}

/// Writes an sRGB QOI file of `width`×`height` texels, from the top row.
pub(crate) fn write_image(path: &Path, width: u32, height: u32, texels: &[[u8; 4]]) {
    assert_eq!(
        texels.len(),
        (width * height) as usize,
        "{}",
        path.display()
    );
    let header = Header {
        width,
        height,
        channels: Channels::Rgba,
        color_space: ColorSpace::Srgb,
    };
    let bytes = qoi::encode(&header, texels.as_flattened()).expect("an image");
    fs::write(path, bytes).expect("write the file");
}

/// A render target's pixels, from the top row.
pub(crate) fn read(device: &mut Device, target: RenderTargetId) -> Vec<[u8; 4]> {
    let pixels = device.read_pixels(target).expect("the target's pixels");
    pixels.as_chunks::<4>().0.to_vec()
}

/// A device, an asset server over a fresh root, and a renderer.
pub(crate) struct Stage {
    // The renderer and the textures are the device's, so it goes last.
    pub(crate) renderer: Renderer,
    pub(crate) server: AssetServer,
    root: PathBuf,
    pub(crate) device: Device,
}

impl Stage {
    /// A stage for the test `test`, with the virtual resolution
    /// `virtual_size`, or `None` where this system has no offscreen context.
    pub(crate) fn new(test: &str, virtual_size: UVec2) -> Option<Self> {
        let mut device = device()?;
        let renderer = Renderer::new(
            &mut device,
            &RenderConfig {
                virtual_resolution: virtual_size,
            },
        )
        .expect("a renderer");
        let root = root(test);
        Some(Self {
            renderer,
            server: AssetServer::new(&root, OnFailure::Placeholder),
            root,
            device,
        })
    }

    /// Writes `texels` as `name` and loads it.
    pub(crate) fn image(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        texels: &[[u8; 4]],
    ) -> Handle<Texture> {
        write_image(&self.root.join(name), width, height, texels);
        self.server.load(name)
    }

    /// Loads `name`, which does not exist.
    pub(crate) fn missing(&mut self, name: &str) -> Handle<Texture> {
        self.server.load(name)
    }

    /// Finishes the loads.
    pub(crate) fn finish(&mut self) {
        finish(&mut self.server, &mut self.device);
    }

    /// Renders `frame` into a render target of `size`, and reads it back.
    pub(crate) fn draw(&mut self, frame: &RenderFrame, size: UVec2) -> (FrameStats, Vec<[u8; 4]>) {
        let target = self
            .device
            .create_render_target(size, TextureFormat::Rgba8Srgb)
            .expect("an output");
        self.device.begin_offscreen_frame().expect("a frame");
        let stats = self
            .renderer
            .render(
                &mut self.device,
                &self.server,
                frame,
                Output {
                    target: target.into(),
                    size,
                },
            )
            .expect("a rendered frame");
        self.device
            .end_offscreen_frame()
            .expect("the end of the frame");
        let pixels = read(&mut self.device, target);
        self.device.destroy(target);
        (stats, pixels)
    }
}

/// `n` texels of `texel`.
pub(crate) fn solid(n: usize, texel: [u8; 4]) -> Vec<[u8; 4]> {
    vec![texel; n]
}

/// Whether two pixels are within `tolerance` in every channel.
pub(crate) fn near(a: [u8; 4], b: [u8; 4], tolerance: u8) -> bool {
    a.iter().zip(b).all(|(&a, b)| a.abs_diff(b) <= tolerance)
}
