//! Tile layers on a GPU: placement, changes, replacement, culling, order
//! among the sprites, animation, flips and texture states.

use std::time::Duration;

use nadir_assets::Handle;
use nadir_core::{IVec2, UVec2, Vec2};
use nadir_render::{
    Camera2D, Flip, RenderFrame, SortMode, SpriteDraw, Texture, Tile, TileLayer, TileLayerDraw,
    TileLayers, TileSet,
};

use crate::common::Stage;

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];

/// A frame whose camera shows the world from (0, 0) at the top left of a
/// view of `size`.
fn frame(size: UVec2) -> RenderFrame {
    let mut frame = RenderFrame::new();
    frame.camera = Camera2D {
        position: size.as_vec2() * 0.5,
        ..Camera2D::default()
    };
    frame
}

/// The texels of a tile set of `colors.len()` tiles in one row, each
/// `side` texels square and of one color.
fn strip(colors: &[[u8; 4]], side: u32) -> Vec<[u8; 4]> {
    let width = side * colors.len() as u32;
    (0..width * side)
        .map(|i| colors[((i % width) / side) as usize])
        .collect()
}

/// Loads a tile set of one row of single-color tiles of `side` texels.
fn tileset(stage: &mut Stage, name: &str, colors: &[[u8; 4]], side: u32) -> TileSet {
    let width = side * colors.len() as u32;
    let texture = stage.image(name, width, side, &strip(colors, side));
    TileSet {
        texture,
        columns: colors.len() as u16,
        rows: 1,
    }
}

/// A layer of `width` × `height` cells of `tile` world units at `origin`.
fn layer(tileset: TileSet, width: u32, height: u32, tile: f32, origin: Vec2) -> TileLayer {
    TileLayer::new(tileset, UVec2::new(width, height), tile, origin)
}

/// Every cell of `layer` showing `tile`, row by row.
fn fill(layer: &mut TileLayer, tile: Tile) {
    let size = layer.size();
    for y in 0..size.y as i32 {
        for x in 0..size.x as i32 {
            layer.set(IVec2::new(x, y), Some(tile));
        }
    }
}

/// Draws `layer` alone, in layer 0, into a view of `size`, at `time`.
fn draw_layer(
    stage: &mut Stage,
    handle: Handle<TileLayer>,
    size: UVec2,
    time: Duration,
) -> (nadir_render::FrameStats, Vec<[u8; 4]>) {
    let mut frame = frame(size);
    frame.tile_layers.push(TileLayerDraw::new(handle));
    frame.time = time;
    stage.draw(&frame, size)
}

fn pixel(pixels: &[[u8; 4]], size: UVec2, x: u32, y: u32) -> [u8; 4] {
    pixels[(y * size.x + x) as usize]
}

#[test]
fn a_tile_layer_draws_its_tiles_in_their_cells() {
    let size = UVec2::new(16, 16);
    let Some(mut stage) = Stage::new("tiles_in_cells", size) else {
        return;
    };
    let set = tileset(&mut stage, "set.qoi", &[RED, GREEN, BLUE, WHITE], 4);
    stage.finish();
    // Three columns and two rows of 4-pixel cells, from (4, 4).
    let mut tiles = layer(set, 3, 2, 4.0, Vec2::new(4.0, 4.0));
    tiles.set(IVec2::new(0, 0), Some(Tile::new(0)));
    tiles.set(IVec2::new(1, 0), Some(Tile::new(1)));
    tiles.set(IVec2::new(2, 1), Some(Tile::new(2)));
    let handle = stage.tiles.add(tiles);
    let (stats, pixels) = draw_layer(&mut stage, handle, size, Duration::ZERO);
    let at = |x, y| pixel(&pixels, size, x, y);
    assert_eq!((at(4, 4), at(7, 7)), (RED, RED));
    assert_eq!((at(8, 4), at(11, 7)), (GREEN, GREEN));
    assert_eq!((at(12, 8), at(15, 11)), (BLUE, BLUE));
    assert_eq!(at(8, 8), CLEAR, "an empty cell");
    assert_eq!(at(3, 4), CLEAR, "left of the layer");
    assert_eq!(at(4, 12), CLEAR, "below it");
    assert_eq!(
        (stats.tiles, stats.chunks, stats.sprites, stats.batches),
        (3, 1, 0, 0)
    );

    // A change shows at the next frame.
    stage
        .tiles
        .get_mut(handle)
        .expect("the layer")
        .set(IVec2::new(1, 1), Some(Tile::new(3)));
    let (stats, pixels) = draw_layer(&mut stage, handle, size, Duration::ZERO);
    assert_eq!(pixel(&pixels, size, 8, 8), WHITE);
    assert_eq!(stats.tiles, 4);

    // A chunk emptied draws nothing.
    let tiles = stage.tiles.get_mut(handle).expect("the layer");
    for (x, y) in [(0, 0), (1, 0), (2, 1), (1, 1)] {
        tiles.set(IVec2::new(x, y), None);
    }
    let (stats, pixels) = draw_layer(&mut stage, handle, size, Duration::ZERO);
    assert_eq!((stats.tiles, stats.chunks), (0, 0));
    assert!(pixels.iter().all(|&texel| texel == CLEAR));
}

