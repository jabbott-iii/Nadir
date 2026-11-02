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

//! Aseprite's sprite sheet data, as
//! `aseprite -b in.aseprite --sheet out.png --data out.json --format json-array --list-tags`
//! writes it: where each frame is in the sheet, and the tags that make
//! clips of them (`docs/subsystems/assets-and-codecs.md`, "Aseprite
//! subset"). The cook reads it with [`parse`]; the game never does.
//!
//! ```text
//! { "frames": [
//!     { "filename": "person 0.aseprite",
//!       "frame": { "x": 0, "y": 0, "w": 12, "h": 20 }, "rotated": false, "trimmed": false,
//!       "spriteSourceSize": { "x": 0, "y": 0, "w": 12, "h": 20 },
//!       "sourceSize": { "w": 12, "h": 20 }, "duration": 100 }, … ],
//!   "meta": { "app": "https://www.aseprite.org/", "version": "1.3.…", "image": "person.png",
//!             "format": "RGBA8888", "size": { "w": 60, "h": 20 }, "scale": "1",
//!             "frameTags": [ { "name": "walk", "from": 1, "to": 4, "direction": "forward",
//!                              "repeat": "2", "color": "#000000ff" } ] } }
//! ```
//!
//! It is read with [`json::parse`], so the JSONC
//! leniencies pass, and so does a raw tab or carriage return nowhere: the
//! JSON parser refuses those in strings, and Aseprite writes a name's or
//! its user data's unescaped. Then it is checked:
//!
//! - **Members:** those below must be there with the types shown; the
//!   others Aseprite writes, or adds in later versions, are ignored.
//!   `frames` must be a list: the json-hash form (`--format json-hash`) is
//!   refused.
//! - **Frames:** one at least. A frame is not rotated (Aseprite never
//!   rotates); its pixels are a rectangle of 1 × 1 or more inside the
//!   sheet's `meta.size`, of the size `spriteSourceSize` gives (they differ
//!   with `--inner-padding`, which is refused), placed inside its
//!   `sourceSize`, which every frame shares (an export of sprites of other
//!   sizes differs; split layers share their sprite's, and are not seen);
//!   it lasts 1 to 65535 ms, as an `.aseprite` file holds.
//! - **Sheet:** `meta.image` is a file name, without `/` or `\`, as
//!   Aseprite writes it: missing when no `--sheet` was given.
//! - **Tags:** `meta.frameTags` may be left out. Each has a name that is not
//!   empty and has no `/`, no two the same; `from` and `to`, with
//!   `from <= to <` the number of frames; a direction of `forward`,
//!   `reverse`, `pingpong` or `pingpong_reverse`; and maybe `repeat`, 1 to 5
//!   digits of at most 65535, as Aseprite writes it, in a string.
//! - **Clips:** each tag is expanded to the frames Aseprite plays ([`Tag`]),
//!   and all of them together are at most `max_entries` frames: each tag's
//!   clip is counted, with those before it, before it is made, in 64 bits,
//!   where the count cannot overflow.
//!
//! Not supported, and not all detectable: `--split-layers`, `--split-tags`,
//! `--ignore-empty`, `--tag` and `--frame-range`, which number the frames
//! of the export apart from those of its tags.
//!
//! A problem after the JSON is a [`CodecError::Aseprite`], which names
//! where it is as a member path, such as `frames[3].frame.w`, since the
//! JSON tree keeps no line and column; the JSON's own errors keep theirs.

use std::fmt;

use crate::json::{self, Value};
use crate::{CodecError, Limits};

/// The longest a frame can last, in milliseconds: an `.aseprite` file
/// holds it in 16 bits.
pub const MAX_DURATION: u32 = 65535;

/// The most times a tag can repeat: an `.aseprite` file holds it in 16
/// bits.
pub const MAX_REPEAT: u32 = 65535;

/// An Aseprite export: its sheet's name and size, its frames and its tags.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Export {
    /// The sheet's file name, as Aseprite wrote it, such as `person.png`.
    pub image: String,
    /// The sheet's width in pixels.
    pub width: u32,
    /// The sheet's height in pixels.
    pub height: u32,
    /// The frames, in the order of the export.
    pub frames: Vec<ExportFrame>,
    /// The tags, in the order of the export.
    pub tags: Vec<Tag>,
}

