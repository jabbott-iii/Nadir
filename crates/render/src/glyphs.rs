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

//! The texts of a frame, laid out and found in their atlases: the glyph
//! quads the renderer plans with its sprites (ADR-0054).
//!
//! A text finds its font's atlas, and the atlas's pages, by their paths in
//! the asset server each frame, so that a reload of any of them is drawn
//! from the next frame, as a texture's is.

use nadir_assets::{AssetServer, Handle, LoadState};
use nadir_core::log::warn;
use nadir_core::{FastHashSet, GenIndex, IVec2, Vec2};

use crate::batcher::Pipe;
use crate::plan::GlyphQuad;
use crate::sort::sortable_bits;
use crate::text::run_color;
use crate::view::snap;
use crate::{
    Atlas, Flip, Font, GlyphMode, PlacedGlyph, SortMode, SpriteDraw, TextDraw, TextList, Texture,
};

/// Whether an asset can be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Found<T> {
    Ready(T),
    /// Still loading.
    Loading,
    /// Not loaded at all: no asset of its type has its path.
    Absent,
    /// It failed, with no placeholder to show: the asset server has said
    /// why.
    Failed,
}

/// Where the texts' fonts, atlases and pages are: the asset server, or a
/// test's own.
pub(crate) trait Library<K> {
    fn font(&self, font: Handle<Font>) -> Found<&Font>;
    /// The font's path, for what is logged.
    fn font_path(&self, font: Handle<Font>) -> &str;
    fn atlas(&self, path: &str) -> Found<&Atlas>;
    /// The page at `path`.
    fn page(&self, path: &str) -> Found<Page<K>>;
}

/// A page found: its handle, its texture, and whether it shows its
/// placeholder.
type Page<K> = (Handle<Texture>, K, bool);

/// What can be wrong with a font for drawing, said once a font.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Problem {
    /// No atlas has the path its font file names.
    NoAtlas,
    /// Its atlas has no sprite of its name, or too few frames.
    Misfit,
    /// No texture has the path of a page of its atlas.
    NoPage,
}

/// The texts of a frame as glyph quads, in lists reused from frame to
/// frame. `K` is a page's texture: the renderer's GPU texture, or a test's
/// number.
#[derive(Debug)]
pub(crate) struct TextPass<K> {
    placed: Vec<PlacedGlyph>,
    /// The pages of the atlas of the text being laid out, by index, once
    /// looked up.
    pages: Vec<Option<Found<Page<K>>>>,
    /// The glyphs drawn, each at its text's layer and depth.
    pub(crate) quads: Vec<GlyphQuad<(K, Pipe)>>,
    /// The texts laid out.
    pub(crate) texts: u32,
    /// The texts not laid out: their font, its atlas or its sprite not
    /// ready, not there, or not the font's.
    pub(crate) skipped_texts: u32,
    /// The glyphs not drawn because their page is not ready.
    pub(crate) skipped_glyphs: u32,
    /// The fonts' problems said.
    warned: FastHashSet<(GenIndex, Problem)>,
    /// The characters a font lacks, said.
    missing: FastHashSet<(GenIndex, char)>,
}

impl<K: Copy> TextPass<K> {
    pub(crate) fn new() -> Self {
        Self {
            placed: Vec::new(),
            pages: Vec::new(),
            quads: Vec::new(),
            texts: 0,
            skipped_texts: 0,
            skipped_glyphs: 0,
            warned: FastHashSet::default(),
            missing: FastHashSet::default(),
        }
    }

    /// Lays out `texts`, each in its font, and finds their glyphs in their
    /// atlases, into `quads`.
    pub(crate) fn build(&mut self, texts: &TextList, library: &impl Library<K>) {
        self.quads.clear();
        self.texts = 0;
        self.skipped_texts = 0;
        self.skipped_glyphs = 0;
        // A frame of more than u32::MAX texts is far past any budget; such
        // texts are not drawn.
        for (index, (draw, text)) in (0..u32::MAX).zip(texts.iter()) {
            if self.add(index, draw, text, library) {
                self.texts += 1;
            } else {
                self.skipped_texts += 1;
            }
        }
    }

