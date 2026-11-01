//! The atlas file, `atlas/<group>.kbj`, version 1: where the cook packed
//! each sprite's frames (`docs/subsystems/assets-and-codecs.md`, "Atlas
//! file"). The cook writes it; the game's atlas asset reads it with
//! [`parse`].
//!
//! ```text
//! { "version": 1,
//!   "pages": [ { "path": "atlas/sprites_0.qoi", "w": 128, "h": 128 } ],
//!   "sprites": [
//!     { "name": "sprites/person",
//!       "frames": [ { "page": 0, "x": 33, "y": 85, "w": 10, "h": 20,
//!                     "ox": 1, "oy": 0, "sw": 12, "sh": 20 } ],
//!       "pivot": [6.0, 20.0] },
//!     { "name": "sprites/tiles",
//!       "frames": [ { "page": 1, "x": 0, "y": 0, "w": 128, "h": 64,
//!                     "ox": 0, "oy": 0, "sw": 128, "sh": 64 } ],
//!       "tiles": [16, 16] } ] }
//! ```
//!
//! It is KBJ, read with the KBJ decoder and its limits, then checked:
//!
//! - **Members:** each object has the members above and no others, and the
//!   version is 1: any other is not supported.
//! - **Pages:** one at least, each with a path and a size of 1 to 4096 on
//!   each side.
//! - **Sprites:** each with a name, and the names in byte order with no two
//!   the same, as the cook writes them; each with one frame at least, and
//!   either a pivot of two finite numbers or a tile size of two whole
//!   numbers of 1 or more, not both.
//! - **Frames:** whole numbers that fit 32 bits. A frame lies on a page that
//!   exists, inside it, and inside its untrimmed cell of `sw` × `sh`, which
//!   is 1 × 1 at least. An empty frame, of width and height 0, is on page 0
//!   at (0, 0) with no offset; a frame of one side 0 is not one.
//! - **Tile sets:** one frame, which is the whole of its page, untrimmed,
//!   and a whole number of tiles across and down.
//!
//! A problem is a [`CodecError::Atlas`], which names where it is, such as
//! `sprites[2].frames[0].w`.

use std::fmt;

use crate::json::Value;
use crate::{CodecError, Limits, kbj};

/// The version of the atlas file that [`parse`] reads.
pub const VERSION: i64 = 1;

/// The largest side of a page, in pixels.
pub const MAX_PAGE_SIDE: u32 = 4096;

/// An atlas file: its pages and its sprites.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct AtlasFile {
    /// The pages: the packed pages, then the tile sets'.
    pub pages: Vec<Page>,
    /// The sprites, in byte order of their names.
    pub sprites: Vec<SpriteEntry>,
}

/// A page of an atlas: a QOI texture of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Page {
    /// Its path among the cooked files, such as `atlas/sprites_0.qoi`.
    pub path: String,
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
}

/// A sprite of an atlas: an image, or a sheet of frames.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct SpriteEntry {
    /// Its name: the image's path without `.png`, such as `sprites/person`.
    pub name: String,
    /// Its frames, in the sheet's order.
    pub frames: Vec<Frame>,
    /// A sprite's pivot, or a tile set's tile size.
    pub kind: SpriteKind,
}

/// What a sprite is.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum SpriteKind {
    /// A sprite, with its pivot in its untrimmed frame's pixels, from
    /// (0, 0) at the top-left corner.
    Pivot([f32; 2]),
    /// A tile set, with the width and height of a tile in pixels.
    Tiles([u32; 2]),
}

/// Where a frame is on its page, and where it was in its untrimmed cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Frame {
    /// Its page, from 0.
    pub page: u32,
    /// Its left edge on the page, in pixels.
    pub x: u32,
    /// Its top edge on the page.
    pub y: u32,
    /// Its width once trimmed: 0 for an empty frame.
    pub width: u32,
    /// Its height once trimmed: 0 for an empty frame.
    pub height: u32,
    /// How far its left edge is from its untrimmed cell's.
    pub offset_x: u32,
    /// How far its top edge is from its untrimmed cell's.
    pub offset_y: u32,
    /// The untrimmed cell's width.
    pub source_width: u32,
    /// The untrimmed cell's height.
    pub source_height: u32,
}

impl Frame {
    /// Whether it has no pixels: the cook trimmed it to nothing.
    pub fn is_empty(&self) -> bool {
        self.width == 0 && self.height == 0
    }
}

