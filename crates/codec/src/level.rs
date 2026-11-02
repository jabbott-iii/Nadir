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

//! The level file, `levels/<name>.klvl`, version 1: a level's tile layers,
//! its collision grid, its tall tiles and its entities, which the cook
//! makes of an LDtk level (`docs/subsystems/assets-and-codecs.md`, "Level
//! file"; ADR-0038). The cook writes it; the game reads it with [`parse`].
//!
//! ```text
//! { "version": 1,
//!   "size": [1024, 768],
//!   "background": [64, 70, 91],
//!   "assets": ["atlas/sprites.kbj"],
//!   "tilesets": [ { "atlas": "atlas/sprites.kbj", "sprite": "sprites/tiles",
//!                   "columns": 8, "rows": 4,
//!                   "animations": [ { "tile": 5, "frames": 2, "ms": 600 },
//!                                   { "tile": 8, "frames": 4, "ms": 250 } ] } ],
//!   "layers": [ { "tileset": 0, "size": [64, 48], "cell": 16, "origin": [0, 0],
//!                 "layer": 0, "depth": 0, "cells": [4, 4, 4, …] } ],
//!   "grid": { "size": [64, 48], "cell": 16, "origin": [0, 0], "layer": "World",
//!             "solid": [1, 1, 1, …] },
//!   "tall": [ { "tileset": 0, "tile": 4, "size": [1, 2], "feet": [104, 336], "layer": 1 } ],
//!   "entities": [ { "prefab": "tree", "position": [392, 112], "overrides": {} } ] }
//! ```
//!
//! It is KBJ, read with the KBJ decoder and its limits, then checked:
//!
//! - **The version first:** any whole number other than 1 is not
//!   supported, whatever else the file holds.
//! - **Members:** each object has the members above and no others; `grid`,
//!   and `music`, `ambience` and `camera` at the top, may be left out.
//! - **The level:** a size of 1 to [`MAX_SIDE`] pixels a side; a background
//!   of three channels of 0 to 255; its assets, paths that are not empty, in
//!   byte order with no two the same.
//! - **Tile sets:** an atlas among the level's assets, a sprite that is not
//!   empty, 1 or more tiles across and down and at most 65535 in all; its
//!   animations in order of their first tile, none overlapping the next,
//!   each 2 or more frames of the set's tiles, from the first on, each shown
//!   1 to 65535 ms.
//! - **Layers:** of a tile set the file has; a size of 1 or more cells a
//!   side, of 1 to [`MAX_CELL`] pixels; a sort layer of 0 to 255 and a
//!   depth that fits 32 bits; one cell for each, row by row from the top
//!   left: 0 for none, otherwise 1 + the tile + 65536 × its flip (1 across,
//!   2 down, 3 both), each a tile of the set.
//! - **The grid:** a size and cell as a layer's, the name of its collision
//!   layer, and one 0 or 1 for each cell, 1 for solid.
//! - **Tall tiles:** a block of a set's tiles, of 1 or more a side, from
//!   its top-left tile, inside the set; a place for its feet; a sort layer
//!   of 0 to 255.
//! - **Entities:** a prefab's name ([`prefab::is_name`]); a position; and
//!   overrides, an object of component names
//!   ([`prefab::is_component_name`]) whose values are not `null`.
//! - **The rest:** music and ambience are asset paths that are not empty; a
//!   camera's bounds are `[x, y, w, h]`, its size 1 or more.
//!
//! Positions are whole pixels that fit an `i32`.
//!
//! A problem is a [`CodecError::Level`], which names where it is, such as
//! `layers[0].cells[130]` or `entities[2].overrides.Health`.

use std::fmt;

use crate::json::Value;
use crate::{CodecError, Limits, kbj, prefab};

/// The version of the level file that [`parse`] reads.
pub const VERSION: i64 = 1;

/// The longest side of a level, in pixels: 2^24, where an `f32` still
/// counts every pixel.
pub const MAX_SIDE: u32 = 1 << 24;

/// The largest side of a cell, in pixels.
pub const MAX_CELL: u32 = 4096;

/// The most tiles a tile set may have, so that 1 + a tile fits 16 bits.
pub const MAX_TILES: u32 = 65535;

/// A level file: a level's size, its tile layers, its grid, its tall tiles
/// and its entities.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct LevelFile {
    /// Its width and height in pixels.
    pub size: [u32; 2],
    /// Its background colour: red, green and blue.
    pub background: [u8; 3],
    /// The assets it uses, in byte order: each tile set's atlas among them.
    pub assets: Vec<String>,
    /// Its tile sets, which its layers and tall tiles name by index.
    pub tilesets: Vec<TilesetEntry>,
    /// Its tile layers, in the order they are drawn: the first beneath.
    pub layers: Vec<LayerEntry>,
    /// Its collision grid, if it has one.
    pub grid: Option<GridEntry>,
    /// Its tall tiles, drawn as y-sorted sprites.
    pub tall: Vec<TallEntry>,
    /// Its entities, in the order they spawn.
    pub entities: Vec<EntityEntry>,
    /// The music it plays, an asset path.
    pub music: Option<String>,
    /// The ambient sound it plays, an asset path.
    pub ambience: Option<String>,
    /// Where the camera may look, if it is bounded.
    pub camera: Option<Bounds>,
}