#[test]
fn a_layer_put_in_the_place_of_another_is_drawn_anew() {
    let size = UVec2::new(8, 8);
    let Some(mut stage) = Stage::new("tiles_replaced", size) else {
        return;
    };
    let set = tileset(&mut stage, "set.qoi", &[RED, GREEN, BLUE], 1);
    stage.finish();
    let make = |index| {
        let mut tiles = layer(set, 8, 8, 1.0, Vec2::ZERO);
        fill(&mut tiles, Tile::new(index));
        tiles
    };
    let first = stage.tiles.add(make(0));
    let (_, pixels) = draw_layer(&mut stage, first, size, Duration::ZERO);
    assert_eq!(pixels[0], RED);

    // A new store whose first layer has the first handle again, filled the
    // same way with other tiles: drawn as it is, not as the old one was.
    stage.tiles = TileLayers::new();
    let second = stage.tiles.add(make(1));
    assert_eq!(second, first);
    let (_, pixels) = draw_layer(&mut stage, second, size, Duration::ZERO);
    assert_eq!(pixels[0], GREEN);

    // And a layer replaced in place.
    *stage.tiles.get_mut(second).expect("the layer") = make(2);
    let (_, pixels) = draw_layer(&mut stage, second, size, Duration::ZERO);
    assert_eq!(pixels[63], BLUE);

    // Replaced by a layer of another size, with other chunks.
    let mut wide = layer(set, 40, 2, 1.0, Vec2::ZERO);
    fill(&mut wide, Tile::new(0));
    *stage.tiles.get_mut(second).expect("the layer") = wide;
    let (stats, pixels) = draw_layer(&mut stage, second, size, Duration::ZERO);
    assert_eq!((pixels[0], pixels[2 * 8]), (RED, CLEAR));
    assert_eq!((stats.chunks, stats.tiles), (1, 2 * 32));

    // Replaced by a larger one, whose chunk (1, 1) the old one did not have.
    let mut large = layer(set, 64, 64, 1.0, Vec2::ZERO);
    fill(&mut large, Tile::new(1));
    *stage.tiles.get_mut(second).expect("the layer") = large;
    let mut far = frame(size);
    far.camera.position = Vec2::new(48.0, 48.0);
    far.tile_layers.push(TileLayerDraw::new(second));
    let (stats, pixels) = stage.draw(&far, size);
    assert_eq!((pixels[0], stats.chunks, stats.tiles), (GREEN, 1, 1024));
}

#[test]
fn layers_empty_unknown_or_gone_draw_nothing() {
    let size = UVec2::new(8, 8);
    let Some(mut stage) = Stage::new("tiles_nothing", size) else {
        return;
    };
    let set = tileset(&mut stage, "set.qoi", &[RED], 1);
    stage.finish();
    let empty = stage.tiles.add(layer(set, 0, 5, 1.0, Vec2::ZERO));
    let blank = stage.tiles.add(layer(set, 8, 8, 1.0, Vec2::ZERO));
    let mut full = layer(set, 8, 8, 1.0, Vec2::ZERO);
    fill(&mut full, Tile::new(0));
    let gone = stage.tiles.add(full);
    let (stats, pixels) = draw_layer(&mut stage, gone, size, Duration::ZERO);
    assert_eq!((stats.chunks, pixels[0]), (1, RED));

    assert!(stage.tiles.remove(gone).is_some());
    let mut frame = frame(size);
    for handle in [empty, blank, gone] {
        frame.tile_layers.push(TileLayerDraw::new(handle));
    }
    let (stats, pixels) = stage.draw(&frame, size);
    assert_eq!((stats.tiles, stats.chunks), (0, 0));
    assert!(pixels.iter().all(|&texel| texel == CLEAR));
    // A layer gone stays gone. (`chunks.rs` checks that its buffers are
    // freed.)
    let (stats, _) = stage.draw(&frame, size);
    assert_eq!(stats.chunks, 0);
}