/// A frame of an export: its pixels in the sheet, where they go in the
/// sprite's canvas, and how long it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExportFrame {
    /// Its pixels' left edge in the sheet.
    pub x: u32,
    /// Their top edge.
    pub y: u32,
    /// Their width: 1 or more.
    pub width: u32,
    /// Their height: 1 or more.
    pub height: u32,
    /// How far their left edge is from the canvas's.
    pub offset_x: u32,
    /// How far their top edge is from the canvas's.
    pub offset_y: u32,
    /// The canvas's width, which every frame shares.
    pub source_width: u32,
    /// The canvas's height.
    pub source_height: u32,
    /// How long it shows, in milliseconds: 1 to [`MAX_DURATION`].
    pub duration: u32,
}

/// The way a tag plays its frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Direction {
    /// `forward`: from `from` to `to`.
    Forward,
    /// `reverse`: from `to` to `from`.
    Reverse,
    /// `pingpong`: forward, then back.
    PingPong,
    /// `pingpong_reverse`: back, then forward.
    PingPongReverse,
}

/// A tag of an export, and the clip it makes.
///
/// Its clip is the frames Aseprite plays, from `src/doc/playback.cpp`:
///
/// - **No `repeat`** (or 0) loops: `forward` is `from` to `to`, `reverse`
///   `to` to `from`; `pingpong` is `from` to `to` and back to the frame
///   after `from`, so that the turning frames show once a cycle, and
///   `pingpong_reverse` the same from `to`. A tag of one frame is that
///   frame.
/// - **`repeat` N** plays N passes, then stops on the last frame shown: a
///   pass of `forward` or `reverse` is the whole tag, and of the ping-pongs
///   one way across it, each pass after the first going back the other way
///   without the frame it turned on. A tag of one frame shows it N times.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Tag {
    /// Its name: not empty, with no `/`.
    pub name: String,
    /// Its first frame, an index into [`Export::frames`].
    pub from: u32,
    /// Its last frame: at least `from`.
    pub to: u32,
    /// How it plays.
    pub direction: Direction,
    /// How many passes it plays, or 0 to loop.
    pub repeat: u32,
    /// The frames its clip shows, in order: indices into
    /// [`Export::frames`], one at least.
    pub frames: Vec<u32>,
    /// Whether its clip loops; otherwise it stops on its last frame.
    pub looping: bool,
}

/// What is wrong with an Aseprite export. See [`CodecError::Aseprite`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
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
    /// `frames` is an object: the export is json-hash.
    NotJsonArray,
    /// The export has no frame.
    NoFrames,
    /// A frame is rotated.
    Rotated,
    /// A whole number is outside its member's range.
    OutOfRange {
        /// The smallest it may be.
        min: u32,
        /// The largest it may be.
        max: u32,
    },
    /// A frame's `spriteSourceSize` is not the size of its `frame`, as with
    /// `--inner-padding`.
    InnerPadding,
    /// A frame's pixels do not lie inside its `sourceSize`.
    OutsideSource,
    /// A frame's pixels do not lie inside the sheet.
    OutsideSheet,
    /// A frame's `sourceSize` is not the first frame's.
    SeveralSprites,
    /// `meta.image` is missing: the export has no sheet.
    NoImage,
    /// `meta.image` is empty, or a path rather than a file name.
    BadImage,
    /// A tag's name is empty or has a `/`.
    BadTagName,
    /// Two tags have this name.
    DuplicateTag {
        /// The name.
        name: String,
    },
    /// A tag's `from` is after its `to`, or its `to` is not a frame.
    TagRange,
    /// A tag's direction is not one of the four.
    BadDirection {
        /// The direction it gives.
        found: String,
    },
    /// A tag's `repeat` is not a string of 1 to 5 digits of at most
    /// [`MAX_REPEAT`].
    BadRepeat,
    /// The tags' clips together have more frames than the limit.
    TooManyFrames {
        /// The limit, `max_entries`.
        most: usize,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongType { expected } => write!(f, "expected {expected}"),
            Self::MissingMember { name } => write!(f, "`{name}` is missing"),
            Self::NotJsonArray => write!(
                f,
                "`frames` is an object: export with --format json-array, not json-hash"
            ),
            Self::NoFrames => write!(f, "the export has no frame"),
            Self::Rotated => write!(f, "a rotated frame is not supported"),
            Self::OutOfRange { min, max } => write!(f, "must be from {min} to {max}"),
            Self::InnerPadding => write!(
                f,
                "`spriteSourceSize` is not the size of `frame`: export with no --inner-padding"
            ),
            Self::OutsideSource => write!(f, "the frame does not lie inside its `sourceSize`"),
            Self::OutsideSheet => write!(f, "the frame does not lie inside the sheet's `size`"),
            Self::SeveralSprites => write!(
                f,
                "the frames' `sourceSize` differ: export one sprite a file"
            ),
            Self::NoImage => write!(f, "`image` is missing: export with --sheet"),
            Self::BadImage => write!(f, "must be a file name, beside the export"),
            Self::BadTagName => write!(f, "a tag's name must not be empty or have a `/`"),
            Self::DuplicateTag { name } => write!(f, "two tags are named `{name}`"),
            Self::TagRange => write!(
                f,
                "`from` must be at most `to`, and `to` less than the number of frames"
            ),
            Self::BadDirection { found } => write!(
                f,
                "the direction `{found}` is not forward, reverse, pingpong or pingpong_reverse"
            ),
            Self::BadRepeat => write!(
                f,
                "`repeat` must be a string of 1 to 5 digits, at most {MAX_REPEAT}"
            ),
            Self::TooManyFrames { most } => {
                write!(f, "the tags' clips have more than {most} frames in all")
            }
        }
    }
}

