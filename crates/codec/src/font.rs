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

//! The font file, `atlas/<font>.font.kbj`, version 1: a bitmap font as the
//! cook writes it from a BMFont, with its glyphs' frames in the atlas file
//! beside it (`docs/subsystems/assets-and-codecs.md`, "Font file"). The
//! cook writes it, and reads it back with [`parse`], as the game's font
//! asset will.
//!
//! ```text
//! { "version": 1, "face": "Pixel", "size": 16, "line_height": 18, "base": 14,
//!   "atlas": "atlas/fonts.kbj", "sprite": "fonts/pixel",
//!   "glyphs": [ { "id": 32, "x": 0, "y": 13, "advance": 4 },
//!               { "id": 65, "frame": 0, "x": 0, "y": 2, "advance": 7 } ],
//!   "kerning": [ [65, 86, -1] ],
//!   "fallback": { "frame": 9, "x": 0, "y": 2, "advance": 7 } }
//! ```
//!
//! It is KBJ, read with the KBJ decoder and its limits, then checked:
//!
//! - **Members:** each object has the members above and no others, but a
//!   glyph's `frame`, which a glyph with no pixels leaves out; and the
//!   version is 1: any other is not supported.
//! - **Numbers:** whole; `size`, the glyphs' and the fallback's `x`, `y`
//!   and `advance`, and a kerning pair's amount fit 32 bits, signed, as
//!   BMFont's do (a negative `size` is BMFont's "match the characters'
//!   height"); `line_height`, `base` and the frames fit 32 bits, unsigned.
//! - **Strings:** `face` may be empty, as BMFont's may; `atlas` and
//!   `sprite` may not.
//! - **Characters:** the glyphs' ids and a kerning pair's two are code
//!   points of characters: from 0 to U+10FFFF, and no surrogate.
//! - **Order:** the glyphs by id and the kerning pairs by their first, then
//!   their second, each after the one before, with no two the same, as the
//!   cook writes them; a kerning pair is a list of three.
//! - **The fallback,** what a missing character draws, has a frame.
//!
//! Not checked here: that a frame is one of the sprite's in the atlas file,
//! which the font asset reads beside it; and what BMFont does not promise,
//! that a kerning pair's characters are the font's, or that `base` is
//! within `line_height`.
//!
//! A problem is a [`CodecError::Font`], which names where it is, such as
//! `glyphs[3].id`.

use std::fmt;

use crate::json::Value;
use crate::{CodecError, Limits, kbj};

/// The version of the font file that [`parse`] reads.
pub const VERSION: i64 = 1;

/// A font file: a bitmap font's metrics, its glyphs and its kerning.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FontFile {
    /// The font's name, BMFont's `face`: it may be empty.
    pub face: String,
    /// BMFont's `size`: negative where it matched the characters' height,
    /// rather than the cell's.
    pub size: i32,
    /// The distance from one line to the next, in pixels.
    pub line_height: u32,
    /// The distance from the top of a line to its baseline.
    pub base: u32,
    /// The atlas file that holds the glyphs' frames, such as
    /// `atlas/fonts.kbj`.
    pub atlas: String,
    /// The sprite of that atlas whose frames the glyphs are, such as
    /// `fonts/pixel`.
    pub sprite: String,
    /// The glyphs, by id.
    pub glyphs: Vec<Glyph>,
    /// The kerning pairs, by their first character, then their second.
    pub kerning: Vec<Kerning>,
    /// What a character the font lacks draws: BMFont's fallback glyph, or
    /// its `?`.
    pub fallback: Fallback,
}

/// A character's glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Glyph {
    /// The character.
    pub id: char,
    /// Its frame of the sprite, trimmed: `None` for one with no pixels, as
    /// a space.
    pub frame: Option<u32>,
    /// BMFont's offset of its untrimmed rectangle from the pen, across; the
    /// frame's offset places the trimmed pixels in it.
    pub x: i32,
    /// The same offset down, from the top of the line.
    pub y: i32,
    /// How far the pen moves after it.
    pub advance: i32,
}

/// What a character the font lacks draws: a glyph with pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Fallback {
    /// Its frame of the sprite.
    pub frame: u32,
    /// As [`Glyph::x`].
    pub x: i32,
    /// As [`Glyph::y`].
    pub y: i32,
    /// As [`Glyph::advance`].
    pub advance: i32,
}

