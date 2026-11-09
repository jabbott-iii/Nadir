// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! What the tests share: a headless device, asset roots and images.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use nadir_assets::{AssetServer, FinalizeCtx, Handle, OnFailure, Scope};
use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
use nadir_codec::{Limits, json, kbj};
use nadir_core::UVec2;
use nadir_gfx::{Device, GfxError, RenderTargetId, TextureFormat};
use nadir_render::{
    Font, FrameStats, Output, RenderConfig, RenderFrame, Renderer, Texture, TileLayers,
};

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

/// Waits for every load, and makes the textures on `device`.
pub(crate) fn finish(server: &mut AssetServer, device: &mut Device) {
    server.finish_all_loads(&mut FinalizeCtx::new().with(device));
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

/// A device, an asset server over a fresh root, a renderer, and the tile
/// layers it draws from.
pub(crate) struct Stage {
    // The renderer and the textures are the device's, so it goes last.
    pub(crate) renderer: Renderer,
    pub(crate) server: AssetServer,
    pub(crate) tiles: TileLayers,
    pub(crate) root: PathBuf,
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
            tiles: TileLayers::new(),
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
                &self.tiles,
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

/// A glyph of a test font: its character, its pixels by row (`#` white,
/// `o` black, `.` clear, all of a row's width), where its rectangle goes
/// from the pen, and its advance.
pub(crate) struct Art {
    pub(crate) id: char,
    pub(crate) rows: &'static [&'static str],
    pub(crate) offset: (i32, i32),
    pub(crate) advance: i32,
}

impl Stage {
    /// Writes the font [`Stage::write_font`] writes, then loads it, its
    /// atlas and its page with [`Font::load_now_in`]; the page is made by
    /// the next finish.
    pub(crate) fn font(
        &mut self,
        name: &str,
        line_height: u32,
        glyphs: &[Art],
        fallback: char,
        kerning: &[(char, char, i32)],
    ) -> Handle<Font> {
        let path = self.write_font(name, line_height, glyphs, fallback, kerning);
        Font::load_now_in(
            &mut self.server,
            &path,
            Scope::Global,
            &mut FinalizeCtx::new(),
        )
        .expect("the font and its atlas")
    }

    /// Writes a font named `name` of `glyphs`, as the cook would: its page,
    /// `atlas/<name>_0.qoi`, with each glyph's pixels trimmed to a frame of
    /// the sprite `fonts/<name>` of the atlas file `atlas/<name>.kbj`, and
    /// its font file, `atlas/fonts/<name>.font.kbj`, whose fallback is the
    /// glyph of `fallback`. Returns the font file's path.
    pub(crate) fn write_font(
        &mut self,
        name: &str,
        line_height: u32,
        glyphs: &[Art],
        fallback: char,
        kerning: &[(char, char, i32)],
    ) -> String {
        const WHITE: [u8; 4] = [255, 255, 255, 255];
        const BLACK: [u8; 4] = [0, 0, 0, 255];
        let width: usize = glyphs
            .iter()
            .map(|art| art.rows.first().map_or(0, |row| row.len()) + 1)
            .sum();
        let height = glyphs
            .iter()
            .map(|art| art.rows.len())
            .max()
            .unwrap_or(0)
            .max(1);
        let mut page = vec![[0u8; 4]; width.max(1) * height];
        let mut frames = Vec::new();
        let mut entries = Vec::new();
        let mut fallback_entry = None;
        let mut glyphs: Vec<&Art> = glyphs.iter().collect();
        glyphs.sort_by_key(|art| art.id);
        let mut left = 0;
        for art in glyphs {
            let w = art.rows.first().map_or(0, |row| row.len());
            let mut ink: Option<(usize, usize, usize, usize)> = None;
            for (y, row) in art.rows.iter().enumerate() {
                assert_eq!(row.len(), w, "{:?}: rows of one width", art.id);
                for (x, pixel) in row.bytes().enumerate() {
                    let texel = match pixel {
                        b'#' => WHITE,
                        b'o' => BLACK,
                        _ => continue,
                    };
                    page[y * width + left + x] = texel;
                    ink = Some(match ink {
                        None => (x, y, x, y),
                        Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                    });
                }
            }
            let frame = ink.map(|(x0, y0, x1, y1)| {
                frames.push(format!(
                    r#"{{ "page": 0, "x": {}, "y": {y0}, "w": {}, "h": {}, "ox": {x0}, "oy": {y0}, "sw": {w}, "sh": {} }}"#,
                    left + x0,
                    x1 - x0 + 1,
                    y1 - y0 + 1,
                    art.rows.len()
                ));
                frames.len() - 1
            });
            let (x, y) = art.offset;
            let frame_member =
                frame.map_or(String::new(), |frame| format!(r#""frame": {frame}, "#));
            entries.push(format!(
                r#"{{ "id": {}, {frame_member}"x": {x}, "y": {y}, "advance": {} }}"#,
                art.id as u32, art.advance
            ));
            if art.id == fallback {
                let frame = frame.expect("the fallback has pixels");
                fallback_entry = Some(format!(
                    r#"{{ "frame": {frame}, "x": {x}, "y": {y}, "advance": {} }}"#,
                    art.advance
                ));
            }
            left += w + 1;
        }
        let mut kerning: Vec<(u32, u32, i32)> = kerning
            .iter()
            .map(|&(a, b, amount)| (a as u32, b as u32, amount))
            .collect();
        kerning.sort();
        let kerning: Vec<String> = kerning
            .iter()
            .map(|(a, b, amount)| format!("[{a}, {b}, {amount}]"))
            .collect();

        let atlas = self.root.join("atlas");
        fs::create_dir_all(atlas.join("fonts")).expect("the atlas folder");
        write_image(
            &atlas.join(format!("{name}_0.qoi")),
            width.max(1) as u32,
            height as u32,
            &page,
        );
        let atlas_file = format!(
            r#"{{ "version": 2,
                "pages": [ {{ "path": "atlas/{name}_0.qoi", "w": {}, "h": {height} }} ],
                "sprites": [ {{ "name": "fonts/{name}", "frames": [ {} ], "pivot": [0.0, 0.0] }} ] }}"#,
            width.max(1),
            frames.join(", ")
        );
        write_kbj(&atlas.join(format!("{name}.kbj")), &atlas_file);
        let font_file = format!(
            r#"{{ "version": 1, "face": "{name}", "size": {line_height}, "line_height": {line_height},
                "base": {line_height}, "atlas": "atlas/{name}.kbj", "sprite": "fonts/{name}",
                "glyphs": [ {} ], "kerning": [ {} ], "fallback": {} }}"#,
            entries.join(", "),
            kerning.join(", "),
            fallback_entry.expect("the fallback among the glyphs")
        );
        let path = format!("atlas/fonts/{name}.font.kbj");
        write_kbj(&self.root.join(&path), &font_file);
        path
    }
}

/// Writes the KBJ file that `text`, JSON, describes.
pub(crate) fn write_kbj(path: &Path, text: &str) {
    let value = json::parse(text, Limits::default()).expect("the test's JSON");
    let bytes = kbj::encode(&value, Limits::default()).expect("encodable");
    fs::write(path, bytes).expect("write the file");
}

/// `n` texels of `texel`.
pub(crate) fn solid(n: usize, texel: [u8; 4]) -> Vec<[u8; 4]> {
    vec![texel; n]
}

/// Whether two pixels are within `tolerance` in every channel.
pub(crate) fn near(a: [u8; 4], b: [u8; 4], tolerance: u8) -> bool {
    a.iter().zip(b).all(|(&a, b)| a.abs_diff(b) <= tolerance)
}