/// What is wrong with an atlas file. See [`CodecError::Atlas`].
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
        min: u32,
        /// The largest it may be.
        max: u32,
    },
    /// A number is not finite, or too large for an `f32`.
    NotFinite,
    /// A list does not have the number of items it needs.
    WrongLength {
        /// How many it needs.
        expected: usize,
    },
    /// The file has no page, or a sprite no frame.
    Empty,
    /// A name or path is empty.
    EmptyString,
    /// A sprite's name does not come after the one before it in byte
    /// order.
    NotSorted,
    /// A frame's page does not exist.
    NoSuchPage {
        /// The page.
        page: u32,
    },
    /// A frame runs off its page.
    OffPage,
    /// A frame does not lie inside its untrimmed cell.
    OutsideCell,
    /// An empty frame is not on page 0 at (0, 0) with no offset, or a frame
    /// has one side of 0.
    BadEmptyFrame,
    /// A sprite has both a pivot and a tile size, or neither.
    PivotOrTiles,
    /// A tile set does not have one frame that is the whole of its page, in
    /// whole tiles.
    BadTileSet,
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
            Self::NotFinite => write!(f, "must be a finite number that fits 32 bits"),
            Self::WrongLength { expected } => write!(f, "must have {expected} items"),
            Self::Empty => write!(f, "must not be empty"),
            Self::EmptyString => write!(f, "must not be an empty string"),
            Self::NotSorted => write!(
                f,
                "the sprites must be in byte order of their names, with no two the same"
            ),
            Self::NoSuchPage { page } => write!(f, "page {page} does not exist"),
            Self::OffPage => write!(f, "the frame runs off its page"),
            Self::OutsideCell => write!(f, "the frame does not lie inside its untrimmed cell"),
            Self::BadEmptyFrame => write!(
                f,
                "an empty frame is 0 × 0 on page 0 at (0, 0) with no offset; no frame is 0 on one side only"
            ),
            Self::PivotOrTiles => write!(f, "a sprite has a `pivot` or `tiles`, one of them"),
            Self::BadTileSet => write!(
                f,
                "a tile set has one frame, the whole of its page, untrimmed, in whole tiles"
            ),
        }
    }
}

/// Reads an atlas file: KBJ, decoded within `limits`, then checked as the
/// module says.
///
/// # Errors
///
/// [`CodecError::Kbj`] if the bytes are not KBJ, and
/// [`CodecError::Atlas`], naming where, if they are not an atlas file of
/// version 1.
pub fn parse(bytes: &[u8], limits: Limits) -> Result<AtlasFile, CodecError> {
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
        CodecError::Atlas {
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

    fn require(&self, name: &'static str, at: &At<'_>) -> Result<&'v Value, CodecError> {
        self.get(name)
            .ok_or_else(|| at.error(ErrorKind::MissingMember { name }))
    }
}

fn array<'v>(value: &'v Value, at: &At<'_>) -> Result<&'v [Value], CodecError> {
    value
        .as_array()
        .ok_or_else(|| at.error(ErrorKind::WrongType { expected: "a list" }))
}

fn string(value: &Value, at: &At<'_>) -> Result<String, CodecError> {
    match value.as_str() {
        Some("") => Err(at.error(ErrorKind::EmptyString)),
        Some(text) => Ok(text.to_string()),
        None => Err(at.error(ErrorKind::WrongType {
            expected: "a string",
        })),
    }
}

/// A whole number from `min` to `max`.
fn whole(value: &Value, at: &At<'_>, min: u32, max: u32) -> Result<u32, CodecError> {
    let Value::Int(number) = value else {
        return Err(at.error(ErrorKind::WrongType {
            expected: "a whole number",
        }));
    };
    u32::try_from(*number)
        .ok()
        .filter(|number| (min..=max).contains(number))
        .ok_or_else(|| at.error(ErrorKind::OutOfRange { min, max }))
}

/// A list of exactly two items.
fn pair<'v>(value: &'v Value, at: &At<'_>) -> Result<[&'v Value; 2], CodecError> {
    match array(value, at)? {
        [first, second] => Ok([first, second]),
        _ => Err(at.error(ErrorKind::WrongLength { expected: 2 })),
    }
}

/// A finite number that fits an `f32`.
fn float(value: &Value, at: &At<'_>) -> Result<f32, CodecError> {
    let number = match value {
        Value::Int(number) => *number as f64,
        Value::Float(number) => *number,
        _ => {
            return Err(at.error(ErrorKind::WrongType {
                expected: "a number",
            }));
        }
    };
    Some(number as f32)
        .filter(|number| number.is_finite())
        .ok_or_else(|| at.error(ErrorKind::NotFinite))
}