/// How much closer, or farther, the pen moves between two characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Kerning {
    /// The character before.
    pub first: char,
    /// The character after.
    pub second: char,
    /// The pixels added to the first's advance: negative brings them
    /// closer.
    pub amount: i32,
}

/// What is wrong with a font file. See [`CodecError::Font`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The version is not 1.
    Version {
        /// The version the file gives.
        found: i64,
    },
    /// A value is not of the type its member needs.
    WrongType {
        /// What it should be: "an object", "a whole number" and so on.
        expected: &'static str,
    },
    /// An object lacks a member.
    MissingMember {
        /// The member.
        name: &'static str,
    },
    /// An object has a member the format does not have.
    UnknownMember {
        /// The member.
        name: String,
    },
    /// A whole number is outside its member's range.
    OutOfRange {
        /// The smallest it may be.
        min: i64,
        /// The largest it may be.
        max: i64,
    },
    /// An id is not the code point of a character: a negative number, a
    /// surrogate, or past U+10FFFF.
    NotACharacter,
    /// A list does not have the number of items it needs.
    WrongLength {
        /// How many it needs.
        expected: usize,
    },
    /// The atlas or the sprite is an empty string.
    EmptyString,
    /// A glyph's id, or a kerning pair, does not come after the one before
    /// it.
    NotSorted,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Version { found } => write!(
                f,
                "version {found} is not supported: this build reads version {VERSION}"
            ),
            Self::WrongType { expected } => write!(f, "expected {expected}"),
            Self::MissingMember { name } => write!(f, "`{name}` is missing"),
            Self::UnknownMember { name } => write!(f, "`{name}` is not a member it has"),
            Self::OutOfRange { min, max } => write!(f, "must be from {min} to {max}"),
            Self::NotACharacter => write!(
                f,
                "must be the code point of a character: from 0 to 0x10FFFF, and not a surrogate \
                 (0xD800 to 0xDFFF)"
            ),
            Self::WrongLength { expected } => write!(f, "must have {expected} items"),
            Self::EmptyString => write!(f, "must not be an empty string"),
            Self::NotSorted => write!(f, "must come after the one before it, with no two the same"),
        }
    }
}

/// Reads a font file: KBJ, decoded within `limits`, then checked as the
/// module says.
///
/// # Errors
///
/// [`CodecError::Kbj`] if the bytes are not KBJ, and [`CodecError::Font`],
/// naming where, if they are not a font file of version 1.
pub fn parse(bytes: &[u8], limits: Limits) -> Result<FontFile, CodecError> {
    read(&kbj::decode(bytes, limits)?)
}

/// Where a value is in the file, for errors: made into text only for one.
#[derive(Clone, Copy)]
enum At<'a> {
    Root,
    Member(&'a At<'a>, &'static str),
    Item(&'a At<'a>, usize),
}

impl At<'_> {
    fn error(&self, kind: ErrorKind) -> CodecError {
        CodecError::Font {
            at: self.to_string(),
            kind,
        }
    }
}

impl fmt::Display for At<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root => Ok(()),
            Self::Member(At::Root, name) => write!(f, "{name}"),
            Self::Member(parent, name) => write!(f, "{parent}.{name}"),
            Self::Item(parent, index) => write!(f, "{parent}[{index}]"),
        }
    }
}

/// An object's members, checked against those it may have.
struct Members<'v> {
    members: &'v [(String, Value)],
}

impl<'v> Members<'v> {
    fn of(value: &'v Value, at: &At<'_>, allowed: &[&'static str]) -> Result<Self, CodecError> {
        let members = value.as_object().ok_or_else(|| {
            at.error(ErrorKind::WrongType {
                expected: "an object",
            })
        })?;
        if let Some((name, _)) = members
            .iter()
            .find(|(name, _)| !allowed.contains(&name.as_str()))
        {
            return Err(at.error(ErrorKind::UnknownMember { name: name.clone() }));
        }
        Ok(Self { members })
    }

    fn get(&self, name: &str) -> Option<&'v Value> {
        self.members
            .iter()
            .find(|(member, _)| member == name)
            .map(|(_, value)| value)
    }

