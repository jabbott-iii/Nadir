//! The renderer on a GPU: order, batches, snapping, placeholders, upscale.

use nadir_core::{Color, Rect, UVec2, Vec2};
use nadir_gfx::GfxError;
use nadir_render::{Camera2D, RenderConfig, RenderError, RenderFrame, Renderer, SpriteDraw};

use crate::common::{Stage, device, near, solid};

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 255];

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

#[test]
fn the_sprite_lower_on_screen_is_in_front() {
    let size = UVec2::new(16, 16);
    let Some(mut stage) = Stage::new("in_front", size) else {
        return;
    };
    let red = stage.image("red.qoi", 8, 8, &solid(64, RED));
    let blue = stage.image("blue.qoi", 8, 8, &solid(64, BLUE));
    stage.finish();
    let mut frame = frame(size);
    // The lower one is submitted first, and still drawn last.
    frame.sprites.push(SpriteDraw::new(
        blue,
        Vec2::new(8.0, 12.0),
        Vec2::splat(8.0),
    ));
    frame
        .sprites
        .push(SpriteDraw::new(red, Vec2::new(8.0, 10.0), Vec2::splat(8.0)));
    let (stats, pixels) = stage.draw(&frame, size);
    let at = |x: usize, y: usize| pixels[y * 16 + x];
    assert_eq!(at(8, 8), BLUE, "the overlap");
    assert_eq!(at(8, 3), RED);
    assert_eq!(at(8, 11), BLUE);
    assert_eq!(at(1, 1), CLEAR);
    assert_eq!((stats.sprites, stats.batches, stats.skipped), (2, 2, 0));
}

#[test]
fn interleaved_textures_take_a_draw_each() {
    let size = UVec2::new(16, 16);
    let Some(mut stage) = Stage::new("batches", size) else {
        return;
    };
    let red = stage.image("red.qoi", 1, 1, &[RED]);
    let blue = stage.image("blue.qoi", 1, 1, &[BLUE]);
    let failed = stage.missing("failed.qoi");
    stage.finish();
    // Loaded after the finish: still loading when the frame is drawn.
    let late = stage.image("late.qoi", 1, 1, &[RED]);
    let mut frame = frame(size);
    for (texture, y) in [(red, 2.0), (blue, 4.0), (red, 6.0), (late, 8.0)] {
        frame
            .sprites
            .push(SpriteDraw::new(texture, Vec2::new(4.0, y), Vec2::ONE));
    }
    let (stats, _) = stage.draw(&frame, size);
    assert_eq!((stats.sprites, stats.batches, stats.skipped), (3, 3, 1));
    // One texture, however many sprites: one draw.
    frame.sprites.clear();
    for y in 1..10 {
        frame
            .sprites
            .push(SpriteDraw::new(red, Vec2::new(4.0, y as f32), Vec2::ONE));
    }
    frame
        .sprites
        .push(SpriteDraw::new(failed, Vec2::new(9.0, 12.0), Vec2::ONE));
    let (stats, _) = stage.draw(&frame, size);
    assert_eq!(
        (stats.sprites, stats.batches, stats.skipped),
        (10, 2, 0),
        "the placeholder too"
    );
}

#[test]
fn an_odd_sprite_shows_each_texel_once() {
    // 13 texels wide, centred on a whole pixel: its corner is at 9.5, which
    // snaps to 10, so each texel covers exactly one pixel.
    let size = UVec2::new(32, 8);
    let Some(mut stage) = Stage::new("odd", size) else {
        return;
    };
    let texels: Vec<[u8; 4]> = (0..13u8)
        .map(|k| [k * 20, 255 - k * 20, if k % 2 == 0 { 0 } else { 255 }, 255])
        .collect();
    let strip = stage.image("strip.qoi", 13, 1, &texels);
    stage.finish();
    let mut frame = frame(size);
    frame.sprites.push(SpriteDraw::new(
        strip,
        Vec2::new(16.0, 5.0),
        Vec2::new(13.0, 1.0),
    ));
    let (_, pixels) = stage.draw(&frame, size);
    let row = &pixels[4 * 32..5 * 32];
    assert_eq!(row[9], CLEAR);
    assert_eq!(row[23], CLEAR);
    for (k, texel) in texels.iter().enumerate() {
        assert!(
            near(row[10 + k], *texel, 2),
            "texel {k}: {:?} for {texel:?}",
            row[10 + k]
        );
    }
}