/// A tile set of a level: a tile set sprite of an atlas, with its grid and
/// its animated tiles.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TilesetEntry {
    /// The atlas file it is in, such as `atlas/sprites.kbj`.
    pub atlas: String,
    /// Its sprite's name in the atlas, such as `sprites/tiles`.
    pub sprite: String,
    /// Its tiles across: 1 or more.
    pub columns: u16,
    /// Its tiles down: 1 or more.
    pub rows: u16,
    /// Its animated tiles, in order of their first tile.
    pub animations: Vec<Animation>,
}

impl TilesetEntry {
    /// How many tiles it has: at most [`MAX_TILES`].
    pub fn tile_count(&self) -> u32 {
        u32::from(self.columns) * u32::from(self.rows)
    }

    /// The animation that starts at `tile`, if one does.
    pub fn animation(&self, tile: u16) -> Option<&Animation> {
        self.animations
            .binary_search_by_key(&tile, |animation| animation.tile)
            .ok()
            .map(|index| &self.animations[index])
    }
}

/// An animated tile: `frames` tiles from `tile`, each shown `millis`
/// milliseconds in turn, as `render::Tile::animated` draws them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Animation {
    /// The first frame's tile.
    pub tile: u16,
    /// How many frames: 2 or more.
    pub frames: u16,
    /// How long each frame shows, in milliseconds: 1 or more.
    pub millis: u16,
}

/// A tile layer of a level.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct LayerEntry {
    /// Its tile set, an index into the level's.
    pub tileset: u32,
    /// Its cells across and down: 1 or more.
    pub size: [u32; 2],
    /// A cell's side in pixels: 1 to [`MAX_CELL`].
    pub cell: u32,
    /// Its top-left corner in the level, in pixels.
    pub origin: [i32; 2],
    /// Its sort layer.
    pub layer: u8,
    /// Its depth among the sprites of its sort layer, as a sprite's fixed
    /// depth.
    pub depth: u32,
    /// Its cells, row by row from the top left.
    pub cells: Vec<Option<Cell>>,
}

/// What a cell of a tile layer shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Cell {
    /// The tile, of the layer's tile set: an animation's first tile if one
    /// starts there.
    pub tile: u16,
    /// Whether it is mirrored across.
    pub flip_x: bool,
    /// Whether it is mirrored down.
    pub flip_y: bool,
}

/// The collision grid of a level.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct GridEntry {
    /// Its cells across and down: 1 or more.
    pub size: [u32; 2],
    /// A cell's side in pixels: 1 to [`MAX_CELL`].
    pub cell: u32,
    /// Its top-left corner in the level, in pixels.
    pub origin: [i32; 2],
    /// The name of the collision layer its solid cells are on, such as
    /// `World`.
    pub layer: String,
    /// Whether each cell is solid, row by row from the top left.
    pub solid: Vec<bool>,
}

/// A tall tile: a block of tiles drawn as one y-sorted sprite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TallEntry {
    /// Its tile set, an index into the level's.
    pub tileset: u32,
    /// The block's top-left tile.
    pub tile: u16,
    /// The block's tiles across and down: 1 or more, inside the set.
    pub size: [u16; 2],
    /// Where its feet are in the level, in pixels: the bottom middle of the
    /// block.
    pub feet: [i32; 2],
    /// Its sort layer.
    pub layer: u8,
}

/// An entity of a level: the prefab it spawns, where, and what of the
/// prefab it overrides.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct EntityEntry {
    /// The prefab's name.
    pub prefab: String,
    /// Where it is in the level, in pixels.
    pub position: [i32; 2],
    /// The values it gives its components over the prefab's, by type name,
    /// in the file's order.
    pub overrides: Vec<(String, Value)>,
}

/// Where a camera may look: a rectangle in the level, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Bounds {
    /// Its left edge.
    pub x: i32,
    /// Its top edge.
    pub y: i32,
    /// Its width: 1 or more.
    pub width: u32,
    /// Its height: 1 or more.
    pub height: u32,
}

/// What is wrong with a level file. See [`CodecError::Level`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
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
    /// A list does not have the number of items it needs.
    WrongLength {
        /// How many it needs.
        expected: u64,
    },
    /// A name or path is empty.
    EmptyString,
    /// An asset, or an animation, does not come after the one before it.
    NotSorted,
    /// A tile set's atlas is not one of the level's assets.
    NotAnAsset,
    /// A tile set has more than [`MAX_TILES`] tiles.
    TooManyTiles,
    /// An index names no tile set of the level.
    NoSuchTileset {
        /// The index.
        index: u32,
    },
    /// A tile is not one of its tile set's.
    NoSuchTile {
        /// The tile.
        tile: u32,
    },
    /// A cell's flips are not 0 to 3.
    BadFlip,
    /// An animation overlaps the next one.
    Overlap,
    /// An animation, or a tall tile's block, runs past its tile set.
    PastTileset,
    /// A grid's cell is not 0 or 1.
    NotSolidOrEmpty,
    /// A prefab's name is not one [`prefab::is_name`] takes.
    BadPrefabName,
    /// An override's component name is not one
    /// [`prefab::is_component_name`] takes.
    BadComponentName,
    /// An override's value is `null`.
    NullOverride,
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
            Self::WrongLength { expected } => write!(f, "must have {expected} items"),
            Self::EmptyString => write!(f, "must not be an empty string"),
            Self::NotSorted => write!(f, "must come after the one before it, with no two the same"),
            Self::NotAnAsset => write!(f, "the atlas is not one of the level's assets"),
            Self::TooManyTiles => write!(f, "a tile set has at most {MAX_TILES} tiles"),
            Self::NoSuchTileset { index } => write!(f, "the level has no tile set {index}"),
            Self::NoSuchTile { tile } => write!(f, "the tile set has no tile {tile}"),
            Self::BadFlip => write!(f, "a cell's flips are 0 to 3"),
            Self::Overlap => write!(f, "the animation overlaps the next"),
            Self::PastTileset => write!(f, "it runs past its tile set's tiles"),
            Self::NotSolidOrEmpty => write!(f, "a grid's cell is 0 or 1"),
            Self::BadPrefabName => write!(
                f,
                "a prefab's name is lowercase letters, digits and `_`, one at least"
            ),
            Self::BadComponentName => write!(
                f,
                "a component's name is a letter, then letters, digits and `_`"
            ),
            Self::NullOverride => write!(f, "an override's value is not `null`"),
        }
    }
}

