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

//! Laying text out in a [`Font`]: lines, wrapping, alignment, kerning and
//! the color markup (`docs/subsystems/render.md`, "Text").
//!
//! Every position is a whole number of the font's pixels, from the top-left
//! corner of the text's block. A line is scanned once to find where it
//! ends, and again to place its glyphs, so that measuring and laying out
//! agree and neither needs a list of its own.

use std::fmt;

use nadir_core::{Color, IVec2, UVec2};

use crate::{Font, Glyph};

/// How a text is laid out.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextLayout {
    /// The widest a line may be, in the font's pixels, before it wraps;
    /// `None` wraps only at `\n`.
    ///
    /// A line breaks at its last run of spaces before a character that
    /// would pass it, and the spaces of that run are dropped. A word wider
    /// than the width alone breaks between its characters, with one at
    /// least on each line, so a line may still be wider.
    pub max_width: Option<u32>,
    /// Where each line goes across the block.
    pub align: Align,
    /// Whether the color markup is read: `{c:#ff0}` starts a run of a
    /// color, `{/c}` ends it, and `{{` is a `{`. Off, every character is
    /// drawn as it is.
    pub markup: bool,
}

impl TextLayout {
    /// No wrapping, left aligned, with markup.
    pub const fn new() -> Self {
        Self {
            max_width: None,
            align: Align::Left,
            markup: true,
        }
    }
}

impl Default for TextLayout {
    fn default() -> Self {
        Self::new()
    }
}

/// Where the lines of a text go across its block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    /// Each line starts at the block's left edge.
    #[default]
    Left,
    /// Each line is in the middle, the odd pixel to its right.
    Center,
    /// Each line ends at the block's right edge.
    Right,
}

/// A character of a text, laid out: which glyph it draws, and where.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacedGlyph {
    /// The character, as the text has it.
    pub character: char,
    /// Where it starts in the text, in bytes.
    pub byte: usize,
    /// The glyph it draws: its font's, its font's space for a no-break
    /// space (U+00A0) the font lacks, or the fallback.
    pub glyph: Glyph,
    /// Where the glyph's rectangle's top-left corner is, in the font's
    /// pixels from the block's top-left corner.
    pub position: IVec2,
    /// The color of the markup run it is in, if any.
    pub run: Option<Color>,
    /// Whether the font lacks the character, so that it draws the fallback.
    pub missing: bool,
    /// Its line, from 0 at the top.
    pub line: u32,
}

/// `text` with each `{` doubled, so that markup draws it as it is: for a
/// name a player typed, say, in a text that has markup of its own.
///
/// ```
/// use nadir_render::Escaped;
///
/// assert_eq!(Escaped("{c:#f00}").to_string(), "{{c:#f00}");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Escaped<'a>(pub &'a str);

impl fmt::Display for Escaped<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for piece in self.0.split_inclusive('{') {
            f.write_str(piece)?;
            if piece.ends_with('{') {
                f.write_str("{")?;
            }
        }
        Ok(())
    }
}

impl Font {
    /// The size of `text`'s block as `layout` lays it out, in the font's
    /// pixels: as wide as its widest line, or `layout.max_width` if that is
    /// wider, and a line height for each line. A text has one line, and one
    /// more for each `\n` and each wrap: `""` is one line of width 0.
    ///
    /// The block is the pen's: a glyph may reach past it by its offsets.
    pub fn measure(&self, text: &str, layout: &TextLayout) -> UVec2 {
        let (widest, lines) = self.lines(text, layout);
        self.block(layout, widest, lines)
    }