#[test]
fn a_failed_texture_shows_all_of_its_placeholder() {
    let size = UVec2::new(8, 8);
    let Some(mut stage) = Stage::new("placeholder", size) else {
        return;
    };
    let missing = stage.missing("missing.qoi");
    stage.finish();
    let mut frame = frame(size);
    let mut sprite = SpriteDraw::new(missing, Vec2::new(4.0, 8.0), Vec2::splat(8.0));
    // A region of an atlas page: the placeholder shows whole anyway.
    sprite.uv = Rect::new(Vec2::new(0.5, 0.5), Vec2::new(0.625, 0.625));
    frame.sprites.push(sprite);
    let (_, pixels) = stage.draw(&frame, size);
    for (i, pixel) in pixels.iter().enumerate() {
        let (x, y) = (i % 8, i / 8);
        let expected = if (x + y) % 2 == 0 {
            [255, 0, 255, 255]
        } else {
            CLEAR
        };
        assert_eq!(*pixel, expected, "({x}, {y})");
    }
}

#[test]
fn the_image_is_scaled_up_whole_and_letterboxed() {
    let size = UVec2::new(4, 4);
    let Some(mut stage) = Stage::new("letterbox", size) else {
        return;
    };
    let red = stage.image("red.qoi", 1, 1, &[RED]);
    stage.finish();
    let mut frame = frame(size);
    frame.clear = Color::rgb(0, 0, 255);
    // The top-left virtual pixel.
    let mut dot = SpriteDraw::new(red, Vec2::ZERO, Vec2::ONE);
    dot.pivot = Vec2::ZERO;
    frame.sprites.push(dot);
    // 10×9: a factor of 2, the image at (1, 0) to (9, 8).
    let output = UVec2::new(10, 9);
    let (_, pixels) = stage.draw(&frame, output);
    let at = |x: usize, y: usize| pixels[y * 10 + x];
    assert_eq!([at(1, 0), at(2, 1)], [RED, RED], "the dot, two by two");
    assert_eq!([at(3, 0), at(1, 2)], [BLUE, BLUE], "the image's background");
    assert_eq!([at(0, 0), at(9, 0), at(5, 8)], [BLUE; 3], "the bars");
    assert_eq!(stage.renderer.upscale(output).factor, 2);
    let camera = frame.camera;
    assert_eq!(
        stage.renderer.world_to_screen(&camera, output, Vec2::ZERO),
        Vec2::new(1.0, 0.0)
    );
    let world = stage
        .renderer
        .screen_to_world(&camera, output, Vec2::new(2.5, 1.5));
    assert!(
        (0.0..1.0).contains(&world.x) && (0.0..1.0).contains(&world.y),
        "{world:?}"
    );
}

#[test]
fn a_renderer_needs_a_virtual_image_the_device_can_make() {
    let Some(mut device) = device() else { return };
    let config = |x, y| RenderConfig {
        virtual_resolution: UVec2::new(x, y),
    };
    assert!(matches!(
        Renderer::new(&mut device, &config(0, 10)),
        Err(RenderError::BadConfig(_))
    ));
    let max = device.caps().max_texture_size;
    assert!(matches!(
        Renderer::new(&mut device, &config(max + 1, 1)),
        Err(RenderError::Gfx(GfxError::TooLarge { .. }))
    ));
}

/// Where `pixels` (a `width`-wide image) are `color`, as (x, y) pairs.
fn find(pixels: &[[u8; 4]], width: usize, color: [u8; 4]) -> Vec<(usize, usize)> {
    (0..pixels.len())
        .filter(|&i| pixels[i] == color)
        .map(|i| (i % width, i / width))
        .collect()
}