/// Reads an Aseprite export: JSON, parsed within `limits`, then checked as
/// the module says, with each tag expanded to its clip.
///
/// # Errors
///
/// [`CodecError::Json`] if the text is not JSON, and
/// [`CodecError::Aseprite`], naming where, if it is not an export the cook
/// takes.
pub fn parse(text: &str, limits: Limits) -> Result<Export, CodecError> {
    read(&json::parse(text, limits)?, limits)
}

/// Where a value is in the export, for errors: made into text only for
/// one.
#[derive(Clone, Copy)]
enum At<'a> {
    Root,
    Member(&'a At<'a>, &'static str),
    Item(&'a At<'a>, usize),
}

impl At<'_> {
    fn error(&self, kind: ErrorKind) -> CodecError {
        CodecError::Aseprite {
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

/// The member `name` of the object `value`.
fn member<'v>(value: &'v Value, name: &'static str, at: &At<'_>) -> Result<&'v Value, CodecError> {
    let members = value.as_object().ok_or_else(|| {
        at.error(ErrorKind::WrongType {
            expected: "an object",
        })
    })?;
    members
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .ok_or_else(|| at.error(ErrorKind::MissingMember { name }))
}

/// The member `name` of the object `value`, if it has one.
fn optional<'v>(value: &'v Value, name: &str) -> Option<&'v Value> {
    value
        .as_object()?
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
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

/// The whole numbers `names` of the object `value`, each from `min` up.
fn numbers<const N: usize>(
    value: &Value,
    at: &At<'_>,
    names: [&'static str; N],
    min: [u32; N],
) -> Result<[u32; N], CodecError> {
    let mut out = [0; N];
    for ((out, name), min) in out.iter_mut().zip(names).zip(min) {
        *out = whole(
            member(value, name, at)?,
            &At::Member(at, name),
            min,
            u32::MAX,
        )?;
    }
    Ok(out)
}

fn string<'v>(value: &'v Value, at: &At<'_>) -> Result<&'v str, CodecError> {
    value.as_str().ok_or_else(|| {
        at.error(ErrorKind::WrongType {
            expected: "a string",
        })
    })
}

/// Whether `start + length` is at most `end`, with no overflow.
fn fits(start: u32, length: u32, end: u32) -> bool {
    start.checked_add(length).is_some_and(|edge| edge <= end)
}

fn read(value: &Value, limits: Limits) -> Result<Export, CodecError> {
    let root = At::Root;
    let meta_at = At::Member(&root, "meta");
    let meta = member(value, "meta", &root)?;
    let image_at = At::Member(&meta_at, "image");
    let image = match optional(meta, "image") {
        Some(image) => string(image, &image_at)?,
        None if meta.as_object().is_some() => return Err(meta_at.error(ErrorKind::NoImage)),
        None => {
            return Err(meta_at.error(ErrorKind::WrongType {
                expected: "an object",
            }));
        }
    };
    if image.is_empty() || image.contains(['/', '\\']) {
        return Err(image_at.error(ErrorKind::BadImage));
    }
    let size_at = At::Member(&meta_at, "size");
    let [width, height] = numbers(
        member(meta, "size", &meta_at)?,
        &size_at,
        ["w", "h"],
        [1, 1],
    )?;

    let frames_at = At::Member(&root, "frames");
    let items = match member(value, "frames", &root)? {
        Value::Array(items) => items,
        Value::Object(_) => return Err(frames_at.error(ErrorKind::NotJsonArray)),
        _ => return Err(frames_at.error(ErrorKind::WrongType { expected: "a list" })),
    };
    if items.is_empty() {
        return Err(frames_at.error(ErrorKind::NoFrames));
    }
    let mut frames: Vec<ExportFrame> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = At::Item(&frames_at, index);
        let frame = frame(item, &at, width, height)?;
        if let Some(first) = frames.first()
            && (first.source_width, first.source_height)
                != (frame.source_width, frame.source_height)
        {
            return Err(At::Member(&at, "sourceSize").error(ErrorKind::SeveralSprites));
        }
        frames.push(frame);
    }

    let tags_at = At::Member(&meta_at, "frameTags");
    let items = match optional(meta, "frameTags") {
        None => &[][..],
        Some(Value::Array(items)) => items.as_slice(),
        Some(_) => return Err(tags_at.error(ErrorKind::WrongType { expected: "a list" })),
    };
    let mut tags = Vec::with_capacity(items.len());
    let mut total: usize = 0;
    for (index, item) in items.iter().enumerate() {
        let at = At::Item(&tags_at, index);
        let tag = tag(item, &at, frames.len(), total, limits.max_entries)?;
        total += tag.frames.len();
        tags.push(tag);
    }
    // No two tags of one name, found by sorting so that the check is not
    // quadratic: the later of the two is named.
    let mut names: Vec<(&str, usize)> = tags
        .iter()
        .enumerate()
        .map(|(index, tag)| (tag.name.as_str(), index))
        .collect();
    names.sort_unstable();
    if let Some(pair) = names.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        let at = At::Item(&tags_at, pair[1].1);
        return Err(At::Member(&at, "name").error(ErrorKind::DuplicateTag {
            name: pair[1].0.to_string(),
        }));
    }
    Ok(Export {
        image: image.to_string(),
        width,
        height,
        frames,
        tags,
    })
}

