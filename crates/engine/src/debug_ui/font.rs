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

//! The engine's own files for the debug UI: a pixel font of printable
//! ASCII, drawn here as rows of `#`, and a white texel. They are made in
//! memory, as the cook would write them, and read through the asset
//! server's source at paths below `nadir/debug/` (ADR-0058).

use nadir_codec::json::Value;
use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
use nadir_codec::{Limits, kbj};

/// The font file.
pub(crate) const FONT: &str = "nadir/debug/font.font.kbj";
/// The font's atlas file.
const ATLAS: &str = "nadir/debug/font.kbj";
/// The atlas's one page.
const PAGE: &str = "nadir/debug/font_0.qoi";
/// A texture of one white texel, for the debug UI's boxes.
pub(crate) const WHITE: &str = "nadir/debug/white.qoi";
/// The atlas's sprite whose frames are the glyphs.
const SPRITE: &str = "fonts/nadir_debug";

/// A glyph's width, and its height: seven rows from the top of a capital
/// to the baseline, and two below it for descenders.
const WIDTH: usize = 5;
const HEIGHT: usize = 9;
/// The pen's move after each glyph: every glyph is as wide.
const ADVANCE: i32 = 6;
/// From one line to the next, and from a line's top to its baseline: a
/// row of space above the capitals, which start a row down.
const LINE_HEIGHT: u32 = 10;
const BASE: u32 = 8;
/// The glyphs on the page: in cells of `WIDTH + 1` by `HEIGHT + 1` texels,
/// so that no two touch, this many to a row of cells.
const COLUMNS: usize = 16;

