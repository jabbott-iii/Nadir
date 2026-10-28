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
use std::time::Duration;

use nadir_core::{Color, IVec2, Rect, UVec2, Vec2};
use nadir_gfx::Device;
use nadir_render::{
    Camera2D, Flip, FrameStats, RenderFrame, SortMode, SpriteDraw, Tile, TileLayer, TileLayerDraw,
    TileSet, Upscale,
};

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
    shoot_counting(stage, frame, size).0
}

/// Renders `frame` into an output of `size`, and says what it drew.
fn shoot_counting(stage: &mut Stage, frame: &RenderFrame, size: UVec2) -> (Shot, FrameStats) {
    let (stats, pixels) = stage.draw(frame, size);
    (Shot { size, pixels }, stats)
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

const GRASS_TILE: [u8; 4] = opaque(60, 140, 60);
const GRASS_DOT: [u8; 4] = opaque(40, 100, 40);
const PATH: [u8; 4] = opaque(190, 160, 110);
const WATER: [u8; 4] = opaque(40, 80, 200);
const RIPPLE: [u8; 4] = opaque(150, 190, 255);
const ARROW: [u8; 4] = opaque(230, 120, 30);
const ARROW_BACK: [u8; 4] = opaque(80, 50, 20);
const ROOF: [u8; 4] = opaque(150, 50, 40);

/// The tile set of the tile scene: 8 × 4 tiles of 16 texels, numbered
/// row by row.
/// - 0: grass, with two dark dots
/// - 1: path
/// - 2 and 3: water, with a ripple on row 4, then on row 10
/// - 5: an arrow, light on its left with a white mark at its top left, dark
///   on its right: what flips show
/// - 6: roof
/// - 7 over 15: a tree, a tall tile: leaves, then a trunk 4 texels wide
/// - the rest: clear
fn tile_atlas() -> Vec<[u8; 4]> {
    (0..128 * 64)
        .map(|i| {
            let (x, y) = (i % 128, i / 128);
            let (tile, tx, ty) = (x / 16 + y / 16 * 8, x % 16, y % 16);
            match tile {
                0 if (3..5).contains(&tx) && (3..5).contains(&ty) => GRASS_DOT,
                0 if (11..13).contains(&tx) && (9..11).contains(&ty) => GRASS_DOT,
                0 => GRASS_TILE,
                1 => PATH,
                2 if ty == 4 => RIPPLE,
                3 if ty == 10 => RIPPLE,
                2 | 3 => WATER,
                5 if tx < 4 && ty < 4 => BUCKLE,
                5 if tx < 8 => ARROW,
                5 => ARROW_BACK,
                6 => ROOF,
                7 => LEAVES,
                15 if (6..10).contains(&tx) => BARK,
                _ => CLEAR_PIXEL,
            }
        })
        .collect()
}

scene! {
    /// Tile layers among sprites. A ground layer of grass, a path and
    /// animated water, at time 0, crosses from its first chunk into its
    /// second at x = 160, where four arrows show the flips. A decal in the
    /// ground's layer and depth lies over it. A tree from the tile set, a
    /// tall tile, is a y-sorted sprite between two people. A roof layer in
    /// a higher layer covers a third person, and a layer whose tile set
    /// failed shows its placeholder in every tile.
    fn tile_layers(stage: UVec2::new(320, 180), checks) {
        let atlas = stage.image("tiles.qoi", 128, 64, &tile_atlas());
        let person = stage.image("person.qoi", 12, 20, &person());
        let yellow = stage.image("yellow.qoi", 1, 1, &[[255, 255, 0, 255]]);
        let ghost = stage.missing("ghost.qoi");
        stage.finish();
        let set = TileSet { texture: atlas, columns: 8, rows: 4 };

        // 44 × 12 cells of 16 pixels from x = -352: cell 22 is at x = 0,
        // and chunk 1 starts at cell 32, x = 160.
        let mut ground = TileLayer::new(set, UVec2::new(44, 12), 16.0, Vec2::new(-352.0, 0.0));
        for y in 0..12 {
            for x in 0..44 {
                let tile = match (x, y) {
                    (_, 8) => Tile::new(1),
                    (24..28, 2..5) => Tile::animated(2, 2, 250),
                    _ => Tile::new(0),
                };
                ground.set(IVec2::new(x, y), Some(tile));
            }
        }
        // At y = 96: plain, flipped across, flipped down, both.
        for (x, (fx, fy)) in (30..).zip([(false, false), (true, false), (false, true), (true, true)]) {
            let mut arrow = Tile::new(5);
            arrow.flip = Flip { x: fx, y: fy };
            ground.set(IVec2::new(x, 6), Some(arrow));
        }
        let mut roof = TileLayer::new(set, UVec2::new(4, 2), 16.0, Vec2::new(240.0, 32.0));
        for y in 0..2 {
            for x in 0..4 {
                roof.set(IVec2::new(x, y), Some(Tile::new(6)));
            }
        }
        let broken = TileSet { texture: ghost, ..set };
        let mut lost = TileLayer::new(broken, UVec2::new(2, 1), 16.0, Vec2::new(8.0, 152.0));
        lost.set(IVec2::new(0, 0), Some(Tile::new(3)));
        lost.set(IVec2::new(1, 0), Some(Tile::new(4)));

        let mut frame = RenderFrame::new();
        frame.camera = Camera2D {
            position: Vec2::new(160.0, 90.0),
            ..Camera2D::default()
        };
        frame.clear = Color::rgb(SKY[0], SKY[1], SKY[2]);
        // The roof first: layers are drawn by layer and depth, whatever
        // their order here.
        frame.tile_layers.push(TileLayerDraw {
            layer: 2,
            ..TileLayerDraw::new(stage.tiles.add(roof))
        });
        frame.tile_layers.push(TileLayerDraw::new(stage.tiles.add(ground)));
        frame.tile_layers.push(TileLayerDraw::new(stage.tiles.add(lost)));

        // The decal: 24 × 8 from (60, 140), over the path.
        let mut decal = SpriteDraw::new(yellow, Vec2::new(60.0, 140.0), Vec2::new(24.0, 8.0));
        decal.pivot = Vec2::ZERO;
        decal.sort = SortMode::Fixed(0);
        frame.sprites.push(decal);
        let standing = |texture, x: f32, y: f32, size: Vec2| {
            let mut sprite = SpriteDraw::new(texture, Vec2::new(x, y), size);
            sprite.layer = 1;
            sprite
        };
        // The tree: feet at (216, 112), so (208, 80) to (224, 112), its trunk
        // at x 214 to 218 below y 96.
        let mut tree = standing(atlas, 216.0, 112.0, Vec2::new(16.0, 32.0));
        tree.uv = set.block_uv(7, UVec2::new(1, 2));
        // In front: feet at 126, corner (210, 106). Behind: feet at 104,
        // corner (210, 84). Under the roof: feet at 70, corner (250, 50).
        frame.sprites.push(standing(person, 216.0, 126.0, Vec2::new(12.0, 20.0)));
        frame.sprites.push(tree);
        frame.sprites.push(standing(person, 216.0, 104.0, Vec2::new(12.0, 20.0)));
        frame.sprites.push(standing(person, 256.0, 70.0, Vec2::new(12.0, 20.0)));

        let (shot, stats) = shoot_counting(stage, &frame, UVec2::new(320, 180));
        // Grass on both sides of the chunk edge, with its dots: cell 22's
        // at (3, 3), cell 33's at x 176 + 11, y 9.
        checks.pixel(&shot, (0, 0), GRASS_TILE);
        checks.pixel(&shot, (3, 3), GRASS_DOT);
        checks.pixel(&shot, (187, 9), GRASS_DOT);
        checks.pixel(&shot, (186, 9), GRASS_TILE);
        checks.pixel(&shot, (100, 130), PATH);
        // Water at time 0: the first frame, rippled on its row 4.
        checks.pixel(&shot, (40, 37), WATER);
        checks.pixel(&shot, (40, 36), RIPPLE);
        checks.pixel(&shot, (40, 42), WATER);
        // The arrows, at x 128, 144, 160 and 176, y 96: the white mark in
        // the top left, the top right, the bottom left, the bottom right.
        checks.pixel(&shot, (128, 96), BUCKLE);
        checks.pixel(&shot, (130, 104), ARROW);
        checks.pixel(&shot, (141, 104), ARROW_BACK);
        checks.pixel(&shot, (159, 96), BUCKLE);
        checks.pixel(&shot, (144, 104), ARROW_BACK);
        checks.pixel(&shot, (160, 111), BUCKLE);
        checks.pixel(&shot, (160, 96), ARROW);
        checks.pixel(&shot, (191, 111), BUCKLE);
        checks.pixel(&shot, (176, 96), ARROW_BACK);
        // The decal over the path.
        checks.pixel(&shot, (60, 140), YELLOW);
        checks.pixel(&shot, (59, 140), PATH);
        // The tree's leaves over the person behind; its trunk between the
        // two; the person in front over the trunk.
        checks.pixel(&shot, (212, 90), LEAVES);
        checks.pixel(&shot, (215, 100), BARK);
        checks.pixel(&shot, (211, 100), CLOTH);
        checks.pixel(&shot, (215, 110), CLOTH);
        checks.pixel(&shot, (215, 114), CLOTH);
        // The roof over the person beneath it, who shows below it.
        checks.pixel(&shot, (252, 55), ROOF);
        checks.pixel(&shot, (252, 66), CLOTH);
        checks.pixel(&shot, (239, 40), GRASS_TILE);
        // The failed layer: the placeholder's checker in each tile, two
        // pixels a texel.
        checks.pixel(&shot, (8, 152), MAGENTA);
        checks.pixel(&shot, (10, 152), BLACK);
        checks.pixel(&shot, (24, 152), MAGENTA);
        checks.pixel(&shot, (26, 152), BLACK);
        checks.pixel(&shot, (40, 152), GRASS_TILE);
        // Two chunks of the ground (32 + 12 columns of 12 rows), the roof's
        // and the failed layer's.
        checks.that(
            (stats.chunks, stats.tiles) == (4, 44 * 12 + 8 + 2),
            &format!("{} chunks and {} tiles drawn", stats.chunks, stats.tiles),
        );
        checks.that(
            (stats.sprites, stats.batches) == (5, 4),
            &format!("{} sprites in {} batches", stats.sprites, stats.batches),
        );
        shot
    }
}

/// The colors of the animated scene's tiles, each with a white mark in its
/// top-left 4 × 4 texels.
const FRAMES: [[u8; 4]; 8] = [
    opaque(255, 0, 0),
    opaque(0, 255, 0),
    opaque(0, 0, 255),
    opaque(255, 255, 0),
    opaque(0, 255, 255),
    opaque(255, 0, 255),
    opaque(255, 128, 0),
    opaque(128, 0, 255),
];

/// Draws four animated tiles, of 16 pixels, at `ms` milliseconds:
/// - 2 frames of 100 ms from tile 0
/// - 3 frames of 150 ms from tile 2, flipped across
/// - 4 frames of 50 ms from tile 5, which wrap round past tile 7 to 0
/// - tile 7, still, flipped down
///
/// `expected` is each cell's tile at that time.
fn animated(stage: &mut Stage, checks: &mut Checks, ms: u64, expected: [usize; 4]) -> Shot {
    let texels: Vec<[u8; 4]> = (0..128 * 16)
        .map(|i| {
            let (x, y) = (i % 128, i / 128);
            if x % 16 < 4 && y < 4 {
                BUCKLE
            } else {
                FRAMES[x / 16]
            }
        })
        .collect();
    let atlas = stage.image("frames.qoi", 128, 16, &texels);
    stage.finish();
    let set = TileSet {
        texture: atlas,
        columns: 8,
        rows: 1,
    };
    let mut tiles = TileLayer::new(set, UVec2::new(4, 1), 16.0, Vec2::ZERO);
    let mut across = Tile::animated(2, 3, 150);
    across.flip.x = true;
    let mut down = Tile::new(7);
    down.flip.y = true;
    let cells = [
        Tile::animated(0, 2, 100),
        across,
        Tile::animated(5, 4, 50),
        down,
    ];
    for (x, tile) in (0..).zip(cells) {
        tiles.set(IVec2::new(x, 0), Some(tile));
    }
    let size = UVec2::new(64, 16);
    let mut frame = RenderFrame::new();
    frame.camera.position = size.as_vec2() * 0.5;
    frame.time = Duration::from_millis(ms);
    frame
        .tile_layers
        .push(TileLayerDraw::new(stage.tiles.add(tiles)));
    let shot = shoot(stage, &frame, size);
    for (cell, tile) in expected.into_iter().enumerate() {
        checks.pixel(&shot, (cell as i32 * 16 + 8, 8), FRAMES[tile]);
    }
    // The marks: top left, top right, top left, bottom left.
    checks.pixel(&shot, (0, 0), BUCKLE);
    checks.pixel(&shot, (31, 0), BUCKLE);
    checks.pixel(&shot, (16, 0), FRAMES[expected[1]]);
    checks.pixel(&shot, (32, 0), BUCKLE);
    checks.pixel(&shot, (48, 15), BUCKLE);
    checks.pixel(&shot, (48, 0), FRAMES[7]);
    shot
}

scene! {
    /// The animated tiles at 0 ms: each on its first frame.
    fn animated_tiles_0ms(stage: UVec2::new(64, 16), checks) {
        animated(stage, checks, 0, [0, 2, 5, 7])
    }
}

scene! {
    /// The animated tiles at 350 ms: frame 1 of 2, frame 2 of 3, and frame
    /// 3 of 4, which is tile 8, past the last: tile 0.
    fn animated_tiles_350ms(stage: UVec2::new(64, 16), checks) {
        animated(stage, checks, 350, [1, 4, 0, 7])
    }
}