fn frame(
    value: &Value,
    at: &At<'_>,
    sheet_w: u32,
    sheet_h: u32,
) -> Result<ExportFrame, CodecError> {
    let rect_at = At::Member(at, "frame");
    let [x, y, width, height] = numbers(
        member(value, "frame", at)?,
        &rect_at,
        ["x", "y", "w", "h"],
        [0, 0, 1, 1],
    )?;
    if let Some(rotated) = optional(value, "rotated") {
        match rotated {
            Value::Bool(false) => {}
            Value::Bool(true) => return Err(At::Member(at, "rotated").error(ErrorKind::Rotated)),
            _ => {
                return Err(At::Member(at, "rotated").error(ErrorKind::WrongType {
                    expected: "true or false",
                }));
            }
        }
    }
    let placed_at = At::Member(at, "spriteSourceSize");
    let [offset_x, offset_y, placed_w, placed_h] = numbers(
        member(value, "spriteSourceSize", at)?,
        &placed_at,
        ["x", "y", "w", "h"],
        [0, 0, 1, 1],
    )?;
    let source_at = At::Member(at, "sourceSize");
    let [source_width, source_height] = numbers(
        member(value, "sourceSize", at)?,
        &source_at,
        ["w", "h"],
        [1, 1],
    )?;
    let duration = whole(
        member(value, "duration", at)?,
        &At::Member(at, "duration"),
        1,
        MAX_DURATION,
    )?;
    if (placed_w, placed_h) != (width, height) {
        return Err(placed_at.error(ErrorKind::InnerPadding));
    }
    if !(fits(offset_x, width, source_width) && fits(offset_y, height, source_height)) {
        return Err(placed_at.error(ErrorKind::OutsideSource));
    }
    if !(fits(x, width, sheet_w) && fits(y, height, sheet_h)) {
        return Err(rect_at.error(ErrorKind::OutsideSheet));
    }
    Ok(ExportFrame {
        x,
        y,
        width,
        height,
        offset_x,
        offset_y,
        source_width,
        source_height,
        duration,
    })
}

/// A tag of an export of `frame_count` frames, whose clip, with the
/// `before` frames of the tags before it, may have at most `most` frames.
fn tag(
    value: &Value,
    at: &At<'_>,
    frame_count: usize,
    before: usize,
    most: usize,
) -> Result<Tag, CodecError> {
    let name_at = At::Member(at, "name");
    let name = string(member(value, "name", at)?, &name_at)?;
    if name.is_empty() || name.contains('/') {
        return Err(name_at.error(ErrorKind::BadTagName));
    }
    let [from, to] = numbers(value, at, ["from", "to"], [0, 0])?;
    if from > to || usize::try_from(to).ok().is_none_or(|to| to >= frame_count) {
        return Err(at.error(ErrorKind::TagRange));
    }
    let direction_at = At::Member(at, "direction");
    let direction = match string(member(value, "direction", at)?, &direction_at)? {
        "forward" => Direction::Forward,
        "reverse" => Direction::Reverse,
        "pingpong" => Direction::PingPong,
        "pingpong_reverse" => Direction::PingPongReverse,
        found => {
            return Err(direction_at.error(ErrorKind::BadDirection {
                found: found.to_string(),
            }));
        }
    };
    let repeat = match optional(value, "repeat") {
        None => 0,
        Some(repeat) => {
            let repeat_at = At::Member(at, "repeat");
            let text = string(repeat, &repeat_at)?;
            let digits = (1..=5).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit());
            text.parse::<u32>()
                .ok()
                .filter(|&repeat| digits && repeat <= MAX_REPEAT)
                .ok_or_else(|| repeat_at.error(ErrorKind::BadRepeat))?
        }
    };
    let length = clip_length(from, to, direction, repeat);
    if before as u64 + length > most as u64 {
        return Err(at.error(ErrorKind::TooManyFrames { most }));
    }
    let (frames, looping) = clip(from, to, direction, repeat);
    Ok(Tag {
        name: name.to_string(),
        from,
        to,
        direction,
        repeat,
        frames,
        looping,
    })
}