#[test]
fn the_zoom_is_whole_and_shake_moves_the_view() {
    let size = UVec2::new(16, 16);
    let Some(mut stage) = Stage::new("zoom", size) else {
        return;
    };
    let red = stage.image("red.qoi", 1, 1, &[RED]);
    stage.finish();
    let mut frame = frame(size);
    // The camera shows world (0, 0) at the top left: at zoom 2, its centre
    // is (4, 4).
    frame.camera.position = Vec2::splat(4.0);
    let mut dot = SpriteDraw::new(red, Vec2::ONE, Vec2::ONE);
    dot.pivot = Vec2::ZERO;
    frame.sprites.push(dot);
    let square = |x: usize, y: usize| vec![(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)];

    for zoom in [2.0, 1.6, 2.4] {
        frame.camera.zoom = zoom;
        let (_, pixels) = stage.draw(&frame, size);
        assert_eq!(find(&pixels, 16, RED), square(2, 2), "zoom {zoom}");
    }
    // Shaking right by a world unit moves the world left by two pixels.
    frame.camera.zoom = 2.0;
    frame.camera.shake = Vec2::new(1.0, 0.0);
    let (_, pixels) = stage.draw(&frame, size);
    assert_eq!(find(&pixels, 16, RED), square(0, 2));
}

#[test]
fn the_instance_buffer_grows_and_is_reused() {
    let size = UVec2::new(32, 32);
    let Some(mut stage) = Stage::new("growth", size) else {
        return;
    };
    let red = stage.image("red.qoi", 1, 1, &[RED]);
    stage.finish();
    // More than the 256 instances the buffer starts with, then fewer, then
    // more again: one pixel each.
    for count in [300usize, 5, 1000] {
        let mut frame = frame(size);
        for i in 0..count {
            let mut dot =
                SpriteDraw::new(red, Vec2::new((i % 32) as f32, (i / 32) as f32), Vec2::ONE);
            dot.pivot = Vec2::ZERO;
            frame.sprites.push(dot);
        }
        let (stats, pixels) = stage.draw(&frame, size);
        assert_eq!(stats.sprites as usize, count);
        assert_eq!(find(&pixels, 32, RED).len(), count, "{count} sprites");
    }
}

#[test]
fn a_tint_multiplies_the_texture_in_linear_light() {
    let size = UVec2::new(4, 4);
    let Some(mut stage) = Stage::new("tint", size) else {
        return;
    };
    let white = stage.image("white.qoi", 1, 1, &[[255, 255, 255, 255]]);
    stage.finish();
    let mut frame = frame(size);
    let mut sprite = SpriteDraw::new(white, Vec2::new(2.0, 4.0), Vec2::splat(4.0));
    sprite.color = Color::rgb(128, 64, 200);
    frame.sprites.push(sprite);
    let (_, pixels) = stage.draw(&frame, size);
    // Linear light in, sRGB out: the tint's own bytes come back.
    assert!(near(pixels[5], [128, 64, 200, 255], 2), "{:?}", pixels[5]);
}

#[test]
fn a_positive_rotation_turns_clockwise_on_screen() {
    let size = UVec2::new(16, 16);
    let Some(mut stage) = Stage::new("rotation", size) else {
        return;
    };
    let red = stage.image("red.qoi", 1, 1, &[RED]);
    stage.finish();
    let mut frame = frame(size);
    // Four long and two high, pivoting on the middle of its left end, at
    // (8, 8): a quarter turn clockwise points it down the screen.
    let mut bar = SpriteDraw::new(red, Vec2::splat(8.0), Vec2::new(4.0, 2.0));
    bar.pivot = Vec2::new(0.0, 0.5);
    bar.rotation = std::f32::consts::FRAC_PI_2;
    frame.sprites.push(bar);
    let (_, pixels) = stage.draw(&frame, size);
    let at = |x: usize, y: usize| pixels[y * 16 + x];
    assert_eq!(
        [at(7, 10), at(8, 10), at(7, 9), at(8, 11)],
        [RED; 4],
        "below the pivot"
    );
    assert_eq!(
        [at(10, 8), at(7, 5), at(5, 8)],
        [CLEAR; 3],
        "not right, above or left"
    );
}