    /// The member `name` of the object at `at`, and where it is.
    fn require<'a>(
        &self,
        name: &'static str,
        at: &'a At<'a>,
    ) -> Result<(&'v Value, At<'a>), CodecError> {
        self.get(name)
            .map(|value| (value, At::Member(at, name)))
            .ok_or_else(|| at.error(ErrorKind::MissingMember { name }))
    }
}

fn array<'v>(value: &'v Value, at: &At<'_>) -> Result<&'v [Value], CodecError> {
    value
        .as_array()
        .ok_or_else(|| at.error(ErrorKind::WrongType { expected: "a list" }))
}

/// A string, which may be empty only if `empty` says so.
fn string(value: &Value, at: &At<'_>, empty: bool) -> Result<String, CodecError> {
    match value.as_str() {
        Some("") if !empty => Err(at.error(ErrorKind::EmptyString)),
        Some(text) => Ok(text.to_string()),
        None => Err(at.error(ErrorKind::WrongType {
            expected: "a string",
        })),
    }
}

/// A whole number.
fn int(value: &Value, at: &At<'_>) -> Result<i64, CodecError> {
    match value {
        Value::Int(number) => Ok(*number),
        _ => Err(at.error(ErrorKind::WrongType {
            expected: "a whole number",
        })),
    }
}

/// A whole number that fits 32 bits, signed.
fn signed(value: &Value, at: &At<'_>) -> Result<i32, CodecError> {
    i32::try_from(int(value, at)?).map_err(|_| {
        at.error(ErrorKind::OutOfRange {
            min: i32::MIN.into(),
            max: i32::MAX.into(),
        })
    })
}

/// A whole number that fits 32 bits, unsigned.
fn unsigned(value: &Value, at: &At<'_>) -> Result<u32, CodecError> {
    u32::try_from(int(value, at)?).map_err(|_| {
        at.error(ErrorKind::OutOfRange {
            min: 0,
            max: u32::MAX.into(),
        })
    })
}

/// The code point of a character.
fn character(value: &Value, at: &At<'_>) -> Result<char, CodecError> {
    u32::try_from(int(value, at)?)
        .ok()
        .and_then(char::from_u32)
        .ok_or_else(|| at.error(ErrorKind::NotACharacter))
}

fn read(value: &Value) -> Result<FontFile, CodecError> {
    let root = At::Root;
    // The version first: a later one may have members this one lacks, and
    // is not supported, rather than wrong.
    let at = At::Member(&root, "version");
    let version = value
        .as_object()
        .and_then(|members| members.iter().find(|(name, _)| name == "version"));
    match version.map(|(_, version)| version) {
        Some(Value::Int(VERSION)) => {}
        Some(Value::Int(found)) => return Err(at.error(ErrorKind::Version { found: *found })),
        Some(_) => {
            return Err(at.error(ErrorKind::WrongType {
                expected: "a whole number",
            }));
        }
        // Not an object, or no version: said below.
        None => {}
    }
    let top = Members::of(
        value,
        &root,
        &[
            "version",
            "face",
            "size",
            "line_height",
            "base",
            "atlas",
            "sprite",
            "glyphs",
            "kerning",
            "fallback",
        ],
    )?;
    top.require("version", &root)?;
    let field = |name| top.require(name, &root);
    let (face, at) = field("face")?;
    let face = string(face, &at, true)?;
    let (size, at) = field("size")?;
    let size = signed(size, &at)?;
    let (line_height, at) = field("line_height")?;
    let line_height = unsigned(line_height, &at)?;
    let (base, at) = field("base")?;
    let base = unsigned(base, &at)?;
    let (atlas, at) = field("atlas")?;
    let atlas = string(atlas, &at, false)?;
    let (sprite, at) = field("sprite")?;
    let sprite = string(sprite, &at, false)?;

    let (items, at) = field("glyphs")?;
    let items = array(items, &at)?;
    let mut glyphs: Vec<Glyph> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = At::Item(&at, index);
        let glyph = glyph(item, &at)?;
        if glyphs.last().is_some_and(|before| before.id >= glyph.id) {
            return Err(At::Member(&at, "id").error(ErrorKind::NotSorted));
        }
        glyphs.push(glyph);
    }

    let (items, at) = field("kerning")?;
    let items = array(items, &at)?;
    let mut kerning: Vec<Kerning> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = At::Item(&at, index);
        let pair = kerning_pair(item, &at)?;
        if kerning
            .last()
            .is_some_and(|before| (before.first, before.second) >= (pair.first, pair.second))
        {
            return Err(at.error(ErrorKind::NotSorted));
        }
        kerning.push(pair);
    }

    let (fallback, at) = field("fallback")?;
    let fallback = self::fallback(fallback, &at)?;
    Ok(FontFile {
        face,
        size,
        line_height,
        base,
        atlas,
        sprite,
        glyphs,
        kerning,
        fallback,
    })
}