#[test]
fn a_layer_not_drawn_does_not_stop_the_ones_after_it() {
    let size = UVec2::new(8, 8);
    let Some(mut stage) = Stage::new("tiles_skipped", size) else {
        return;
    };
    let set = tileset(&mut stage, "set.qoi", &[RED], 1);
    stage.finish();
    let late = TileSet {
        texture: stage.image("late.qoi", 1, 1, &[GREEN]),
        columns: 1,
        rows: 1,
    };
    let full = |set, origin| {
        let mut tiles = layer(set, 8, 8, 1.0, origin);
        fill(&mut tiles, Tile::new(0));
        tiles
    };
    let loading = stage.tiles.add(full(late, Vec2::ZERO));
    let gone = stage.tiles.add(full(set, Vec2::ZERO));
    stage.tiles.remove(gone);
    let away = stage.tiles.add(full(set, Vec2::splat(1000.0)));
    let shown = stage.tiles.add(full(set, Vec2::ZERO));
    let mut frame = frame(size);
    for handle in [loading, gone, away, shown] {
        frame.tile_layers.push(TileLayerDraw::new(handle));
    }
    let (stats, pixels) = stage.draw(&frame, size);
    assert_eq!((stats.chunks, stats.tiles, pixels[0]), (1, 64, RED));
}

#[test]
fn a_layer_waits_for_its_tile_set_and_shows_a_failed_ones_placeholder() {
    let size = UVec2::new(16, 8);
    let Some(mut stage) = Stage::new("tiles_texture_states", size) else {
        return;
    };
    let missing = stage.missing("missing.qoi");
    stage.finish();
    // Loaded after the finish: still loading when the frame is drawn.
    let late = stage.image("late.qoi", 1, 1, &[RED]);
    let mut frame = frame(size);
    for (texture, x) in [(missing, 0.0), (late, 8.0)] {
        let set = TileSet {
            texture,
            columns: 4,
            rows: 4,
        };
        let mut tiles = layer(set, 1, 1, 8.0, Vec2::new(x, 0.0));
        tiles.set(IVec2::ZERO, Some(Tile::new(5)));
        frame
            .tile_layers
            .push(TileLayerDraw::new(stage.tiles.add(tiles)));
    }
    let (stats, pixels) = stage.draw(&frame, size);
    // All of the 8 × 8 checker, a texel a pixel, whatever the tile.
    let at = |x, y| pixel(&pixels, size, x, y);
    assert_eq!(
        (at(0, 0), at(1, 0), at(0, 1), at(7, 7)),
        (MAGENTA, CLEAR, CLEAR, MAGENTA)
    );
    assert_eq!(at(8, 0), CLEAR, "still loading");
    assert_eq!((stats.tiles, stats.chunks), (1, 1));
}

#[test]
fn a_view_draws_only_the_chunks_it_shows() {
    let size = UVec2::new(16, 16);
    let Some(mut stage) = Stage::new("tiles_culled", size) else {
        return;
    };
    let set = tileset(&mut stage, "set.qoi", &[RED, GREEN], 1);
    stage.finish();
    // 200 × 200 cells of a pixel: chunks of 32 pixels.
    let mut tiles = layer(set, 200, 200, 1.0, Vec2::ZERO);
    fill(&mut tiles, Tile::new(0));
    tiles.set(IVec2::new(100, 100), Some(Tile::new(1)));
    let handle = stage.tiles.add(tiles);
    let mut frame = frame(size);
    frame.camera.position = Vec2::new(100.0, 100.0);
    frame.tile_layers.push(TileLayerDraw::new(handle));
    let (stats, pixels) = stage.draw(&frame, size);
    // The view is cells 92 to 107, and a cell more: 91 to 108, chunks 2
    // and 3 both ways.
    assert_eq!((stats.chunks, stats.tiles), (4, 4 * 1024));
    assert_eq!(pixel(&pixels, size, 8, 8), GREEN);
    assert_eq!(pixel(&pixels, size, 7, 8), RED);
    // Moved inside one chunk, with a cell to spare all round.
    frame.camera.position = Vec2::new(48.0, 48.0);
    let (stats, _) = stage.draw(&frame, size);
    assert_eq!(stats.chunks, 1);
}