fn read(value: &Value) -> Result<AtlasFile, CodecError> {
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
    let top = Members::of(value, &root, &["version", "pages", "sprites"])?;
    top.require("version", &root)?;

    let at = At::Member(&root, "pages");
    let items = array(top.require("pages", &root)?, &at)?;
    if items.is_empty() {
        return Err(at.error(ErrorKind::Empty));
    }
    let mut pages = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        pages.push(page(item, &At::Item(&at, index))?);
    }

    let at = At::Member(&root, "sprites");
    let items = array(top.require("sprites", &root)?, &at)?;
    let mut sprites: Vec<SpriteEntry> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = At::Item(&at, index);
        let sprite = sprite(item, &at, &pages)?;
        if let Some(before) = sprites.last()
            && before.name.as_bytes() >= sprite.name.as_bytes()
        {
            return Err(At::Member(&at, "name").error(ErrorKind::NotSorted));
        }
        sprites.push(sprite);
    }
    Ok(AtlasFile { pages, sprites })
}

fn page(value: &Value, at: &At<'_>) -> Result<Page, CodecError> {
    let members = Members::of(value, at, &["path", "w", "h"])?;
    let field = |name| {
        members
            .require(name, at)
            .map(|value| (value, At::Member(at, name)))
    };
    let (path, path_at) = field("path")?;
    let (w, w_at) = field("w")?;
    let (h, h_at) = field("h")?;
    Ok(Page {
        path: string(path, &path_at)?,
        width: whole(w, &w_at, 1, MAX_PAGE_SIDE)?,
        height: whole(h, &h_at, 1, MAX_PAGE_SIDE)?,
    })
}

fn sprite(value: &Value, at: &At<'_>, pages: &[Page]) -> Result<SpriteEntry, CodecError> {
    let members = Members::of(value, at, &["name", "frames", "pivot", "tiles"])?;
    let name = string(members.require("name", at)?, &At::Member(at, "name"))?;

    let frames_at = At::Member(at, "frames");
    let items = array(members.require("frames", at)?, &frames_at)?;
    if items.is_empty() {
        return Err(frames_at.error(ErrorKind::Empty));
    }
    let mut frames = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        frames.push(frame(item, &At::Item(&frames_at, index), pages)?);
    }

    let kind = match (members.get("pivot"), members.get("tiles")) {
        (Some(pivot), None) => {
            let at = At::Member(at, "pivot");
            let [x, y] = pair(pivot, &at)?;
            SpriteKind::Pivot([float(x, &At::Item(&at, 0))?, float(y, &At::Item(&at, 1))?])
        }
        (None, Some(tiles)) => {
            let at = At::Member(at, "tiles");
            let [w, h] = pair(tiles, &at)?;
            let tile = [
                whole(w, &At::Item(&at, 0), 1, MAX_PAGE_SIDE)?,
                whole(h, &At::Item(&at, 1), 1, MAX_PAGE_SIDE)?,
            ];
            check_tile_set(&frames, pages, tile, &at)?;
            SpriteKind::Tiles(tile)
        }
        _ => return Err(at.error(ErrorKind::PivotOrTiles)),
    };
    Ok(SpriteEntry { name, frames, kind })
}

const FRAME_MEMBERS: [&str; 9] = ["page", "x", "y", "w", "h", "ox", "oy", "sw", "sh"];