/// Reads a level file: KBJ, decoded within `limits`, then checked as the
/// module says.
///
/// # Errors
///
/// [`CodecError::Kbj`] if the bytes are not KBJ, and [`CodecError::Level`],
/// naming where, if they are not a level file of version 1.
pub fn parse(bytes: &[u8], limits: Limits) -> Result<LevelFile, CodecError> {
    read(&kbj::decode(bytes, limits)?)
}

/// Where a value is in the file, for errors: made into text only for one.
#[derive(Clone, Copy)]
enum At<'a> {
    Root,
    Member(&'a At<'a>, &'static str),
    /// A member whose name the file gives, such as an override's.
    Key(&'a At<'a>, &'a str),
    Item(&'a At<'a>, usize),
}

impl At<'_> {
    fn error(&self, kind: ErrorKind) -> CodecError {
        CodecError::Level {
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
            Self::Key(At::Root, name) => write!(f, "{name}"),
            Self::Member(parent, name) => write!(f, "{parent}.{name}"),
            Self::Key(parent, name) => write!(f, "{parent}.{name}"),
            Self::Item(parent, index) => write!(f, "{parent}[{index}]"),
        }
    }
}

/// An object's members, checked against those it may have.
struct Members<'v, 'a> {
    members: &'v [(String, Value)],
    at: &'a At<'a>,
}