#[test]
fn tile_layers_go_among_the_sprites_by_layer_and_depth() {
    let size = UVec2::new(16, 16);
    let Some(mut stage) = Stage::new("tiles_order", size) else {
        return;
    };
    let set = tileset(&mut stage, "set.qoi", &[BLUE], 1);
    let red = stage.image("red.qoi", 1, 1, &[RED]);
    let green = stage.image("green.qoi", 1, 1, &[GREEN]);
    stage.finish();
    let mut tiles = layer(set, 16, 16, 1.0, Vec2::ZERO);
    fill(&mut tiles, Tile::new(0));
    let handle = stage.tiles.add(tiles);
    let sprite = |texture: Handle<Texture>, x: f32, layer: u8| {
        let mut sprite = SpriteDraw::new(texture, Vec2::new(x, 8.0), Vec2::splat(4.0));
        sprite.layer = layer;
        sprite
    };

    // Two red sprites, one batch, with the tiles of layer 1 between them:
    // the batch is split, and the sprites drawn again after the tiles.
    let mut frame = frame(size);
    frame.tile_layers.push(TileLayerDraw {
        layer: 1,
        ..TileLayerDraw::new(handle)
    });
    frame.sprites.push(sprite(red, 4.0, 0));
    frame.sprites.push(sprite(red, 12.0, 2));
    let (stats, pixels) = stage.draw(&frame, size);
    let at = |pixels: &[[u8; 4]], x, y| pixel(pixels, size, x, y);
    assert_eq!(at(&pixels, 4, 6), BLUE, "under the tiles");
    assert_eq!(at(&pixels, 12, 6), RED, "over them");
    assert_eq!((stats.sprites, stats.batches, stats.chunks), (2, 2, 1));

    // A sprite of the same layer and depth is drawn over the tiles, and a
    // y-sorted one is in front of tiles at depth 0 and behind tiles at
    // `u32::MAX`.
    frame.sprites.clear();
    let mut decal = sprite(green, 4.0, 1);
    decal.sort = SortMode::Fixed(0);
    frame.sprites.push(decal);
    frame.sprites.push(sprite(red, 12.0, 1));
    let (stats, pixels) = stage.draw(&frame, size);
    assert_eq!((at(&pixels, 4, 6), at(&pixels, 12, 6)), (GREEN, RED));
    assert_eq!(stats.batches, 2);
    frame.tile_layers[0].depth = u32::MAX;
    let (_, pixels) = stage.draw(&frame, size);
    assert_eq!((at(&pixels, 4, 6), at(&pixels, 12, 6)), (BLUE, BLUE));

    // Layers in any order: a roof over the left half in layer 2, given
    // before white ground in layer 0, with red sprites in layer 1.
    let white = tileset(&mut stage, "white.qoi", &[WHITE], 1);
    stage.finish();
    let mut ground = layer(white, 16, 16, 1.0, Vec2::ZERO);
    fill(&mut ground, Tile::new(0));
    let mut roof = layer(set, 8, 16, 1.0, Vec2::ZERO);
    fill(&mut roof, Tile::new(0));
    frame.tile_layers.clear();
    frame.tile_layers.push(TileLayerDraw {
        layer: 2,
        ..TileLayerDraw::new(stage.tiles.add(roof))
    });
    frame
        .tile_layers
        .push(TileLayerDraw::new(stage.tiles.add(ground)));
    frame.sprites.clear();
    frame.sprites.push(sprite(red, 4.0, 1));
    frame.sprites.push(sprite(red, 12.0, 1));
    let (stats, pixels) = stage.draw(&frame, size);
    let found = (at(&pixels, 4, 6), at(&pixels, 12, 6), at(&pixels, 12, 1));
    assert_eq!(found, (BLUE, RED, WHITE));
    assert_eq!((stats.batches, stats.chunks), (1, 2));
}