/// How many frames the clip of a tag has, as [`clip`] makes it: at most
/// 65535 passes of 2^32 frames, which fits 64 bits.
fn clip_length(from: u32, to: u32, direction: Direction, repeat: u32) -> u64 {
    let length = u64::from(to - from) + 1;
    let repeat = u64::from(repeat);
    match direction {
        Direction::Forward | Direction::Reverse => length * repeat.max(1),
        _ if length == 1 => repeat.max(1),
        _ if repeat == 0 => 2 * length - 2,
        _ => length + (repeat - 1) * (length - 1),
    }
}

/// The frames a tag's clip shows, and whether it loops ([`Tag`]).
fn clip(from: u32, to: u32, direction: Direction, repeat: u32) -> (Vec<u32>, bool) {
    let forward: Vec<u32> = (from..=to).collect();
    let backward: Vec<u32> = (from..=to).rev().collect();
    let (first, second) = match direction {
        Direction::Forward | Direction::PingPong => (forward, backward),
        Direction::Reverse | Direction::PingPongReverse => (backward, forward),
    };
    let ping_pong = matches!(direction, Direction::PingPong | Direction::PingPongReverse);
    let looping = repeat == 0;
    let mut frames = Vec::with_capacity(clip_length(from, to, direction, repeat) as usize);
    if !ping_pong || first.len() == 1 {
        for _ in 0..repeat.max(1) {
            frames.extend_from_slice(&first);
        }
    } else if looping {
        // Forward, then back without either end, which the next cycle and
        // this one show.
        frames.extend_from_slice(&first);
        frames.extend_from_slice(&second[1..second.len() - 1]);
    } else {
        for pass in 0..repeat {
            if pass == 0 {
                frames.extend_from_slice(&first);
            } else if pass % 2 == 1 {
                frames.extend_from_slice(&second[1..]);
            } else {
                frames.extend_from_slice(&first[1..]);
            }
        }
    }
    (frames, looping)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An export of `count` frames of 12 × 20 across a sheet, 100 ms each
    /// but the second, of 50, with `tags`, a list in JSON.
    fn export(count: u32, tags: &str) -> String {
        let frames: Vec<String> = (0..count)
            .map(|i| {
                format!(
                    r#"{{ "filename": "person {i}.aseprite",
                         "frame": {{ "x": {}, "y": 0, "w": 12, "h": 20 }}, "rotated": false, "trimmed": false,
                         "spriteSourceSize": {{ "x": 0, "y": 0, "w": 12, "h": 20 }},
                         "sourceSize": {{ "w": 12, "h": 20 }}, "duration": {} }}"#,
                    i * 12,
                    if i == 1 { 50 } else { 100 }
                )
            })
            .collect();
        format!(
            r#"{{ "frames": [{}],
                 "meta": {{ "app": "https://www.aseprite.org/", "version": "1.3", "image": "person.png",
                           "format": "RGBA8888", "size": {{ "w": {}, "h": 20 }}, "scale": "1",
                           "frameTags": {tags} }} }}"#,
            frames.join(", "),
            count * 12
        )
    }

    fn parsed(text: &str) -> Result<Export, CodecError> {
        parse(text, Limits::default())
    }

    /// The error `text` gives: where, and what.
    fn error(text: &str) -> (String, ErrorKind) {
        match parsed(text) {
            Err(CodecError::Aseprite { at, kind }) => (at, kind),
            other => panic!("expected an Aseprite error, got {other:?}"),
        }
    }

    #[test]
    fn an_export_as_aseprite_writes_it_is_read() {
        let text = export(
            5,
            r##"[ { "name": "stand", "from": 0, "to": 0, "direction": "forward", "color": "#000000ff" },
                 { "name": "walk", "from": 1, "to": 4, "direction": "forward", "repeat": "2",
                   "color": "#fe5b59ff", "data": "a\nb" } ]"##,
        );
        let export = parsed(&text).expect("an export");
        assert_eq!(export.image, "person.png");
        assert_eq!((export.width, export.height), (60, 20));
        assert_eq!(export.frames.len(), 5);
        assert_eq!(
            export.frames[1],
            ExportFrame {
                x: 12,
                y: 0,
                width: 12,
                height: 20,
                offset_x: 0,
                offset_y: 0,
                source_width: 12,
                source_height: 20,
                duration: 50,
            }
        );
        assert_eq!(export.tags[0].name, "stand");
        assert_eq!(
            (export.tags[0].frames.as_slice(), export.tags[0].looping),
            (&[0][..], true)
        );
        let walk = &export.tags[1];
        assert_eq!((walk.from, walk.to, walk.repeat), (1, 4, 2));
        assert_eq!(walk.direction, Direction::Forward);
        assert_eq!(walk.frames, [1, 2, 3, 4, 1, 2, 3, 4]);
        assert!(!walk.looping);
        // Tags may be left out, as without --list-tags.
        let untagged = export_without_tags(3);
        assert!(parsed(&untagged).expect("an export").tags.is_empty());
    }

    fn export_without_tags(count: u32) -> String {
        export(count, "[]").replace(r#", "frameTags": []"#, "")
    }

    #[test]
    fn a_trimmed_frame_is_placed_in_its_canvas() {
        let text = export(1, "[]").replacen(
            r#""frame": { "x": 0, "y": 0, "w": 12, "h": 20 }, "rotated": false, "trimmed": false,
                         "spriteSourceSize": { "x": 0, "y": 0, "w": 12, "h": 20 }"#,
            r#""frame": { "x": 2, "y": 3, "w": 10, "h": 17 }, "rotated": false, "trimmed": true,
                         "spriteSourceSize": { "x": 1, "y": 3, "w": 10, "h": 17 }"#,
            1,
        );
        let frame = parsed(&text).expect("an export").frames[0];
        assert_eq!(
            (frame.x, frame.y, frame.width, frame.height),
            (2, 3, 10, 17)
        );
        assert_eq!((frame.offset_x, frame.offset_y), (1, 3));
        assert_eq!((frame.source_width, frame.source_height), (12, 20));
    }

    /// The clip of a tag `from..=to` of `direction`, repeated `repeat`
    /// times or looping, in an export of 6 frames.
    fn clip_of(from: u32, to: u32, direction: &str, repeat: Option<u32>) -> (Vec<u32>, bool) {
        let repeat = repeat.map_or(String::new(), |n| format!(r#", "repeat": "{n}""#));
        let tags = format!(
            r#"[ {{ "name": "t", "from": {from}, "to": {to}, "direction": "{direction}"{repeat} }} ]"#
        );
        let tag = parsed(&export(6, &tags)).expect("an export").tags.remove(0);
        assert_eq!(
            tag.frames.len() as u64,
            clip_length(from, to, tag.direction, tag.repeat)
        );
        (tag.frames, tag.looping)
    }

    /// A tag's `from`, `to`, direction and repeat, and the frames of its
    /// clip and whether it loops.
    type Case<'a> = (u32, u32, &'a str, Option<u32>, &'a [u32], bool);

    #[test]
    fn each_direction_plays_the_frames_aseprite_plays() {
        let cases: &[Case<'_>] = &[
            // One frame: shown once a cycle, or once a repeat.
            (2, 2, "forward", None, &[2], true),
            (2, 2, "reverse", Some(3), &[2, 2, 2], false),
            (2, 2, "pingpong", None, &[2], true),
            (2, 2, "pingpong", Some(1), &[2], false),
            (2, 2, "pingpong", Some(3), &[2, 2, 2], false),
            (2, 2, "pingpong_reverse", Some(2), &[2, 2], false),
            // Two frames.
            (0, 1, "forward", None, &[0, 1], true),
            (0, 1, "reverse", Some(2), &[1, 0, 1, 0], false),
            (0, 1, "pingpong", None, &[0, 1], true),
            (0, 1, "pingpong", Some(3), &[0, 1, 0, 1], false),
            (0, 1, "pingpong_reverse", None, &[1, 0], true),
            (0, 1, "pingpong_reverse", Some(2), &[1, 0, 1], false),
            // Four frames.
            (1, 4, "forward", Some(1), &[1, 2, 3, 4], false),
            (1, 4, "reverse", None, &[4, 3, 2, 1], true),
            (1, 4, "pingpong", None, &[1, 2, 3, 4, 3, 2], true),
            (1, 4, "pingpong", Some(1), &[1, 2, 3, 4], false),
            (1, 4, "pingpong", Some(2), &[1, 2, 3, 4, 3, 2, 1], false),
            (
                1,
                4,
                "pingpong",
                Some(3),
                &[1, 2, 3, 4, 3, 2, 1, 2, 3, 4],
                false,
            ),
            (1, 4, "pingpong_reverse", None, &[4, 3, 2, 1, 2, 3], true),
            (
                1,
                4,
                "pingpong_reverse",
                Some(2),
                &[4, 3, 2, 1, 2, 3, 4],
                false,
            ),
            (
                1,
                4,
                "pingpong_reverse",
                Some(3),
                &[4, 3, 2, 1, 2, 3, 4, 3, 2, 1],
                false,
            ),
            // A repeat of 0 loops, as no repeat does.
            (1, 4, "forward", Some(0), &[1, 2, 3, 4], true),
        ];
        for &(from, to, direction, repeat, frames, looping) in cases {
            assert_eq!(
                clip_of(from, to, direction, repeat),
                (frames.to_vec(), looping),
                "{from}..={to} {direction} {repeat:?}"
            );
        }
    }

    #[test]
    fn clips_are_bounded_in_all_before_they_are_made() {
        // 65535 passes of 6 frames, twice: 786,420 frames, within the
        // default limit of a million, and then not.
        let tag = |name: &str| {
            format!(
                r#"{{ "name": "{name}", "from": 0, "to": 5, "direction": "forward", "repeat": "65535" }}"#
            )
        };
        let one = export(6, &format!("[{}]", tag("a")));
        assert_eq!(
            parsed(&one).expect("an export").tags[0].frames.len(),
            393_210
        );
        let two = export(6, &format!("[{}, {}, {}]", tag("a"), tag("b"), tag("c")));
        assert_eq!(
            error(&two),
            (
                "meta.frameTags[2]".to_string(),
                ErrorKind::TooManyFrames { most: 1_000_000 }
            )
        );
        // A tighter limit counts too.
        let limits = Limits {
            max_entries: 100,
            ..Limits::default()
        };
        assert!(matches!(
            parse(&one, limits),
            Err(CodecError::Aseprite {
                kind: ErrorKind::TooManyFrames { most: 100 },
                ..
            })
        ));
    }

    #[test]
    fn each_rule_is_checked_and_names_where() {
        let tags = |tag: &str| export(5, &format!("[{tag}]"));
        let frame0 = r#""frame": { "x": 0, "y": 0, "w": 12, "h": 20 }"#;
        let cases: Vec<(String, &str, ErrorKind)> = vec![
            (
                export(2, "[]").replacen(r#""frames": ["#, r#""frames": {"a": 1}, "x": ["#, 1),
                "frames",
                ErrorKind::NotJsonArray,
            ),
            (
                r#"{ "frames": [], "meta": { "image": "a.png", "size": { "w": 1, "h": 1 } } }"#
                    .to_string(),
                "frames",
                ErrorKind::NoFrames,
            ),
            (
                export(2, "[]").replacen(r#""rotated": false"#, r#""rotated": true"#, 1),
                "frames[0].rotated",
                ErrorKind::Rotated,
            ),
            (
                export(2, "[]").replacen(
                    frame0,
                    r#""frame": { "x": 0, "y": 0, "w": 0, "h": 20 }"#,
                    1,
                ),
                "frames[0].frame.w",
                ErrorKind::OutOfRange {
                    min: 1,
                    max: u32::MAX,
                },
            ),
            (
                export(2, "[]").replacen(
                    frame0,
                    r#""frame": { "x": 0, "y": 0, "w": 14, "h": 20 }"#,
                    1,
                ),
                "frames[0].spriteSourceSize",
                ErrorKind::InnerPadding,
            ),
            (
                export(2, "[]").replacen(
                    r#""spriteSourceSize": { "x": 0"#,
                    r#""spriteSourceSize": { "x": 1"#,
                    1,
                ),
                "frames[0].spriteSourceSize",
                ErrorKind::OutsideSource,
            ),
            (
                export(2, "[]").replacen(r#""w": 24, "h": 20"#, r#""w": 23, "h": 20"#, 1),
                "frames[1].frame",
                ErrorKind::OutsideSheet,
            ),
            (
                export(2, "[]")
                    .replacen(
                        r#""sourceSize": { "w": 12"#,
                        r#""sourceSize": { "w": 13"#,
                        2,
                    )
                    .replacen(
                        r#""sourceSize": { "w": 13"#,
                        r#""sourceSize": { "w": 12"#,
                        1,
                    ),
                "frames[1].sourceSize",
                ErrorKind::SeveralSprites,
            ),
            (
                export(2, "[]").replacen(r#""duration": 100"#, r#""duration": 65536"#, 1),
                "frames[0].duration",
                ErrorKind::OutOfRange {
                    min: 1,
                    max: MAX_DURATION,
                },
            ),
            (
                export(2, "[]").replacen(r#""duration": 100"#, r#""duration": 0"#, 1),
                "frames[0].duration",
                ErrorKind::OutOfRange {
                    min: 1,
                    max: MAX_DURATION,
                },
            ),
            (
                export(2, "[]").replacen(r#""image": "person.png","#, "", 1),
                "meta",
                ErrorKind::NoImage,
            ),
            (
                export(2, "[]").replacen(r#""person.png""#, r#""sheets/person.png""#, 1),
                "meta.image",
                ErrorKind::BadImage,
            ),
            (
                export(2, "[]").replacen(r#""person.png""#, r#""C:\\person.png""#, 1),
                "meta.image",
                ErrorKind::BadImage,
            ),
            (
                tags(r#"{ "name": "", "from": 0, "to": 0, "direction": "forward" }"#),
                "meta.frameTags[0].name",
                ErrorKind::BadTagName,
            ),
            (
                tags(r#"{ "name": "a/b", "from": 0, "to": 0, "direction": "forward" }"#),
                "meta.frameTags[0].name",
                ErrorKind::BadTagName,
            ),
            (
                tags(r#"{ "name": "a", "from": 2, "to": 1, "direction": "forward" }"#),
                "meta.frameTags[0]",
                ErrorKind::TagRange,
            ),
            (
                tags(r#"{ "name": "a", "from": 0, "to": 5, "direction": "forward" }"#),
                "meta.frameTags[0]",
                ErrorKind::TagRange,
            ),
            (
                tags(r#"{ "name": "a", "from": 0, "to": 0, "direction": "backward" }"#),
                "meta.frameTags[0].direction",
                ErrorKind::BadDirection {
                    found: "backward".to_string(),
                },
            ),
            (
                tags(r#"{ "name": "a", "from": 0, "to": 0, "direction": "forward", "repeat": 2 }"#),
                "meta.frameTags[0].repeat",
                ErrorKind::WrongType {
                    expected: "a string",
                },
            ),
            (
                tags(
                    r#"{ "name": "a", "from": 0, "to": 0, "direction": "forward" },
                       { "name": "b", "from": 0, "to": 0, "direction": "forward" },
                       { "name": "a", "from": 1, "to": 1, "direction": "forward" }"#,
                ),
                "meta.frameTags[2].name",
                ErrorKind::DuplicateTag {
                    name: "a".to_string(),
                },
            ),
            (
                export(2, "[]").replacen(r#""frameTags": []"#, r#""frameTags": {}"#, 1),
                "meta.frameTags",
                ErrorKind::WrongType { expected: "a list" },
            ),
        ];
        for (text, at, kind) in cases {
            assert!(parsed(&text).is_err(), "{at}: {text}");
            assert_eq!(error(&text), (at.to_string(), kind), "{text}");
        }
        // `repeat` as Aseprite writes it, and nothing else.
        for (repeat, ok) in [
            ("1", true),
            ("65535", true),
            ("00003", true),
            ("65536", false),
            ("+3", false),
            ("", false),
            (" 3", false),
            ("123456", false),
            ("-1", false),
        ] {
            let text = tags(&format!(
                r#"{{ "name": "a", "from": 0, "to": 0, "direction": "forward", "repeat": "{repeat}" }}"#
            ));
            assert_eq!(parsed(&text).is_ok(), ok, "repeat {repeat:?}");
            if !ok {
                assert_eq!(error(&text).1, ErrorKind::BadRepeat, "repeat {repeat:?}");
            }
        }
    }

    #[test]
    fn json_errors_keep_their_line_and_column() {
        // A raw tab in a tag's user data, which Aseprite does not escape.
        let text = export(
            1,
            "[ { \"name\": \"a\", \"from\": 0, \"to\": 0, \"direction\": \"forward\", \"data\": \"x\ty\" } ]",
        );
        assert!(matches!(parsed(&text), Err(CodecError::Json { .. })));
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
        let error = CodecError::Aseprite {
            at: "frames[3].frame.w".to_string(),
            kind: ErrorKind::OutOfRange { min: 1, max: 9 },
        };
        assert_eq!(
            error.to_string(),
            "Aseprite export error at `frames[3].frame.w`: must be from 1 to 9"
        );
        let error = CodecError::Aseprite {
            at: String::new(),
            kind: ErrorKind::MissingMember { name: "meta" },
        };
        assert_eq!(
            error.to_string(),
            "Aseprite export error in the file: `meta` is missing"
        );
    }
}