    /// Lays `text` out as `layout` says, appending a [`PlacedGlyph`] to
    /// `out` for each character drawn, spaces too, in the text's order, and
    /// returns the block's size, as [`Font::measure`] does.
    ///
    /// - `\n` breaks a line, and a `\r` takes no room. A control character
    ///   the font has no glyph for takes no room either, and places
    ///   nothing.
    /// - The pen starts each line at 0. Each glyph's rectangle is placed at
    ///   the pen plus its [`Glyph::offset`], and the pen then moves by its
    ///   advance. Before a glyph, the pen moves by the kerning of the
    ///   glyph before it on the line and this one ([`Font::kerning`]).
    /// - A character the font lacks draws its fallback, with no kerning on
    ///   either side, and is `missing`.
    /// - A line's width is the pen after its last character that is not a
    ///   space, at least 0: spaces at a line's end take no room.
    /// - With markup, a tag places nothing, takes no room, and kerns
    ///   through: `A{c:#f00}V` kerns as `AV`. A run lasts across lines.
    pub fn layout(&self, text: &str, layout: &TextLayout, out: &mut Vec<PlacedGlyph>) -> UVec2 {
        let widest = if layout.align == Align::Left {
            0
        } else {
            self.lines(text, layout).0
        };
        let width = i64::from(layout.max_width.unwrap_or(0)).max(widest);
        let mut start = 0;
        let mut lines: u32 = 0;
        let mut widest_seen = 0;
        let mut run = None;
        loop {
            let line = self.line(text, start, layout);
            widest_seen = widest_seen.max(line.width);
            let offset = match layout.align {
                Align::Left => 0,
                Align::Center => (width - line.width) / 2,
                Align::Right => width - line.width,
            };
            let at = Placement {
                start,
                line,
                left: offset,
                top: i64::from(lines) * i64::from(self.line_height()),
                index: lines,
            };
            self.place(text, layout, &at, &mut run, out);
            lines = lines.saturating_add(1);
            if line.last {
                break;
            }
            start = line.next;
        }
        self.block(layout, widest_seen, lines)
    }

    /// The widest line of `text`, and how many lines it has.
    fn lines(&self, text: &str, layout: &TextLayout) -> (i64, u32) {
        let mut start = 0;
        let mut widest = 0;
        let mut lines: u32 = 0;
        loop {
            let line = self.line(text, start, layout);
            widest = widest.max(line.width);
            lines = lines.saturating_add(1);
            if line.last {
                return (widest, lines);
            }
            start = line.next;
        }
    }

    /// The block of `lines` lines whose widest is `widest`.
    fn block(&self, layout: &TextLayout, widest: i64, lines: u32) -> UVec2 {
        let width = i64::from(layout.max_width.unwrap_or(0)).max(widest);
        let height = i64::from(lines) * i64::from(self.line_height());
        UVec2::new(saturate_u32(width), saturate_u32(height))
    }

    /// Finds where the line that starts at byte `start` of `text` ends.
    fn line(&self, text: &str, start: usize, layout: &TextLayout) -> Line {
        let max = layout.max_width.map(i64::from);
        let mut pen: i64 = 0;
        let mut width: i64 = 0;
        let mut previous: Option<char> = None;
        let mut placed_word = false;
        // The run of spaces being read, after a word: its first byte, and
        // the line's width before it.
        let mut spaces: Option<(usize, i64)> = None;
        // The last run of spaces after a word that a word followed: where
        // the line ends if it breaks there, its width then, and where the
        // next line starts.
        let mut opening: Option<(usize, i64, usize)> = None;
        let mut tokens = Tokens::new(text, start, layout.markup);
        while let Some((byte, token)) = tokens.next() {
            let Token::Char(character) = token else {
                continue;
            };
            let (glyph, kerns, drawn) = match self.classify(character) {
                Kind::Break => {
                    return Line {
                        end: byte,
                        next: tokens.at,
                        width: width.max(0),
                        last: false,
                    };
                }
                Kind::Nothing => continue,
                Kind::Glyph {
                    glyph,
                    missing,
                    character: drawn,
                } => (glyph, !missing, drawn),
            };
            let kerning = match previous {
                Some(before) if kerns => i64::from(self.kerning(before, drawn)),
                _ => 0,
            };
            let right = pen + kerning + i64::from(glyph.advance());
            if character == ' ' {
                if placed_word && spaces.is_none() {
                    spaces = Some((byte, width));
                }
            } else {
                if let Some((first, before)) = spaces.take() {
                    opening = Some((first, before, byte));
                }
                if let Some(max) = max
                    && right > max
                    && placed_word
                {
                    return match opening {
                        Some((end, before, next)) => Line {
                            end,
                            next,
                            width: before.max(0),
                            last: false,
                        },
                        None => Line {
                            end: byte,
                            next: byte,
                            width: width.max(0),
                            last: false,
                        },
                    };
                }
                width = right;
                placed_word = true;
            }
            pen = right;
            previous = kerns.then_some(drawn);
        }
        Line {
            end: text.len(),
            next: text.len(),
            width: width.max(0),
            last: true,
        }
    }

