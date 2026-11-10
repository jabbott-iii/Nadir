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

//! What the game asks the renderer to draw in a frame.

use std::fmt::{self, Write};
use std::ops::Range;
use std::time::Duration;

use nadir_assets::Handle;
use nadir_core::{Color, Rect, Vec2};

use crate::{Font, TextLayout, Texture, TileLayerDraw, UiList};

/// The view of the world: what is at the centre of the screen, and how
/// large.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera2D {
    /// The world point at the centre of the view.
    pub position: Vec2,
    /// Virtual pixels per world unit. Pixel-perfect rendering rounds it to
    /// a whole number, at least 1.
    pub zoom: f32,
    /// An offset added to the position, for screen shake.
    pub shake: Vec2,
}

impl Default for Camera2D {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            zoom: 1.0,
            shake: Vec2::ZERO,
        }
    }
}

/// How a sprite is ordered among the others of its layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SortMode {
    /// By its pivot's y: lower on screen is drawn later, in front.
    #[default]
    YSort,
    /// At a fixed depth, for ground decals and the like. `Fixed(0)` is
    /// behind every y-sorted sprite of its layer, and `Fixed(u32::MAX)` in
    /// front of every one.
    Fixed(u32),
}

/// Which way a sprite or a tile is mirrored: a sprite about its pivot
/// ([`SpriteDraw::flip`]), a tile in its cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flip {
    /// Left to right.
    pub x: bool,
    /// Top to bottom.
    pub y: bool,
}

/// One sprite to draw in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteDraw {
    /// The texture, usually an atlas page.
    pub texture: Handle<Texture>,
    /// The region of the texture, normalized: (0, 0) to (1, 1) is all of
    /// it. A texture that failed to load shows all of its placeholder
    /// instead.
    pub uv: Rect,
    /// Where the pivot is in the world.
    pub position: Vec2,
    /// The size in world units.
    pub size: Vec2,
    /// The point of the sprite at `position`, normalized: (0.5, 1) is the
    /// middle of its bottom edge, its feet.
    pub pivot: Vec2,
    /// Radians, clockwise on screen, about the pivot.
    pub rotation: f32,
    /// A tint, sRGB, multiplied with the texture in linear light.
    pub color: Color,
    /// Higher layers are drawn in front of lower ones.
    pub layer: u8,
    /// How it is ordered within its layer.
    pub sort: SortMode,
    /// Which way it is mirrored, about its pivot, so that a trimmed frame
    /// mirrors as its untrimmed cell would: one whose pivot is in the middle
    /// of its width mirrors in place left to right, and one with its pivot
    /// at its feet hangs below them when mirrored top to bottom.
    pub flip: Flip,
}

impl SpriteDraw {
    /// A sprite of all of `texture`, `size` large, with its feet at
    /// `position`: untinted, unrotated, y-sorted in layer 0.
    pub fn new(texture: Handle<Texture>, position: Vec2, size: Vec2) -> Self {
        Self {
            texture,
            uv: Rect::new(Vec2::ZERO, Vec2::ONE),
            position,
            size,
            pivot: Vec2::new(0.5, 1.0),
            rotation: 0.0,
            color: Color::WHITE,
            layer: 0,
            sort: SortMode::YSort,
            flip: Flip::default(),
        }
    }
}

/// How a text's glyphs are colored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GlyphMode {
    /// The glyphs' alpha is a mask, in the text's color: the `text`
    /// program. A page's glyphs may then be any color: only their alpha is
    /// read.
    #[default]
    Mask,
    /// The glyphs' own colors times the text's color, as a sprite's: for a
    /// font whose pages hold colors, such as an outline darker than its
    /// glyphs (BMFont's `alphaChnl=2`).
    Rgba,
}