/// The glyphs of U+0020 to U+007E, twelve to a band: a band's characters,
/// then its nine rows, each the five pixels of every glyph's row, `#` ink
/// and `.` none, a space between two glyphs.
const BANDS: [(&str, [&str; HEIGHT]); 8] = [
    (
        " !\"#$%&'()*+",
        [
            "..... ..#.. .#.#. .#.#. ..#.. ##... .##.. ..#.. ...#. .#... ..... .....",
            "..... ..#.. .#.#. .#.#. .#### ##..# #..#. ..#.. ..#.. ..#.. ..#.. ..#..",
            "..... ..#.. ..... ##### #.#.. ...#. #.#.. ..... .#... ...#. #.#.# ..#..",
            "..... ..#.. ..... .#.#. .###. ..#.. .#... ..... .#... ...#. .###. #####",
            "..... ..#.. ..... ##### ..#.# .#... #.#.# ..... .#... ...#. #.#.# ..#..",
            "..... ..... ..... .#.#. ####. #..## #..#. ..... ..#.. ..#.. ..#.. ..#..",
            "..... ..#.. ..... .#.#. ..#.. ...## .##.# ..... ...#. .#... ..... .....",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
        ],
    ),
    (
        ",-./01234567",
        [
            "..... ..... ..... ....# .###. ..#.. .###. ##### ...#. ##### ..##. #####",
            "..... ..... ..... ....# #...# .##.. #...# ...#. ..##. #.... .#... ....#",
            "..... ..... ..... ...#. #..## ..#.. ....# ..#.. .#.#. ####. #.... ...#.",
            "..... ##### ..... ..#.. #.#.# ..#.. ...#. ...#. #..#. ....# ####. ..#..",
            "..... ..... ..... .#... ##..# ..#.. ..#.. ....# ##### ....# #...# .#...",
            "..... ..... ..... #.... #...# ..#.. .#... #...# ...#. #...# #...# .#...",
            "..#.. ..... ..#.. #.... .###. .###. ##### .###. ...#. .###. .###. .#...",
            "..#.. ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
            ".#... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
        ],
    ),
    (
        "89:;<=>?@ABC",
        [
            ".###. .###. ..... ..... ...#. ..... .#... .###. .###. .###. ####. .###.",
            "#...# #...# ..... ..... ..#.. ..... ..#.. #...# #...# #...# #...# #...#",
            "#...# #...# ..#.. ..#.. .#... ##### ...#. ....# #.### #...# #...# #....",
            ".###. .#### ..... ..... #.... ..... ....# ...#. #.#.# ##### ####. #....",
            "#...# ....# ..... ..... .#... ##### ...#. ..#.. #.### #...# #...# #....",
            "#...# ...#. ..#.. ..#.. ..#.. ..... ..#.. ..... #.... #...# #...# #...#",
            ".###. .##.. ..... ..#.. ...#. ..... .#... ..#.. .###. #...# ####. .###.",
            "..... ..... ..... .#... ..... ..... ..... ..... ..... ..... ..... .....",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
        ],
    ),
    (
        "DEFGHIJKLMNO",
        [
            "###.. ##### ##### .###. #...# .###. ..### #...# #.... #...# #...# .###.",
            "#..#. #.... #.... #...# #...# ..#.. ...#. #..#. #.... ##.## #...# #...#",
            "#...# #.... #.... #.... #...# ..#.. ...#. #.#.. #.... #.#.# ##..# #...#",
            "#...# ####. ####. #.### ##### ..#.. ...#. ##... #.... #.#.# #.#.# #...#",
            "#...# #.... #.... #...# #...# ..#.. ...#. #.#.. #.... #...# #..## #...#",
            "#..#. #.... #.... #...# #...# ..#.. #..#. #..#. #.... #...# #...# #...#",
            "###.. ##### #.... .#### #...# .###. .##.. #...# ##### #...# #...# .###.",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
        ],
    ),
    (
        "PQRSTUVWXYZ[",
        [
            "####. .###. ####. .#### ##### #...# #...# #...# #...# #...# ##### .###.",
            "#...# #...# #...# #.... ..#.. #...# #...# #...# #...# #...# ....# .#...",
            "#...# #...# #...# #.... ..#.. #...# #...# #...# .#.#. .#.#. ...#. .#...",
            "####. #...# ####. .###. ..#.. #...# #...# #.#.# ..#.. ..#.. ..#.. .#...",
            "#.... #.#.# #.#.. ....# ..#.. #...# #...# #.#.# .#.#. ..#.. .#... .#...",
            "#.... #..#. #..#. ....# ..#.. #...# .#.#. #.#.# #...# ..#.. #.... .#...",
            "#.... .##.# #...# ####. ..#.. .###. ..#.. .#.#. #...# ..#.. ##### .###.",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
        ],
    ),
    (
        "\\]^_`abcdefg",
        [
            "#.... .###. ..#.. ..... .#... ..... #.... ..... ....# ..... ..##. .....",
            "#.... ...#. .#.#. ..... ..#.. ..... #.... ..... ....# ..... .#..# .....",
            ".#... ...#. #...# ..... ..... .###. ####. .###. .#### .###. .#... .####",
            "..#.. ...#. ..... ..... ..... ....# #...# #.... #...# #...# ###.. #...#",
            "...#. ...#. ..... ..... ..... .#### #...# #.... #...# ##### .#... #...#",
            "....# ...#. ..... ..... ..... #...# #...# #...# #...# #.... .#... #...#",
            "....# .###. ..... ..... ..... .#### ####. .###. .#### .###. .#... .####",
            "..... ..... ..... ##### ..... ..... ..... ..... ..... ..... ..... ....#",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .###.",
        ],
    ),
    (
        "hijklmnopqrs",
        [
            "#.... ..#.. ...#. #.... .##.. ..... ..... ..... ..... ..... ..... .....",
            "#.... ..... ..... #.... ..#.. ..... ..... ..... ..... ..... ..... .....",
            "#.##. .##.. ..##. #..#. ..#.. ##.#. #.##. .###. ####. .#### #.##. .####",
            "##..# ..#.. ...#. #.#.. ..#.. #.#.# ##..# #...# #...# #...# ##..# #....",
            "#...# ..#.. ...#. ##... ..#.. #.#.# #...# #...# #...# #...# #.... .###.",
            "#...# ..#.. ...#. #.#.. ..#.. #.#.# #...# #...# #...# #...# #.... ....#",
            "#...# .###. ...#. #..#. .###. #.#.# #...# .###. ####. .#### #.... ####.",
            "..... ..... #..#. ..... ..... ..... ..... ..... #.... ....# ..... .....",
            "..... ..... .##.. ..... ..... ..... ..... ..... #.... ....# ..... .....",
        ],
    ),
    (
        "tuvwxyz{|}~",
        [
            ".#... ..... ..... ..... ..... ..... ..... ...## ..#.. ##... .....",
            ".#... ..... ..... ..... ..... ..... ..... ..#.. ..#.. ..#.. .....",
            "###.. #...# #...# #...# #...# #...# ##### ..#.. ..#.. ..#.. .#...",
            ".#... #...# #...# #...# .#.#. #...# ...#. .#... ..#.. ...#. #.#.#",
            ".#... #...# #...# #.#.# ..#.. #...# ..#.. ..#.. ..#.. ..#.. ...#.",
            ".#..# #..## .#.#. #.#.# .#.#. #...# .#... ..#.. ..#.. ..#.. .....",
            "..##. .##.# ..#.. .#.#. #...# .#### ##### ...## ..#.. ##... .....",
            "..... ..... ..... ..... ..... ....# ..... ..... ..... ..... .....",
            "..... ..... ..... ..... ..... .###. ..... ..... ..... ..... .....",
        ],
    ),
];