    /// Places the glyphs of a line, as `at` says, reading the markup through
    /// to the next line's start, so that `run` is the run there.
    fn place(
        &self,
        text: &str,
        layout: &TextLayout,
        at: &Placement,
        run: &mut Option<Color>,
        out: &mut Vec<PlacedGlyph>,
    ) {
        let Placement {
            start,
            line,
            left,
            top,
            index,
        } = *at;
        let mut pen: i64 = 0;
        let mut previous: Option<char> = None;
        for (byte, token) in Tokens::new(text, start, layout.markup) {
            if byte >= line.next {
                break;
            }
            let character = match token {
                Token::Color(color) => {
                    *run = Some(color);
                    continue;
                }
                Token::End => {
                    *run = None;
                    continue;
                }
                Token::Char(character) => character,
            };
            if byte >= line.end {
                continue;
            }
            let Kind::Glyph {
                glyph,
                missing,
                character: drawn,
            } = self.classify(character)
            else {
                continue;
            };
            let kerns = !missing;
            if let Some(before) = previous
                && kerns
            {
                pen += i64::from(self.kerning(before, drawn));
            }
            let position = IVec2::new(
                saturate_i32(left + pen + i64::from(glyph.offset().x)),
                saturate_i32(top + i64::from(glyph.offset().y)),
            );
            out.push(PlacedGlyph {
                character,
                byte,
                glyph,
                position,
                run: *run,
                missing,
                line: index,
            });
            pen += i64::from(glyph.advance());
            previous = kerns.then_some(drawn);
        }
    }

    /// What `character` is to the layout.
    fn classify(&self, character: char) -> Kind {
        match character {
            '\n' => return Kind::Break,
            '\r' => return Kind::Nothing,
            _ => {}
        }
        if let Some(glyph) = self.glyph(character) {
            return Kind::Glyph {
                glyph,
                missing: false,
                character,
            };
        }
        if character.is_control() {
            return Kind::Nothing;
        }
        if character == '\u{a0}'
            && let Some(glyph) = self.glyph(' ')
        {
            return Kind::Glyph {
                glyph,
                missing: false,
                character: ' ',
            };
        }
        Kind::Glyph {
            glyph: self.fallback(),
            missing: true,
            character,
        }
    }
}

/// The color a glyph of a markup run is drawn in, in a text of `color`:
/// the run's red, green and blue, and its alpha times the text's.
pub(crate) fn run_color(color: Color, run: Option<Color>) -> Color {
    match run {
        None => color,
        Some(run) => {
            let alpha = (u16::from(run.a) * u16::from(color.a) + 127) / 255;
            Color::rgba(run.r, run.g, run.b, alpha as u8)
        }
    }
}

/// Where a line ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Line {
    /// The byte its characters end at: the `\n`, the run of spaces it broke
    /// at, the character it broke before, or the text's end.
    end: usize,
    /// The byte the next line starts at.
    next: usize,
    /// Its width in pixels, at least 0.
    width: i64,
    /// Whether it ends the text.
    last: bool,
}

/// Where a line's glyphs go.
#[derive(Clone, Copy, Debug)]
struct Placement {
    /// The byte the line starts at.
    start: usize,
    line: Line,
    /// The pen's origin, from the block's top-left corner.
    left: i64,
    top: i64,
    /// The line's index, from 0 at the top.
    index: u32,
}

/// What a character is to the layout.
enum Kind {
    /// A `\n`.
    Break,
    /// Nothing: no glyph, no room.
    Nothing,
    /// A glyph: the font's, or the fallback when the font lacks the
    /// character (`missing`). `character` is the one it kerns as.
    Glyph {
        glyph: Glyph,
        missing: bool,
        character: char,
    },
}

/// A piece of a text: a character, or a tag of its markup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    Char(char),
    /// `{c:#…}`: a run of this color starts.
    Color(Color),
    /// `{/c}`: the run ends.
    End,
}

/// The tokens of a text from a byte on, each with the byte it starts at.
struct Tokens<'a> {
    text: &'a str,
    /// The byte the next token starts at.
    at: usize,
    markup: bool,
}

impl<'a> Tokens<'a> {
    fn new(text: &'a str, at: usize, markup: bool) -> Self {
        Self { text, at, markup }
    }
}