fn frame(value: &Value, at: &At<'_>, pages: &[Page]) -> Result<Frame, CodecError> {
    let members = Members::of(value, at, &FRAME_MEMBERS)?;
    let mut numbers = [0u32; FRAME_MEMBERS.len()];
    for (number, name) in numbers.iter_mut().zip(FRAME_MEMBERS) {
        *number = whole(
            members.require(name, at)?,
            &At::Member(at, name),
            0,
            u32::MAX,
        )?;
    }
    let [
        page,
        x,
        y,
        width,
        height,
        offset_x,
        offset_y,
        source_width,
        source_height,
    ] = numbers;
    let frame = Frame {
        page,
        x,
        y,
        width,
        height,
        offset_x,
        offset_y,
        source_width,
        source_height,
    };
    let Some(on) = usize::try_from(page).ok().and_then(|page| pages.get(page)) else {
        return Err(At::Member(at, "page").error(ErrorKind::NoSuchPage { page }));
    };
    if source_width == 0 || source_height == 0 {
        let name = if source_width == 0 { "sw" } else { "sh" };
        return Err(At::Member(at, name).error(ErrorKind::OutOfRange {
            min: 1,
            max: u32::MAX,
        }));
    }
    if width == 0 || height == 0 {
        if frame.is_empty() && page == 0 && x == 0 && y == 0 && offset_x == 0 && offset_y == 0 {
            return Ok(frame);
        }
        return Err(at.error(ErrorKind::BadEmptyFrame));
    }
    let fits = |start: u32, length: u32, end: u32| {
        start.checked_add(length).is_some_and(|edge| edge <= end)
    };
    if !(fits(x, width, on.width) && fits(y, height, on.height)) {
        return Err(at.error(ErrorKind::OffPage));
    }
    if !(fits(offset_x, width, source_width) && fits(offset_y, height, source_height)) {
        return Err(at.error(ErrorKind::OutsideCell));
    }
    Ok(frame)
}

