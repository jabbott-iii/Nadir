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

//! BMFont tests: hand-written files in the dialects of the generators that
//! write the text format (AngelCode's BMFont, libGDX's Hiero, and the
//! smaller tools), each checked field by field, and damaged files. The
//! error cases, one by one, are beside the parser, in `src/bmfont.rs`.

use nadir_codec::bmfont::{self, Font, Glyph, Kerning};
use nadir_codec::{CodecError, Limits};
use nadir_core::testing::check;

const VECTORS: [&str; 6] = [
    "angelcode",
    "angelcode_ansi_fallback",
    "hiero_two_pages",
    "hiero_unicode_0",
    "crlf_tabs_bom",
    "minimal",
];

fn vector(name: &str) -> String {
    let path = format!("{}/tests/bmfont/{name}.fnt", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn parse(text: &str) -> Result<Font, CodecError> {
    bmfont::parse(text, Limits::default())
}

const fn glyph(id: u32, rect: [u32; 4], offsets: [i32; 3], page: u32, channel: u8) -> Glyph {
    Glyph {
        id,
        x: rect[0],
        y: rect[1],
        width: rect[2],
        height: rect[3],
        x_offset: offsets[0],
        y_offset: offsets[1],
        x_advance: offsets[2],
        page,
        channel,
    }
}

#[test]
fn angelcodes_bmfont() {
    let font = parse(&vector("angelcode")).unwrap();
    assert_eq!(
        font,
        Font {
            face: "Arial".to_string(),
            size: 32,
            line_height: 32,
            base: 26,
            scale_w: 256,
            scale_h: 256,
            pages: vec!["arial_0.png".to_string()],
            glyphs: vec![
                glyph(32, [0, 0, 0, 0], [0, 26, 9], 0, 15),
                glyph(65, [0, 0, 20, 21], [-1, 5, 19], 0, 15),
                glyph(86, [21, 0, 21, 21], [-1, 5, 19], 0, 15),
            ],
            kernings: vec![
                Kerning {
                    first: 65,
                    second: 86,
                    amount: -2
                },
                Kerning {
                    first: 86,
                    second: 65,
                    amount: -2
                },
            ],
            channels: [Some(1), Some(0), Some(0), Some(0)],
            fallback: None,
        }
    );
}

#[test]
fn hiero_with_two_pages() {
    let font = parse(&vector("hiero_two_pages")).unwrap();
    assert_eq!(font.face, "Hack Regular");
    assert_eq!(font.pages, ["hack1.png", "hack2.png"]);
    assert_eq!(
        font.glyphs,
        [
            glyph(33, [0, 0, 6, 20], [0, 3, 13], 0, 0),
            glyph(8364, [100, 40, 28, 24], [-1, 1, 14], 1, 0),
        ]
    );
    assert!(font.kernings.is_empty());
}

#[test]
fn crlf_tabs_a_byte_order_mark_and_keys_of_another_tool() {
    let font = parse(&vector("crlf_tabs_bom")).unwrap();
    assert_eq!((font.face.as_str(), font.size), ("Pixel", -8));
    assert_eq!(font.pages, ["pixel.png"]);
    // No `chnl`: all channels. `letter="a"` is ignored.
    assert_eq!(font.glyphs, [glyph(97, [8, 8, 8, 8], [0, 1, 6], 0, 15)]);
}

#[test]
fn a_font_of_no_glyphs() {
    let font = parse(&vector("minimal")).unwrap();
    assert_eq!(
        (font.line_height, font.base, font.scale_w, font.scale_h),
        (10, 8, 8, 8)
    );
    assert!(font.glyphs.is_empty());
}

#[test]
fn every_vector_cut_short_is_parsed_or_rejected_without_a_panic() {
    for name in VECTORS {
        let text = vector(name);
        for (end, _) in text.char_indices() {
            match parse(&text[..end]) {
                Ok(_) | Err(CodecError::BmFont { .. }) => {}
                Err(other) => panic!("{name}, {end} bytes: {other}"),
            }
        }
    }
}

#[test]
fn damaged_files_are_parsed_or_rejected_without_a_panic() {
    let texts: Vec<String> = VECTORS.into_iter().map(vector).collect();
    let pieces = [
        " ",
        "=",
        "\"",
        "\n",
        "-",
        "9999999999",
        "char",
        "page",
        "0",
        "\t",
        "é",
    ];
    check(
        "bmfont damage",
        3000,
        |rng| {
            let mut text: Vec<char> = rng.choose(&texts).expect("texts").chars().collect();
            for _ in 0..1 + rng.range(0..4u32) {
                let at = rng.range(0..text.len() as u32 + 1) as usize;
                let piece = rng.choose(&pieces).expect("pieces");
                if rng.range(0..2u32) == 0 && at < text.len() {
                    text.remove(at);
                } else {
                    for (i, c) in piece.chars().enumerate() {
                        text.insert(at + i, c);
                    }
                }
            }
            text.into_iter().collect::<String>()
        },
        |text| match parse(text) {
            Ok(font)
                if font
                    .glyphs
                    .iter()
                    .all(|glyph| (glyph.page as usize) < font.pages.len()) =>
            {
                Ok(())
            }
            Ok(_) => Err("a glyph on a page that does not exist".to_string()),
            Err(CodecError::BmFont { .. }) => Ok(()),
            Err(other) => Err(format!("not a BMFont error: {other:?}")),
        },
    );
}

#[test]
fn angelcodes_ansi_ids_and_fallback_glyph() {
    let font = parse(&vector("angelcode_ansi_fallback")).unwrap();
    // `charset="ANSI"`: id 145 is Windows-1252's left single quote.
    let ids: Vec<u32> = font.glyphs.iter().map(|glyph| glyph.id).collect();
    assert_eq!(ids, [97, 0x2018]);
    assert_eq!(
        font.kernings,
        [Kerning {
            first: 0x2018,
            second: 97,
            amount: -1
        }]
    );
    // id -1, counted in `chars count`, is the glyph for missing ones.
    assert_eq!(
        font.fallback,
        Some(glyph(0, [0, 0, 6, 12], [1, 6, 8], 0, 15))
    );
    assert_eq!(font.size, -20);
    assert_eq!(font.channels, [Some(0), Some(4), Some(4), Some(4)]);
}

#[test]
fn hieros_unicode_0_holds_code_points() {
    // Hiero writes `unicode=0` and an empty charset, with code points.
    let font = parse(&vector("hiero_unicode_0")).unwrap();
    assert_eq!(font.glyphs[0].id, 0x2018);
    assert_eq!(font.channels, [None; 4]);
}
