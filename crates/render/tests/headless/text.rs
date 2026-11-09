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

//! Texts on a GPU: the mask and the glyphs' own colors, order among
//! sprites, and fonts, atlases and pages not ready.

use std::fs;

use nadir_assets::{FinalizeCtx, Handle, LoadState, Scope};
use nadir_core::{Color, UVec2, Vec2};
use nadir_render::{Camera2D, Font, GlyphMode, RenderFrame, SortMode, SpriteDraw, TextDraw};

use crate::common::{Art, Stage};

const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];

const SIZE: UVec2 = UVec2::new(16, 8);

/// A glyph 4 × 4: white with a black pixel at its top left, as an outline
/// would be.
const RING: Art = Art {
    id: 'o',
    rows: &["o###", "####", "####", "####"],
    offset: (0, 0),
    advance: 5,
};

/// A frame whose camera shows the world from (0, 0) at the top left, on a
/// clear background.
fn frame() -> RenderFrame {
    let mut frame = RenderFrame::new();
    frame.camera = Camera2D {
        position: SIZE.as_vec2() * 0.5,
        ..Camera2D::default()
    };
    frame.clear = Color::TRANSPARENT;
    frame
}

fn text(font: Handle<Font>, x: f32, color: Color) -> TextDraw {
    let mut text = TextDraw::new(font, Vec2::new(x, 0.0));
    text.color = color;
    text
}

#[test]
fn a_mask_draws_the_text_s_color_and_rgba_the_glyph_s_own() {
    let Some(mut stage) = Stage::new("text_modes", SIZE) else {
        return;
    };
    let font = stage.font("ring", 4, &[RING], 'o', &[]);
    stage.finish();
    let mut frame = frame();
    frame
        .texts
        .push(text(font, 0.0, Color::rgb(255, 0, 0)), "o");
    let mut colored = text(font, 8.0, Color::WHITE);
    colored.glyphs = GlyphMode::Rgba;
    frame.texts.push(colored, "o");
    let (stats, pixels) = stage.draw(&frame, SIZE);
    let at = |x: u32, y: u32| pixels[(y * SIZE.x + x) as usize];
    // The mask reads only the alpha: the black pixel is red too.
    assert_eq!((at(0, 0), at(2, 2)), (RED, RED));
    assert_eq!((at(8, 0), at(10, 2)), (BLACK, WHITE));
    assert_eq!((at(5, 0), at(4, 5)), (CLEAR, CLEAR));
    assert_eq!(
        (stats.texts, stats.glyphs, stats.sprites, stats.batches),
        (2, 2, 0, 2),
        "one draw a program"
    );
    // Through either program, a white glyph in white is the same.
    let mut frame = self::frame();
    frame.texts.push(text(font, 0.0, Color::WHITE), "o");
    let (_, masked) = stage.draw(&frame, SIZE);
    assert_eq!(masked[(2 * SIZE.x + 2) as usize], WHITE);
    assert_eq!(masked[(2 * SIZE.x + 2) as usize], at(10, 2));
}

#[test]
fn a_mask_fades_with_its_color_s_alpha_as_rgba_does() {
    let Some(mut stage) = Stage::new("text_fade", SIZE) else {
        return;
    };
    let font = stage.font("ring", 4, &[RING], 'o', &[]);
    stage.finish();
    let mut frame = frame();
    let half = Color::rgba(255, 255, 255, 128);
    frame.texts.push(text(font, 0.0, half), "o");
    let mut colored = text(font, 8.0, half);
    colored.glyphs = GlyphMode::Rgba;
    frame.texts.push(colored, "o");
    let (_, pixels) = stage.draw(&frame, SIZE);
    let at = |x: u32, y: u32| pixels[(y * SIZE.x + x) as usize];
    assert_ne!(at(2, 2), WHITE, "faded");
    assert_ne!(at(2, 2), CLEAR);
    assert_eq!(at(2, 2), at(10, 2));
}

#[test]
fn a_font_loaded_in_a_scope_goes_with_its_atlas_and_pages() {
    let Some(mut stage) = Stage::new("text_scope", SIZE) else {
        return;
    };
    let path = stage.write_font("ring", 4, &[RING], 'o', &[]);
    let font = Font::load_now_in(
        &mut stage.server,
        &path,
        Scope::Level,
        &mut FinalizeCtx::new(),
    )
    .expect("the font and its atlas");
    stage.finish();
    let page = |stage: &Stage| {
        stage
            .server
            .assets::<nadir_render::Texture>()
            .and_then(|textures| textures.handle("atlas/ring_0.qoi"))
    };
    assert!(page(&stage).is_some());
    assert_eq!(
        stage.server.unload_scope(Scope::Level),
        3,
        "font, atlas and page"
    );
    assert_eq!(stage.server.state(font), None);
    assert_eq!(page(&stage), None);
}

