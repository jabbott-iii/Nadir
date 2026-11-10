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

//! The UI on a GPU: where each scale puts it, clips, order between views,
//! the camera's part in it (none), textures and texts, and the instance
//! buffer it shares with the world.

use nadir_assets::Handle;
use nadir_core::{Color, Rect, UVec2, Vec2};
use nadir_render::{
    Camera2D, GlyphMode, RenderFrame, SpriteDraw, TextDraw, Texture, UiScale, UiSpace, UiView,
};

use crate::common::{Art, Stage, solid};

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];

/// 8 × 6 into 21 × 15: a factor of 2, the image at (2, 1) to (18, 13),
/// bars of 2 and 3, and 1 and 2.
const VIRTUAL: UVec2 = UVec2::new(8, 6);
const OUTPUT: UVec2 = UVec2::new(21, 15);

/// A glyph 2 × 2, all white.
const BLOCK: Art = Art {
    id: 'b',
    rows: &["##", "##"],
    offset: (0, 0),
    advance: 3,
};

/// A frame of nothing in the world, on black.
fn frame() -> RenderFrame {
    let mut frame = RenderFrame::new();
    frame.camera = Camera2D {
        position: VIRTUAL.as_vec2() * 0.5,
        ..Camera2D::default()
    };
    frame
}

/// A sprite of all of `texture`, its top-left corner at `(x, y)`, `w` × `h`.
fn rect(texture: Handle<Texture>, x: f32, y: f32, w: f32, h: f32) -> SpriteDraw {
    let mut sprite = SpriteDraw::new(texture, Vec2::new(x, y), Vec2::new(w, h));
    sprite.pivot = Vec2::ZERO;
    sprite
}

/// The output pixels of `color`, as (x, y), from the top left.
fn find(pixels: &[[u8; 4]], color: [u8; 4]) -> Vec<(u32, u32)> {
    (0..OUTPUT.y)
        .flat_map(|y| (0..OUTPUT.x).map(move |x| (x, y)))
        .filter(|&(x, y)| pixels[(y * OUTPUT.x + x) as usize] == color)
        .collect()
}

/// Every pixel from (x0, y0) to (x1, y1), the far corner left out.
fn block(x0: u32, y0: u32, x1: u32, y1: u32) -> Vec<(u32, u32)> {
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .collect()
}

#[test]
fn pixel_ui_lands_where_its_space_says_and_stops_at_the_image() {
    let Some(mut stage) = Stage::new("ui_pixel", VIRTUAL) else {
        return;
    };
    let red = stage.image("red.qoi", 1, 1, &[RED]);
    let blue = stage.image("blue.qoi", 1, 1, &[BLUE]);
    stage.finish();
    let mut frame = frame();
    frame.ui.sprite(rect(red, 0.0, 0.0, 2.0, 1.0));
    // Half on the left bar, and past the bottom-right corner: cut to the
    // image.
    frame.ui.sprite(rect(blue, -1.0, 3.0, 2.0, 1.0));
    frame.ui.sprite(rect(blue, 7.0, 5.0, 3.0, 3.0));
    let (stats, pixels) = stage.draw(&frame, OUTPUT);
    let space = stage.renderer.ui_space(UiScale::Pixel(1), OUTPUT);
    assert_eq!(space, UiSpace::new(UiScale::Pixel(1), VIRTUAL, OUTPUT));
    assert_eq!(space.to_screen(Vec2::ZERO), Vec2::new(2.0, 1.0));
    assert_eq!(find(&pixels, RED), block(2, 1, 6, 3));
    let mut blue_pixels = block(2, 7, 4, 9);
    blue_pixels.extend(block(16, 11, 18, 13));
    blue_pixels.sort_by_key(|&(x, y)| (y, x));
    assert_eq!(find(&pixels, BLUE), blue_pixels);
    // The pointer on a red pixel is inside the red sprite.
    let pointer = space.to_ui(Vec2::new(5.5, 2.5));
    assert!(Rect::new(Vec2::ZERO, Vec2::new(2.0, 1.0)).contains(pointer));
    assert_eq!(
        (
            stats.ui_sprites,
            stats.ui_batches,
            stats.sprites,
            stats.batches
        ),
        (3, 2, 0, 0)
    );
}

