//! The golden scenes, which `cargo xtask golden` compares with
//! `tests/golden/<scene>.qoi` (`docs/04-quality.md`, ADR-0017).
//!
//! Each scene draws into a 320×180 render target and checks a few of its
//! pixels. When `NADIR_GOLDEN_DIR` names a directory, it also writes all of
//! them there, as `<scene>.pam`, for xtask to compare. Without it the scenes
//! still draw, so that a scene that fails fails `cargo test` too.
//!
//! Colors are sRGB bytes, so that an sRGB target stores them exactly, and
//! alphas are whole bytes, so that no value lies half way between two:
//! vendors round those differently. Rectangles have whole-pixel corners, so
//! no edge passes through a pixel's center.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use nadir_core::{Color, LinearColor, UVec2};
use nadir_gfx::{
    BlendMode, BufferId, IndexFormat, Pass, PipelineDesc, PipelineId, RenderTargetId, Sampler,
    TextureDesc, TextureFormat, Topology,
};

use crate::common::{
    EXPECT_RENDERER, FULL, Gpu, POSITIONS_UVS, TEXTURED_FS, TEXTURED_VS, UNIT, color_pipeline,
    instanced_pipeline, linear, near, rect, shaders, srgb8, textured_pipeline,
};

/// The size of every scene.
const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;
const SIZE: UVec2 = UVec2::new(WIDTH, HEIGHT);

/// Names the directory the scenes write their pixels to.
const DIR_VAR: &str = "NADIR_GOLDEN_DIR";