/// What a character the font lacks draws: a box.
const FALLBACK: [&str; HEIGHT] = [
    "#####", "#...#", "#...#", "#...#", "#...#", "#...#", "#####", ".....", ".....",
];

/// Whether `path` is one of the engine's files, which hide a file of the
/// game's of the same path.
pub(crate) fn is_file(path: &str) -> bool {
    [FONT, ATLAS, PAGE, WHITE].contains(&path)
}

/// A glyph's pixels, by row: whether each has ink.
type Pixels = [[bool; WIDTH]; HEIGHT];

/// The engine's files: each its path and its bytes.
pub(crate) fn files() -> Vec<(&'static str, Vec<u8>)> {
    let glyphs = glyphs();
    // Every glyph with ink has a cell, as the space has not; the
    // fallback's is the last.
    let fallback_pixels = pixels(|y, x| FALLBACK[y].as_bytes()[x]);
    let inked: Vec<&Pixels> = glyphs
        .iter()
        .map(|(_, pixels)| pixels)
        .filter(|pixels| has_ink(pixels))
        .chain([&fallback_pixels])
        .collect();
    let (width, height) = (
        COLUMNS * (WIDTH + 1),
        inked.len().div_ceil(COLUMNS) * (HEIGHT + 1),
    );
    let mut page = vec![0u8; width * height * 4];
    let mut frames = Vec::with_capacity(inked.len());
    for (index, pixels) in inked.iter().enumerate() {
        let left = index % COLUMNS * (WIDTH + 1);
        let top = index / COLUMNS * (HEIGHT + 1);
        for (y, row) in pixels.iter().enumerate() {
            for (x, &ink) in row.iter().enumerate() {
                if ink {
                    let at = ((top + y) * width + left + x) * 4;
                    page[at..at + 4].copy_from_slice(&[255; 4]);
                }
            }
        }
        frames.push(frame(left, top));
    }

    let mut entries = Vec::with_capacity(glyphs.len());
    let mut next = 0;
    for (character, pixels) in &glyphs {
        let mut members = vec![member("id", Value::Int(i64::from(u32::from(*character))))];
        if has_ink(pixels) {
            members.push(member("frame", Value::Int(next)));
            next += 1;
        }
        members.extend(placement());
        entries.push(Value::Object(members));
    }
    let mut fallback = vec![member("frame", Value::Int(next))];
    fallback.extend(placement());

    let atlas = Value::Object(vec![
        member("version", Value::Int(2)),
        member(
            "pages",
            Value::Array(vec![Value::Object(vec![
                member("path", Value::String(PAGE.to_string())),
                member("w", Value::Int(width as i64)),
                member("h", Value::Int(height as i64)),
            ])]),
        ),
        member(
            "sprites",
            Value::Array(vec![Value::Object(vec![
                member("name", Value::String(SPRITE.to_string())),
                member("frames", Value::Array(frames)),
                member(
                    "pivot",
                    Value::Array(vec![Value::Float(0.0), Value::Float(0.0)]),
                ),
            ])]),
        ),
    ]);
    let font = Value::Object(vec![
        member("version", Value::Int(1)),
        member("face", Value::String("Nadir debug".to_string())),
        member("size", Value::Int(i64::from(LINE_HEIGHT))),
        member("line_height", Value::Int(i64::from(LINE_HEIGHT))),
        member("base", Value::Int(i64::from(BASE))),
        member("atlas", Value::String(ATLAS.to_string())),
        member("sprite", Value::String(SPRITE.to_string())),
        member("glyphs", Value::Array(entries)),
        member("kerning", Value::Array(Vec::new())),
        member("fallback", Value::Object(fallback)),
    ]);
    vec![
        (FONT, kbj_file(&font)),
        (ATLAS, kbj_file(&atlas)),
        (PAGE, image(width, height, &page)),
        (WHITE, image(1, 1, &[255; 4])),
    ]
}

/// Every glyph of the bands, in their order, which is the characters'.
fn glyphs() -> Vec<(char, Pixels)> {
    let mut glyphs = Vec::with_capacity(95);
    for (characters, rows) in &BANDS {
        for (index, character) in characters.chars().enumerate() {
            let left = index * (WIDTH + 1);
            glyphs.push((character, pixels(|y, x| rows[y].as_bytes()[left + x])));
        }
    }
    glyphs
}

/// A glyph's pixels, `at(y, x)` giving each one's `#` or `.`.
fn pixels(at: impl Fn(usize, usize) -> u8) -> Pixels {
    let mut pixels = [[false; WIDTH]; HEIGHT];
    for (y, row) in pixels.iter_mut().enumerate() {
        for (x, pixel) in row.iter_mut().enumerate() {
            *pixel = at(y, x) == b'#';
        }
    }
    pixels
}