#[test]
fn a_free_scale_is_the_output_s_pixels_from_its_corner_bars_and_all() {
    let Some(mut stage) = Stage::new("ui_free", VIRTUAL) else {
        return;
    };
    let red = stage.image("red.qoi", 1, 1, &[RED]);
    let blue = stage.image("blue.qoi", 1, 1, &[BLUE]);
    stage.finish();
    let mut frame = frame();
    // One unit one pixel, on the corner of the bars: not the image's.
    frame.ui.set_view(UiView::new(UiScale::Free(1.0)));
    frame.ui.sprite(rect(red, 0.0, 0.0, 1.0, 1.0));
    frame.ui.set_view(UiView::new(UiScale::Free(3.0)));
    frame.ui.sprite(rect(blue, 4.0, 2.0, 2.0, 1.0));
    let (_, pixels) = stage.draw(&frame, OUTPUT);
    assert_eq!(find(&pixels, RED), [(0, 0)]);
    assert_eq!(find(&pixels, BLUE), block(12, 6, 18, 9));
}

#[test]
fn the_ui_does_not_move_with_the_camera() {
    let Some(mut stage) = Stage::new("ui_camera", VIRTUAL) else {
        return;
    };
    let red = stage.image("red.qoi", 1, 1, &[RED]);
    stage.finish();
    let mut frame = frame();
    frame.ui.sprite(rect(red, 1.0, 1.0, 2.0, 2.0));
    frame.ui.set_view(UiView::new(UiScale::Free(1.0)));
    frame.ui.sprite(rect(red, 19.0, 0.0, 2.0, 1.0));
    let (_, still) = stage.draw(&frame, OUTPUT);
    frame.camera = Camera2D {
        position: Vec2::new(-40.0, 13.0),
        zoom: 3.0,
        shake: Vec2::new(2.0, -1.0),
    };
    let (_, moved) = stage.draw(&frame, OUTPUT);
    assert_eq!(find(&still, RED).len(), 4 * 4 + 2);
    assert_eq!(still, moved);
}

#[test]
fn a_clip_cuts_sprites_and_texts_and_one_inside_another_is_cut_by_both() {
    let Some(mut stage) = Stage::new("ui_clip", VIRTUAL) else {
        return;
    };
    let font = stage.font("block", 2, &[BLOCK], 'b', &[]);
    let red = stage.image("red.qoi", 1, 1, &[RED]);
    stage.finish();
    let mut frame = frame();
    let view = UiView::new(UiScale::Free(1.0));
    frame
        .ui
        .set_view(view.clipped(Rect::new(Vec2::new(2.0, 2.0), Vec2::new(5.0, 4.0))));
    frame.ui.sprite(rect(red, 0.0, 0.0, 10.0, 10.0));
    // A window from 10 to 16, and a list inside it from 13 to 20: 13 to 16.
    let window = view.clipped(Rect::new(Vec2::new(10.0, 0.0), Vec2::new(16.0, 10.0)));
    frame
        .ui
        .set_view(window.clipped(Rect::new(Vec2::new(13.0, 0.0), Vec2::new(20.0, 1.0))));
    frame
        .ui
        .text(TextDraw::new(font, Vec2::new(12.0, 0.0)), "bbb");
    // A list scrolled out of its window draws nothing.
    frame
        .ui
        .set_view(window.clipped(Rect::new(Vec2::new(17.0, 0.0), Vec2::new(20.0, 10.0))));
    frame.ui.sprite(rect(red, 10.0, 5.0, 10.0, 5.0));
    let (stats, pixels) = stage.draw(&frame, OUTPUT);
    assert_eq!(find(&pixels, RED), block(2, 2, 5, 4));
    // Glyphs at 12, 15 and 18, two wide: 13 and 15 of them, one row.
    assert_eq!(find(&pixels, WHITE), [(13, 0), (15, 0)]);
    assert_eq!(
        (stats.ui_sprites, stats.ui_texts, stats.ui_glyphs),
        (1, 1, 2),
        "the glyph at 18 is outside, and so is the last sprite"
    );
}

