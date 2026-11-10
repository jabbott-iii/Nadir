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

//! How wide a text is, and where a caret goes in it.

use std::cell::RefCell;

use nadir_render::{Font, PlacedGlyph, TextLayout};

/// The measures of the debug UI's font, in UI units (a UI unit is a pixel
/// of the font).
pub trait TextMetrics {
    /// The height of a line.
    fn line_height(&self) -> f32;

    /// Where the pen is before the character at `byte` of the one-line
    /// `text`, from the text's start: the kerning before that character
    /// included, and spaces taking their room. At `text.len()`, it is the
    /// pen after the last character, the text's width. `byte` is a
    /// character boundary of `text`, or its length.
    fn caret_x(&self, text: &str, byte: usize) -> f32;
}

/// Where the pen is before `byte`, from a text's glyphs in order: each its
/// byte, the pen before it, and its advance.
pub(crate) fn caret_in(glyphs: impl IntoIterator<Item = (usize, f32, f32)>, byte: usize) -> f32 {
    let mut end = 0.0;
    for (start, pen, advance) in glyphs {
        if start >= byte {
            return pen;
        }
        end = pen + advance;
    }
    end
}

/// A [`Font`]'s measures: the text laid out as `Font::layout` lays it out,
/// with no markup, so that a caret is where the glyphs are drawn.
#[derive(Debug)]
pub struct FontMetrics {
    font: Font,
    /// The last text laid out, reused.
    placed: RefCell<Vec<PlacedGlyph>>,
}

impl FontMetrics {
    /// The measures of `font`, a copy of the font the debug UI draws with.
    pub fn new(font: Font) -> Self {
        Self {
            font,
            placed: RefCell::new(Vec::new()),
        }
    }

    /// The font.
    pub fn font(&self) -> &Font {
        &self.font
    }
}

impl TextMetrics for FontMetrics {
    fn line_height(&self) -> f32 {
        self.font.line_height() as f32
    }

    fn caret_x(&self, text: &str, byte: usize) -> f32 {
        let mut layout = TextLayout::new();
        layout.markup = false;
        let mut placed = self.placed.borrow_mut();
        placed.clear();
        self.font.layout(text, &layout, &mut placed);
        // A glyph's rectangle is at the pen plus its offset.
        let glyphs = placed.iter().map(|placed| {
            let pen = placed.position.x - placed.glyph.offset().x;
            (placed.byte, pen as f32, placed.glyph.advance() as f32)
        });
        caret_in(glyphs, byte)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A font for tests: 8 high; a space 3 wide and every other character
    /// 6, `A` then `V` kerned by -1, and the control characters taking no
    /// room, as `Font::layout` places them.
    pub(crate) struct Fixed;

    impl Fixed {
        /// The glyphs of `text`: (byte, pen, advance).
        fn glyphs(text: &str) -> impl Iterator<Item = (usize, f32, f32)> + '_ {
            let mut pen = 0.0;
            let mut previous = None;
            text.char_indices()
                .filter(|(_, c)| !c.is_control())
                .map(move |(byte, c)| {
                    if (previous, c) == (Some('A'), 'V') {
                        pen -= 1.0;
                    }
                    previous = Some(c);
                    let advance = if c == ' ' { 3.0 } else { 6.0 };
                    let glyph = (byte, pen, advance);
                    pen += advance;
                    glyph
                })
        }
    }

    impl TextMetrics for Fixed {
        fn line_height(&self) -> f32 {
            8.0
        }

        fn caret_x(&self, text: &str, byte: usize) -> f32 {
            caret_in(Self::glyphs(text), byte)
        }
    }

    #[test]
    fn a_caret_is_at_the_pen_kerning_included_and_spaces_take_room() {
        let text = "AV b ";
        let at = |byte| Fixed.caret_x(text, byte);
        // A 6, kerned -1 before V, then V 6, a space 3, b 6, a space 3.
        assert_eq!(
            [at(0), at(1), at(2), at(3), at(4), at(5)],
            [0.0, 5.0, 11.0, 14.0, 20.0, 23.0]
        );
        assert_eq!(Fixed.caret_x("", 0), 0.0);
        // Past every glyph: the pen after the last.
        assert_eq!(caret_in([(0, 0.0, 6.0), (2, 6.0, 6.0)], 1), 6.0);
        assert_eq!(caret_in([(0, 0.0, 6.0), (2, 6.0, 6.0)], 9), 12.0);
        // A multi-byte character: the caret after it is at its byte end.
        assert_eq!(Fixed.caret_x("éa", 2), 6.0);
    }
}
