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

//! Font file tests: files as the cook wrote them from fonts made for the
//! purpose, in `tests/font/`, each checked field by field, and damaged
//! ones. The error cases, one by one, are beside the reader, in
//! `src/font.rs`. The same files are the fuzz target's seeds, in
//! `fuzz/corpus/font/`.

use nadir_codec::font::{self, FontFile};
use nadir_codec::{CodecError, Limits};

fn vector(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/font/{name}.font.kbj", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn parse(name: &str) -> FontFile {
    font::parse(&vector(name), Limits::default()).unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// The glyphs, as (id, frame, x, y, advance).
fn glyphs(font: &FontFile) -> Vec<(char, Option<u32>, i32, i32, i32)> {
    font.glyphs
        .iter()
        .map(|glyph| (glyph.id, glyph.frame, glyph.x, glyph.y, glyph.advance))
        .collect()
}

/// The kerning pairs, as (first, second, amount).
fn kerning(font: &FontFile) -> Vec<(char, char, i32)> {
    font.kerning
        .iter()
        .map(|pair| (pair.first, pair.second, pair.amount))
        .collect()
}

/// The fallback, as (frame, x, y, advance).
fn fallback(font: &FontFile) -> (u32, i32, i32, i32) {
    let fallback = &font.fallback;
    (fallback.frame, fallback.x, fallback.y, fallback.advance)
}

#[test]
fn a_unicode_font_with_kerning() {
    // AngelCode's BMFont: a space and a glyph with no pixels have no frame;
    // `?` is the fallback, as there is no glyph of id −1.
    let font = parse("unicode_kerning");
    assert_eq!(font.face, "Seed Sans");
    assert_eq!((font.size, font.line_height, font.base), (16, 19, 15));
    assert_eq!(
        (font.atlas.as_str(), font.sprite.as_str()),
        ("atlas/fonts.kbj", "fonts/unicode_kerning")
    );
    assert_eq!(
        glyphs(&font),
        [
            (' ', None, 0, 0, 4),
            ('?', Some(0), 0, 4, 7),
            ('A', Some(1), -1, 4, 8),
            ('V', Some(2), -1, 4, 8),
            ('\u{a0}', None, 0, 0, 4),
            ('é', Some(3), 0, 1, 7),
            ('€', Some(4), -1, 4, 8),
        ]
    );
    assert_eq!(
        kerning(&font),
        [('A', 'V', -2), ('V', 'A', -2), ('é', '€', 1)]
    );
    assert_eq!(fallback(&font), (0, 0, 4, 7));
}

#[test]
fn an_ansi_font_with_bmfont_s_fallback_glyph() {
    // Windows-1252's 145 is U+2018; the glyph of id −1 is the fallback, its
    // frame after the others'.
    let font = parse("ansi_fallback");
    assert_eq!(font.face, "Garamond");
    assert_eq!((font.size, font.line_height, font.base), (-20, 24, 19));
    assert_eq!(font.sprite, "fonts/ansi_fallback");
    assert_eq!(
        glyphs(&font),
        [('a', Some(0), 0, 9, 9), ('\u{2018}', Some(1), 1, 4, 5)]
    );
    assert_eq!(kerning(&font), [('\u{2018}', 'a', -1)]);
    assert_eq!(fallback(&font), (2, 1, 6, 8));
}

#[test]
fn a_font_on_two_pages() {
    // Hiero: `unicode=0` with code points.
    let font = parse("two_pages");
    assert_eq!(font.face, "Seed Mono");
    assert_eq!((font.size, font.line_height, font.base), (24, 29, 22));
    assert_eq!(
        glyphs(&font),
        [('?', Some(0), 0, 3, 13), ('€', Some(1), -1, 1, 14)]
    );
    assert!(font.kerning.is_empty());
    assert_eq!(fallback(&font), (0, 0, 3, 13));
}

#[test]
fn the_least_a_font_file_holds() {
    // A face that is empty, no glyph but BMFont's fallback, no kerning.
    let font = parse("minimal");
    assert_eq!(font.face, "");
    assert_eq!((font.size, font.line_height, font.base), (8, 8, 7));
    assert!(font.glyphs.is_empty() && font.kerning.is_empty());
    assert_eq!(fallback(&font), (0, 0, 1, 5));
}

#[test]
fn a_damaged_file_is_an_error_and_never_a_panic() {
    for name in ["unicode_kerning", "ansi_fallback", "two_pages", "minimal"] {
        let bytes = vector(name);
        // Every cut short of the whole file.
        for len in 0..bytes.len() {
            assert!(
                font::parse(&bytes[..len], Limits::default()).is_err(),
                "{name}: {len} bytes"
            );
        }
        // Every byte changed, one at a time: an error or a font, but no
        // panic, and a font that is still in order.
        for index in 0..bytes.len() {
            for flip in [0x01, 0x80, 0xff] {
                let mut damaged = bytes.clone();
                damaged[index] ^= flip;
                if let Ok(font) = font::parse(&damaged, Limits::default()) {
                    assert!(
                        font.glyphs.windows(2).all(|pair| pair[0].id < pair[1].id),
                        "{name}: byte {index}"
                    );
                }
            }
        }
    }
    // Tight limits are kept.
    let tight = Limits {
        max_entries: 4,
        ..Limits::default()
    };
    assert!(matches!(
        font::parse(&vector("unicode_kerning"), tight),
        Err(CodecError::Kbj { .. })
    ));
}