impl Iterator for Tokens<'_> {
    type Item = (usize, Token);

    fn next(&mut self) -> Option<(usize, Token)> {
        let start = self.at;
        let rest = self.text.get(start..)?;
        let character = rest.chars().next()?;
        if self.markup && character == '{' {
            let tag = if rest.starts_with("{{") {
                Some((Token::Char('{'), 2))
            } else if rest.starts_with("{/c}") {
                Some((Token::End, 4))
            } else {
                color_tag(rest).map(|(color, length)| (Token::Color(color), length))
            };
            if let Some((token, length)) = tag {
                self.at += length;
                return Some((start, token));
            }
        }
        self.at += character.len_utf8();
        Some((start, Token::Char(character)))
    }
}

/// The color of the tag `rest` starts with, `{c:#` then 3, 4, 6 or 8 hex
/// digits then `}`, and the tag's length in bytes. Three or four digits are
/// each one channel's twice (`#f80` is `#ff8800`); with no alpha, alpha is
/// 255.
fn color_tag(rest: &str) -> Option<(Color, usize)> {
    let body = rest.strip_prefix("{c:#")?;
    let digits = body.bytes().take_while(u8::is_ascii_hexdigit).count();
    if !matches!(digits, 3 | 4 | 6 | 8) || body.as_bytes().get(digits) != Some(&b'}') {
        return None;
    }
    let hex = body.as_bytes();
    let nibble = |i: usize| (hex[i] as char).to_digit(16).unwrap_or(0) as u8;
    let channel = |i: usize| {
        if digits <= 4 {
            nibble(i) * 17
        } else {
            nibble(2 * i) * 16 + nibble(2 * i + 1)
        }
    };
    let alpha = if digits == 4 || digits == 8 {
        channel(3)
    } else {
        255
    };
    let color = Color::rgba(channel(0), channel(1), channel(2), alpha);
    Some((color, "{c:#".len() + digits + 1))
}

/// `value` as a `u32`, at most `u32::MAX` and at least 0.
fn saturate_u32(value: i64) -> u32 {
    value.clamp(0, i64::from(u32::MAX)) as u32
}

