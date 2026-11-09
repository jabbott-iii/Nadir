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

//! BMFont fonts: their glyphs join their folder's atlas group, and a file
//! beside the atlas says where each glyph is and how it is placed.

use std::path::Path;

use nadir_codec::json::Value;
use nadir_cook::CookError;

use crate::common::{cook, dir, files, frame_pixels, frames, int, jobs, kbj, plain, png, write};

/// A page of 32 × 32, clear but for `blocks`: each its corner, size and
/// color.
fn page(blocks: &[(u32, u32, u32, u32, [u8; 4])]) -> Vec<u8> {
    let mut pixels = vec![0u8; 32 * 32 * 4];
    for &(left, top, w, h, color) in blocks {
        for y in top..top + h {
            for x in left..left + w {
                let at = ((y * 32 + x) * 4) as usize;
                pixels[at..at + 4].copy_from_slice(&color);
            }
        }
    }
    png(32, 32, &pixels, &[])
}

const TRANSLUCENT: [u8; 4] = [255, 255, 255, 200];
const OPAQUE: [u8; 4] = [255, 255, 255, 255];

/// The glyphs of [`FONT`], out of order as a generator may write them: a
/// `B` on page 1, a space with no rectangle, an `A` whose 3 × 5 pixels sit
/// a pixel in from its rectangle's corner, a `C` of clear pixels, and a `?`.
const FONT: &str = r#"info face="Tiny" size=8 unicode=1 charset=""
common lineHeight=9 base=7 scaleW=32 scaleH=32 pages=2 packed=0 alphaChnl=0 redChnl=4 greenChnl=4 blueChnl=4
page id=0 file="tiny_0.png"
page id=1 file="Tiny_1.png"
chars count=5
char id=66 x=0 y=0 width=4 height=4 xoffset=0 yoffset=3 xadvance=5 page=1 chnl=15
char id=32 x=0 y=0 width=0 height=0 xoffset=0 yoffset=0 xadvance=3 page=0 chnl=15
char id=65 x=1 y=1 width=5 height=7 xoffset=-1 yoffset=1 xadvance=5 page=0 chnl=15
char id=67 x=20 y=1 width=4 height=4 xoffset=0 yoffset=2 xadvance=5 page=0 chnl=15
char id=63 x=10 y=1 width=4 height=6 xoffset=0 yoffset=1 xadvance=5 page=0 chnl=15
kernings count=3
kerning first=66 second=65 amount=-1
kerning first=65 second=86 amount=-2
kerning first=65 second=66 amount=1
"#;

fn tiny(source: &Path, font: &str) {
    write(source, "fonts/tiny.fnt", font);
    write(
        source,
        "fonts/tiny_0.png",
        page(&[
            (2, 2, 3, 5, TRANSLUCENT),
            (10, 1, 4, 6, OPAQUE),
            (14, 1, 3, 3, OPAQUE),
        ]),
    );
    write(source, "fonts/tiny_1.png", page(&[(0, 0, 4, 4, OPAQUE)]));
}

fn number(value: &Value, key: &str) -> i64 {
    value
        .get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("no `{key}` in {value:?}"))
}