impl<'v, 'a> Members<'v, 'a> {
    fn of(value: &'v Value, at: &'a At<'a>, allowed: &[&'static str]) -> Result<Self, CodecError> {
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
        Ok(Self { members, at })
    }

    fn get(&self, name: &str) -> Option<&'v Value> {
        self.members
            .iter()
            .find(|(member, _)| member == name)
            .map(|(_, value)| value)
    }

    fn require(&self, name: &'static str) -> Result<&'v Value, CodecError> {
        self.get(name)
            .ok_or_else(|| self.at.error(ErrorKind::MissingMember { name }))
    }

    fn string(&self, name: &'static str) -> Result<String, CodecError> {
        string(self.require(name)?, &At::Member(self.at, name))
    }

    fn optional_string(&self, name: &'static str) -> Result<Option<String>, CodecError> {
        self.get(name)
            .map(|value| string(value, &At::Member(self.at, name)))
            .transpose()
    }

    fn whole(&self, name: &'static str, min: i64, max: i64) -> Result<i64, CodecError> {
        whole(self.require(name)?, &At::Member(self.at, name), min, max)
    }

    fn u32(&self, name: &'static str, min: u32, max: u32) -> Result<u32, CodecError> {
        let number = self.whole(name, i64::from(min), i64::from(max))?;
        Ok(u32::try_from(number).expect("whole checked the range"))
    }

    fn u16(&self, name: &'static str, min: u16, max: u16) -> Result<u16, CodecError> {
        let number = self.whole(name, i64::from(min), i64::from(max))?;
        Ok(u16::try_from(number).expect("whole checked the range"))
    }

    fn u8(&self, name: &'static str) -> Result<u8, CodecError> {
        let number = self.whole(name, 0, 255)?;
        Ok(u8::try_from(number).expect("whole checked the range"))
    }

    fn array(&self, name: &'static str) -> Result<&'v [Value], CodecError> {
        array(self.require(name)?, &At::Member(self.at, name))
    }

    /// Whole numbers from `min` to `max`, `N` of them.
    fn numbers<const N: usize>(
        &self,
        name: &'static str,
        min: i64,
        max: i64,
    ) -> Result<[i64; N], CodecError> {
        numbers(self.require(name)?, &At::Member(self.at, name), min, max)
    }

    /// Two whole numbers that fit an `i32`.
    fn point(&self, name: &'static str) -> Result<[i32; 2], CodecError> {
        let [x, y] = self.numbers::<2>(name, i64::from(i32::MIN), i64::from(i32::MAX))?;
        let fit = |number: i64| i32::try_from(number).expect("numbers checked the range");
        Ok([fit(x), fit(y)])
    }

    /// A size of two sides from 1 to `max`.
    fn size(&self, name: &'static str, max: u32) -> Result<[u32; 2], CodecError> {
        let [w, h] = self.numbers::<2>(name, 1, i64::from(max))?;
        let fit = |number: i64| u32::try_from(number).expect("numbers checked the range");
        Ok([fit(w), fit(h)])
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
fn whole(value: &Value, at: &At<'_>, min: i64, max: i64) -> Result<i64, CodecError> {
    let Value::Int(number) = value else {
        return Err(at.error(ErrorKind::WrongType {
            expected: "a whole number",
        }));
    };
    if (min..=max).contains(number) {
        Ok(*number)
    } else {
        Err(at.error(ErrorKind::OutOfRange { min, max }))
    }
}

/// A list of exactly `N` whole numbers from `min` to `max`.
fn numbers<const N: usize>(
    value: &Value,
    at: &At<'_>,
    min: i64,
    max: i64,
) -> Result<[i64; N], CodecError> {
    let items = array(value, at)?;
    if items.len() != N {
        return Err(at.error(ErrorKind::WrongLength { expected: N as u64 }));
    }
    let mut out = [0; N];
    for (index, (out, item)) in out.iter_mut().zip(items).enumerate() {
        *out = whole(item, &At::Item(at, index), min, max)?;
    }
    Ok(out)
}

/// Checks that a list of cells has one for each of a grid of `size`.
fn cell_count(items: &[Value], size: [u32; 2], at: &At<'_>) -> Result<(), CodecError> {
    let expected = u64::from(size[0]) * u64::from(size[1]);
    if items.len() as u64 == expected {
        Ok(())
    } else {
        Err(at.error(ErrorKind::WrongLength { expected }))
    }
}

/// Members of a component's values: names and values a prefab could have.
fn overrides(value: &Value, at: &At<'_>) -> Result<Vec<(String, Value)>, CodecError> {
    let members = value.as_object().ok_or_else(|| {
        at.error(ErrorKind::WrongType {
            expected: "an object",
        })
    })?;
    for (name, value) in members {
        let at = At::Key(at, name);
        if !prefab::is_component_name(name) {
            return Err(at.error(ErrorKind::BadComponentName));
        }
        if value.is_null() {
            return Err(at.error(ErrorKind::NullOverride));
        }
    }
    Ok(members.to_vec())
}

fn read(value: &Value) -> Result<LevelFile, CodecError> {
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
            "size",
            "background",
            "assets",
            "tilesets",
            "layers",
            "grid",
            "tall",
            "entities",
            "music",
            "ambience",
            "camera",
        ],
    )?;
    top.require("version")?;
    let size = top.size("size", MAX_SIDE)?;
    let [r, g, b] = top.numbers::<3>("background", 0, 255)?;
    let channel = |number: i64| u8::try_from(number).expect("numbers checked the range");
    let background = [channel(r), channel(g), channel(b)];

    let assets_at = At::Member(&root, "assets");
    let items = top.array("assets")?;
    let mut assets: Vec<String> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = At::Item(&assets_at, index);
        let asset = string(item, &at)?;
        if assets
            .last()
            .is_some_and(|before| before.as_bytes() >= asset.as_bytes())
        {
            return Err(at.error(ErrorKind::NotSorted));
        }
        assets.push(asset);
    }

    let tilesets_at = At::Member(&root, "tilesets");
    let items = top.array("tilesets")?;
    let mut tilesets = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        tilesets.push(tileset(item, &At::Item(&tilesets_at, index), &assets)?);
    }

    let layers_at = At::Member(&root, "layers");
    let items = top.array("layers")?;
    let mut layers = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        layers.push(layer(item, &At::Item(&layers_at, index), &tilesets)?);
    }

    let grid = match top.get("grid") {
        Some(value) => Some(grid(value, &At::Member(&root, "grid"))?),
        None => None,
    };

    let tall_at = At::Member(&root, "tall");
    let items = top.array("tall")?;
    let mut tall = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        tall.push(tall_tile(item, &At::Item(&tall_at, index), &tilesets)?);
    }

    let entities_at = At::Member(&root, "entities");
    let items = top.array("entities")?;
    let mut entities = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        entities.push(entity(item, &At::Item(&entities_at, index))?);
    }

    let camera = match top.get("camera") {
        Some(value) => {
            let at = At::Member(&root, "camera");
            let [x, y, w, h] = numbers::<4>(value, &at, i64::from(i32::MIN), i64::from(i32::MAX))?;
            let side = |number: i64, index: usize| {
                u32::try_from(number)
                    .ok()
                    .filter(|side| *side >= 1)
                    .ok_or_else(|| {
                        At::Item(&at, index).error(ErrorKind::OutOfRange {
                            min: 1,
                            max: i64::from(i32::MAX),
                        })
                    })
            };
            let fit = |number: i64| i32::try_from(number).expect("numbers checked the range");
            Some(Bounds {
                x: fit(x),
                y: fit(y),
                width: side(w, 2)?,
                height: side(h, 3)?,
            })
        }
        None => None,
    };

    Ok(LevelFile {
        size,
        background,
        assets,
        tilesets,
        layers,
        grid,
        tall,
        entities,
        music: top.optional_string("music")?,
        ambience: top.optional_string("ambience")?,
        camera,
    })
}