#[test]
fn at_one_depth_a_text_is_in_front_of_a_sprite_whatever_the_textures() {
    let Some(mut stage) = Stage::new("text_order", SIZE) else {
        return;
    };
    // The font's page is loaded first, so its texture's handle is the
    // lower.
    let font = stage.font("ring", 4, &[RING], 'o', &[]);
    let panel = stage.image("panel.qoi", 1, 1, &[BLUE]);
    stage.finish();
    let mut frame = frame();
    let mut text = text(font, 0.0, Color::rgb(255, 0, 0));
    text.sort = SortMode::Fixed(5);
    frame.texts.push(text, "o");
    let mut sprite = SpriteDraw::new(panel, Vec2::ZERO, Vec2::splat(8.0));
    sprite.pivot = Vec2::ZERO;
    sprite.sort = SortMode::Fixed(5);
    frame.sprites.push(sprite);
    let (_, pixels) = stage.draw(&frame, SIZE);
    assert_eq!(pixels[(2 * SIZE.x + 2) as usize], RED, "the text in front");
    assert_eq!(pixels[(6 * SIZE.x + 6) as usize], BLUE, "the panel behind");
}

#[test]
fn a_page_still_loading_skips_its_glyphs_and_a_failed_one_shows_its_placeholder() {
    let Some(mut stage) = Stage::new("text_pages", SIZE) else {
        return;
    };
    // Still loading: no finish yet.
    let font = stage.font("ring", 4, &[RING], 'o', &[]);
    let mut frame = frame();
    frame
        .texts
        .push(text(font, 0.0, Color::rgb(255, 0, 0)), "oo");
    let (stats, pixels) = stage.draw(&frame, SIZE);
    assert_eq!((stats.texts, stats.glyphs, stats.skipped), (1, 0, 2));
    assert!(pixels.iter().all(|&pixel| pixel == CLEAR));

    // A page that is not an image fails, and shows the placeholder, tinted
    // as a sprite's, with the sprite program: the mask would make it a
    // white block.
    let path = stage.write_font("broken", 4, &[RING], 'o', &[]);
    fs::write(stage.root.join("atlas/broken_0.qoi"), b"not an image").expect("write the file");
    let broken = Font::load_now_in(
        &mut stage.server,
        &path,
        Scope::Global,
        &mut FinalizeCtx::new(),
    )
    .expect("the font and its atlas");
    stage.finish();
    let mut frame = self::frame();
    frame.texts.push(text(broken, 0.0, Color::WHITE), "o");
    let (stats, pixels) = stage.draw(&frame, SIZE);
    assert_eq!((stats.glyphs, stats.skipped), (1, 0));
    // The checker's top-left texel, magenta, over the glyph's quad.
    assert_eq!(pixels[0], MAGENTA);
    assert!(
        pixels[..4]
            .iter()
            .all(|&pixel| pixel == MAGENTA || pixel == BLACK),
        "{:?}",
        &pixels[..4]
    );
}

#[test]
fn a_font_loaded_without_its_atlas_draws_nothing() {
    let Some(mut stage) = Stage::new("text_no_atlas", SIZE) else {
        return;
    };
    let path = stage.write_font("ring", 4, &[RING], 'o', &[]);
    let font = stage
        .server
        .load_now::<Font>(&path, &mut FinalizeCtx::new())
        .expect("the font alone");
    assert_eq!(stage.server.state(font), Some(LoadState::Loaded));
    let mut frame = frame();
    frame.texts.push(text(font, 0.0, Color::WHITE), "o");
    // And a font still loading.
    let late = stage.server.load::<Font>(&path.replace("ring", "late"));
    frame.texts.push(text(late, 0.0, Color::WHITE), "o");
    let (stats, pixels) = stage.draw(&frame, SIZE);
    assert_eq!((stats.texts, stats.skipped_texts, stats.glyphs), (0, 2, 0));
    assert!(pixels.iter().all(|&pixel| pixel == CLEAR));
}