/// A text to draw in the world, in a [`Font`]: what [`TextList::push`]
/// takes with the text itself.
///
/// Its glyphs are drawn as sprites of its layer at its depth, unrotated,
/// each glyph's corner on a whole virtual pixel: grouped by page, and in
/// the order its characters are laid out on each, so that the glyphs of a
/// font of two pages that overlap may draw out of their order.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextDraw {
    /// The font.
    pub font: Handle<Font>,
    /// Where the pivot is in the world.
    pub position: Vec2,
    /// The point of the text's block at `position`, normalized over the
    /// block [`Font::measure`] gives: (0, 0) is its top-left corner, and
    /// (0.5, 1) the middle of its bottom edge. The block's corner is then
    /// moved by whole pixels, so that its glyphs stay on whole pixels.
    pub pivot: Vec2,
    /// How it is laid out: wrapping, alignment and markup.
    pub layout: TextLayout,
    /// Its color, sRGB, which tints the glyphs; a markup run's color takes
    /// its place, with its alpha times this one's.
    pub color: Color,
    /// Whether the glyphs are a mask in the color, or keep their own.
    pub glyphs: GlyphMode,
    /// Higher layers are drawn in front of lower ones.
    pub layer: u8,
    /// How it is ordered within its layer: [`SortMode::YSort`] by
    /// `position.y`, as a sprite by its pivot. At one depth, the sprites go
    /// first, then the texts in the order they were pushed. A shadow drawn
    /// under its text needs the same depth, pushed first: one pixel lower,
    /// a y-sorted shadow would be in front.
    pub sort: SortMode,
}

impl TextDraw {
    /// A text in `font` with its block's top-left corner at `position`:
    /// laid out with [`TextLayout::new`], white, its glyphs a mask, y-sorted
    /// in layer 0.
    pub fn new(font: Handle<Font>, position: Vec2) -> Self {
        Self {
            font,
            position,
            pivot: Vec2::ZERO,
            layout: TextLayout::new(),
            color: Color::WHITE,
            glyphs: GlyphMode::Mask,
            layer: 0,
            sort: SortMode::YSort,
        }
    }
}

/// The texts of a frame, each a [`TextDraw`] with its text, kept in one
/// buffer of characters, so that a frame that pushes the texts of the last
/// allocates nothing, formatted ones included ([`TextList::write`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextList {
    text: String,
    draws: Vec<(TextDraw, Range<usize>)>,
}

impl TextList {
    /// An empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `text`, drawn as `draw` says.
    pub fn push(&mut self, draw: TextDraw, text: &str) {
        let start = self.text.len();
        self.text.push_str(text);
        self.draws.push((draw, start..self.text.len()));
    }

    /// Adds the text `args` formats, drawn as `draw` says, with no
    /// allocation once the list has held as much:
    /// `texts.write(draw, format_args!("Score {score}"))`. A text whose
    /// formatting fails is left out.
    pub fn write(&mut self, draw: TextDraw, args: fmt::Arguments<'_>) {
        let start = self.text.len();
        if self.text.write_fmt(args).is_err() {
            self.text.truncate(start);
            return;
        }
        self.draws.push((draw, start..self.text.len()));
    }

    /// The texts, in the order they were added.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&TextDraw, &str)> + '_ {
        self.draws
            .iter()
            .map(|(draw, range)| (draw, &self.text[range.clone()]))
    }

    /// The `index`th text, which must be one of the list's.
    pub(crate) fn at(&self, index: usize) -> (&TextDraw, &str) {
        let (draw, range) = &self.draws[index];
        (draw, &self.text[range.clone()])
    }

    /// How many texts there are.
    pub fn len(&self) -> usize {
        self.draws.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.draws.is_empty()
    }

    /// Empties the list, keeping its capacity.
    pub fn clear(&mut self) {
        self.text.clear();
        self.draws.clear();
    }
}

/// Everything the renderer draws in a frame. Keep one and clear it each
/// frame, so that its lists keep their capacity.
///
/// It is `#[non_exhaustive]`, so that the fields that come with later tasks
/// break nothing: make it with [`RenderFrame::new`].
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct RenderFrame {
    /// The view of the world.
    pub camera: Camera2D,
    /// The background, and the color of the bars around an upscaled image.
    pub clear: Color,
    /// The sprites of the world, in any order.
    pub sprites: Vec<SpriteDraw>,
    /// The tile layers of the world, in any order: each in the
    /// [`TileLayers`](crate::TileLayers) the renderer is given.
    pub tile_layers: Vec<TileLayerDraw>,
    /// The texts of the world, in any order.
    pub texts: TextList,
    /// The UI: sprites and texts drawn on the output after the world is
    /// scaled up into it, each in its [`UiView`](crate::UiView)'s scale and
    /// clip, by layer and depth and then in the order they are pushed.
    pub ui: UiList,
    /// The time animated tiles show. The shader reads it in whole
    /// milliseconds, which wrap after 2^32 (49.7 days).
    pub time: Duration,
}