fn glyph(value: &Value, at: &At<'_>) -> Result<Glyph, CodecError> {
    let members = Members::of(value, at, &["id", "frame", "x", "y", "advance"])?;
    let (id, id_at) = members.require("id", at)?;
    let frame = match members.get("frame") {
        Some(frame) => Some(unsigned(frame, &At::Member(at, "frame"))?),
        None => None,
    };
    let (x, x_at) = members.require("x", at)?;
    let (y, y_at) = members.require("y", at)?;
    let (advance, advance_at) = members.require("advance", at)?;
    Ok(Glyph {
        id: character(id, &id_at)?,
        frame,
        x: signed(x, &x_at)?,
        y: signed(y, &y_at)?,
        advance: signed(advance, &advance_at)?,
    })
}

fn fallback(value: &Value, at: &At<'_>) -> Result<Fallback, CodecError> {
    let members = Members::of(value, at, &["frame", "x", "y", "advance"])?;
    let (frame, frame_at) = members.require("frame", at)?;
    let (x, x_at) = members.require("x", at)?;
    let (y, y_at) = members.require("y", at)?;
    let (advance, advance_at) = members.require("advance", at)?;
    Ok(Fallback {
        frame: unsigned(frame, &frame_at)?,
        x: signed(x, &x_at)?,
        y: signed(y, &y_at)?,
        advance: signed(advance, &advance_at)?,
    })
}