#[test]
fn animated_tiles_change_frame_with_the_time() {
    let size = UVec2::new(16, 4);
    let Some(mut stage) = Stage::new("tiles_animated", size) else {
        return;
    };
    // Two rows of two tiles: red, green, then blue, white. A frame past the
    // last tile that did not wrap would take the clamped bottom row.
    let texture = stage.image("set.qoi", 2, 2, &[RED, GREEN, BLUE, WHITE]);
    stage.finish();
    let set = TileSet {
        texture,
        columns: 2,
        rows: 2,
    };
    let mut tiles = layer(set, 4, 1, 4.0, Vec2::ZERO);
    // Three frames of 100 ms from red; three of 100 ms from blue, which
    // wrap round to red past the last tile; green with 0 frames, which is
    // still; and three frames of 0 ms, which count as 1 ms.
    let cells = [
        Tile::animated(0, 3, 100),
        Tile::animated(2, 3, 100),
        Tile::animated(1, 0, 100),
        Tile::animated(0, 3, 0),
    ];
    for (x, tile) in (0..).zip(cells) {
        tiles.set(IVec2::new(x, 0), Some(tile));
    }
    let handle = stage.tiles.add(tiles);
    let cases = [
        (0, [RED, BLUE, GREEN, RED]),
        (99, [RED, BLUE, GREEN, RED]),
        (100, [GREEN, WHITE, GREEN, GREEN]),
        (250, [BLUE, RED, GREEN, GREEN]),
        (251, [BLUE, RED, GREEN, BLUE]),
        (252, [BLUE, RED, GREEN, RED]),
        (300, [RED, BLUE, GREEN, RED]),
        // 2^32 ms and 150 more: the clock wraps, and shows 150 ms.
        ((1 << 32) + 150, [GREEN, WHITE, GREEN, RED]),
        // Past 2^24 ms, where an f32 would miss: 335,544 frames and 99 ms.
        (33_554_499, [RED, BLUE, GREEN, RED]),
    ];
    for (ms, expected) in cases {
        let (stats, pixels) = draw_layer(&mut stage, handle, size, Duration::from_millis(ms));
        let found = [0, 4, 8, 12].map(|x| pixel(&pixels, size, x, 0));
        assert_eq!(found, expected, "at {ms} ms");
        assert_eq!(stats.tiles, 4);
    }
}

#[test]
fn a_flipped_tile_is_mirrored_in_its_cell() {
    let size = UVec2::new(4, 4);
    let Some(mut stage) = Stage::new("tiles_flipped", size) else {
        return;
    };
    // One tile of 2 × 2 texels: red, green above blue, white.
    let texture = stage.image("set.qoi", 2, 2, &[RED, GREEN, BLUE, WHITE]);
    stage.finish();
    let set = TileSet {
        texture,
        columns: 1,
        rows: 1,
    };
    let handle = stage.tiles.add(layer(set, 1, 1, 4.0, Vec2::ZERO));
    let flips = [
        ((false, false), [RED, GREEN, BLUE, WHITE]),
        ((true, false), [GREEN, RED, WHITE, BLUE]),
        ((false, true), [BLUE, WHITE, RED, GREEN]),
        ((true, true), [WHITE, BLUE, GREEN, RED]),
    ];
    for ((x, y), corners) in flips {
        let mut tile = Tile::new(0);
        tile.flip = Flip { x, y };
        stage
            .tiles
            .get_mut(handle)
            .expect("the layer")
            .set(IVec2::ZERO, Some(tile));
        let (_, pixels) = draw_layer(&mut stage, handle, size, Duration::ZERO);
        let found = [(0, 0), (3, 0), (0, 3), (3, 3)].map(|(px, py)| pixel(&pixels, size, px, py));
        assert_eq!(found, corners, "flip x {x}, y {y}");
    }
}

#[test]
fn a_tile_set_block_is_a_tall_tiles_sprite() {
    let size = UVec2::new(8, 8);
    let Some(mut stage) = Stage::new("tiles_block", size) else {
        return;
    };
    // Two columns, two rows: red, green / blue, white.
    let texture = stage.image("set.qoi", 2, 2, &[RED, GREEN, BLUE, WHITE]);
    stage.finish();
    let set = TileSet {
        texture,
        columns: 2,
        rows: 2,
    };
    // The right column, one tile wide and two high, as a sprite 4 × 8.
    let mut tree = SpriteDraw::new(texture, Vec2::new(4.0, 8.0), Vec2::new(4.0, 8.0));
    tree.uv = set.block_uv(1, UVec2::new(1, 2));
    let mut frame = frame(size);
    frame.sprites.push(tree);
    let (_, pixels) = stage.draw(&frame, size);
    assert_eq!(pixel(&pixels, size, 3, 1), GREEN);
    assert_eq!(pixel(&pixels, size, 4, 6), WHITE);
    assert_eq!(pixel(&pixels, size, 1, 1), CLEAR);
}