/// Checks that a tile set's frames are one frame, the whole of its page,
/// untrimmed, in whole tiles of `tile`.
fn check_tile_set(
    frames: &[Frame],
    pages: &[Page],
    [tile_width, tile_height]: [u32; 2],
    at: &At<'_>,
) -> Result<(), CodecError> {
    let whole_page = |frame: &Frame| {
        let page = usize::try_from(frame.page)
            .ok()
            .and_then(|page| pages.get(page));
        page.is_some_and(|page| {
            frame.x == 0
                && frame.y == 0
                && frame.offset_x == 0
                && frame.offset_y == 0
                && frame.width == page.width
                && frame.height == page.height
                && frame.source_width == page.width
                && frame.source_height == page.height
        })
    };
    match frames {
        [frame]
            if whole_page(frame)
                && frame.width.is_multiple_of(tile_width)
                && frame.height.is_multiple_of(tile_height) =>
        {
            Ok(())
        }
        _ => Err(at.error(ErrorKind::BadTileSet)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    /// The bytes of the atlas file `text` describes, in JSON.
    fn kbj(text: &str) -> Vec<u8> {
        let value = json::parse(text, Limits::default()).expect("the test's JSON");
        kbj::encode(&value, Limits::default()).expect("encodable")
    }

    fn parsed(text: &str) -> Result<AtlasFile, CodecError> {
        parse(&kbj(text), Limits::default())
    }

    /// The error `text` gives: where, and what.
    fn error(text: &str) -> (String, ErrorKind) {
        match parsed(text) {
            Err(CodecError::Atlas { at, kind }) => (at, kind),
            other => panic!("expected an atlas error, got {other:?}"),
        }
    }

    const PERSON: &str = r#"{ "name": "sprites/person",
        "frames": [ { "page": 0, "x": 33, "y": 85, "w": 10, "h": 20, "ox": 1, "oy": 0, "sw": 12, "sh": 20 },
                    { "page": 0, "x": 1, "y": 85, "w": 12, "h": 20, "ox": 0, "oy": 0, "sw": 12, "sh": 20 } ], "pivot": [6.0, 20.0] }"#;
    const TILES: &str = r#"{ "name": "sprites/tiles",
        "frames": [ { "page": 1, "x": 0, "y": 0, "w": 128, "h": 64, "ox": 0, "oy": 0, "sw": 128, "sh": 64 } ],
        "tiles": [16, 16] }"#;
    const PAGES: &str = r#"[ { "path": "atlas/sprites_0.qoi", "w": 128, "h": 128 },
                             { "path": "atlas/sprites/tiles.tiles.qoi", "w": 128, "h": 64 } ]"#;

    /// An atlas file of `sprites`, a list in JSON, on [`PAGES`].
    fn file(sprites: &str) -> String {
        format!(r#"{{ "version": 1, "pages": {PAGES}, "sprites": {sprites} }}"#)
    }

    /// [`file`] with one frame of the person changed: `from` replaced by
    /// `to` in its text.
    fn person_with(from: &str, to: &str) -> String {
        assert!(PERSON.contains(from), "{from}");
        file(&format!("[{}]", PERSON.replacen(from, to, 1)))
    }

    #[test]
    fn a_file_as_the_cook_writes_it_is_read() {
        let atlas = parsed(&file(&format!("[{PERSON}, {TILES}]"))).expect("an atlas");
        assert_eq!(
            atlas.pages[1],
            Page {
                path: "atlas/sprites/tiles.tiles.qoi".to_string(),
                width: 128,
                height: 64,
            }
        );
        let person = &atlas.sprites[0];
        assert_eq!(person.name, "sprites/person");
        assert_eq!(person.kind, SpriteKind::Pivot([6.0, 20.0]));
        assert_eq!(
            person.frames[0],
            Frame {
                page: 0,
                x: 33,
                y: 85,
                width: 10,
                height: 20,
                offset_x: 1,
                offset_y: 0,
                source_width: 12,
                source_height: 20,
            }
        );
        assert_eq!(atlas.sprites[1].kind, SpriteKind::Tiles([16, 16]));
        // A pivot may be given in whole numbers, and outside the frame.
        let whole = person_with("[6.0, 20.0]", "[-3, 40]");
        let atlas = parsed(&whole).expect("an atlas");
        assert_eq!(atlas.sprites[0].kind, SpriteKind::Pivot([-3.0, 40.0]));
        // No sprites at all is an atlas of pages alone.
        assert!(parsed(&file("[]")).expect("an atlas").sprites.is_empty());
    }

    #[test]
    fn a_later_version_is_not_supported_whatever_it_holds() {
        let later = file("[]").replace(r#""version": 1"#, r#""version": 2, "frames_by_tag": []"#);
        assert_eq!(
            error(&later),
            ("version".to_string(), ErrorKind::Version { found: 2 })
        );
    }

    #[test]
    fn an_empty_frame_is_read() {
        let empty = r#"{ "page": 0, "x": 0, "y": 0, "w": 0, "h": 0, "ox": 0, "oy": 0, "sw": 12, "sh": 20 }"#;
        let text = person_with(
            r#"{ "page": 0, "x": 1, "y": 85, "w": 12, "h": 20, "ox": 0, "oy": 0, "sw": 12, "sh": 20 }"#,
            empty,
        );
        let atlas = parsed(&text).expect("an atlas");
        assert!(atlas.sprites[0].frames[1].is_empty());
        assert!(!atlas.sprites[0].frames[0].is_empty());
        for (from, to) in [
            (r#""x": 0, "y": 0, "w": 0"#, r#""x": 4, "y": 0, "w": 0"#),
            (
                r#""ox": 0, "oy": 0, "sw": 12"#,
                r#""ox": 1, "oy": 0, "sw": 12"#,
            ),
            (r#""w": 0, "h": 0"#, r#""w": 3, "h": 0"#),
        ] {
            assert!(text.contains(from), "{from}");
            assert_eq!(
                error(&text.replacen(from, to, 1)),
                ("sprites[0].frames[1]".to_string(), ErrorKind::BadEmptyFrame),
                "{to}"
            );
        }
    }

    #[test]
    fn each_rule_is_checked_and_names_where() {
        let cases: [(String, &str, ErrorKind); 16] = [
            (
                file("[]").replace(r#""version": 1"#, r#""version": 2"#),
                "version",
                ErrorKind::Version { found: 2 },
            ),
            (
                file("[]").replace(r#""version": 1"#, r#""version": "1""#),
                "version",
                ErrorKind::WrongType {
                    expected: "a whole number",
                },
            ),
            (
                file("[]").replace(r#""version": 1, "#, ""),
                "",
                ErrorKind::MissingMember { name: "version" },
            ),
            (
                file("[]").replace(r#""version": 1"#, r#""version": 1, "extra": 0"#),
                "",
                ErrorKind::UnknownMember {
                    name: "extra".to_string(),
                },
            ),
            (
                r#"{ "version": 1, "pages": [], "sprites": [] }"#.to_string(),
                "pages",
                ErrorKind::Empty,
            ),
            (
                file("[]").replacen(r#""w": 128"#, r#""w": 4097"#, 1),
                "pages[0].w",
                ErrorKind::OutOfRange {
                    min: 1,
                    max: MAX_PAGE_SIDE,
                },
            ),
            (
                file("[]").replacen(r#""atlas/sprites_0.qoi""#, r#""""#, 1),
                "pages[0].path",
                ErrorKind::EmptyString,
            ),
            (
                file(&format!("[{TILES}, {PERSON}]")),
                "sprites[1].name",
                ErrorKind::NotSorted,
            ),
            (
                file(&format!("[{PERSON}, {PERSON}]")),
                "sprites[1].name",
                ErrorKind::NotSorted,
            ),
            (
                person_with(r#""page": 0, "x": 33"#, r#""page": 2, "x": 33"#),
                "sprites[0].frames[0].page",
                ErrorKind::NoSuchPage { page: 2 },
            ),
            (
                person_with(r#""x": 33"#, r#""x": 119"#),
                "sprites[0].frames[0]",
                ErrorKind::OffPage,
            ),
            (
                person_with(r#""ox": 1"#, r#""ox": 3"#),
                "sprites[0].frames[0]",
                ErrorKind::OutsideCell,
            ),
            (
                person_with(r#""x": 33"#, r#""x": -1"#),
                "sprites[0].frames[0].x",
                ErrorKind::OutOfRange {
                    min: 0,
                    max: u32::MAX,
                },
            ),
            (
                person_with(r#""sw": 12"#, r#""sw": 0"#),
                "sprites[0].frames[0].sw",
                ErrorKind::OutOfRange {
                    min: 1,
                    max: u32::MAX,
                },
            ),
            (
                person_with(r#""pivot": [6.0, 20.0]"#, r#""pivot": [6.0]"#),
                "sprites[0].pivot",
                ErrorKind::WrongLength { expected: 2 },
            ),
            (
                person_with(r#""pivot": [6.0, 20.0]"#, r#""pivot": [6.0, 1e300]"#),
                "sprites[0].pivot[1]",
                ErrorKind::NotFinite,
            ),
        ];
        for (text, at, kind) in cases {
            assert_eq!(error(&text), (at.to_string(), kind), "{text}");
        }
    }

    #[test]
    fn a_sprite_has_a_pivot_or_tiles_and_a_tile_set_is_a_whole_page() {
        let both = person_with(
            r#""pivot": [6.0, 20.0]"#,
            r#""pivot": [6.0, 20.0], "tiles": [4, 4]"#,
        );
        assert_eq!(
            error(&both),
            ("sprites[0]".to_string(), ErrorKind::PivotOrTiles)
        );
        let neither = person_with(r#", "pivot": [6.0, 20.0]"#, "");
        assert_eq!(
            error(&neither),
            ("sprites[0]".to_string(), ErrorKind::PivotOrTiles)
        );
        let tiles = |from: &str, to: &str| {
            assert!(TILES.contains(from), "{from}");
            error(&file(&format!("[{}]", TILES.replacen(from, to, 1))))
        };
        let bad = ("sprites[0].tiles".to_string(), ErrorKind::BadTileSet);
        // Not in whole tiles; trimmed; not the whole page; two frames.
        assert_eq!(tiles("[16, 16]", "[48, 16]"), bad);
        assert_eq!(tiles(r#""h": 64, "ox": 0"#, r#""h": 63, "ox": 0"#), bad);
        assert_eq!(
            tiles(
                r#""w": 128, "h": 64, "ox": 0, "oy": 0, "sw": 128"#,
                r#""w": 64, "h": 64, "ox": 0, "oy": 0, "sw": 64"#
            ),
            bad
        );
        let frame = r#"{ "page": 1, "x": 0, "y": 0, "w": 128, "h": 64, "ox": 0, "oy": 0, "sw": 128, "sh": 64 }"#;
        assert_eq!(tiles(frame, &format!("{frame}, {frame}")), bad);
        assert_eq!(
            tiles("[16, 16]", "[0, 16]"),
            (
                "sprites[0].tiles[0]".to_string(),
                ErrorKind::OutOfRange {
                    min: 1,
                    max: MAX_PAGE_SIDE
                }
            )
        );
    }

    #[test]
    fn a_file_that_is_not_kbj_is_a_kbj_error() {
        assert!(matches!(
            parse(b"not KBJ", Limits::default()),
            Err(CodecError::Kbj { .. })
        ));
        assert_eq!(
            error("[]"),
            (
                String::new(),
                ErrorKind::WrongType {
                    expected: "an object"
                }
            )
        );
    }

    #[test]
    fn errors_say_where_and_what() {
        let error = CodecError::Atlas {
            at: "sprites[2].frames[0].w".to_string(),
            kind: ErrorKind::OffPage,
        };
        assert_eq!(
            error.to_string(),
            "atlas error at `sprites[2].frames[0].w`: the frame runs off its page"
        );
        let error = CodecError::Atlas {
            at: String::new(),
            kind: ErrorKind::MissingMember { name: "pages" },
        };
        assert_eq!(
            error.to_string(),
            "atlas error in the file: `pages` is missing"
        );
    }
}