    /// Adds the glyphs of the `index`th text, `text` drawn as `draw` says,
    /// or says why it cannot be laid out.
    fn add(&mut self, index: u32, draw: &TextDraw, text: &str, library: &impl Library<K>) -> bool {
        let Found::Ready(font) = library.font(draw.font) else {
            return false;
        };
        let atlas = match library.atlas(font.atlas()) {
            Found::Ready(atlas) => atlas,
            Found::Absent => {
                self.warn(draw.font, Problem::NoAtlas, || {
                    format!(
                        "font `{}`: its atlas `{}` is not loaded, so its texts are not drawn: \
                         load the font with `Font::load_now_in`",
                        library.font_path(draw.font),
                        font.atlas()
                    )
                });
                return false;
            }
            Found::Loading | Found::Failed => return false,
        };
        let sprite = match font.sprite_in(atlas) {
            Ok(sprite) => sprite,
            Err(misfit) => {
                self.warn(draw.font, Problem::Misfit, || {
                    format!(
                        "font `{}`: {}, so its texts are not drawn",
                        library.font_path(draw.font),
                        font.misfit(misfit)
                    )
                });
                return false;
            }
        };

        let Self {
            placed,
            pages,
            quads,
            skipped_glyphs,
            warned,
            missing,
            ..
        } = self;
        placed.clear();
        let size = font.layout(text, &draw.layout, placed);
        // The block's corner, moved by whole pixels from the pivot, so
        // that every glyph's corner is the same fraction of a pixel from a
        // whole one, and they snap alike.
        let origin = draw.position - snap(draw.pivot * size.as_vec2());
        let depth = match draw.sort {
            SortMode::YSort => sortable_bits(draw.position.y),
            SortMode::Fixed(depth) => depth,
        };
        pages.clear();
        pages.resize(atlas.pages().len(), None);
        for glyph in placed.iter() {
            if glyph.missing && missing.insert((draw.font.id(), glyph.character)) {
                warn!(
                    "font `{}` has no glyph for {:?} (U+{:04X}): its texts draw its fallback",
                    library.font_path(draw.font),
                    glyph.character,
                    glyph.character as u32
                );
            }
            // `sprite_in` found every frame the glyphs name, and the codec
            // that each frame lies on a page.
            let Some(frame) = glyph.glyph.frame().and_then(|frame| sprite.frame(frame)) else {
                continue;
            };
            let (Some(page), Some(slot)) =
                (atlas.pages().get(frame.page()), pages.get_mut(frame.page()))
            else {
                continue;
            };
            if frame.is_empty() {
                continue;
            }
            let found = *slot.get_or_insert_with(|| library.page(page.path()));
            let (handle, texture, placeholder) = match found {
                Found::Ready(page) => page,
                Found::Absent => {
                    *skipped_glyphs += 1;
                    if warned.insert((draw.font.id(), Problem::NoPage)) {
                        warn!(
                            "font `{}`: its atlas's page `{}` is not loaded, so its glyphs \
                             there are not drawn: load the font with `Font::load_now_in`",
                            library.font_path(draw.font),
                            page.path()
                        );
                    }
                    continue;
                }
                Found::Loading | Found::Failed => {
                    *skipped_glyphs += 1;
                    continue;
                }
            };
            let trim = frame.offset();
            let pixels = IVec2::new(
                glyph.position.x.saturating_add(trim.x as i32),
                glyph.position.y.saturating_add(trim.y as i32),
            );
            // A page that failed shows its placeholder, which the mask
            // would make a solid block of the text's color.
            let pipe = if placeholder || draw.glyphs == GlyphMode::Rgba {
                Pipe::Sprite
            } else {
                Pipe::Text
            };
            quads.push(GlyphQuad {
                sprite: SpriteDraw {
                    texture: handle,
                    uv: frame.uv(),
                    position: origin + pixels.as_vec2(),
                    size: frame.size().as_vec2(),
                    pivot: Vec2::ZERO,
                    rotation: 0.0,
                    color: run_color(draw.color, glyph.run),
                    layer: draw.layer,
                    sort: SortMode::Fixed(depth),
                    flip: Flip::default(),
                },
                text: index,
                texture: (texture, pipe),
                placeholder,
            });
        }
        true
    }