#[test]
fn a_later_view_draws_over_an_earlier_one_and_texts_keep_their_place() {
    let Some(mut stage) = Stage::new("ui_order", VIRTUAL) else {
        return;
    };
    let font = stage.font("block", 2, &[BLOCK], 'b', &[]);
    let red = stage.image("red.qoi", 1, 1, &[RED]);
    let blue = stage.image("blue.qoi", 1, 1, &[BLUE]);
    stage.finish();
    let mut frame = frame();
    // Pixel UI: a red panel at (2, 1) to (8, 7), and its label at (6, 3)
    // to (10, 7).
    frame.ui.sprite(rect(red, 0.0, 0.0, 3.0, 3.0));
    let mut label = TextDraw::new(font, Vec2::new(2.0, 1.0));
    label.color = Color::rgb(0, 255, 0);
    frame.ui.text(label, "b");
    // Then a free window from x 7, over the label's right, and its own
    // label on it.
    frame.ui.set_view(UiView::new(UiScale::Free(1.0)));
    frame.ui.sprite(rect(blue, 7.0, 0.0, 6.0, 9.0));
    frame.ui.text(TextDraw::new(font, Vec2::new(9.0, 5.0)), "b");
    let (stats, pixels) = stage.draw(&frame, OUTPUT);
    let at = |x: u32, y: u32| pixels[(y * OUTPUT.x + x) as usize];
    assert_eq!(at(3, 2), RED);
    assert_eq!(find(&pixels, GREEN), block(6, 3, 7, 7));
    assert_eq!((at(7, 3), at(8, 8), at(12, 0)), (BLUE, BLUE, BLUE));
    assert_eq!(find(&pixels, WHITE), block(9, 5, 11, 7));
    assert_eq!(at(1, 1), BLACK, "the bar");
    assert_eq!(
        (
            stats.ui_batches,
            stats.ui_texts,
            stats.ui_glyphs,
            stats.texts
        ),
        (4, 2, 2, 0)
    );
}

#[test]
fn ui_texts_draw_with_either_program_after_the_world() {
    let Some(mut stage) = Stage::new("ui_text", VIRTUAL) else {
        return;
    };
    let font = stage.font("block", 2, &[BLOCK], 'b', &[]);
    let blue = stage.image("blue.qoi", 1, 1, &[BLUE]);
    stage.finish();
    let mut frame = frame();
    // A world sprite under the UI's texts.
    frame.sprites.push(rect(blue, 0.0, 0.0, 8.0, 6.0));
    let mut masked = TextDraw::new(font, Vec2::ZERO);
    masked.color = Color::rgb(255, 0, 0);
    frame.ui.text(masked, "b");
    let mut own = TextDraw::new(font, Vec2::new(3.0, 0.0));
    own.glyphs = GlyphMode::Rgba;
    frame.ui.text(own, "b");
    frame
        .texts
        .push(TextDraw::new(font, Vec2::new(0.0, 4.0)), "b");
    let (stats, pixels) = stage.draw(&frame, OUTPUT);
    assert_eq!(find(&pixels, RED), block(2, 1, 6, 5));
    let mut white = block(8, 1, 12, 5);
    white.extend(block(2, 9, 6, 13));
    white.sort_by_key(|&(x, y)| (y, x));
    assert_eq!(find(&pixels, WHITE), white);
    assert_eq!(
        (stats.texts, stats.glyphs, stats.ui_texts, stats.ui_glyphs),
        (1, 1, 2, 2)
    );
    assert_eq!((stats.batches, stats.ui_batches), (2, 2));
}