fn tileset(value: &Value, at: &At<'_>, assets: &[String]) -> Result<TilesetEntry, CodecError> {
    let members = Members::of(
        value,
        at,
        &["atlas", "sprite", "columns", "rows", "animations"],
    )?;
    let atlas = members.string("atlas")?;
    if assets.binary_search(&atlas).is_err() {
        return Err(At::Member(at, "atlas").error(ErrorKind::NotAnAsset));
    }
    let sprite = members.string("sprite")?;
    let columns = members.u16("columns", 1, u16::MAX)?;
    let rows = members.u16("rows", 1, u16::MAX)?;
    let count = u32::from(columns) * u32::from(rows);
    if count > MAX_TILES {
        return Err(at.error(ErrorKind::TooManyTiles));
    }

    let list_at = At::Member(at, "animations");
    let items = members.array("animations")?;
    let mut animations: Vec<Animation> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = At::Item(&list_at, index);
        let entry = Members::of(item, &at, &["tile", "frames", "ms"])?;
        let tile = entry.u16("tile", 0, u16::MAX)?;
        if u32::from(tile) >= count {
            return Err(At::Member(&at, "tile").error(ErrorKind::NoSuchTile {
                tile: u32::from(tile),
            }));
        }
        let frames = entry.u16("frames", 2, u16::MAX)?;
        let millis = entry.u16("ms", 1, u16::MAX)?;
        if u32::from(tile) + u32::from(frames) > count {
            return Err(at.error(ErrorKind::PastTileset));
        }
        if let Some(before) = animations.last() {
            if before.tile >= tile {
                return Err(At::Member(&at, "tile").error(ErrorKind::NotSorted));
            }
            if u32::from(before.tile) + u32::from(before.frames) > u32::from(tile) {
                return Err(At::Item(&list_at, index - 1).error(ErrorKind::Overlap));
            }
        }
        animations.push(Animation {
            tile,
            frames,
            millis,
        });
    }
    Ok(TilesetEntry {
        atlas,
        sprite,
        columns,
        rows,
        animations,
    })
}

/// The tile set an index names.
fn tileset_of<'t>(
    members: &Members<'_, '_>,
    at: &At<'_>,
    tilesets: &'t [TilesetEntry],
) -> Result<(u32, &'t TilesetEntry), CodecError> {
    let index = members.u32("tileset", 0, u32::MAX)?;
    usize::try_from(index)
        .ok()
        .and_then(|index| tilesets.get(index))
        .map(|tileset| (index, tileset))
        .ok_or_else(|| At::Member(at, "tileset").error(ErrorKind::NoSuchTileset { index }))
}

fn layer(value: &Value, at: &At<'_>, tilesets: &[TilesetEntry]) -> Result<LayerEntry, CodecError> {
    let members = Members::of(
        value,
        at,
        &[
            "tileset", "size", "cell", "origin", "layer", "depth", "cells",
        ],
    )?;
    let (index, tileset) = tileset_of(&members, at, tilesets)?;
    let size = members.size("size", u32::MAX)?;
    let cells_at = At::Member(at, "cells");
    let items = members.array("cells")?;
    cell_count(items, size, &cells_at)?;
    let count = tileset.tile_count();
    let mut cells = Vec::with_capacity(items.len());
    for (position, item) in items.iter().enumerate() {
        let at = At::Item(&cells_at, position);
        let value = whole(item, &at, 0, i64::from(u32::MAX))?;
        if value == 0 {
            cells.push(None);
            continue;
        }
        let packed = u32::try_from(value - 1).expect("whole checked the range");
        let tile = packed & 0xFFFF;
        let flip = packed >> 16;
        if flip > 3 {
            return Err(at.error(ErrorKind::BadFlip));
        }
        if tile >= count {
            return Err(at.error(ErrorKind::NoSuchTile { tile }));
        }
        cells.push(Some(Cell {
            tile: u16::try_from(tile).expect("a tile of at most 65535"),
            flip_x: flip & 1 != 0,
            flip_y: flip & 2 != 0,
        }));
    }
    Ok(LayerEntry {
        tileset: index,
        size,
        cell: members.u32("cell", 1, MAX_CELL)?,
        origin: members.point("origin")?,
        layer: members.u8("layer")?,
        depth: members.u32("depth", 0, u32::MAX)?,
        cells,
    })
}

fn grid(value: &Value, at: &At<'_>) -> Result<GridEntry, CodecError> {
    let members = Members::of(value, at, &["size", "cell", "origin", "layer", "solid"])?;
    let size = members.size("size", u32::MAX)?;
    let solid_at = At::Member(at, "solid");
    let items = members.array("solid")?;
    cell_count(items, size, &solid_at)?;
    let mut solid = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        match item {
            Value::Int(0) => solid.push(false),
            Value::Int(1) => solid.push(true),
            _ => return Err(At::Item(&solid_at, index).error(ErrorKind::NotSolidOrEmpty)),
        }
    }
    Ok(GridEntry {
        size,
        cell: members.u32("cell", 1, MAX_CELL)?,
        origin: members.point("origin")?,
        layer: members.string("layer")?,
        solid,
    })
}

