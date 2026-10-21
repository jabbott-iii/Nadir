//! The renderer's golden scenes, which `cargo xtask golden` compares with
//! `tests/golden/<scene>.qoi` (`docs/04-quality.md`, ADR-0017).
//!
//! Each scene renders into an output target and checks a few of its
//! pixels. When `NADIR_GOLDEN_DIR` names a directory, it also writes all of
//! them there, as `<scene>.pam`, for xtask to compare. Without it the
//! scenes still render, so that a scene that fails fails `cargo test` too.
//!
//! So that every GPU draws them alike:
//! - colors are sRGB bytes, and texel alphas are 0 or 255
//! - every sprite is drawn at a whole number of pixels per texel, with its
//!   corner on a whole pixel, and unrotated
//! - tints are white, or have channels of 0 and 255 only

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use nadir_core::{Color, IVec2, Rect, UVec2, Vec2};
use nadir_gfx::Device;
use nadir_render::{Camera2D, Flip, RenderFrame, SortMode, SpriteDraw, Upscale};

use crate::common::{EXPECT_RENDERER, Stage, near};

/// Names the directory the scenes write their pixels to.
const DIR_VAR: &str = "NADIR_GOLDEN_DIR";

/// What a scene drew: its size and pixels, from the top row.
struct Shot {
    size: UVec2,
    pixels: Vec<[u8; 4]>,
}