fn has_ink(pixels: &Pixels) -> bool {
    pixels.iter().flatten().any(|&ink| ink)
}

/// The atlas frame of the cell at (`left`, `top`): the whole glyph, not
/// trimmed.
fn frame(left: usize, top: usize) -> Value {
    let int = |value: usize| Value::Int(value as i64);
    Value::Object(vec![
        member("page", Value::Int(0)),
        member("x", int(left)),
        member("y", int(top)),
        member("w", int(WIDTH)),
        member("h", int(HEIGHT)),
        member("ox", Value::Int(0)),
        member("oy", Value::Int(0)),
        member("sw", int(WIDTH)),
        member("sh", int(HEIGHT)),
    ])
}

/// Where every glyph goes from the pen, a row below the line's top, and
/// how far it moves it.
fn placement() -> [(String, Value); 3] {
    [
        member("x", Value::Int(0)),
        member("y", Value::Int(1)),
        member("advance", Value::Int(i64::from(ADVANCE))),
    ]
}

fn member(name: &str, value: Value) -> (String, Value) {
    (name.to_string(), value)
}

fn kbj_file(value: &Value) -> Vec<u8> {
    kbj::encode(value, Limits::default()).expect("the engine's files are within the limits")
}

/// An sRGB RGBA image of `width` × `height` texels.
fn image(width: usize, height: usize, pixels: &[u8]) -> Vec<u8> {
    let header = Header {
        width: width as u32,
        height: height as u32,
        channels: Channels::Rgba,
        color_space: ColorSpace::Srgb,
    };
    qoi::encode(&header, pixels).expect("the engine's images are valid")
}

#[cfg(test)]
mod tests {
    use nadir_assets::{AssetServer, FinalizeCtx, OnFailure, Scope};
    use nadir_codec::font as font_file;
    use nadir_render::{Font, TextLayout};

    use super::*;
    use crate::debug_ui::EngineFiles;

    #[test]
    fn the_bands_hold_every_printable_ascii_character_once_in_order() {
        let mut expected = ' ';
        for (characters, rows) in &BANDS {
            let count = characters.chars().count();
            for row in rows {
                // Five pixels a glyph, a space between two.
                assert_eq!(
                    row.len(),
                    count * (WIDTH + 1) - 1,
                    "{characters:?}: {row:?}"
                );
                for (at, byte) in row.bytes().enumerate() {
                    let gap = at % (WIDTH + 1) == WIDTH;
                    assert!(
                        if gap {
                            byte == b' '
                        } else {
                            byte == b'#' || byte == b'.'
                        },
                        "{characters:?}: {row:?}"
                    );
                }
            }
            for character in characters.chars() {
                assert_eq!(character, expected);
                expected = char::from_u32(u32::from(expected) + 1).expect("ASCII");
            }
        }
        assert_eq!(expected, '\u{7f}');
        assert!(FALLBACK.iter().all(|row| row.len() == WIDTH));
    }

    #[test]
    fn the_font_loads_as_the_cook_would_write_it_and_is_monospaced() {
        let server_files = files();
        let file = |path: &str| {
            &server_files
                .iter()
                .find(|(name, _)| *name == path)
                .expect("the file")
                .1
        };
        let parsed = font_file::parse(file(FONT), Limits::default()).expect("a font file");
        assert_eq!(parsed.glyphs.len(), 95);
        assert_eq!(parsed.glyphs[0].frame, None, "the space has no pixels");

        // Through the asset server, as the debug UI loads it.
        let mut server = AssetServer::with_source(
            EngineFiles::new(std::env::temp_dir().join("nadir-engine-no-assets")),
            OnFailure::Nothing,
        );
        let font = Font::load_now_in(&mut server, FONT, Scope::Global, &mut FinalizeCtx::new())
            .expect("the engine's font and its atlas");
        let font = server.get(font).expect("loaded");
        assert_eq!((font.line_height(), font.base()), (LINE_HEIGHT, BASE));
        for code in 0x20..0x7f {
            let character = char::from_u32(code).expect("ASCII");
            let glyph = font
                .glyph(character)
                .expect("every printable ASCII character");
            assert_eq!(glyph.advance(), ADVANCE, "{character:?}");
        }
        assert_eq!(font.glyph('é'), None);
        assert_eq!(font.fallback().advance(), ADVANCE);
        let mut layout = TextLayout::new();
        layout.markup = false;
        assert_eq!(font.measure("Nadir 0.3", &layout).x, 9 * ADVANCE as u32);
    }
}