fn tall_tile(
    value: &Value,
    at: &At<'_>,
    tilesets: &[TilesetEntry],
) -> Result<TallEntry, CodecError> {
    let members = Members::of(value, at, &["tileset", "tile", "size", "feet", "layer"])?;
    let (index, tileset) = tileset_of(&members, at, tilesets)?;
    let tile = members.u16("tile", 0, u16::MAX)?;
    if u32::from(tile) >= tileset.tile_count() {
        return Err(At::Member(at, "tile").error(ErrorKind::NoSuchTile {
            tile: u32::from(tile),
        }));
    }
    let [w, h] = members.numbers::<2>("size", 1, i64::from(u16::MAX))?;
    let side = |number: i64| u16::try_from(number).expect("numbers checked the range");
    let size = [side(w), side(h)];
    let columns = u32::from(tileset.columns);
    let (column, row) = (u32::from(tile) % columns, u32::from(tile) / columns);
    if column + u32::from(size[0]) > columns || row + u32::from(size[1]) > u32::from(tileset.rows) {
        return Err(At::Member(at, "size").error(ErrorKind::PastTileset));
    }
    Ok(TallEntry {
        tileset: index,
        tile,
        size,
        feet: members.point("feet")?,
        layer: members.u8("layer")?,
    })
}