/// Defines a scene: a test that makes a device, runs `$body` (which draws
/// into a `SIZE` render target, checks some of its pixels into `$checks`,
/// and returns it), and saves what it drew under the test's own name. Two
/// scenes cannot share a name, and none can forget to save.
///
/// A failed check fails the test only once the pixels are saved, so that
/// `cargo xtask golden` can still show how the scene differs.
macro_rules! scene {
    ($(#[$meta:meta])* fn $name:ident($gpu:ident, $checks:ident) $body:block) => {
        $(#[$meta])*
        #[test]
        fn $name() {
            let Some(mut gpu) = Gpu::new() else { return };
            let mut checks = Checks::default();
            let target = {
                let $gpu: &mut Gpu = &mut gpu;
                let $checks: &mut Checks = &mut checks;
                $body
            };
            save(stringify!($name), &mut gpu, target);
            check_renderer(&gpu, &mut checks);
            assert!(checks.0.is_empty(), "{}", checks.0.join("\n"));
        }
    };
}

/// The checks of a scene's pixels that failed.
#[derive(Default)]
struct Checks(Vec<String>);

impl Checks {
    /// Checks the pixel at `(x, y)` from the top left against `expected`,
    /// within `tolerance`.
    #[track_caller]
    fn pixel(&mut self, pixels: &[[u8; 4]], (x, y): (u32, u32), expected: [u8; 4], tolerance: u8) {
        let found = pixels[(y * WIDTH + x) as usize];
        if !near(found, expected, tolerance) {
            let line = std::panic::Location::caller().line();
            self.0.push(format!(
                "line {line}: ({x}, {y}) is {found:?}, not {expected:?}"
            ));
        }
    }

    /// Checks that `holds` holds.
    #[track_caller]
    fn that(&mut self, holds: bool, what: &str) {
        if !holds {
            let line = std::panic::Location::caller().line();
            self.0.push(format!("line {line}: {what}"));
        }
    }
}

/// Checks that the renderer is the one `NADIR_EXPECT_GL_RENDERER` asks
/// for, as `frames::the_caps_say_what_the_device_runs_on` does: the scenes
/// run on their own under `cargo xtask golden`. It runs after the scene is
/// saved, which names the renderer, so that xtask can say how to choose
/// another.
fn check_renderer(gpu: &Gpu, checks: &mut Checks) {
    let expected = env::var(EXPECT_RENDERER).unwrap_or_default();
    let renderer = &gpu.device.caps().renderer;
    checks.that(
        renderer.contains(&expected),
        &format!("the renderer is `{renderer}`, and {EXPECT_RENDERER} asks for `{expected}`"),
    );
}

/// Writes the scene's pixels as `<name>.pam` (Netpbm PAM, RGBA8, from the
/// top row) in the `NADIR_GOLDEN_DIR` directory, if it is set. The header
/// names the renderer in a `# renderer` comment.
fn save(name: &str, gpu: &mut Gpu, target: RenderTargetId) {
    let pixels = gpu.device.read_pixels(target).expect("the scene's pixels");
    assert_eq!(
        pixels.len(),
        WIDTH as usize * HEIGHT as usize * 4,
        "a scene is {WIDTH}×{HEIGHT}"
    );
    let Some(dir) = env::var_os(DIR_VAR).filter(|dir| !dir.is_empty()) else {
        return;
    };
    // The header is one line of text a field: keep the name to printable
    // ASCII.
    let renderer: String = gpu
        .device
        .caps()
        .renderer
        .chars()
        .map(|c| {
            if c == ' ' || c.is_ascii_graphic() {
                c
            } else {
                '?'
            }
        })
        .collect();
    let mut bytes = format!(
        "P7\nWIDTH {WIDTH}\nHEIGHT {HEIGHT}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\n\
         # renderer {renderer}\nENDHDR\n"
    )
    .into_bytes();
    bytes.extend_from_slice(&pixels);
    let dir = Path::new(&dir);
    fs::create_dir_all(dir).expect("the golden directory");
    let path = dir.join(format!("{name}.pam"));
    // A file that is there already is another scene's, or a stale one: xtask
    // empties the directory before each run.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("cannot create `{}`: {error}", path.display()));
    file.write_all(&bytes).expect("the scene's pixels written");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// An opaque color from sRGB bytes, as the shaders take it: linear.
fn srgb(r: u8, g: u8, b: u8) -> [f32; 4] {
    srgba(r, g, b, 255)
}

/// A color from sRGB bytes, and an alpha of `a / 255`.
fn srgba(r: u8, g: u8, b: u8, a: u8) -> [f32; 4] {
    Color::rgba(r, g, b, a).to_linear().to_array()
}

/// The clip-space corners of a rectangle of a `size` target, given in
/// pixels from the top left: `[left, top, right, bottom]`.
fn clip(size: UVec2, [x0, y0, x1, y1]: [u32; 4]) -> [f32; 4] {
    let x = |x: u32| 2.0 * x as f32 / size.x as f32 - 1.0;
    let y = |y: u32| 1.0 - 2.0 * y as f32 / size.y as f32;
    [x(x0), y(y0), x(x1), y(y1)]
}

/// A rectangle of a `size` target, in pixels from the top left, as a
/// triangle strip: top left, top right, bottom left, bottom right.
fn quad_in(size: UVec2, rect: [u32; 4]) -> [f32; 8] {
    let [left, top, right, bottom] = clip(size, rect);
    [left, top, right, top, left, bottom, right, bottom]
}

/// A rectangle as [`quad_in`] gives it, with texture coordinates `[u0, v0]`
/// at its top left and `[u1, v1]` at its bottom right.
fn textured_in(size: UVec2, rect: [u32; 4], [u0, v0, u1, v1]: [f32; 4]) -> [f32; 16] {
    let [left, top, right, bottom] = clip(size, rect);
    [
        left, top, u0, v0, //
        right, top, u1, v0, //
        left, bottom, u0, v1, //
        right, bottom, u1, v1,
    ]
}

/// The pixel at `(x, y)` from the top left of what `read` gave.
fn at(pixels: &[[u8; 4]], x: u32, y: u32) -> [u8; 4] {
    pixels[(y * WIDTH + x) as usize]
}

/// The sRGB bytes of a linear color, with its alpha as a byte.
fn encoded([r, g, b, a]: [f32; 4]) -> [u8; 4] {
    [srgb8(r), srgb8(g), srgb8(b), (a * 255.0).round() as u8]
}

/// A color as an sRGB target stores it, decoded again.
fn stored(color: [f32; 4]) -> [f32; 4] {
    let [r, g, b, a] = encoded(color);
    [linear(r), linear(g), linear(b), f32::from(a) / 255.0]
}

/// Blends `src` over `dst` (both linear, as the target holds them) as the
/// device's blend table says.
fn blend(mode: BlendMode, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    let [sr, sg, sb, sa] = src;
    let [dr, dg, db, da] = dst;
    let mix = |s: f32, d: f32, fs: f32, fd: f32| (s * fs + d * fd).min(1.0);
    match mode {
        BlendMode::Opaque => src,
        BlendMode::Alpha => [
            mix(sr, dr, sa, 1.0 - sa),
            mix(sg, dg, sa, 1.0 - sa),
            mix(sb, db, sa, 1.0 - sa),
            mix(sa, da, 1.0, 1.0 - sa),
        ],
        BlendMode::PremultipliedAlpha => [
            mix(sr, dr, 1.0, 1.0 - sa),
            mix(sg, dg, 1.0, 1.0 - sa),
            mix(sb, db, 1.0, 1.0 - sa),
            mix(sa, da, 1.0, 1.0 - sa),
        ],
        BlendMode::Additive => [
            mix(sr, dr, sa, 1.0),
            mix(sg, dg, sa, 1.0),
            mix(sb, db, sa, 1.0),
            da,
        ],
        BlendMode::Multiply => [sr * dr, sg * dg, sb * db, da],
    }
}

/// Draws quad `i` of `quads` (four vertices each) in `colors[i]`, with
/// `pipelines[i]`, a pipeline of [`color_pipeline`].
fn fill_quads(pass: &mut Pass<'_>, pipelines: &[PipelineId], quads: BufferId, colors: &[[f32; 4]]) {
    for (i, (&pipeline, color)) in pipelines.iter().zip(colors).enumerate() {
        pass.set_pipeline(pipeline).expect("a pipeline");
        pass.set_vertex_buffer(0, quads, 0).expect("the quads");
        pass.set_uniforms(0, color).expect("the color");
        let first = i as u32 * 4;
        pass.draw(first..first + 4, 0..1).expect("a quad");
    }
}

// ---------------------------------------------------------------------------
// The scenes
// ---------------------------------------------------------------------------

scene! {
    /// A clear, and nothing else: the clear color in an sRGB target, with an
    /// alpha that is not 1.
    fn clear(gpu, checks) {
        let target = gpu.target(WIDTH, HEIGHT, TextureFormat::Rgba8Srgb);
        gpu.clear(target, srgba(37, 99, 180, 200));
        let pixels = gpu.read(target);
        checks.that(
            pixels.iter().all(|&p| near(p, [37, 99, 180, 200], 1)),
            "every pixel is the clear color",
        );
        target
    }
}

/// The 16×16 sRGB texture of `textured_quad`: four quadrants, a white first
/// row and first column, so that a flip or a mirror shows.
fn quadrants() -> Vec<u8> {
    let mut texels = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            let texel = match (x, y) {
                (0, _) | (_, 0) => [255, 255, 255, 255],
                (..8, ..8) => [220, 40, 40, 255],
                (8.., ..8) => [40, 180, 60, 255],
                (..8, 8..) => [40, 80, 220, 255],
                (8.., 8..) => [230, 200, 40, 255],
            };
            texels.extend_from_slice(&texel);
        }
    }
    texels
}

scene! {
    /// Textures, three ways side by side:
    /// - a 16×16 sRGB texture, sampled nearest at 6×, with a 4×4 marker that
    ///   `update_texture` wrote
    /// - a 4×4 linear gradient, sampled linearly at 24×
    /// - the first again, repeated twice each way at 3×, as an indexed draw
    ///   of two triangles from a base vertex.
    fn textured_quad(gpu, checks) {
        let target = gpu.target(WIDTH, HEIGHT, TextureFormat::Rgba8Srgb);
        let pattern = gpu
            .device
            .create_texture(
                &TextureDesc {
                    label: "quadrants",
                    size: UVec2::new(16, 16),
                    format: TextureFormat::Rgba8Srgb,
                },
                Some(&quadrants()),
            )
            .expect("a texture");
        gpu.device
            .update_texture(pattern, rect(10, 2, 14, 6), &[20, 20, 20, 255].repeat(16))
            .expect("the marker");
        // Every channel that is filtered stays at 64 or above: below that,
        // an sRGB target magnifies the vendors' rounding of the filtered
        // value past the tolerance.
        let mut gradient = Vec::with_capacity(4 * 4 * 4);
        for j in 0..4u8 {
            for i in 0..4u8 {
                gradient.extend_from_slice(&[64 + 32 * i, 64 + 32 * j, 64 + 32 * (3 - i), 255]);
            }
        }
        let gradient = gpu
            .device
            .create_texture(
                &TextureDesc {
                    label: "gradient",
                    size: UVec2::new(4, 4),
                    format: TextureFormat::Rgba8,
                },
                Some(&gradient),
            )
            .expect("a texture");

        let textured = textured_pipeline(gpu, TextureFormat::Rgba8Srgb);
        let indexed = gpu
            .device
            .create_pipeline(&PipelineDesc {
                label: "textured triangles",
                shader: shaders(TEXTURED_VS, TEXTURED_FS),
                vertex_layouts: &POSITIONS_UVS,
                uniform_blocks: &[],
                textures: &["tex"],
                blend: BlendMode::Opaque,
                topology: Topology::Triangles,
                target_format: TextureFormat::Rgba8Srgb,
            })
            .expect("a pipeline");
        let quads: Vec<f32> = [
            textured_in(SIZE, [8, 42, 104, 138], [0.0, 0.0, 1.0, 1.0]),
            textured_in(SIZE, [112, 42, 208, 138], [0.0, 0.0, 1.0, 1.0]),
            textured_in(SIZE, [216, 42, 312, 138], [0.0, 0.0, 2.0, 2.0]),
        ]
        .concat();
        let quads = gpu.vertices(&quads);
        // The third quad's corners are vertices 8 to 11: 4 past the second
        // quad, which the indices name.
        let indices = gpu.indices_u16(&[4, 5, 6, 6, 5, 7]);

        gpu.begin();
        {
            let clear = Color::rgb(32, 32, 32).to_linear();
            let mut pass = gpu.device.begin_pass(target.into(), Some(clear)).expect("a pass");
            pass.set_pipeline(textured).expect("the pipeline");
            pass.set_vertex_buffer(0, quads, 0).expect("the quads");
            pass.set_texture(0, pattern, Sampler::NearestClamp).expect("the texture");
            pass.draw(0..4, 0..1).expect("the nearest quad");
            pass.set_texture(0, gradient, Sampler::LinearClamp).expect("the gradient");
            pass.draw(4..8, 0..1).expect("the linear quad");
            pass.set_pipeline(indexed).expect("the pipeline");
            pass.set_vertex_buffer(0, quads, 0).expect("the quads");
            pass.set_index_buffer(indices, IndexFormat::U16).expect("the indices");
            pass.set_texture(0, pattern, Sampler::NearestRepeat).expect("the texture");
            pass.draw_indexed(0..6, 4, 0..1).expect("the repeated quad");
        }
        gpu.end();

        let pixels = gpu.read(target);
        checks.pixel(&pixels, (2, 2), [32, 32, 32, 255], 1);
        // Nearest at 6×: texel (x, y) covers pixels 6x to 6x + 5 across.
        let texel = |x: u32, y: u32| (8 + 6 * x + 3, 42 + 6 * y + 3);
        checks.pixel(&pixels, texel(0, 0), [255, 255, 255, 255], 1);
        checks.pixel(&pixels, texel(4, 4), [220, 40, 40, 255], 1);
        checks.pixel(&pixels, texel(12, 6), [40, 180, 60, 255], 1);
        checks.pixel(&pixels, texel(4, 12), [40, 80, 220, 255], 1);
        checks.pixel(&pixels, texel(12, 12), [230, 200, 40, 255], 1);
        checks.pixel(&pixels, texel(11, 3), [20, 20, 20, 255], 1);
        // Repeated at 3×: texel (x, y) of the second copy across.
        let repeated = |x: u32, y: u32| (216 + 3 * (16 + x) + 1, 42 + 3 * y + 1);
        checks.pixel(&pixels, repeated(4, 4), [220, 40, 40, 255], 1);
        checks.pixel(&pixels, repeated(11, 3), [20, 20, 20, 255], 1);
        checks.pixel(&pixels, repeated(12, 12), [230, 200, 40, 255], 1);
        // The gradient rises to the right in red and falls in blue.
        let (left, right) = (at(&pixels, 113, 90), at(&pixels, 207, 90));
        checks.that(
            left[0] < right[0] && left[2] > right[2],
            &format!("the gradient goes from {left:?} to {right:?}"),
        );
        target
    }
}

scene! {
    /// Each blend mode, in its own column: two overlapping quads, over a
    /// translucent background and an opaque-ish band, so that the alpha
    /// factors show too.
    fn alpha_blend(gpu, checks) {
        let target = gpu.target(WIDTH, HEIGHT, TextureFormat::Rgba8Srgb);
        let format = TextureFormat::Rgba8Srgb;
        let modes = [
            BlendMode::Alpha,
            BlendMode::PremultipliedAlpha,
            BlendMode::Additive,
            BlendMode::Multiply,
        ];
        let background = srgba(230, 225, 210, 64);
        let band = srgba(40, 44, 60, 192);
        // The quads' alphas keep two blends' rounding from adding up: the
        // top quad covers most of the bottom one, or they use different
        // channels, or a channel passes through unchanged.
        let colors = |mode: BlendMode| -> [[f32; 4]; 2] {
            match mode {
                BlendMode::Alpha => [srgba(220, 40, 40, 96), srgba(40, 80, 220, 208)],
                BlendMode::PremultipliedAlpha => {
                    let premultiply = |[r, g, b, a]: [f32; 4]| [r * a, g * a, b * a, a];
                    [
                        premultiply(srgba(220, 40, 40, 96)),
                        premultiply(srgba(40, 80, 220, 208)),
                    ]
                }
                BlendMode::Additive => [srgba(200, 0, 0, 160), srgba(0, 0, 200, 96)],
                _ => [srgb(255, 160, 96), srgb(96, 255, 160)],
            }
        };
        let column = |g: u32| 8 + 78 * g;
        let mut quads = quad_in(SIZE, [0, 70, WIDTH, 110]).to_vec();
        for g in 0..4 {
            let x = column(g);
            quads.extend_from_slice(&quad_in(SIZE, [x, 20, x + 44, 140]));
            quads.extend_from_slice(&quad_in(SIZE, [x + 22, 40, x + 66, 160]));
        }
        let quads = gpu.vertices(&quads);
        let opaque = color_pipeline(gpu, format, BlendMode::Opaque);
        let mut pipelines = vec![opaque];
        let mut fills = vec![band];
        for mode in modes {
            let pipeline = color_pipeline(gpu, format, mode);
            pipelines.extend([pipeline, pipeline]);
            fills.extend(colors(mode));
        }

        gpu.begin();
        {
            let [r, g, b, a] = background;
            let clear = LinearColor::new(r, g, b, a);
            let mut pass = gpu.device.begin_pass(target.into(), Some(clear)).expect("a pass");
            fill_quads(&mut pass, &pipelines, quads, &fills);
        }
        gpu.end();

        // What each part of a column should hold, from the blend table, with
        // the target's own rounding between the two quads.
        let pixels = gpu.read(target);
        for (g, mode) in modes.into_iter().enumerate() {
            let x = column(g as u32);
            let [bottom, top] = colors(mode);
            // Over the background, then over the band: the first quad alone,
            // both, and the second alone.
            for (under, y) in [(stored(background), 50), (stored(band), 90)] {
                let first = blend(mode, bottom, under);
                checks.pixel(&pixels, (x + 10, y), encoded(first), 2);
                let both = blend(mode, top, stored(first));
                checks.pixel(&pixels, (x + 30, y), encoded(both), 2);
                checks.pixel(&pixels, (x + 55, y), encoded(blend(mode, top, under)), 2);
            }
        }
        target
    }
}

scene! {
    /// 1000 instanced quads, a 40×25 grid of 6×5 quads, each in its own
    /// color, in two draws of 500 from different first instances.
    fn instanced_1000(gpu, checks) {
        // The instanced pipeline draws to linear targets: its colors are
        // stored as given.
        let target = gpu.target(WIDTH, HEIGHT, TextureFormat::Rgba8);
        let color = |col: u32, row: u32| -> [u8; 4] {
            [
                (6 * col) as u8,
                (10 * row) as u8,
                (255 - 3 * col - 2 * row) as u8,
                255,
            ]
        };
        let mut instances = Vec::with_capacity(1000 * 8);
        for row in 0..25 {
            for col in 0..40 {
                let (x, y) = (8 * col + 1, 2 + 7 * row + 1);
                let [x0, y0, x1, y1] = clip(SIZE, [x, y, x + 6, y + 5]);
                let [r, g, b, a] = color(col, row).map(|byte| f32::from(byte) / 255.0);
                instances.extend_from_slice(&[x0, y0, x1, y1, r, g, b, a]);
            }
        }
        let corners = gpu.vertices(&UNIT);
        let instances = gpu.vertices(&instances);
        let pipeline = instanced_pipeline(gpu);

        gpu.begin();
        {
            let clear = Some(LinearColor::new(0.0, 0.0, 0.0, 1.0));
            let mut pass = gpu.device.begin_pass(target.into(), clear).expect("a pass");
            pass.set_pipeline(pipeline).expect("the pipeline");
            pass.set_vertex_buffer(0, corners, 0).expect("the corners");
            pass.set_vertex_buffer(1, instances, 0).expect("the instances");
            pass.draw(0..4, 0..500).expect("the first half");
            pass.draw(0..4, 500..1000).expect("the second half");
        }
        gpu.end();

        let pixels = gpu.read(target);
        for (col, row) in [(0, 0), (3, 4), (39, 12), (20, 13), (0, 24), (39, 24)] {
            let (x, y) = (8 * col + 1, 2 + 7 * row + 1);
            checks.pixel(&pixels, (x + 2, y + 2), color(col, row), 0);
            checks.pixel(&pixels, (x - 1, y + 2), [0, 0, 0, 255], 0);
        }
        target
    }
}

scene! {
    /// The whole target filled under scissors: one inside, one partly
    /// outside the target, and an empty one; then a draw into a viewport,
    /// and one with neither.
    fn scissor(gpu, checks) {
        let target = gpu.target(WIDTH, HEIGHT, TextureFormat::Rgba8Srgb);
        let pipeline = color_pipeline(gpu, TextureFormat::Rgba8Srgb, BlendMode::Opaque);
        let full = gpu.vertices(&FULL);
        let small = gpu.vertices(&quad_in(SIZE, [150, 130, 170, 170]));

        gpu.begin();
        {
            let clear = Color::rgb(20, 20, 60).to_linear();
            let mut pass = gpu.device.begin_pass(target.into(), Some(clear)).expect("a pass");
            pass.set_pipeline(pipeline).expect("the pipeline");
            pass.set_vertex_buffer(0, full, 0).expect("the quad");
            let fill = |pass: &mut Pass<'_>, color: [f32; 4]| {
                pass.set_uniforms(0, &color).expect("the color");
                pass.draw(0..4, 0..1).expect("a draw");
            };
            pass.set_scissor(Some(rect(20, 20, 140, 100)));
            fill(&mut pass, srgb(200, 60, 60));
            pass.set_scissor(Some(rect(100, 60, 400, 250)));
            fill(&mut pass, srgb(60, 200, 90));
            pass.set_scissor(Some(rect(50, 50, 50, 90)));
            fill(&mut pass, srgb(255, 255, 255));
            pass.set_scissor(None);
            pass.set_viewport(rect(200, 10, 300, 50));
            fill(&mut pass, srgb(230, 200, 40));
            pass.set_viewport(rect(0, 0, WIDTH as i32, HEIGHT as i32));
            pass.set_vertex_buffer(0, small, 0).expect("the small quad");
            fill(&mut pass, srgb(250, 250, 250));
        }
        gpu.end();

        let pixels = gpu.read(target);
        let (background, red, green) = ([20, 20, 60, 255], [200, 60, 60, 255], [60, 200, 90, 255]);
        checks.pixel(&pixels, (10, 10), background, 1);
        checks.pixel(&pixels, (30, 30), red, 1);
        checks.pixel(&pixels, (99, 80), red, 1);
        checks.pixel(&pixels, (100, 80), green, 1);
        checks.pixel(&pixels, (319, 179), green, 1);
        checks.pixel(&pixels, (139, 59), red, 1);
        checks.pixel(&pixels, (140, 59), background, 1);
        checks.pixel(&pixels, (50, 70), red, 1);
        checks.pixel(&pixels, (200, 10), [230, 200, 40, 255], 1);
        checks.pixel(&pixels, (299, 49), [230, 200, 40, 255], 1);
        checks.pixel(&pixels, (199, 30), background, 1);
        checks.pixel(&pixels, (300, 30), background, 1);
        checks.pixel(&pixels, (160, 150), [250, 250, 250, 255], 1);
        target
    }
}