#[test]
fn a_ui_texture_loading_is_skipped_and_a_failed_one_shows_its_placeholder() {
    let Some(mut stage) = Stage::new("ui_textures", VIRTUAL) else {
        return;
    };
    let failed = stage.missing("failed.qoi");
    stage.finish();
    // Loaded after the finish: still loading when the frame is drawn.
    let loading = stage.image("late.qoi", 1, 1, &[RED]);
    let mut frame = frame();
    frame.ui.set_view(UiView::new(UiScale::Free(1.0)));
    frame.ui.sprite(rect(loading, 0.0, 0.0, 4.0, 4.0));
    frame.ui.sprite(rect(failed, 10.0, 0.0, 8.0, 8.0));
    let (stats, pixels) = stage.draw(&frame, OUTPUT);
    assert!(find(&pixels, RED).is_empty());
    assert_eq!(pixels[10], MAGENTA, "the placeholder's corner");
    assert_eq!(pixels[11], BLACK);
    assert_eq!((stats.ui_sprites, stats.skipped), (1, 1));
}

#[test]
fn the_world_and_the_ui_share_an_instance_buffer_that_grows() {
    let size = UVec2::new(32, 32);
    let Some(mut stage) = Stage::new("ui_growth", size) else {
        return;
    };
    let red = stage.image("red.qoi", 1, 1, &[RED]);
    let blue = stage.image("blue.qoi", 1, 1, &[BLUE]);
    stage.finish();
    // Past the 256 instances the buffer starts with, the UI alone, then the
    // world's and the UI's together: a pixel each, the UI on the bottom
    // half.
    for (world, ui) in [(0usize, 300usize), (300, 300), (5, 2)] {
        let mut frame = RenderFrame::new();
        frame.camera.position = size.as_vec2() * 0.5;
        for i in 0..world {
            frame
                .sprites
                .push(rect(red, (i % 32) as f32, (i / 32) as f32, 1.0, 1.0));
        }
        for i in 0..ui {
            let (x, y) = ((i % 32) as f32, 16.0 + (i / 32) as f32);
            frame.ui.sprite(rect(blue, x, y, 1.0, 1.0));
        }
        let (stats, pixels) = stage.draw(&frame, size);
        let count = |color| pixels.iter().filter(|&&pixel| pixel == color).count();
        assert_eq!((count(RED), count(BLUE)), (world, ui), "{world} and {ui}");
        assert_eq!(
            (stats.sprites as usize, stats.ui_sprites as usize),
            (world, ui)
        );
    }
}

#[test]
fn a_ui_sprite_of_a_whole_texture_shows_each_texel_on_its_pixels() {
    let Some(mut stage) = Stage::new("ui_texels", VIRTUAL) else {
        return;
    };
    // Red, blue over white, black: two units of two pixels each.
    let texels = [RED, BLUE, WHITE, BLACK];
    let quad = stage.image("quad.qoi", 2, 2, &texels);
    let green = stage.image("green.qoi", 1, 1, &solid(1, GREEN));
    stage.finish();
    let mut frame = frame();
    frame.ui.sprite(rect(green, 0.0, 0.0, 8.0, 6.0));
    frame.ui.sprite(rect(quad, 1.0, 1.0, 2.0, 2.0));
    let (_, pixels) = stage.draw(&frame, OUTPUT);
    assert_eq!(find(&pixels, RED), block(4, 3, 6, 5));
    assert_eq!(find(&pixels, BLUE), block(6, 3, 8, 5));
    assert_eq!(find(&pixels, WHITE), block(4, 5, 6, 7));
    // Black on the bars too: the texel's and the bars'.
    assert!(find(&pixels, BLACK).contains(&(6, 5)));
    assert_eq!(find(&pixels, GREEN).len(), 16 * 12 - 16);
}