/// `value` as an `i32`, within the `i32` range.
fn saturate_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use nadir_core::testing::check;

    use super::*;
    use crate::font::tests::{FONT, decoded};

    /// The tiny test font: A and V 4 wide, kerned -1 together; i 3 wide,
    /// placed 1 in; a space 2 wide; the fallback 4 wide; lines 6 high. It
    /// kerns A and B, and B and V, though it lacks B.
    fn font() -> Font {
        decoded(FONT).expect("a font")
    }

    /// The characters placed, with their x and line, and the block.
    fn placed(text: &str, layout: TextLayout) -> (Vec<(char, i32, u32)>, UVec2) {
        let font = font();
        let mut out = Vec::new();
        let size = font.layout(text, &layout, &mut out);
        assert_eq!(size, font.measure(text, &layout), "{text:?}");
        let placed = out
            .iter()
            .map(|glyph| (glyph.character, glyph.position.x, glyph.line))
            .collect();
        (placed, size)
    }

    fn wrapped(width: u32) -> TextLayout {
        TextLayout {
            max_width: Some(width),
            ..TextLayout::new()
        }
    }

    fn aligned(align: Align, max_width: Option<u32>) -> TextLayout {
        TextLayout {
            max_width,
            align,
            ..TextLayout::new()
        }
    }

    #[test]
    fn by_default_it_reads_markup_and_neither_wraps_nor_aligns() {
        let layout = TextLayout::default();
        assert_eq!(layout, TextLayout::new());
        assert!(layout.markup);
        assert_eq!((layout.max_width, layout.align), (None, Align::Left));
    }

    #[test]
    fn kerning_moves_the_second_glyph_and_never_crosses_a_line() {
        // A at 0, then V at 4 - 1: the pair's kerning adds to A's advance.
        let (glyphs, size) = placed("AV", TextLayout::new());
        assert_eq!(glyphs, [('A', 0, 0), ('V', 3, 0)]);
        assert_eq!(size, UVec2::new(7, 6));
        let (glyphs, size) = placed("AVA", TextLayout::new());
        assert_eq!(glyphs, [('A', 0, 0), ('V', 3, 0), ('A', 6, 0)]);
        assert_eq!(size.x, 10);
        // Not across a `\n`, or a space.
        let (glyphs, size) = placed("A\nV", TextLayout::new());
        assert_eq!(glyphs, [('A', 0, 0), ('V', 0, 1)]);
        assert_eq!(size, UVec2::new(4, 12));
        assert_eq!(placed("A V", TextLayout::new()).0[2], ('V', 6, 0));
        // Each glyph's rectangle at the pen plus its offset: i is 1 in.
        assert_eq!(
            placed("Ai", TextLayout::new()).0,
            [('A', 0, 0), ('i', 5, 0)]
        );
    }

    #[test]
    fn a_pair_is_kerned_in_its_own_order() {
        // V then A kerned -2, A then V -1.
        let font = decoded(&FONT.replacen("[86, 65, -1]", "[86, 65, -2]", 1)).expect("a font");
        let mut out = Vec::new();
        assert_eq!(font.layout("AV", &TextLayout::new(), &mut out).x, 7);
        assert_eq!(out[1].position.x, 3);
        out.clear();
        assert_eq!(font.layout("VA", &TextLayout::new(), &mut out).x, 6);
        assert_eq!(out[1].position.x, 2);
    }

    #[test]
    fn a_glyph_s_rectangle_is_placed_by_its_offsets_on_its_line() {
        let font = font();
        let mut out = Vec::new();
        font.layout("A\ni", &TextLayout::new(), &mut out);
        assert_eq!(out[0].position, IVec2::new(0, 1));
        assert_eq!(out[1].position, IVec2::new(1, 6 + 1));
        assert_eq!(out[1].byte, 2);
        assert_eq!(out[1].glyph, font.glyph('i').expect("i"));
    }

    #[test]
    fn lines_count_every_break_and_the_empty_ones() {
        assert_eq!(placed("", TextLayout::new()), (vec![], UVec2::new(0, 6)));
        assert_eq!(placed("\n", TextLayout::new()).1, UVec2::new(0, 12));
        let (glyphs, size) = placed("Ai\n", TextLayout::new());
        assert_eq!(glyphs.len(), 2);
        assert_eq!(size, UVec2::new(7, 12));
        // `\r` takes no room, so `\r\n` is one break.
        assert_eq!(
            placed("A\r\nV", TextLayout::new()),
            placed("A\nV", TextLayout::new())
        );
        assert_eq!(
            placed("A\rV", TextLayout::new()).0,
            [('A', 0, 0), ('V', 3, 0)]
        );
        // An empty text in a box is as wide as the box.
        assert_eq!(placed("", wrapped(10)).1, UVec2::new(10, 6));
    }

    #[test]
    fn each_line_is_aligned_in_the_widest_or_the_box() {
        // Lines of 7 (AV, kerned) and 3 (i): the block is 7 wide.
        let text = "AV\ni";
        let (left, size) = placed(text, aligned(Align::Left, None));
        assert_eq!(size, UVec2::new(7, 12));
        assert_eq!(left[2], ('i', 1, 1));
        let (right, _) = placed(text, aligned(Align::Right, None));
        assert_eq!(right, [('A', 0, 0), ('V', 3, 0), ('i', 4 + 1, 1)]);
        // 7 - 3 = 4, halved.
        let (center, _) = placed(text, aligned(Align::Center, None));
        assert_eq!(center[2], ('i', 2 + 1, 1));
        // In a box of 12: the odd pixel to the right.
        let (center, size) = placed(text, aligned(Align::Center, Some(12)));
        assert_eq!(size, UVec2::new(12, 12));
        assert_eq!(center[..2], [('A', 2, 0), ('V', 5, 0)]);
        assert_eq!(center[2], ('i', 4 + 1, 1));
        let (right, _) = placed(text, aligned(Align::Right, Some(12)));
        assert_eq!(right[0], ('A', 5, 0));
        // Spaces at a line's end take no room, and are still placed.
        let (right, size) = placed("A  ", aligned(Align::Right, Some(10)));
        assert_eq!(size.x, 10);
        assert_eq!(right, [('A', 6, 0), (' ', 10, 0), (' ', 12, 0)]);
    }

    #[test]
    fn a_line_wraps_at_its_last_spaces_and_drops_them() {
        // "A A A": each A 4, each space 2. At 10, "A A" (10) fits, and the
        // third A goes down; the space between is dropped.
        let (glyphs, size) = placed("A A A", wrapped(10));
        assert_eq!(glyphs, [('A', 0, 0), (' ', 4, 0), ('A', 6, 0), ('A', 0, 1)]);
        assert_eq!(size, UVec2::new(10, 12));
        // A run of spaces is dropped whole.
        let (glyphs, _) = placed("A   A", wrapped(8));
        assert_eq!(glyphs, [('A', 0, 0), ('A', 0, 1)]);
        // Spaces before a break, or at the end, never wrap.
        let (glyphs, size) = placed("A      ", wrapped(4));
        assert_eq!(glyphs.len(), 7);
        assert_eq!(size, UVec2::new(4, 6));
        // The width counts kerning: "AV" is 7.
        assert_eq!(placed("AV AV", wrapped(7)).1, UVec2::new(7, 12));
        assert_eq!(placed("AV AV", wrapped(16)).1, UVec2::new(16, 6));
    }

    #[test]
    fn spaces_that_start_a_paragraph_are_kept_and_never_a_break() {
        let (glyphs, size) = placed("  A", wrapped(4));
        assert_eq!(glyphs, [(' ', 0, 0), (' ', 2, 0), ('A', 4, 0)]);
        assert_eq!(size, UVec2::new(8, 6), "wider than its box: one word");
        let (glyphs, _) = placed("A\n  A", wrapped(4));
        assert_eq!(glyphs[1..], [(' ', 0, 1), (' ', 2, 1), ('A', 4, 1)]);
        // Nor are they a place to break a word that goes on past the box.
        let (glyphs, size) = placed("  AAAA", wrapped(9));
        assert_eq!(
            glyphs,
            [
                (' ', 0, 0),
                (' ', 2, 0),
                ('A', 4, 0),
                ('A', 0, 1),
                ('A', 4, 1),
                ('A', 0, 2)
            ]
        );
        assert_eq!(size, UVec2::new(9, 18));
    }

    #[test]
    fn a_word_wider_than_the_box_breaks_between_its_characters() {
        // "AAAA" at 9: two of 4 a line.
        let (glyphs, size) = placed("AAAA", wrapped(9));
        assert_eq!(glyphs, [('A', 0, 0), ('A', 4, 0), ('A', 0, 1), ('A', 4, 1)]);
        assert_eq!(size, UVec2::new(9, 12));
        // A box narrower than a glyph still takes one a line.
        let (glyphs, size) = placed("AiA", wrapped(0));
        assert_eq!(glyphs, [('A', 0, 0), ('i', 1, 1), ('A', 0, 2)]);
        assert_eq!(size, UVec2::new(4, 18));
        // A word that does not fit after another goes down whole first.
        let (glyphs, _) = placed("i AAAA", wrapped(9));
        assert_eq!(
            glyphs,
            [
                ('i', 1, 0),
                ('A', 0, 1),
                ('A', 4, 1),
                ('A', 0, 2),
                ('A', 4, 2)
            ]
        );
    }

    #[test]
    fn a_wrapped_line_is_aligned_by_what_stays_on_it() {
        // "i AAAA" at 9: "i" (3), "AA" (8), "AA" (8), in a box of 9.
        let (right, _) = placed("i AAAA", aligned(Align::Right, Some(9)));
        assert_eq!(right[0], ('i', 6 + 1, 0));
        assert_eq!(right[1..3], [('A', 1, 1), ('A', 5, 1)]);
        let (center, _) = placed("i AAAA", aligned(Align::Center, Some(9)));
        assert_eq!(center[0], ('i', 3 + 1, 0));
        assert_eq!(center[1], ('A', 0, 1));
    }

    #[test]
    fn a_missing_character_draws_the_fallback_and_kerns_with_nothing() {
        let font = font();
        let mut out = Vec::new();
        // B is missing: no kerning on either side, though the font kerns
        // A and B, and B and V.
        font.layout("ABV", &TextLayout::new(), &mut out);
        let b = out[1];
        assert_eq!(
            (b.character, b.missing, b.glyph),
            ('B', true, font.fallback())
        );
        assert_eq!(out[2].position.x, 8);
        assert!(!out[0].missing && !out[2].missing);
        // A no-break space the font lacks is its space, and never a break:
        // "i A A" at 12 would break after the second space, and with a
        // no-break space for it breaks after the first.
        out.clear();
        font.layout("i A\u{a0}A", &wrapped(12), &mut out);
        let space = out[2];
        assert_eq!((space.character, space.missing), ('\u{a0}', false));
        assert_eq!(space.glyph, font.glyph(' ').expect("a space"));
        let lines: Vec<(char, u32)> = out
            .iter()
            .map(|glyph| (glyph.character, glyph.line))
            .collect();
        assert_eq!(lines, [('i', 0), ('A', 1), ('\u{a0}', 1), ('A', 1)]);
        assert_eq!(placed("i A A", wrapped(12)).0.last(), Some(&('A', 0, 1)));
        // A control character the font lacks takes no room, and kerns
        // through.
        assert_eq!(
            placed("A\tV", TextLayout::new()).0,
            [('A', 0, 0), ('V', 3, 0)]
        );
        assert_eq!(placed("A\u{7f}", TextLayout::new()).1.x, 4);
        // C1 too.
        assert_eq!(
            placed("A\u{85}V", TextLayout::new()),
            (vec![('A', 0, 0), ('V', 3, 0)], UVec2::new(7, 6))
        );
    }

    #[test]
    fn markup_colors_runs_and_takes_no_room() {
        let font = font();
        let mut out = Vec::new();
        let size = font.layout("A{c:#f80}V{/c}A", &TextLayout::new(), &mut out);
        let orange = Color::rgb(0xff, 0x88, 0x00);
        let runs: Vec<_> = out.iter().map(|glyph| glyph.run).collect();
        assert_eq!(runs, [None, Some(orange), None]);
        // The tags kern through: AVA as before.
        let xs: Vec<i32> = out.iter().map(|glyph| glyph.position.x).collect();
        assert_eq!(xs, [0, 3, 6]);
        assert_eq!(size.x, 10);
        assert_eq!(out[1].byte, 9, "V's place in the text");
        // Each form of color, either case; a run lasts to the next tag or
        // the end, across lines.
        for (tag, color) in [
            ("{c:#F80}", orange),
            ("{c:#f808}", Color::rgba(0xff, 0x88, 0x00, 0x88)),
            ("{c:#12aB34}", Color::rgb(0x12, 0xab, 0x34)),
            ("{c:#12ab3456}", Color::rgba(0x12, 0xab, 0x34, 0x56)),
        ] {
            out.clear();
            font.layout(
                &format!("A{tag}V\nA{{c:#000}}A"),
                &TextLayout::new(),
                &mut out,
            );
            let runs: Vec<_> = out.iter().map(|glyph| glyph.run).collect();
            assert_eq!(
                runs,
                [None, Some(color), Some(color), Some(Color::BLACK)],
                "{tag}"
            );
        }
        // `{/c}` with no run is a tag still.
        assert_eq!(placed("{/c}A", TextLayout::new()).0, [('A', 0, 0)]);
        // A tag in the spaces a line breaks at still starts its run.
        out.clear();
        font.layout("A {c:#f00} A", &wrapped(4), &mut out);
        let lines: Vec<(u32, Option<Color>)> =
            out.iter().map(|glyph| (glyph.line, glyph.run)).collect();
        assert_eq!(lines, [(0, None), (1, Some(Color::rgb(255, 0, 0)))]);
    }

    #[test]
    fn what_is_not_a_tag_is_drawn_as_it_is() {
        let characters = |text: &str, markup: bool| -> String {
            let layout = TextLayout {
                markup,
                ..TextLayout::new()
            };
            placed(text, layout).0.iter().map(|&(c, _, _)| c).collect()
        };
        assert_eq!(characters("{{c:#f00}A", true), "{c:#f00}A");
        assert_eq!(characters("{{", true), "{");
        assert_eq!(characters("}}", true), "}}");
        for text in [
            "{c:#f0}",
            "{c:#f0000}",
            "{c:#ff00000}",
            "{c:#gg0}",
            "{C:#f00}",
            "{c: #f00}",
            "{c:#f00",
            "{/C}",
            "{",
        ] {
            assert_eq!(characters(text, true), text, "{text}");
        }
        // After a `{` that starts no tag, a tag may still follow.
        let font = font();
        let mut out = Vec::new();
        font.layout("{c:{c:#f00}A", &TextLayout::new(), &mut out);
        let drawn: String = out.iter().map(|glyph| glyph.character).collect();
        assert_eq!(drawn, "{c:A");
        assert_eq!(out[3].run, Some(Color::rgb(255, 0, 0)));
        // Off, every character is drawn.
        assert_eq!(characters("{c:#f00}A{/c}{{", false), "{c:#f00}A{/c}{{");
        // A tag split by a line is no tag, and the `\n` still breaks.
        assert_eq!(placed("{c:#f\n00}", TextLayout::new()).1.y, 12);
    }

    #[test]
    fn a_run_s_color_keeps_its_rgb_and_takes_the_text_s_alpha_too() {
        let text = Color::rgba(10, 20, 30, 128);
        assert_eq!(run_color(text, None), text);
        assert_eq!(
            run_color(text, Some(Color::rgb(255, 0, 0))),
            Color::rgba(255, 0, 0, 128)
        );
        assert_eq!(
            run_color(text, Some(Color::rgba(0, 255, 0, 128))),
            Color::rgba(0, 255, 0, 64)
        );
        assert_eq!(
            run_color(Color::WHITE, Some(Color::rgba(1, 2, 3, 77))),
            Color::rgba(1, 2, 3, 77)
        );
        // Rounded: 200 × 200 / 255 is 156.9.
        assert_eq!(
            run_color(Color::rgba(0, 0, 0, 200), Some(Color::rgba(1, 2, 3, 200))),
            Color::rgba(1, 2, 3, 157)
        );
    }

    #[test]
    fn escaped_text_draws_as_written() {
        assert_eq!(Escaped("a{b}{{c").to_string(), "a{{b}{{{{c");
        assert_eq!(Escaped("").to_string(), "");
        let name = "{c:#f00}Eve{/c}";
        let text = format!("{{c:#0f0}}{}{{/c}}", Escaped(name));
        let font = font();
        let mut out = Vec::new();
        font.layout(&text, &TextLayout::new(), &mut out);
        let drawn: String = out.iter().map(|glyph| glyph.character).collect();
        assert_eq!(drawn, name);
        assert!(
            out.iter()
                .all(|glyph| glyph.run == Some(Color::rgb(0, 255, 0)))
        );
    }

    #[test]
    fn measure_and_layout_agree_and_every_text_ends() {
        const PIECES: &[&str] = &[
            "A",
            "V",
            "i",
            " ",
            "  ",
            "\u{a0}",
            "\n",
            "\r",
            "\t",
            "\u{85}",
            "B",
            "é",
            "{",
            "}",
            "{{",
            "{c:#f00}",
            "{c:#12345678}",
            "{/c}",
            "{c:#zz}",
        ];
        let font = font();
        check(
            "layout",
            3_000,
            |rng| {
                let length = rng.range(0..24u32);
                let text: String = (0..length)
                    .map(|_| PIECES[rng.range(0..PIECES.len() as u32) as usize])
                    .collect();
                let max_width = match rng.range(0..3u32) {
                    0 => None,
                    _ => Some(rng.range(0..20u32)),
                };
                let align = [Align::Left, Align::Center, Align::Right][rng.range(0..3u32) as usize];
                let markup = rng.range(0..4u32) != 0;
                (text, max_width, align, markup)
            },
            |(text, max_width, align, markup)| {
                let layout = TextLayout {
                    max_width: *max_width,
                    align: *align,
                    markup: *markup,
                };
                let mut out = Vec::new();
                let size = font.layout(text, &layout, &mut out);
                if size != font.measure(text, &layout) {
                    return Err(format!("measured {:?}", font.measure(text, &layout)));
                }
                let lines = size.y / font.line_height();
                if out.iter().any(|glyph| glyph.line >= lines) {
                    return Err(format!("a glyph past {lines} lines"));
                }
                if out.windows(2).any(|pair| pair[0].byte >= pair[1].byte) {
                    return Err("out of the text's order".to_string());
                }
                let at = |byte: usize| text.get(byte..).and_then(|rest| rest.chars().next());
                if out
                    .iter()
                    .any(|glyph| at(glyph.byte) != Some(glyph.character))
                {
                    return Err("a glyph's byte is not its character".to_string());
                }
                // The pen is in the block before each glyph but a space,
                // which may go on past a line's end.
                for glyph in out.iter().filter(|glyph| glyph.character != ' ') {
                    let left = glyph.position.x - glyph.glyph.offset().x;
                    if left < 0 || left as u32 > size.x {
                        return Err(format!("{glyph:?} outside a block of {size:?}"));
                    }
                }
                Ok(())
            },
        );
    }
}