scene! {
    /// A glyph drawn into a 64×64 render target, then sampled from it into
    /// the scene at 2× and at 1×, upright.
    fn render_target_round_trip(gpu, checks) {
        let inner_size = UVec2::new(64, 64);
        let inner = gpu.target(64, 64, TextureFormat::Rgba8Srgb);
        let target = gpu.target(WIDTH, HEIGHT, TextureFormat::Rgba8Srgb);
        let pipeline = color_pipeline(gpu, TextureFormat::Rgba8Srgb, BlendMode::Opaque);
        // An F, and a red square at its bottom right.
        let glyph: Vec<f32> = [
            [8, 8, 20, 56],
            [8, 8, 52, 20],
            [8, 28, 40, 38],
            [44, 44, 58, 58],
        ]
        .into_iter()
        .flat_map(|part| quad_in(inner_size, part))
        .collect();
        let glyph = gpu.vertices(&glyph);
        let textured = textured_pipeline(gpu, TextureFormat::Rgba8Srgb);
        // A render target's v = 0 is the bottom of what was drawn, so the
        // top of each quad takes v = 1.
        let copies: Vec<f32> = [
            textured_in(SIZE, [32, 26, 160, 154], [0.0, 1.0, 1.0, 0.0]),
            textured_in(SIZE, [192, 58, 256, 122], [0.0, 1.0, 1.0, 0.0]),
        ]
        .concat();
        let copies = gpu.vertices(&copies);
        let dark = srgb(30, 30, 30);
        let red = srgb(220, 40, 40);

        gpu.begin();
        {
            let clear = Color::rgb(240, 240, 240).to_linear();
            let mut pass = gpu.device.begin_pass(inner.into(), Some(clear)).expect("a pass");
            fill_quads(&mut pass, &[pipeline; 4], glyph, &[dark, dark, dark, red]);
        }
        {
            let clear = Color::rgb(60, 60, 60).to_linear();
            let mut pass = gpu.device.begin_pass(target.into(), Some(clear)).expect("a pass");
            pass.set_pipeline(textured).expect("the pipeline");
            pass.set_vertex_buffer(0, copies, 0).expect("the copies");
            pass.set_texture(0, inner, Sampler::NearestClamp).expect("the render target");
            pass.draw(0..4, 0..1).expect("the 2× copy");
            pass.draw(4..8, 0..1).expect("the 1× copy");
        }
        gpu.end();

        let pixels = gpu.read(target);
        let (light, dark, red) = ([240, 240, 240, 255], [30, 30, 30, 255], [220, 40, 40, 255]);
        checks.pixel(&pixels, (10, 10), [60, 60, 60, 255], 1);
        // Pixel (x, y) of the render target, in each copy.
        let twice = |x: u32, y: u32| (32 + 2 * x, 26 + 2 * y);
        let once = |x: u32, y: u32| (192 + x, 58 + y);
        for copy in [twice, once] {
            checks.pixel(&pixels, copy(2, 2), light, 1);
            checks.pixel(&pixels, copy(10, 50), dark, 1);
            checks.pixel(&pixels, copy(50, 12), dark, 1);
            checks.pixel(&pixels, copy(50, 30), light, 1);
            checks.pixel(&pixels, copy(50, 50), red, 1);
            checks.pixel(&pixels, copy(30, 50), light, 1);
        }
        target
    }
}