/// A kerning pair: its first character, its second, and the amount.
fn kerning_pair(value: &Value, at: &At<'_>) -> Result<Kerning, CodecError> {
    let [first, second, amount] = array(value, at)? else {
        return Err(at.error(ErrorKind::WrongLength { expected: 3 }));
    };
    Ok(Kerning {
        first: character(first, &At::Item(at, 0))?,
        second: character(second, &At::Item(at, 1))?,
        amount: signed(amount, &At::Item(at, 2))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    /// The bytes of the font file `text` describes, in JSON.
    fn kbj(text: &str) -> Vec<u8> {
        let value = json::parse(text, Limits::default()).expect("the test's JSON");
        kbj::encode(&value, Limits::default()).expect("encodable")
    }

    fn parsed(text: &str) -> Result<FontFile, CodecError> {
        parse(&kbj(text), Limits::default())
    }

    /// The error `text` gives: where, and what.
    fn error(text: &str) -> (String, ErrorKind) {
        match parsed(text) {
            Err(CodecError::Font { at, kind }) => (at, kind),
            other => panic!("expected a font file error, got {other:?}"),
        }
    }

    /// A font file as the cook writes it.
    const FILE: &str = r#"{ "version": 1, "face": "Pixel", "size": -16, "line_height": 18, "base": 14,
        "atlas": "atlas/fonts.kbj", "sprite": "fonts/pixel",
        "glyphs": [ { "id": 32, "x": 0, "y": 13, "advance": 4 },
                    { "id": 65, "frame": 0, "x": -1, "y": 2, "advance": 7 },
                    { "id": 86, "frame": 1, "x": 0, "y": 2, "advance": 7 },
                    { "id": 8364, "frame": 2, "x": 0, "y": -2, "advance": 8 } ],
        "kerning": [ [65, 86, -1], [86, 65, -2], [86, 8364, 1] ],
        "fallback": { "frame": 3, "x": -1, "y": 2, "advance": 7 } }"#;

    /// [`FILE`] with `from` replaced by `to`.
    fn with(from: &str, to: &str) -> String {
        assert!(FILE.contains(from), "{from}");
        FILE.replacen(from, to, 1)
    }

    #[test]
    fn a_file_as_the_cook_writes_it_is_read() {
        let font = parsed(FILE).expect("a font file");
        assert_eq!(font.face, "Pixel");
        assert_eq!(font.size, -16);
        assert_eq!((font.line_height, font.base), (18, 14));
        assert_eq!(font.atlas, "atlas/fonts.kbj");
        assert_eq!(font.sprite, "fonts/pixel");
        assert_eq!(
            font.glyphs[..2],
            [
                Glyph {
                    id: ' ',
                    frame: None,
                    x: 0,
                    y: 13,
                    advance: 4,
                },
                Glyph {
                    id: 'A',
                    frame: Some(0),
                    x: -1,
                    y: 2,
                    advance: 7,
                },
            ]
        );
        assert_eq!((font.glyphs[3].id, font.glyphs[3].y), ('€', -2));
        assert_eq!(
            font.kerning,
            [
                Kerning {
                    first: 'A',
                    second: 'V',
                    amount: -1,
                },
                Kerning {
                    first: 'V',
                    second: 'A',
                    amount: -2,
                },
                Kerning {
                    first: 'V',
                    second: '€',
                    amount: 1,
                },
            ]
        );
        assert_eq!(
            font.fallback,
            Fallback {
                frame: 3,
                x: -1,
                y: 2,
                advance: 7,
            }
        );
    }

    #[test]
    fn a_face_may_be_empty_and_a_font_have_no_glyphs_or_kerning() {
        let font = parsed(&with(r#""face": "Pixel""#, r#""face": """#).replacen(
            &FILE[FILE.find(r#""glyphs""#).unwrap()..FILE.find(r#""fallback""#).unwrap()],
            r#""glyphs": [], "kerning": [], "#,
            1,
        ))
        .expect("a font file");
        assert_eq!(font.face, "");
        assert!(font.glyphs.is_empty() && font.kerning.is_empty());
    }

    #[test]
    fn the_atlas_and_the_sprite_are_named() {
        for (from, to, at) in [
            (r#""atlas/fonts.kbj""#, r#""""#, "atlas"),
            (r#""fonts/pixel""#, r#""""#, "sprite"),
        ] {
            assert_eq!(
                error(&with(from, to)),
                (at.to_string(), ErrorKind::EmptyString)
            );
        }
        assert_eq!(
            error(&with(r#""fonts/pixel""#, "7")),
            (
                "sprite".to_string(),
                ErrorKind::WrongType {
                    expected: "a string"
                }
            )
        );
    }

    #[test]
    fn another_version_is_not_supported_whatever_it_holds() {
        for (version, found) in [("2", 2), ("0", 0), ("-1", -1)] {
            let later = with(
                r#""version": 1"#,
                &format!(r#""version": {version}, "pages": []"#),
            );
            assert_eq!(
                error(&later),
                ("version".to_string(), ErrorKind::Version { found })
            );
        }
        assert_eq!(
            error(&with(r#""version": 1"#, r#""version": "1""#)),
            (
                "version".to_string(),
                ErrorKind::WrongType {
                    expected: "a whole number"
                }
            )
        );
        let error = CodecError::Font {
            at: "version".to_string(),
            kind: ErrorKind::Version { found: 2 },
        };
        assert_eq!(
            error.to_string(),
            "font file error at `version`: version 2 is not supported: this build reads version 1"
        );
    }

    #[test]
    fn each_object_has_its_members_and_no_others() {
        for name in [
            "version",
            "face",
            "size",
            "line_height",
            "base",
            "atlas",
            "sprite",
            "glyphs",
            "kerning",
            "fallback",
        ] {
            let mut value = json::parse(FILE, Limits::default()).unwrap();
            let Value::Object(members) = &mut value else {
                unreachable!()
            };
            members.retain(|(member, _)| member != name);
            let bytes = kbj::encode(&value, Limits::default()).unwrap();
            match parse(&bytes, Limits::default()) {
                Err(CodecError::Font { at, kind }) => {
                    assert_eq!((at.as_str(), kind), ("", ErrorKind::MissingMember { name }));
                }
                other => panic!("without `{name}`: {other:?}"),
            }
        }
        assert_eq!(
            error(&with(r#""base": 14,"#, r#""base": 14, "pages": [],"#)),
            (
                String::new(),
                ErrorKind::UnknownMember {
                    name: "pages".to_string()
                }
            )
        );
        for name in ["id", "x", "y", "advance"] {
            let glyph = r#"{ "id": 32, "x": 0, "y": 13, "advance": 4 }"#;
            let without = glyph.replacen(&format!(r#""{name}": "#), r#""gone": "#, 1);
            assert_eq!(
                error(&with(glyph, &without)),
                (
                    "glyphs[0]".to_string(),
                    ErrorKind::UnknownMember {
                        name: "gone".to_string()
                    }
                )
            );
        }
        assert_eq!(
            error(&with(r#""fallback": { "frame": 3, "#, r#""fallback": { "#)),
            (
                "fallback".to_string(),
                ErrorKind::MissingMember { name: "frame" }
            )
        );
        assert_eq!(
            error(&with(
                r#""advance": 7 } }"#,
                r#""advance": 7, "id": 63 } }"#
            )),
            (
                "fallback".to_string(),
                ErrorKind::UnknownMember {
                    name: "id".to_string()
                }
            )
        );
    }

    #[test]
    fn a_glyph_lacking_a_member_names_it() {
        for (from, to, name) in [
            (r#"{ "id": 32, "#, "{ ", "id"),
            (r#""id": 32, "x": 0, "#, r#""id": 32, "#, "x"),
            (r#""x": 0, "y": 13, "#, r#""x": 0, "#, "y"),
            (r#""y": 13, "advance": 4 }"#, r#""y": 13 }"#, "advance"),
        ] {
            assert_eq!(
                error(&with(from, to)),
                ("glyphs[0]".to_string(), ErrorKind::MissingMember { name })
            );
        }
    }

    #[test]
    fn numbers_are_whole_and_fit_their_members() {
        let signed = ErrorKind::OutOfRange {
            min: i64::from(i32::MIN),
            max: i64::from(i32::MAX),
        };
        let unsigned = ErrorKind::OutOfRange {
            min: 0,
            max: i64::from(u32::MAX),
        };
        for (from, to, at, kind) in [
            (r#""size": -16"#, r#""size": 2147483648"#, "size", &signed),
            (r#""size": -16"#, r#""size": -2147483649"#, "size", &signed),
            (
                r#""line_height": 18"#,
                r#""line_height": -1"#,
                "line_height",
                &unsigned,
            ),
            (r#""base": 14"#, r#""base": 4294967296"#, "base", &unsigned),
            (
                r#""frame": 0,"#,
                r#""frame": -1,"#,
                "glyphs[1].frame",
                &unsigned,
            ),
            (
                r#""x": -1,"#,
                r#""x": -2147483649,"#,
                "glyphs[1].x",
                &signed,
            ),
            (
                r#""advance": 8 }"#,
                r#""advance": 2147483648 }"#,
                "glyphs[3].advance",
                &signed,
            ),
            (
                r#"[86, 65, -2]"#,
                r#"[86, 65, 2147483648]"#,
                "kerning[1][2]",
                &signed,
            ),
            (
                r#""frame": 3,"#,
                r#""frame": 4294967296,"#,
                "fallback.frame",
                &unsigned,
            ),
            (r#""y": 13,"#, r#""y": 2147483648,"#, "glyphs[0].y", &signed),
            (
                r#""x": -1, "y": 2, "advance": 7 } }"#,
                r#""x": -2147483649, "y": 2, "advance": 7 } }"#,
                "fallback.x",
                &signed,
            ),
            (
                r#""y": 2, "advance": 7 } }"#,
                r#""y": 2147483648, "advance": 7 } }"#,
                "fallback.y",
                &signed,
            ),
            (
                r#""advance": 7 } }"#,
                r#""advance": 2147483648 } }"#,
                "fallback.advance",
                &signed,
            ),
        ] {
            assert_eq!(
                error(&with(from, to)),
                (at.to_string(), kind.clone()),
                "{to}"
            );
        }
        // The edges themselves are taken.
        let font = parsed(
            &with(r#""size": -16"#, r#""size": -2147483648"#)
                .replacen(r#""base": 14"#, r#""base": 4294967295"#, 1)
                .replacen(r#""x": -1,"#, r#""x": 2147483647,"#, 1),
        )
        .expect("a font file");
        assert_eq!(
            (font.size, font.base, font.glyphs[1].x),
            (i32::MIN, u32::MAX, i32::MAX)
        );
        for (from, to, at) in [
            (r#""size": -16"#, r#""size": 16.0"#, "size"),
            (r#""x": -1,"#, r#""x": "-1","#, "glyphs[1].x"),
            (r#""frame": 0,"#, r#""frame": null,"#, "glyphs[1].frame"),
            (
                r#""y": 2, "advance": 7 } }"#,
                r#""y": true, "advance": 7 } }"#,
                "fallback.y",
            ),
        ] {
            assert_eq!(
                error(&with(from, to)),
                (
                    at.to_string(),
                    ErrorKind::WrongType {
                        expected: "a whole number"
                    }
                ),
                "{to}"
            );
        }
    }

    #[test]
    fn ids_are_characters() {
        for id in ["55296", "57343", "1114112", "-1"] {
            assert_eq!(
                error(&with(r#""id": 32,"#, &format!(r#""id": {id},"#))),
                ("glyphs[0].id".to_string(), ErrorKind::NotACharacter),
                "{id}"
            );
        }
        assert_eq!(
            error(&with("[65, 86, -1]", "[65, 55296, -1]")),
            ("kerning[0][1]".to_string(), ErrorKind::NotACharacter)
        );
        assert_eq!(
            error(&with("[65, 86, -1]", "[-65, 86, -1]")),
            ("kerning[0][0]".to_string(), ErrorKind::NotACharacter)
        );
        // The last character, and the first, are characters.
        let font = parsed(&with(r#""id": 32,"#, r#""id": 0,"#).replacen(
            r#""id": 8364,"#,
            r#""id": 1114111,"#,
            1,
        ))
        .expect("a font file");
        assert_eq!(font.glyphs[0].id, '\0');
        assert_eq!(font.glyphs[3].id, char::MAX);
        assert_eq!(
            CodecError::Font {
                at: "glyphs[0].id".to_string(),
                kind: ErrorKind::NotACharacter
            }
            .to_string(),
            "font file error at `glyphs[0].id`: must be the code point of a character: from 0 to \
             0x10FFFF, and not a surrogate (0xD800 to 0xDFFF)"
        );
    }

    #[test]
    fn glyphs_and_kerning_pairs_come_in_order_once_each() {
        for (from, to, at) in [
            (r#""id": 86,"#, r#""id": 65,"#, "glyphs[2].id"),
            (r#""id": 86,"#, r#""id": 64,"#, "glyphs[2].id"),
            ("[86, 65, -2]", "[65, 86, -2]", "kerning[1]"),
            ("[86, 65, -2]", "[64, 90, -2]", "kerning[1]"),
            ("[86, 8364, 1]", "[86, 65, 1]", "kerning[2]"),
            ("[86, 8364, 1]", "[86, 64, 1]", "kerning[2]"),
        ] {
            assert_eq!(
                error(&with(from, to)),
                (at.to_string(), ErrorKind::NotSorted),
                "{to}"
            );
        }
    }

    #[test]
    fn a_kerning_pair_is_three_numbers() {
        for to in ["[65, 86]", "[65, 86, -1, 0]", "[]"] {
            assert_eq!(
                error(&with("[65, 86, -1]", to)),
                (
                    "kerning[0]".to_string(),
                    ErrorKind::WrongLength { expected: 3 }
                ),
                "{to}"
            );
        }
        assert_eq!(
            error(&with("[65, 86, -1]", r#"{ "first": 65 }"#)),
            (
                "kerning[0]".to_string(),
                ErrorKind::WrongType { expected: "a list" }
            )
        );
    }

    #[test]
    fn what_is_not_a_font_file_says_so() {
        assert!(matches!(
            parse(b"not KBJ", Limits::default()),
            Err(CodecError::Kbj { .. })
        ));
        assert_eq!(
            error("[1]"),
            (
                String::new(),
                ErrorKind::WrongType {
                    expected: "an object"
                }
            )
        );
        assert_eq!(
            error(&with(
                r#""glyphs": [ { "id": 32, "x": 0, "y": 13, "advance": 4 },"#,
                r#""glyphs": [ 32,"#
            )),
            (
                "glyphs[0]".to_string(),
                ErrorKind::WrongType {
                    expected: "an object"
                }
            )
        );
        assert_eq!(
            CodecError::Font {
                at: String::new(),
                kind: ErrorKind::MissingMember { name: "face" }
            }
            .to_string(),
            "font file error in the file: `face` is missing"
        );
    }
}