/// Defines a scene: a test that makes a stage of the virtual resolution
/// `$virtual`, runs `$body` (which renders, checks some pixels into
/// `$checks` and returns the [`Shot`]), and saves the shot under the
/// test's own name. Two scenes cannot share a name, and none can forget to
/// save.
///
/// A failed check fails the test only once the pixels are saved, so that
/// `cargo xtask golden` can still show how the scene differs.
macro_rules! scene {
    ($(#[$meta:meta])* fn $name:ident($stage:ident: $virtual:expr, $checks:ident) $body:block) => {
        $(#[$meta])*
        #[test]
        fn $name() {
            let Some(mut stage) = Stage::new(stringify!($name), $virtual) else { return };
            let mut checks = Checks::default();
            let shot = {
                let $stage: &mut Stage = &mut stage;
                let $checks: &mut Checks = &mut checks;
                $body
            };
            save(stringify!($name), &stage.device, &shot);
            check_renderer(&stage.device, &mut checks);
            assert!(checks.0.is_empty(), "{}", checks.0.join("\n"));
        }
    };
}

/// The checks of a scene's pixels that failed.
#[derive(Default)]
struct Checks(Vec<String>);

impl Checks {
    /// Checks the pixel at `(x, y)` from the top left against `expected`,
    /// within 2 a channel.
    #[track_caller]
    fn pixel(&mut self, shot: &Shot, (x, y): (i32, i32), expected: [u8; 4]) {
        let found = (0..shot.size.x as i32)
            .contains(&x)
            .then_some(())
            .and((0..shot.size.y as i32).contains(&y).then_some(()))
            .map(|()| shot.pixels[(y as u32 * shot.size.x + x as u32) as usize]);
        if found.is_none_or(|found| !near(found, expected, 2)) {
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
/// for. It runs after the scene is saved, which names the renderer, so
/// that xtask can say how to choose another.
fn check_renderer(device: &Device, checks: &mut Checks) {
    let expected = env::var(EXPECT_RENDERER).unwrap_or_default();
    let renderer = &device.caps().renderer;
    checks.that(
        renderer.contains(&expected),
        &format!("the renderer is `{renderer}`, and {EXPECT_RENDERER} asks for `{expected}`"),
    );
}

/// Writes the shot as `<name>.pam` (Netpbm PAM, RGBA8, from the top row)
/// in the `NADIR_GOLDEN_DIR` directory, if it is set, as gfx's scenes do.
/// The header names the renderer in a `# renderer` comment.
fn save(name: &str, device: &Device, shot: &Shot) {
    assert_eq!(shot.pixels.len(), (shot.size.x * shot.size.y) as usize);
    let Some(dir) = env::var_os(DIR_VAR).filter(|dir| !dir.is_empty()) else {
        return;
    };
    // The header is one line of text a field: keep the name to printable
    // ASCII.
    let renderer: String = device
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
    let (width, height) = (shot.size.x, shot.size.y);
    let mut bytes = format!(
        "P7\nWIDTH {width}\nHEIGHT {height}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\n\
         # renderer {renderer}\nENDHDR\n"
    )
    .into_bytes();
    bytes.extend_from_slice(shot.pixels.as_flattened());
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

/// Renders `frame` into an output of `size`.
fn shoot(stage: &mut Stage, frame: &RenderFrame, size: UVec2) -> Shot {
    let (_, pixels) = stage.draw(frame, size);
    Shot { size, pixels }
}

const fn opaque(r: u8, g: u8, b: u8) -> [u8; 4] {
    [r, g, b, 255]
}

const CLEAR_PIXEL: [u8; 4] = [0, 0, 0, 0];
const SKY: [u8; 4] = opaque(90, 140, 200);
const LEAVES: [u8; 4] = opaque(34, 139, 34);
const BARK: [u8; 4] = opaque(101, 67, 33);
const CLOTH: [u8; 4] = opaque(40, 80, 200);
const BUCKLE: [u8; 4] = opaque(255, 255, 255);
const EARTH: [u8; 4] = opaque(120, 100, 60);
const GRASS: [u8; 4] = opaque(140, 120, 80);
const MAGENTA: [u8; 4] = opaque(255, 0, 255);
const BLACK: [u8; 4] = opaque(0, 0, 0);

/// A tree, 24×48: leaves over its top 32 rows, and a trunk 8 wide below.
fn tree() -> Vec<[u8; 4]> {
    (0..24 * 48)
        .map(|i| {
            let (x, y) = (i % 24, i / 24);
            match (x, y) {
                (_, 0..32) => LEAVES,
                (8..16, _) => BARK,
                _ => CLEAR_PIXEL,
            }
        })
        .collect()
}

/// A person, 12×20, with a buckle on their left: what flips shows.
fn person() -> Vec<[u8; 4]> {
    (0..12 * 20)
        .map(|i| {
            let (x, y) = (i % 12, i / 12);
            if (1..3).contains(&x) && (14..16).contains(&y) {
                BUCKLE
            } else {
                CLOTH
            }
        })
        .collect()
}

/// Ground, 32×16, in rows of earth and grass.
fn ground() -> Vec<[u8; 4]> {
    (0..32 * 16)
        .map(|i| if (i / 32) % 2 == 0 { EARTH } else { GRASS })
        .collect()
}

scene! {
    /// A person behind a tree, another in front of it, a ground decal, and
    /// a missing texture's placeholder. They are submitted in the reverse of
    /// the order they are drawn in, and the person behind has their feet
    /// only 8 pixels above the tree's: sorting by the sprite's middle or
    /// top would draw them in front.
    fn y_sort(stage: UVec2::new(320, 180), checks) {
        let tree = stage.image("tree.qoi", 24, 48, &tree());
        let person = stage.image("person.qoi", 12, 20, &person());
        let ground = stage.image("ground.qoi", 32, 16, &ground());
        let ghost = stage.missing("ghost.qoi");
        stage.finish();

        let mut frame = RenderFrame::new();
        frame.camera = Camera2D {
            position: Vec2::new(160.0, 90.0),
            ..Camera2D::default()
        };
        frame.clear = Color::rgb(SKY[0], SKY[1], SKY[2]);
        let sprite = |texture, x: f32, y: f32, w: f32, h: f32| {
            let mut sprite = SpriteDraw::new(texture, Vec2::new(x, y), Vec2::new(w, h));
            sprite.layer = 1;
            sprite
        };
        // In front: feet at 140, flipped. Corner (158, 120).
        let mut front = sprite(person, 164.0, 140.0, 12.0, 20.0);
        front.flip = Flip { x: true, y: false };
        frame.sprites.push(front);
        // The tree: feet at 132. Corner (148, 84); leaves to 116, trunk at
        // x 156 to 164.
        frame.sprites.push(sprite(tree, 160.0, 132.0, 24.0, 48.0));
        // Behind: feet at 124. Corner (150, 104).
        frame.sprites.push(sprite(person, 156.0, 124.0, 12.0, 20.0));
        // A missing texture, 16×16 (two pixels a placeholder texel), with
        // the uv of an atlas region it does not have. Corner (212, 84).
        let mut missing = sprite(ghost, 220.0, 100.0, 16.0, 16.0);
        missing.uv = Rect::new(Vec2::new(0.25, 0.25), Vec2::new(0.5, 0.5));
        frame.sprites.push(missing);
        // Ground, 64×32 (two pixels a texel), under everything in layer 0.
        // Corner (128, 118).
        let mut decal = SpriteDraw::new(ground, Vec2::new(160.0, 150.0), Vec2::new(64.0, 32.0));
        decal.sort = SortMode::Fixed(0);
        frame.sprites.push(decal);

        let shot = shoot(stage, &frame, UVec2::new(320, 180));
        checks.pixel(&shot, (5, 5), SKY);
        checks.pixel(&shot, (155, 110), LEAVES);
        checks.pixel(&shot, (152, 120), CLOTH);
        checks.pixel(&shot, (151, 118), BUCKLE);
        checks.pixel(&shot, (160, 125), CLOTH);
        // The trunk, between the two people; the ground beside it.
        checks.pixel(&shot, (157, 125), BARK);
        checks.pixel(&shot, (155, 128), GRASS);
        // The flipped buckle is on the right.
        checks.pixel(&shot, (167, 134), BUCKLE);
        checks.pixel(&shot, (159, 134), CLOTH);
        checks.pixel(&shot, (212, 84), MAGENTA);
        checks.pixel(&shot, (214, 84), BLACK);
        checks.pixel(&shot, (227, 99), MAGENTA);
        checks.pixel(&shot, (130, 145), GRASS);
        checks.pixel(&shot, (130, 147), EARTH);
        shot
    }
}

/// The virtual resolution of the upscale scenes.
const SMALL: UVec2 = UVec2::new(64, 36);
const DARK: [u8; 4] = opaque(32, 32, 32);
const YELLOW: [u8; 4] = opaque(255, 255, 0);

/// Draws the small scene: a block, a person, and a yellow dot in each
/// corner of the virtual image, so that its edges show. Returns the shot
/// of an output of `size`, after checking where the image lands.
fn upscaled(stage: &mut Stage, checks: &mut Checks, size: UVec2) -> Shot {
    let dot = stage.image("dot.qoi", 1, 1, &[[255, 255, 255, 255]]);
    let block = stage.image("block.qoi", 16, 8, &vec![CLOTH; 16 * 8]);
    let person = stage.image("person.qoi", 12, 20, &person());
    stage.finish();

    let mut frame = RenderFrame::new();
    frame.camera = Camera2D {
        position: SMALL.as_vec2() * 0.5,
        ..Camera2D::default()
    };
    frame.clear = Color::rgb(DARK[0], DARK[1], DARK[2]);
    for (x, y) in [(0.0, 0.0), (63.0, 0.0), (0.0, 35.0), (63.0, 35.0)] {
        let mut corner = SpriteDraw::new(dot, Vec2::new(x, y), Vec2::ONE);
        corner.pivot = Vec2::ZERO;
        corner.color = Color::rgb(255, 255, 0);
        frame.sprites.push(corner);
    }
    let mut wall = SpriteDraw::new(block, Vec2::new(8.0, 8.0), Vec2::new(16.0, 8.0));
    wall.pivot = Vec2::ZERO;
    frame.sprites.push(wall);
    frame.sprites.push(SpriteDraw::new(
        person,
        Vec2::new(40.0, 32.0),
        Vec2::new(12.0, 20.0),
    ));

    let shot = shoot(stage, &frame, size);
    let Upscale { factor, rect } = stage.renderer.upscale(size);
    let f = factor as i32;
    let (min, max) = (rect.min, rect.max);
    // Each corner dot fills its f×f square, and the bar is just outside.
    checks.pixel(&shot, (min.x, min.y), YELLOW);
    checks.pixel(&shot, (min.x + f - 1, min.y + f - 1), YELLOW);
    checks.pixel(&shot, (min.x + f, min.y + f), DARK);
    checks.pixel(&shot, (max.x - 1, max.y - 1), YELLOW);
    checks.pixel(&shot, (max.x - f, min.y), YELLOW);
    checks.pixel(&shot, (min.x, max.y - f), YELLOW);
    if min.x > 0 {
        checks.pixel(&shot, (min.x - 1, min.y), DARK);
        checks.pixel(&shot, (max.x, max.y - 1), DARK);
    }
    if min.y > 0 {
        checks.pixel(&shot, (min.x, min.y - 1), DARK);
        checks.pixel(&shot, (max.x - 1, max.y), DARK);
    }
    // The block's top left, at virtual (8, 8).
    checks.pixel(&shot, (min.x + 8 * f, min.y + 8 * f), CLOTH);
    // The mouse maps to what is drawn.
    let camera = frame.camera;
    let screen = stage
        .renderer
        .world_to_screen(&camera, size, Vec2::new(63.0, 35.0));
    checks.that(
        screen == (max - IVec2::splat(f)).as_vec2(),
        &format!("the bottom-right dot is drawn at {screen:?}"),
    );
    let world = stage.renderer.screen_to_world(
        &camera,
        size,
        Vec2::new(min.x as f32 + 0.5, min.y as f32 + 0.5),
    );
    checks.that(
        (0.0..1.0).contains(&world.x) && (0.0..1.0).contains(&world.y),
        &format!("the top-left pixel is world {world:?}"),
    );
    shot
}

scene! {
    /// An exact fit: five times, with no bars.
    fn upscale_320x180(stage: SMALL, checks) {
        let shot = upscaled(stage, checks, UVec2::new(320, 180));
        checks.that(stage.renderer.upscale(shot.size).factor == 5, "a factor of 5");
        shot
    }
}

scene! {
    /// Wide enough for four, high enough for five: four, with bars all
    /// round.
    fn upscale_300x200(stage: SMALL, checks) {
        let shot = upscaled(stage, checks, UVec2::new(300, 200));
        checks.that(stage.renderer.upscale(shot.size).factor == 4, "a factor of 4");
        shot
    }
}

scene! {
    /// High enough for four only, with bars of uneven widths: 38 and 39 at
    /// the sides, 1 and 2 above and below.
    fn upscale_333x147(stage: SMALL, checks) {
        let shot = upscaled(stage, checks, UVec2::new(333, 147));
        checks.that(stage.renderer.upscale(shot.size).factor == 4, "a factor of 4");
        shot
    }
}