    /// Logs `message` the first time `font` has `problem`.
    fn warn(&mut self, font: Handle<Font>, problem: Problem, message: impl FnOnce() -> String) {
        if self.warned.insert((font.id(), problem)) {
            warn!("{}", message());
        }
    }
}

/// The asset server, as the texts' library.
pub(crate) struct Server<'a>(pub(crate) &'a AssetServer);

impl Library<nadir_gfx::TextureId> for Server<'_> {
    fn font(&self, font: Handle<Font>) -> Found<&Font> {
        let Some(fonts) = self.0.assets::<Font>() else {
            return Found::Absent;
        };
        match (fonts.get(font), fonts.state(font)) {
            (Some(font), _) => Found::Ready(font),
            (None, Some(LoadState::Loading)) => Found::Loading,
            (None, Some(_)) => Found::Failed,
            (None, None) => Found::Absent,
        }
    }

    fn font_path(&self, font: Handle<Font>) -> &str {
        self.0.path(font).unwrap_or("(unknown)")
    }

    fn atlas(&self, path: &str) -> Found<&Atlas> {
        let Some(atlases) = self.0.assets::<Atlas>() else {
            return Found::Absent;
        };
        let Some(atlas) = atlases.handle(path) else {
            return Found::Absent;
        };
        match (atlases.get(atlas), atlases.state(atlas)) {
            (Some(atlas), _) => Found::Ready(atlas),
            (None, Some(LoadState::Loading)) => Found::Loading,
            (None, _) => Found::Failed,
        }
    }

    fn page(&self, path: &str) -> Found<Page<nadir_gfx::TextureId>> {
        let Some(textures) = self.0.assets::<Texture>() else {
            return Found::Absent;
        };
        let Some(page) = textures.handle(path) else {
            return Found::Absent;
        };
        let state = textures.state(page);
        match textures.get(page) {
            Some(texture) => Found::Ready((page, texture.id(), state == Some(LoadState::Failed))),
            None if state == Some(LoadState::Loading) => Found::Loading,
            None => Found::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use nadir_core::{Color, Rect, log};

    use super::*;
    use crate::font::tests::{FONT, decoded, kbj};

    fn handle<T>(index: u32) -> Handle<T> {
        Handle::new(GenIndex::new(index, NonZeroU32::MIN))
    }

    /// The frame of i in [`atlas`]: trimmed a pixel from its left.
    const I_FRAME: &str =
        r#"{ "page": 0, "x": 8, "y": 0, "w": 1, "h": 4, "ox": 1, "oy": 0, "sw": 2, "sh": 4 }"#;

    /// An atlas of two pages, 16 × 8 each, whose sprite `fonts/tiny` has
    /// the test font's four frames: A, V and i (`i_frame`) on page 0, the
    /// fallback on page 1, each trimmed a pixel from its left but the
    /// fallback.
    fn atlas(i_frame: &str) -> Atlas {
        use nadir_assets::{Asset, DecodeCtx};
        let text = format!(
            r#"{{ "version": 2,
            "pages": [ {{ "path": "atlas/fonts_0.qoi", "w": 16, "h": 8 }},
                       {{ "path": "atlas/fonts_1.qoi", "w": 16, "h": 8 }} ],
            "sprites": [ {{ "name": "fonts/tiny", "pivot": [0.0, 0.0], "frames": [
                {{ "page": 0, "x": 0, "y": 0, "w": 2, "h": 4, "ox": 1, "oy": 0, "sw": 3, "sh": 4 }},
                {{ "page": 0, "x": 4, "y": 0, "w": 2, "h": 4, "ox": 1, "oy": 0, "sw": 3, "sh": 4 }},
                {i_frame},
                {{ "page": 1, "x": 0, "y": 0, "w": 3, "h": 5, "ox": 0, "oy": 0, "sw": 3, "sh": 5 }} ] }} ] }}"#
        );
        Atlas::decode(
            &kbj(&text),
            &DecodeCtx::new("atlas/fonts.kbj", nadir_codec::Limits::default()),
        )
        .expect("an atlas")
    }

    /// A library of one font (handle 1, at `path`), its atlas if any, and
    /// each page's state: page `n` is texture `n`.
    struct Shelf {
        path: &'static str,
        font: Font,
        atlas: Found<Atlas>,
        pages: [Found<bool>; 2],
    }

    impl Shelf {
        fn new() -> Self {
            Self::at("atlas/fonts/tiny.font.kbj")
        }

        /// A shelf whose font is at `path`: a path of its test's own, so
        /// that its records in the log are its own.
        fn at(path: &'static str) -> Self {
            Self {
                path,
                font: decoded(FONT).expect("a font"),
                atlas: Found::Ready(atlas(I_FRAME)),
                pages: [Found::Ready(false), Found::Ready(false)],
            }
        }
    }

    /// The records in the log that hold every one of `parts`.
    fn logged(parts: &[&str]) -> usize {
        log::tail(log::RING_CAPACITY)
            .iter()
            .filter(|record| parts.iter().all(|part| record.contains(part)))
            .count()
    }

    impl Library<u32> for Shelf {
        fn font(&self, font: Handle<Font>) -> Found<&Font> {
            if font.id().index() == 1 {
                Found::Ready(&self.font)
            } else {
                Found::Loading
            }
        }
        fn font_path(&self, _: Handle<Font>) -> &str {
            self.path
        }
        fn atlas(&self, path: &str) -> Found<&Atlas> {
            assert_eq!(path, "atlas/fonts.kbj");
            match &self.atlas {
                Found::Ready(atlas) => Found::Ready(atlas),
                Found::Loading => Found::Loading,
                Found::Absent => Found::Absent,
                Found::Failed => Found::Failed,
            }
        }
        fn page(&self, path: &str) -> Found<Page<u32>> {
            let index = match path {
                "atlas/fonts_0.qoi" => 0,
                "atlas/fonts_1.qoi" => 1,
                _ => panic!("{path}"),
            };
            match self.pages[index] {
                Found::Ready(placeholder) => {
                    Found::Ready((handle(10 + index as u32), index as u32, placeholder))
                }
                Found::Loading => Found::Loading,
                Found::Absent => Found::Absent,
                Found::Failed => Found::Failed,
            }
        }
    }

    fn text(position: Vec2, text: &str) -> TextList {
        let mut texts = TextList::new();
        texts.push(TextDraw::new(handle(1), position), text);
        texts
    }

    #[test]
    fn each_glyph_is_its_frame_at_the_pen_its_offsets_and_its_trim() {
        let shelf = Shelf::at("atlas/fonts/each_glyph.font.kbj");
        let mut pass = TextPass::new();
        pass.build(&text(Vec2::new(100.0, 50.0), "AiB"), &shelf);
        assert_eq!(
            (pass.texts, pass.skipped_texts, pass.skipped_glyphs),
            (1, 0, 0)
        );
        let quads: Vec<(Vec2, Vec2, (u32, Pipe))> = pass
            .quads
            .iter()
            .map(|quad| (quad.sprite.position, quad.sprite.size, quad.texture))
            .collect();
        assert_eq!(
            quads,
            [
                // A: offset (0, 1), trimmed 1 from the left.
                (Vec2::new(101.0, 51.0), Vec2::new(2.0, 4.0), (0, Pipe::Text)),
                // i: the pen at 4, offset (1, 1), trimmed 1.
                (Vec2::new(106.0, 51.0), Vec2::new(1.0, 4.0), (0, Pipe::Text)),
                // B is missing: the fallback, on page 1, offset (0, 0).
                (Vec2::new(107.0, 50.0), Vec2::new(3.0, 5.0), (1, Pipe::Text)),
            ]
        );
        let a = pass.quads[0].sprite;
        assert_eq!(
            a.uv,
            Rect::new(Vec2::ZERO, Vec2::new(2.0 / 16.0, 4.0 / 8.0))
        );
        assert_eq!(
            (a.pivot, a.rotation, a.flip),
            (Vec2::ZERO, 0.0, Flip::default())
        );
        assert_eq!(a.texture, handle(10));
        assert_eq!(pass.quads[2].sprite.texture, handle(11));
        // B, said once, however many times it is drawn.
        pass.build(&text(Vec2::ZERO, "BB"), &shelf);
        pass.build(&text(Vec2::ZERO, "B"), &shelf);
        assert_eq!(logged(&[shelf.path, "no glyph for 'B' (U+0042)"]), 1);
    }

    #[test]
    fn a_glyph_is_at_its_text_s_layer_and_depth() {
        let mut texts = TextList::new();
        let mut draw = TextDraw::new(handle(1), Vec2::new(0.0, 30.5));
        draw.layer = 3;
        texts.push(draw, "AV");
        draw.sort = SortMode::Fixed(9);
        texts.push(draw, "A");
        let mut pass = TextPass::new();
        pass.build(&texts, &Shelf::new());
        let keys: Vec<(u8, SortMode, u32)> = pass
            .quads
            .iter()
            .map(|quad| (quad.sprite.layer, quad.sprite.sort, quad.text))
            .collect();
        let y = SortMode::Fixed(sortable_bits(30.5));
        assert_eq!(keys, [(3, y, 0), (3, y, 0), (3, SortMode::Fixed(9), 1)]);
        // By its position, its pivot, not its block's corner.
        let mut texts = TextList::new();
        let mut draw = TextDraw::new(handle(1), Vec2::new(0.0, 30.0));
        draw.pivot = Vec2::new(0.0, 1.0);
        texts.push(draw, "A");
        pass.build(&texts, &Shelf::new());
        assert_eq!(
            pass.quads[0].sprite.sort,
            SortMode::Fixed(sortable_bits(30.0))
        );
    }

    #[test]
    fn the_pivot_moves_the_block_by_whole_pixels() {
        // "AiA" is 11 wide and 6 high: its middle is 5.5 in, which goes to
        // 6, and its bottom 6 up.
        let mut texts = TextList::new();
        let mut draw = TextDraw::new(handle(1), Vec2::new(50.25, 20.0));
        draw.pivot = Vec2::new(0.5, 1.0);
        texts.push(draw, "AiA");
        let mut pass = TextPass::new();
        pass.build(&texts, &Shelf::new());
        // A's pixels: offset (0, 1) and its trim of 1.
        assert_eq!(
            pass.quads[0].sprite.position,
            Vec2::new(50.25 - 6.0 + 1.0, 20.0 - 6.0 + 1.0)
        );
    }

    #[test]
    fn a_run_s_color_and_the_text_s_mode_reach_the_glyphs() {
        let mut texts = TextList::new();
        let mut draw = TextDraw::new(handle(1), Vec2::ZERO);
        draw.color = Color::rgba(255, 255, 255, 128);
        texts.push(draw, "A{c:#00f}V");
        draw.glyphs = GlyphMode::Rgba;
        texts.push(draw, "A");
        let mut pass = TextPass::new();
        pass.build(&texts, &Shelf::new());
        let colors: Vec<Color> = pass.quads.iter().map(|quad| quad.sprite.color).collect();
        let half = Color::rgba(255, 255, 255, 128);
        assert_eq!(colors, [half, Color::rgba(0, 0, 255, 128), half]);
        assert_eq!(pass.quads[2].texture, (0, Pipe::Sprite));
    }

    #[test]
    fn a_font_or_atlas_not_ready_skips_the_text_and_an_absent_atlas_is_said_once() {
        let mut texts = text(Vec2::ZERO, "A");
        texts.push(TextDraw::new(handle(2), Vec2::ZERO), "A");
        let mut shelf = Shelf::at("atlas/fonts/no_atlas.font.kbj");
        let mut pass = TextPass::new();
        pass.build(&texts, &shelf);
        assert_eq!(
            (pass.texts, pass.skipped_texts, pass.quads.len()),
            (1, 1, 1),
            "font 2 loading"
        );
        for atlas in [Found::Loading, Found::Failed, Found::Absent] {
            shelf.atlas = atlas;
            pass.build(&texts, &shelf);
            assert_eq!(
                (pass.texts, pass.skipped_texts, pass.quads.len()),
                (0, 2, 0)
            );
        }
        pass.build(&texts, &shelf);
        assert_eq!(
            logged(&[shelf.path, "atlas `atlas/fonts.kbj` is not loaded"]),
            1
        );
        assert!(
            pass.warned
                .contains(&(GenIndex::new(1, NonZeroU32::MIN), Problem::NoAtlas))
        );
    }

    #[test]
    fn a_font_its_atlas_does_not_fit_is_skipped_and_said_once() {
        let mut shelf = Shelf::at("atlas/fonts/misfit.font.kbj");
        shelf.font = decoded(&FONT.replacen("fonts/tiny", "fonts/other", 1)).expect("a font");
        let mut pass = TextPass::new();
        for _ in 0..3 {
            pass.build(&text(Vec2::ZERO, "A"), &shelf);
            assert_eq!((pass.texts, pass.skipped_texts), (0, 1));
        }
        assert_eq!(logged(&[shelf.path, "no sprite `fonts/other`"]), 1);
        // Too few frames: a sprite of one, and the glyphs name up to 3.
        use nadir_assets::{Asset, DecodeCtx};
        let one = r#"{ "version": 2,
            "pages": [ { "path": "atlas/fonts_0.qoi", "w": 16, "h": 8 } ],
            "sprites": [ { "name": "fonts/tiny", "pivot": [0.0, 0.0], "frames": [
                { "page": 0, "x": 0, "y": 0, "w": 2, "h": 4, "ox": 1, "oy": 0, "sw": 3, "sh": 4 } ] } ] }"#;
        let short = Atlas::decode(
            &kbj(one),
            &DecodeCtx::new("atlas/fonts.kbj", nadir_codec::Limits::default()),
        )
        .expect("an atlas");
        let mut shelf = Shelf::at("atlas/fonts/too_few.font.kbj");
        shelf.atlas = Found::Ready(short);
        let mut pass = TextPass::new();
        for _ in 0..3 {
            pass.build(&text(Vec2::ZERO, "A"), &shelf);
            assert_eq!((pass.texts, pass.skipped_texts), (0, 1));
        }
        assert_eq!(logged(&[shelf.path, "name frame 3", "which has 1"]), 1);
    }

    #[test]
    fn glyphs_on_a_page_not_ready_are_skipped_and_a_failed_one_shows_its_placeholder() {
        let mut shelf = Shelf::at("atlas/fonts/pages.font.kbj");
        let mut pass = TextPass::new();
        for (state, skipped) in [(Found::Loading, 1), (Found::Failed, 1), (Found::Absent, 1)] {
            shelf.pages[1] = state;
            pass.build(&text(Vec2::ZERO, "AB"), &shelf);
            assert_eq!(
                (pass.texts, pass.skipped_glyphs, pass.quads.len()),
                (1, skipped, 1)
            );
        }
        pass.build(&text(Vec2::ZERO, "AB"), &shelf);
        let page = "page `atlas/fonts_1.qoi` is not loaded";
        assert_eq!(logged(&[shelf.path, page]), 1, "the absent page, once");
        // A placeholder draws with the sprite program, a mask or not.
        shelf.pages[1] = Found::Ready(true);
        pass.build(&text(Vec2::ZERO, "AB"), &shelf);
        assert_eq!(pass.quads[1].texture, (1, Pipe::Sprite));
        assert!(pass.quads[1].placeholder);
        assert_eq!(pass.quads[0].texture, (0, Pipe::Text));
    }

    #[test]
    fn a_glyph_trimmed_to_nothing_places_no_quad() {
        let mut shelf = Shelf::new();
        let empty =
            r#"{ "page": 0, "x": 0, "y": 0, "w": 0, "h": 0, "ox": 0, "oy": 0, "sw": 2, "sh": 4 }"#;
        shelf.atlas = Found::Ready(atlas(empty));
        let mut pass = TextPass::new();
        pass.build(&text(Vec2::ZERO, "Ai"), &shelf);
        assert_eq!(
            (pass.texts, pass.quads.len(), pass.skipped_glyphs),
            (1, 1, 0)
        );
    }

    #[test]
    fn a_space_places_no_quad_and_the_lists_are_reused() {
        let mut pass = TextPass::new();
        let texts = text(Vec2::ZERO, "A A\nA");
        pass.build(&texts, &Shelf::new());
        assert_eq!(pass.quads.len(), 3);
        let capacity = (
            pass.quads.capacity(),
            pass.placed.capacity(),
            pass.pages.capacity(),
        );
        let pointer = pass.quads.as_ptr();
        pass.build(&texts, &Shelf::new());
        assert_eq!(
            (
                pass.quads.capacity(),
                pass.placed.capacity(),
                pass.pages.capacity()
            ),
            capacity
        );
        assert_eq!(pass.quads.as_ptr(), pointer);
    }
}