#[test]
fn a_font_s_glyphs_join_its_group_and_its_file_places_them() {
    let root = dir("font");
    let (source, out) = (root.join("source"), root.join("out"));
    tiny(&source, FONT);
    let report = cook(&source, &out).expect("a cook");
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);

    // The glyphs with pixels, by id: `?`, `A` and `B`; the pages are the
    // font's, not sprites of their own.
    let atlas = kbj(&out, "atlas/fonts.kbj");
    let sprites = atlas
        .get("sprites")
        .and_then(Value::as_array)
        .expect("sprites");
    assert_eq!(sprites.len(), 1);
    let glyphs = &sprites[0];
    assert_eq!(
        glyphs.get("name").and_then(Value::as_str),
        Some("fonts/tiny")
    );
    let placed: Vec<[u32; 6]> = frames(glyphs)
        .iter()
        .map(|frame| ["w", "h", "ox", "oy", "sw", "sh"].map(|key| int(frame, key)))
        .collect();
    assert_eq!(
        placed,
        [[4, 6, 0, 0, 4, 6], [3, 5, 1, 1, 5, 7], [4, 4, 0, 0, 4, 4]]
    );
    let a = frame_pixels(&out, &atlas, &frames(glyphs)[1]);
    assert_eq!(a, (3, 5, TRANSLUCENT.repeat(15)));
    assert_eq!(
        glyphs.get("pivot"),
        Some(&Value::Array(vec![Value::Float(0.0), Value::Float(0.0)]))
    );

    // The font's file, beside the atlas.
    let font = kbj(&out, "atlas/fonts/tiny.font.kbj");
    assert_eq!(number(&font, "version"), 1);
    assert_eq!(font.get("face").and_then(Value::as_str), Some("Tiny"));
    assert_eq!(
        ["size", "line_height", "base"].map(|key| number(&font, key)),
        [8, 9, 7]
    );
    assert_eq!(
        font.get("atlas").and_then(Value::as_str),
        Some("atlas/fonts.kbj")
    );
    assert_eq!(
        font.get("sprite").and_then(Value::as_str),
        Some("fonts/tiny")
    );
    let entries = font
        .get("glyphs")
        .and_then(Value::as_array)
        .expect("glyphs");
    let glyphs: Vec<(i64, Option<i64>, [i64; 3])> = entries
        .iter()
        .map(|glyph| {
            (
                number(glyph, "id"),
                glyph.get("frame").and_then(Value::as_i64),
                ["x", "y", "advance"].map(|key| number(glyph, key)),
            )
        })
        .collect();
    assert_eq!(
        glyphs,
        [
            (32, None, [0, 0, 3]),
            (63, Some(0), [0, 1, 5]),
            (65, Some(1), [-1, 1, 5]),
            (66, Some(2), [0, 3, 5]),
            (67, None, [0, 2, 5]),
        ]
    );
    let pairs = |value: &Value| -> Vec<Vec<i64>> {
        value
            .as_array()
            .expect("a list")
            .iter()
            .map(|pair| {
                pair.as_array()
                    .expect("a pair")
                    .iter()
                    .map(|n| n.as_i64().expect("a number"))
                    .collect()
            })
            .collect()
    };
    assert_eq!(
        pairs(font.get("kerning").expect("kerning")),
        [vec![65, 66, 1], vec![65, 86, -2], vec![66, 65, -1]]
    );
    // Without a fallback glyph of BMFont's, `?`.
    let fallback = font.get("fallback").expect("a fallback");
    assert_eq!(
        (number(fallback, "frame"), number(fallback, "advance")),
        (0, 5)
    );

    // The game's reader takes it as it was written.
    let bytes = std::fs::read(out.join("atlas/fonts/tiny.font.kbj")).expect("the font file");
    let read = nadir_codec::font::parse(&bytes, nadir_codec::Limits::default()).expect("a font");
    let ids: Vec<char> = read.glyphs.iter().map(|glyph| glyph.id).collect();
    assert_eq!(ids, [' ', '?', 'A', 'B', 'C']);
    assert_eq!((read.kerning.len(), read.fallback.frame), (3, 0));
}

#[test]
fn bmfont_s_fallback_glyph_is_the_last_frame() {
    let root = dir("font_fallback");
    let (source, out) = (root.join("source"), root.join("out"));
    let font = FONT.replacen(
        "chars count=5\n",
        "chars count=6\nchar id=-1 x=14 y=1 width=3 height=3 xoffset=0 yoffset=2 xadvance=4 page=0 chnl=15\n",
        1,
    );
    tiny(&source, &font);
    cook(&source, &out).expect("a cook");
    assert_eq!(
        frames(
            &kbj(&out, "atlas/fonts.kbj")
                .get("sprites")
                .and_then(Value::as_array)
                .expect("sprites")[0]
        )
        .len(),
        4
    );
    let fallback = kbj(&out, "atlas/fonts/tiny.font.kbj")
        .get("fallback")
        .cloned()
        .expect("a fallback");
    assert_eq!(
        ["frame", "x", "y", "advance"].map(|key| number(&fallback, key)),
        [3, 0, 2, 4]
    );
}