fn entity(value: &Value, at: &At<'_>) -> Result<EntityEntry, CodecError> {
    let members = Members::of(value, at, &["prefab", "position", "overrides"])?;
    let prefab = members.string("prefab")?;
    if !prefab::is_name(&prefab) {
        return Err(At::Member(at, "prefab").error(ErrorKind::BadPrefabName));
    }
    Ok(EntityEntry {
        prefab,
        position: members.point("position")?,
        overrides: overrides(members.require("overrides")?, &At::Member(at, "overrides"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    /// The bytes of the level file `text` describes, in JSON.
    fn kbj(text: &str) -> Vec<u8> {
        let value = json::parse(text, Limits::default()).expect("the test's JSON");
        kbj::encode(&value, Limits::default()).expect("encodable")
    }

    fn parsed(text: &str) -> Result<LevelFile, CodecError> {
        parse(&kbj(text), Limits::default())
    }

    /// The error `text` gives: where, and what.
    fn error(text: &str) -> (String, ErrorKind) {
        match parsed(text) {
            Err(CodecError::Level { at, kind }) => (at, kind),
            other => panic!("expected a level error, got {other:?}"),
        }
    }

    /// A level of 3 × 2 cells of 16 pixels: grass, water that ripples, a
    /// path, mirrored grass, nothing, and a stone; walled where the water
    /// and the stone are; a tall wall, a tree, and a rock with an override.
    const LEVEL: &str = r#"{ "version": 1, "size": [48, 32], "background": [64, 70, 91],
        "assets": ["atlas/props.kbj", "atlas/sprites.kbj"],
        "tilesets": [ { "atlas": "atlas/sprites.kbj", "sprite": "sprites/tiles", "columns": 8, "rows": 4,
                        "animations": [ { "tile": 5, "frames": 2, "ms": 600 }, { "tile": 8, "frames": 4, "ms": 250 } ] } ],
        "layers": [ { "tileset": 0, "size": [3, 2], "cell": 16, "origin": [0, 0], "layer": 0, "depth": 0,
                      "cells": [1, 9, 3, 196609, 0, 4] } ],
        "grid": { "size": [3, 2], "cell": 16, "origin": [0, 0], "layer": "World", "solid": [0, 1, 0, 0, 0, 1] },
        "tall": [ { "tileset": 0, "tile": 4, "size": [1, 2], "feet": [8, 32], "layer": 1 } ],
        "entities": [ { "prefab": "tree", "position": [24, 16], "overrides": {} },
                      { "prefab": "rock", "position": [40, 32], "overrides": { "Health": { "max": 3 } } } ] }"#;

    /// [`LEVEL`] with `from` replaced by `to`.
    fn level_with(from: &str, to: &str) -> String {
        assert!(LEVEL.contains(from), "{from}");
        LEVEL.replacen(from, to, 1)
    }

    #[test]
    fn a_level_as_the_cook_writes_it_is_read() {
        let level = parsed(LEVEL).expect("a level");
        assert_eq!(level.size, [48, 32]);
        assert_eq!(level.background, [64, 70, 91]);
        assert_eq!(level.assets, ["atlas/props.kbj", "atlas/sprites.kbj"]);
        let tiles = &level.tilesets[0];
        assert_eq!(
            (tiles.atlas.as_str(), tiles.sprite.as_str()),
            ("atlas/sprites.kbj", "sprites/tiles")
        );
        assert_eq!(tiles.tile_count(), 32);
        assert_eq!(
            tiles.animation(8),
            Some(&Animation {
                tile: 8,
                frames: 4,
                millis: 250
            })
        );
        assert_eq!(tiles.animation(9), None, "only where one starts");
        let cell = |tile, flip_x, flip_y| {
            Some(Cell {
                tile,
                flip_x,
                flip_y,
            })
        };
        assert_eq!(
            level.layers[0].cells,
            [
                cell(0, false, false),
                cell(8, false, false),
                cell(2, false, false),
                cell(0, true, true),
                None,
                cell(3, false, false),
            ]
        );
        let grid = level.grid.as_ref().expect("a grid");
        assert_eq!(grid.layer, "World");
        assert_eq!(grid.solid, [false, true, false, false, false, true]);
        assert_eq!(
            level.tall,
            [TallEntry {
                tileset: 0,
                tile: 4,
                size: [1, 2],
                feet: [8, 32],
                layer: 1,
            }]
        );
        assert_eq!(level.entities[1].prefab, "rock");
        assert_eq!(level.entities[1].position, [40, 32]);
        assert_eq!(level.entities[1].overrides[0].0, "Health");
        assert_eq!((level.music.clone(), level.camera), (None, None));
        // The rest, and no grid.
        let more = level_with(
            r#""version": 1,"#,
            r#""version": 1, "music": "audio/field.qoa", "ambience": "audio/wind.qoa", "camera": [-8, 0, 64, 32],"#,
        );
        let more = more.replacen(
            r#""grid": { "size": [3, 2], "cell": 16, "origin": [0, 0], "layer": "World", "solid": [0, 1, 0, 0, 0, 1] },"#,
            "",
            1,
        );
        let level = parsed(&more).expect("a level");
        assert_eq!(level.music.as_deref(), Some("audio/field.qoa"));
        assert_eq!(level.ambience.as_deref(), Some("audio/wind.qoa"));
        assert_eq!(
            level.camera,
            Some(Bounds {
                x: -8,
                y: 0,
                width: 64,
                height: 32
            })
        );
        assert_eq!(level.grid, None);
    }

    #[test]
    fn another_version_is_not_supported_whatever_it_holds() {
        for (version, found) in [("2", 2), ("0", 0)] {
            let later = format!(r#"{{ "version": {version}, "chunks": [] }}"#);
            assert_eq!(
                error(&later),
                ("version".to_string(), ErrorKind::Version { found })
            );
        }
        let error = CodecError::Level {
            at: "version".to_string(),
            kind: ErrorKind::Version { found: 2 },
        };
        assert_eq!(
            error.to_string(),
            "level file error at `version`: version 2 is not supported: this build reads version 1"
        );
    }

    #[test]
    fn each_rule_is_checked_and_names_where() {
        let cases: [(String, &str, ErrorKind); 34] = [
            (
                level_with(r#""version": 1"#, r#""version": "1""#),
                "version",
                ErrorKind::WrongType {
                    expected: "a whole number",
                },
            ),
            (
                level_with(r#""version": 1, "#, ""),
                "",
                ErrorKind::MissingMember { name: "version" },
            ),
            (
                level_with(r#""version": 1"#, r#""version": 1, "chunks": []"#),
                "",
                ErrorKind::UnknownMember {
                    name: "chunks".to_string(),
                },
            ),
            (
                level_with(r#""size": [48, 32]"#, r#""size": [0, 32]"#),
                "size[0]",
                ErrorKind::OutOfRange {
                    min: 1,
                    max: i64::from(MAX_SIDE),
                },
            ),
            (
                level_with("[64, 70, 91]", "[64, 70, 256]"),
                "background[2]",
                ErrorKind::OutOfRange { min: 0, max: 255 },
            ),
            (
                level_with("[64, 70, 91]", "[64, 70]"),
                "background",
                ErrorKind::WrongLength { expected: 3 },
            ),
            (
                level_with(
                    r#"["atlas/props.kbj", "atlas/sprites.kbj"]"#,
                    r#"["atlas/sprites.kbj", "atlas/props.kbj"]"#,
                ),
                "assets[1]",
                ErrorKind::NotSorted,
            ),
            (
                level_with(
                    r#""atlas": "atlas/sprites.kbj""#,
                    r#""atlas": "atlas/tiles.kbj""#,
                ),
                "tilesets[0].atlas",
                ErrorKind::NotAnAsset,
            ),
            (
                level_with(
                    r#""columns": 8, "rows": 4"#,
                    r#""columns": 256, "rows": 256"#,
                ),
                "tilesets[0]",
                ErrorKind::TooManyTiles,
            ),
            (
                level_with(
                    r#""tile": 8, "frames": 4, "ms": 250"#,
                    r#""tile": 8, "frames": 4, "ms": 0"#,
                ),
                "tilesets[0].animations[1].ms",
                ErrorKind::OutOfRange { min: 1, max: 65535 },
            ),
            (
                level_with(r#""tile": 8, "frames": 4"#, r#""tile": 8, "frames": 1"#),
                "tilesets[0].animations[1].frames",
                ErrorKind::OutOfRange { min: 2, max: 65535 },
            ),
            (
                level_with(r#""tile": 8, "frames": 4"#, r#""tile": 30, "frames": 4"#),
                "tilesets[0].animations[1]",
                ErrorKind::PastTileset,
            ),
            (
                level_with(r#""tile": 5, "frames": 2"#, r#""tile": 5, "frames": 4"#),
                "tilesets[0].animations[0]",
                ErrorKind::Overlap,
            ),
            (
                level_with(r#""tile": 5, "frames": 2"#, r#""tile": 9, "frames": 2"#),
                "tilesets[0].animations[1].tile",
                ErrorKind::NotSorted,
            ),
            (
                level_with(
                    r#""tileset": 0, "size": [3, 2]"#,
                    r#""tileset": 1, "size": [3, 2]"#,
                ),
                "layers[0].tileset",
                ErrorKind::NoSuchTileset { index: 1 },
            ),
            (
                level_with("[1, 9, 3, 196609, 0, 4]", "[1, 9, 3, 196609, 0]"),
                "layers[0].cells",
                ErrorKind::WrongLength { expected: 6 },
            ),
            (
                level_with("[1, 9, 3, 196609, 0, 4]", "[1, 9, 3, 196609, 0, 33]"),
                "layers[0].cells[5]",
                ErrorKind::NoSuchTile { tile: 32 },
            ),
            (
                level_with("[1, 9, 3, 196609, 0, 4]", "[1, 9, 3, 262145, 0, 4]"),
                "layers[0].cells[3]",
                ErrorKind::BadFlip,
            ),
            (
                level_with("[1, 9, 3, 196609, 0, 4]", "[1, 9, 3, -1, 0, 4]"),
                "layers[0].cells[3]",
                ErrorKind::OutOfRange {
                    min: 0,
                    max: i64::from(u32::MAX),
                },
            ),
            (
                level_with(
                    r#""cell": 16, "origin": [0, 0], "layer": 0"#,
                    r#""cell": 4097, "origin": [0, 0], "layer": 0"#,
                ),
                "layers[0].cell",
                ErrorKind::OutOfRange {
                    min: 1,
                    max: i64::from(MAX_CELL),
                },
            ),
            (
                level_with("[0, 1, 0, 0, 0, 1]", "[0, 1, 0, 0, 0, 2]"),
                "grid.solid[5]",
                ErrorKind::NotSolidOrEmpty,
            ),
            (
                level_with("[0, 1, 0, 0, 0, 1]", "[0, 1, 0]"),
                "grid.solid",
                ErrorKind::WrongLength { expected: 6 },
            ),
            (
                level_with(
                    r#""tile": 4, "size": [1, 2]"#,
                    r#""tile": 31, "size": [1, 2]"#,
                ),
                "tall[0].size",
                ErrorKind::PastTileset,
            ),
            (
                level_with(r#""prefab": "tree""#, r#""prefab": "Tree""#),
                "entities[0].prefab",
                ErrorKind::BadPrefabName,
            ),
            (
                level_with(r#"{ "Health": { "max": 3 } }"#, r#"{ "Health": null }"#),
                "entities[1].overrides.Health",
                ErrorKind::NullOverride,
            ),
            (
                level_with(r#"{ "Health": { "max": 3 } }"#, r#"{ "health-bar": {} }"#),
                "entities[1].overrides.health-bar",
                ErrorKind::BadComponentName,
            ),
            (
                level_with(r#""overrides": {}"#, r#""overrides": []"#),
                "entities[0].overrides",
                ErrorKind::WrongType {
                    expected: "an object",
                },
            ),
            (
                level_with(r#""tile": 5, "frames": 2"#, r#""tile": 32, "frames": 2"#),
                "tilesets[0].animations[0].tile",
                ErrorKind::NoSuchTile { tile: 32 },
            ),
            (
                level_with(
                    r#""tile": 4, "size": [1, 2]"#,
                    r#""tile": 32, "size": [1, 2]"#,
                ),
                "tall[0].tile",
                ErrorKind::NoSuchTile { tile: 32 },
            ),
            (
                level_with(
                    r#""tileset": 0, "size": [3, 2]"#,
                    r#""tileset": 0, "size": [0, 2]"#,
                ),
                "layers[0].size[0]",
                ErrorKind::OutOfRange {
                    min: 1,
                    max: i64::from(u32::MAX),
                },
            ),
            (
                level_with(r#""layer": 0, "depth": 0"#, r#""layer": 256, "depth": 0"#),
                "layers[0].layer",
                ErrorKind::OutOfRange { min: 0, max: 255 },
            ),
            (
                level_with(
                    r#"["atlas/props.kbj", "atlas/sprites.kbj"]"#,
                    r#"["atlas/sprites.kbj", "atlas/sprites.kbj"]"#,
                ),
                "assets[1]",
                ErrorKind::NotSorted,
            ),
            (
                level_with(r#""sprite": "sprites/tiles""#, r#""sprite": """#),
                "tilesets[0].sprite",
                ErrorKind::EmptyString,
            ),
            (
                level_with(r#""version": 1,"#, r#""version": 1, "camera": [0, 0, 64],"#),
                "camera",
                ErrorKind::WrongLength { expected: 4 },
            ),
        ];
        for (text, at, kind) in cases {
            assert_eq!(error(&text), (at.to_string(), kind), "{text}");
        }
        // A tall block across the set's right edge, and a camera of no size.
        assert_eq!(
            error(&level_with(
                r#""tile": 4, "size": [1, 2]"#,
                r#""tile": 7, "size": [2, 1]"#
            )),
            ("tall[0].size".to_string(), ErrorKind::PastTileset)
        );
        assert_eq!(
            error(&level_with(
                r#""version": 1,"#,
                r#""version": 1, "camera": [0, 0, 0, 32],"#
            )),
            (
                "camera[2]".to_string(),
                ErrorKind::OutOfRange {
                    min: 1,
                    max: i64::from(i32::MAX)
                }
            )
        );
        assert_eq!(
            error(&level_with(
                r#""version": 1,"#,
                r#""version": 1, "music": "","#
            )),
            ("music".to_string(), ErrorKind::EmptyString)
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
        let error = CodecError::Level {
            at: "layers[0].cells[130]".to_string(),
            kind: ErrorKind::NoSuchTile { tile: 32 },
        };
        assert_eq!(
            error.to_string(),
            "level file error at `layers[0].cells[130]`: the tile set has no tile 32"
        );
        let error = CodecError::Level {
            at: String::new(),
            kind: ErrorKind::MissingMember { name: "size" },
        };
        assert_eq!(
            error.to_string(),
            "level file error in the file: `size` is missing"
        );
    }
}