impl RenderFrame {
    /// An empty frame with a black background, at time 0.
    pub fn new() -> Self {
        Self {
            camera: Camera2D::default(),
            clear: Color::BLACK,
            sprites: Vec::new(),
            tile_layers: Vec::new(),
            texts: TextList::new(),
            ui: UiList::new(),
            time: Duration::ZERO,
        }
    }

    /// Empties the lists for the next frame, keeping their capacity, and
    /// puts the UI's view back to the default. The camera, the clear color
    /// and the time stay as they are.
    pub fn clear(&mut self) {
        self.sprites.clear();
        self.tile_layers.clear();
        self.texts.clear();
        self.ui.clear();
    }
}

impl Default for RenderFrame {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use nadir_core::GenIndex;

    use super::*;

    fn draw(x: f32) -> TextDraw {
        let font = Handle::new(GenIndex::new(0, std::num::NonZeroU32::MIN));
        TextDraw::new(font, Vec2::new(x, 0.0))
    }

    #[test]
    fn a_text_list_keeps_each_text_with_its_draw() {
        let mut texts = TextList::new();
        assert!(texts.is_empty());
        texts.push(draw(1.0), "Hi");
        let score = 42;
        texts.write(draw(2.0), format_args!("Score {score}"));
        texts.push(draw(3.0), "");
        assert_eq!(texts.len(), 3);
        let all: Vec<(f32, &str)> = texts.iter().map(|(d, t)| (d.position.x, t)).collect();
        assert_eq!(all, [(1.0, "Hi"), (2.0, "Score 42"), (3.0, "")]);
    }

    #[test]
    fn a_text_whose_formatting_fails_is_left_out() {
        struct Fails;
        impl fmt::Display for Fails {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("half")?;
                Err(fmt::Error)
            }
        }
        let mut texts = TextList::new();
        texts.push(draw(1.0), "a");
        texts.write(draw(2.0), format_args!("{Fails}"));
        texts.push(draw(3.0), "b");
        let all: Vec<&str> = texts.iter().map(|(_, t)| t).collect();
        assert_eq!(all, ["a", "b"]);
    }

    #[test]
    fn a_frame_cleared_and_filled_again_keeps_its_buffers() {
        let mut frame = RenderFrame::new();
        let fill = |frame: &mut RenderFrame| {
            for i in 0..20 {
                frame
                    .texts
                    .write(draw(i as f32), format_args!("line {i} of the frame"));
            }
        };
        fill(&mut frame);
        frame
            .ui
            .set_view(crate::UiView::new(crate::UiScale::Free(2.0)));
        frame.ui.text(draw(0.0), "a");
        let capacity = (frame.texts.text.capacity(), frame.texts.draws.capacity());
        let pointers = (frame.texts.text.as_ptr(), frame.texts.draws.as_ptr());
        frame.clear();
        assert!(frame.texts.is_empty());
        assert!(frame.ui.is_empty());
        assert_eq!(frame.ui.view(), crate::UiView::default());
        fill(&mut frame);
        assert_eq!(
            (frame.texts.text.capacity(), frame.texts.draws.capacity()),
            capacity
        );
        assert_eq!(
            (frame.texts.text.as_ptr(), frame.texts.draws.as_ptr()),
            pointers
        );
    }

    #[test]
    fn a_new_text_is_white_y_sorted_and_a_mask_at_its_top_left() {
        let text = draw(5.0);
        assert_eq!(
            (text.pivot, text.color, text.glyphs),
            (Vec2::ZERO, Color::WHITE, GlyphMode::Mask)
        );
        assert_eq!((text.layer, text.sort), (0, SortMode::YSort));
        assert_eq!(text.layout, TextLayout::new());
    }
}