#[test]
fn a_font_s_mistakes_are_named() {
    let layout = |error: &CookError, words: &str| matches!(error, CookError::Layout { path, reason } if path == "fonts/tiny.fnt" && reason.contains(words));
    // A test's name, the font, a change to its files, and the error it
    // expects.
    type Case = (&'static str, String, fn(&Path), fn(&CookError) -> bool);
    let cases: [Case; 8] = [
        (
            "page_size",
            FONT.to_string(),
            |source| write(source, "fonts/tiny_0.png", plain(16, 16, [0, 0, 0, 0])),
            |error| matches!(error, CookError::Layout { reason, .. } if reason.contains("16 × 16, not the font's 32 × 32")),
        ),
        (
            "alpha",
            FONT.replacen("alphaChnl=0", "alphaChnl=1", 1),
            |_| {},
            |error| matches!(error, CookError::Layout { reason, .. } if reason.contains("holds 1")),
        ),
        (
            "opaque",
            FONT.to_string(),
            |source| write(source, "fonts/tiny_1.png", plain(32, 32, [0, 0, 0, 255])),
            |error| matches!(error, CookError::Layout { reason, .. } if reason.contains("no transparent pixel")),
        ),
        (
            "no_fallback",
            FONT.replacen(
                "char id=63 x=10 y=1 width=4 height=6 xoffset=0 yoffset=1 xadvance=5 page=0 chnl=15\n",
                "",
                1,
            )
            .replacen("chars count=5", "chars count=4", 1),
            |_| {},
            |error| matches!(error, CookError::Layout { reason, .. } if reason.contains("no `?`")),
        ),
        (
            "clear_fallback",
            FONT.replacen("char id=63 x=10", "char id=63 x=24", 1),
            |_| {},
            |error| matches!(error, CookError::Layout { reason, .. } if reason.contains("has no pixels")),
        ),
        (
            "missing_page",
            FONT.replacen("tiny_0.png", "missing.png", 1),
            |_| {},
            |error| matches!(error, CookError::Config { path, reason } if path == "fonts/tiny.fnt" && reason.contains("`missing.png` is not a PNG")),
        ),
        (
            "page_in_a_folder",
            FONT.replacen("tiny_0.png", "sub/tiny_0.png", 1),
            |source| write(source, "fonts/sub/tiny_0.png", page(&[(10, 1, 4, 6, OPAQUE)])),
            |error| matches!(error, CookError::Config { path, reason } if path == "fonts/tiny.fnt" && reason.contains("must be a file name")),
        ),
        (
            "not_bmfont",
            "<?xml version=\"1.0\"?>\n<font>\n".to_string(),
            |_| {},
            |error| matches!(error, CookError::Decode { path, error: nadir_codec::CodecError::BmFont { .. } } if path == "fonts/tiny.fnt"),
        ),
    ];
    for (test, font, change, expected) in cases {
        let root = dir(&format!("font_{test}"));
        let source = root.join("source");
        tiny(&source, &font);
        change(&source);
        let error = cook(&source, &root.join("out")).expect_err(test);
        assert!(expected(&error), "{test}: {error:?}");
        if let CookError::Layout { .. } = error {
            assert!(layout(&error, ""), "{test}: names the font");
        }
    }
}

#[test]
fn a_font_s_name_and_file_are_its_own() {
    // A PNG of the font's name that is not one of its pages.
    let root = dir("font_name");
    let source = root.join("source");
    tiny(&source, FONT);
    write(&source, "fonts/tiny.png", plain(2, 2, [1, 2, 3, 255]));
    assert!(matches!(
        cook(&source, &root.join("out")),
        Err(CookError::SpriteNameTaken { name, first, second })
            if name == "fonts/tiny" && first == "fonts/tiny.png" && second == "fonts/tiny.fnt"
    ));
    // A group whose atlas file would be the font's file.
    let root = dir("font_file");
    let source = root.join("source");
    tiny(&source, FONT);
    write(
        &source,
        "fonts/tiny.font/a.png",
        plain(2, 2, [1, 2, 3, 255]),
    );
    assert!(matches!(
        cook(&source, &root.join("out")),
        Err(CookError::OutputCollision { first, second })
            if first == "atlas/fonts/tiny.font.kbj" && second == first
    ));
}

#[test]
fn changing_a_font_or_its_page_cooks_only_its_group_again() {
    let root = dir("font_changed");
    let (source, out) = (root.join("source"), root.join("out"));
    tiny(&source, FONT);
    write(&source, "props/rock.png", plain(4, 4, [90, 90, 100, 255]));
    cook(&source, &out).expect("a cook");
    let only_fonts = [
        ("atlas/fonts".to_string(), true),
        ("atlas/props".to_string(), false),
    ];
    tiny(&source, &FONT.replacen("xadvance=3", "xadvance=4", 1));
    assert_eq!(jobs(&cook(&source, &out).expect("a cook")), only_fonts);
    let font = kbj(&out, "atlas/fonts/tiny.font.kbj");
    let space = &font
        .get("glyphs")
        .and_then(Value::as_array)
        .expect("glyphs")[0];
    assert_eq!(number(space, "advance"), 4);
    write(&source, "fonts/tiny_1.png", page(&[(0, 0, 3, 4, OPAQUE)]));
    assert_eq!(jobs(&cook(&source, &out).expect("a cook")), only_fonts);
    let clean = root.join("clean");
    cook(&source, &clean).expect("a cook");
    assert_eq!(files(&out), files(&clean));
}
