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

//! LDtk's project files, as LDtk 1.5.3 saves them: a project, `.ldtk`, and
//! the level files, `.ldtkl`, of a project that saves its levels apart
//! (`docs/subsystems/assets-and-codecs.md`, "LDtk subset"; ADR-0038). The
//! cook reads them with [`parse_project`] and [`parse_level`]; the game
//! never does.
//!
//! ```text
//! { "jsonVersion": "1.5.3", "externalLevels": false, "worlds": [], …,
//!   "defs": {
//!     "tilesets": [ { "uid": 1, "identifier": "Tiles", "relPath": "../sprites/tiles.png",
//!                     "pxWid": 128, "pxHei": 64, "tileGridSize": 16, "spacing": 0, "padding": 0,
//!                     "__cWid": 8, "__cHei": 4,
//!                     "customData": [ { "tileId": 8, "data": "{\"frames\": 4, \"ms\": 250}" } ], … } ],
//!     "layers": [ { "uid": 2, "identifier": "Collision", "type": "IntGrid", "gridSize": 16,
//!                   "tilesetDefUid": null, "intGridValues": [ { "value": 1, "identifier": "solid", … } ], … } ],
//!     "entities": [ { "uid": 3, "identifier": "Tree", "width": 16, "height": 32,
//!                     "pivotX": 0.5, "pivotY": 1, … } ], … },
//!   "levels": [ { "identifier": "Field", "iid": "…", "uid": 0, "worldX": 0, "worldY": 0,
//!                 "pxWid": 1024, "pxHei": 768, "__bgColor": "#40465B", "externalRelPath": null,
//!                 "fieldInstances": [],
//!                 "layerInstances": [ { "__identifier": "Collision", "__type": "IntGrid",
//!                                       "__cWid": 64, "__cHei": 48, "__gridSize": 16, "__opacity": 1,
//!                                       "__pxTotalOffsetX": 0, "__pxTotalOffsetY": 0,
//!                                       "__tilesetDefUid": null, "layerDefUid": 2, "visible": true,
//!                                       "intGridCsv": [1, 1, …], "gridTiles": [], "autoLayerTiles": [],
//!                                       "entityInstances": [], … } ], … } ] }
//! ```
//!
//! It is read with [`json::parse`], then checked:
//!
//! - **The version first:** `jsonVersion` is 1.5.x, as LDtk 1.5.3
//!   ([`VERSION`]) writes; any other is refused with a note to save the
//!   project with that version, whatever else the file holds.
//! - **Members:** those the types below hold must be there with the types
//!   shown, but one that may be `null` may be left out too, as LDtk's
//!   schema allows; the many others LDtk writes are ignored. A project with
//!   Multi-Worlds (a non-empty `worlds`) is refused.
//! - **No quadratic check:** definitions are found by their uids sorted,
//!   and IntGrid values among their layer's sorted, so that a crafted
//!   project's checks cost no more than a sort.
//! - **Definitions:** no two tile sets, layers or entities of one uid. A
//!   tile set's grid of `__cWid` × `__cHei` tiles is at most `max_entries`
//!   tiles, and its `customData` is of tiles it has. A layer's tile set,
//!   if it names one, is defined; its IntGrid values are 1 or more.
//! - **Levels:** a level's size is 1 pixel or more; its background a
//!   colour `#rrggbb`. A level is external (an `externalRelPath`) exactly
//!   when its `layerInstances` is `null`, and only in a project that saves
//!   its levels apart (`externalLevels`).
//! - **Layers:** each of a layer definition the project has, and of its
//!   type; of `__cWid` × `__cHei` cells, at most `max_entries`; an
//!   IntGrid's `intGridCsv` one value a cell, each 0 or one of its
//!   definition's, and other layers' empty. A tile is of the layer's tile
//!   set, which the layer must have; its flips `f` 0 to 3; its alpha `a` 0
//!   to 1. An entity is of an entity definition the project has.
//! - **Level files:** an `.ldtkl` is one level, with its layers, of a
//!   project's: [`parse_level`] checks it against that project's
//!   definitions, and that the project has it as an external level.
//!
//! Field values are kept as JSON values beside their type's name: what the
//! cook makes of them is its business (ADR-0038).
//!
//! A problem after the JSON is a [`CodecError::Ldtk`], which names where it
//! is as a member path, such as `levels[0].layerInstances[2].gridTiles[5].t`;
//! the JSON's own errors keep their line and column.

use std::fmt;

use crate::json::{self, Value};
use crate::{CodecError, Limits};

/// The LDtk version whose files [`parse_project`] reads, and the one a
/// refused project is to be saved with. Any 1.5.x `jsonVersion` is read.
pub const VERSION: &str = "1.5.3";

/// An LDtk project: its settings that matter to the cook, its
/// definitions and its levels.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Project {
    /// `jsonVersion`, as the file gives it: 1.5.x.
    pub json_version: String,
    /// Whether it saves each level in a file of its own (`externalLevels`).
    pub external_levels: bool,
    /// Whether it saves backups beside itself (`backupOnSave`).
    pub backup_on_save: bool,
    /// Whether it writes the simplified export (`simplifiedExport`).
    pub simplified_export: bool,
    /// Whether it writes images of its layers or levels
    /// (`imageExportMode`).
    pub image_export: ImageExport,
    /// Whether it writes a Tiled file beside itself (`exportTiled`).
    pub export_tiled: bool,
    /// Its tile sets (`defs.tilesets`).
    pub tilesets: Vec<TilesetDef>,
    /// Its layer definitions (`defs.layers`), in LDtk's order: the first is
    /// the top-most.
    pub layers: Vec<LayerDef>,
    /// Its entity definitions (`defs.entities`).
    pub entities: Vec<EntityDef>,
    /// Its levels, in the project's order.
    pub levels: Vec<Level>,
}

impl Project {
    /// The tile set of `uid`, if the project defines one.
    pub fn tileset(&self, uid: i64) -> Option<&TilesetDef> {
        self.tilesets.iter().find(|tileset| tileset.uid == uid)
    }

    /// The layer definition of `uid`, if the project has one.
    pub fn layer(&self, uid: i64) -> Option<&LayerDef> {
        self.layers.iter().find(|layer| layer.uid == uid)
    }

    /// The entity definition of `uid`, if the project has one.
    pub fn entity(&self, uid: i64) -> Option<&EntityDef> {
        self.entities.iter().find(|entity| entity.uid == uid)
    }
}

/// What images a project writes when it is saved (`imageExportMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageExport {
    /// None (`None`).
    None,
    /// One image a layer (`OneImagePerLayer`).
    OneImagePerLayer,
    /// One image a level (`OneImagePerLevel`).
    OneImagePerLevel,
    /// Both (`LayersAndLevels`).
    LayersAndLevels,
}

/// A tile set of a project (`defs.tilesets[]`): an image cut into a grid
/// of tiles, numbered row by row from the top left.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TilesetDef {
    /// Its uid, which layers refer to it by.
    pub uid: i64,
    /// Its name.
    pub identifier: String,
    /// Its image's path, relative to the project's file (`relPath`): none
    /// for a tile set with no image, or one of LDtk's own icons.
    pub rel_path: Option<String>,
    /// Whether it is LDtk's own atlas of icons (`embedAtlas`), with no
    /// image of the project's.
    pub embedded: bool,
    /// Its image's width in pixels (`pxWid`).
    pub width: u32,
    /// Its image's height in pixels (`pxHei`).
    pub height: u32,
    /// A tile's side in pixels (`tileGridSize`): 1 or more.
    pub tile_size: u32,
    /// The pixels between two tiles (`spacing`).
    pub spacing: u32,
    /// The pixels between the image's edges and its tiles (`padding`).
    pub padding: u32,
    /// Its tiles across (`__cWid`).
    pub columns: u32,
    /// Its tiles down (`__cHei`).
    pub rows: u32,
    /// The text given to some of its tiles (`customData`), in the file's
    /// order: each tile one of the set's.
    pub custom_data: Vec<CustomData>,
}

impl TilesetDef {
    /// How many tiles it has: `columns` × `rows`.
    pub fn tile_count(&self) -> u64 {
        u64::from(self.columns) * u64::from(self.rows)
    }
}

/// The text a tile set gives one of its tiles (`customData[]`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct CustomData {
    /// The tile (`tileId`).
    pub tile: u32,
    /// The text (`data`), as LDtk keeps it: the cook reads JSON in it
    /// (ADR-0038).
    pub data: String,
}

/// What a layer holds (`type`, `__type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LayerKind {
    /// A grid of whole numbers, which may make tiles by rules too
    /// (`IntGrid`).
    IntGrid,
    /// Entities (`Entities`).
    Entities,
    /// Tiles placed by hand (`Tiles`).
    Tiles,
    /// Tiles placed by rules from another layer's IntGrid (`AutoLayer`).
    AutoLayer,
}

/// A layer definition of a project (`defs.layers[]`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct LayerDef {
    /// Its uid, which layer instances refer to it by.
    pub uid: i64,
    /// Its name.
    pub identifier: String,
    /// What it holds.
    pub kind: LayerKind,
    /// A cell's side in pixels (`gridSize`): 1 or more.
    pub grid_size: u32,
    /// Its tile set, if it has one (`tilesetDefUid`): one the project
    /// defines.
    pub tileset: Option<i64>,
    /// An IntGrid's values (`intGridValues`), in the file's order.
    pub int_grid_values: Vec<IntGridValue>,
}

/// A value an IntGrid layer may hold (`intGridValues[]`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct IntGridValue {
    /// The value (`value`): 1 or more, as 0 is an empty cell.
    pub value: i64,
    /// Its name, if it has one (`identifier`).
    pub identifier: Option<String>,
}

/// An entity definition of a project (`defs.entities[]`).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct EntityDef {
    /// Its uid, which entity instances refer to it by.
    pub uid: i64,
    /// Its name.
    pub identifier: String,
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Its pivot (`pivotX`, `pivotY`), from 0 to 1 of its size.
    pub pivot: [f32; 2],
}

/// A level of a project (`levels[]`), or the level of an `.ldtkl` file.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Level {
    /// Its name.
    pub identifier: String,
    /// Its instance id (`iid`), which an `.ldtkl` file names it by.
    pub iid: String,
    /// Its uid.
    pub uid: i64,
    /// Where it is in the world, in pixels (`worldX`, `worldY`): -1 in a
    /// linear layout.
    pub world: [i64; 2],
    /// Its width in pixels (`pxWid`): 1 or more.
    pub width: u32,
    /// Its height in pixels (`pxHei`): 1 or more.
    pub height: u32,
    /// Its background colour, red, green and blue (`__bgColor`).
    pub background: [u8; 3],
    /// Its `.ldtkl` file, relative to the project's, for a level saved
    /// apart (`externalRelPath`).
    pub external_path: Option<String>,
    /// Its layers, the top-most first (`layerInstances`): none for a level
    /// saved apart, whose `.ldtkl` file has them.
    pub layers: Option<Vec<LayerInstance>>,
    /// Its own fields (`fieldInstances`).
    pub fields: Vec<FieldInstance>,
}

/// A layer of a level (`layerInstances[]`).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct LayerInstance {
    /// Its definition's name (`__identifier`).
    pub identifier: String,
    /// What it holds (`__type`), as its definition says.
    pub kind: LayerKind,
    /// Its definition (`layerDefUid`).
    pub layer_def: i64,
    /// Its cells across (`__cWid`).
    pub columns: u32,
    /// Its cells down (`__cHei`).
    pub rows: u32,
    /// A cell's side in pixels (`__gridSize`): 1 or more.
    pub grid_size: u32,
    /// Its opacity, 0 to 1 (`__opacity`).
    pub opacity: f32,
    /// Its offset in pixels, its definition's and its own together
    /// (`__pxTotalOffsetX`, `__pxTotalOffsetY`).
    pub offset: [i32; 2],
    /// Its tile set, if it has one (`__tilesetDefUid`): one the project
    /// defines.
    pub tileset: Option<i64>,
    /// Whether it is shown (`visible`).
    pub visible: bool,
    /// An IntGrid's values, one a cell, row by row from the top left
    /// (`intGridCsv`): 0 for an empty cell. Empty for other layers.
    pub int_grid: Vec<i64>,
    /// The tiles placed by hand (`gridTiles`), in the order they are drawn.
    pub grid_tiles: Vec<TileInstance>,
    /// The tiles placed by rules (`autoLayerTiles`), in the order they are
    /// drawn.
    pub auto_layer_tiles: Vec<TileInstance>,
    /// The entities (`entityInstances`).
    pub entities: Vec<EntityInstance>,
}

/// A tile placed in a layer (`gridTiles[]`, `autoLayerTiles[]`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct TileInstance {
    /// The tile, of the layer's tile set (`t`).
    pub tile: u32,
    /// Its top-left corner in the layer, in pixels (`px`), before the
    /// layer's offset.
    pub px: [i32; 2],
    /// Its top-left corner in the tile set's image, in pixels (`src`).
    pub src: [i32; 2],
    /// Its flips (`f`): bit 0 across, bit 1 down.
    pub flip: u8,
    /// Its opacity, 0 to 1 (`a`).
    pub alpha: f32,
}

/// An entity placed in a layer (`entityInstances[]`).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct EntityInstance {
    /// Its definition's name (`__identifier`).
    pub identifier: String,
    /// Its instance id (`iid`).
    pub iid: String,
    /// Its definition (`defUid`): one the project has.
    pub entity_def: i64,
    /// Its pivot's place in the level, in pixels (`px`), before the
    /// layer's offset.
    pub px: [i32; 2],
    /// The cell it is in (`__grid`).
    pub cell: [i32; 2],
    /// Its pivot (`__pivot`), from 0 to 1 of its size.
    pub pivot: [f32; 2],
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Its fields (`fieldInstances`).
    pub fields: Vec<FieldInstance>,
}

/// A field's value, of an entity or a level (`fieldInstances[]`).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct FieldInstance {
    /// Its name (`__identifier`).
    pub identifier: String,
    /// Its type's name (`__type`), such as `Int`, `String`,
    /// `LocalEnum.Kind` or `Array<Int>`.
    pub kind: String,
    /// Its value (`__value`), as LDtk writes it for its type: `null` when
    /// it has none.
    pub value: Value,
}

/// What is wrong with an LDtk file. See [`CodecError::Ldtk`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The project's `jsonVersion` is not 1.5.x.
    Version {
        /// The version the file gives.
        found: String,
    },
    /// The project has Multi-Worlds: its `worlds` is not empty.
    MultiWorlds,
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
    /// A whole number is outside its member's range.
    OutOfRange {
        /// The smallest it may be.
        min: i64,
        /// The largest it may be.
        max: i64,
    },
    /// A number is not finite, or not from 0 to 1.
    NotAFraction,
    /// A list does not have the number of items it needs.
    WrongLength {
        /// How many it needs.
        expected: u64,
    },
    /// A string is not one of the values its member may take.
    UnknownValue {
        /// The string.
        found: String,
    },
    /// A colour is not `#rrggbb`.
    BadColor,
    /// Two definitions of one kind share a uid.
    DuplicateUid {
        /// The uid.
        uid: i64,
    },
    /// A uid names no tile set of the project.
    NoSuchTileset {
        /// The uid.
        uid: i64,
    },
    /// A uid names no layer definition of the project.
    NoSuchLayer {
        /// The uid.
        uid: i64,
    },
    /// A uid names no entity definition of the project.
    NoSuchEntity {
        /// The uid.
        uid: i64,
    },
    /// A layer's `__type` is not its definition's.
    KindMismatch,
    /// A grid has more cells, or a tile set more tiles, than `max_entries`.
    TooManyCells,
    /// An IntGrid cell holds a value its layer does not define.
    NoSuchIntGridValue {
        /// The value.
        value: i64,
    },
    /// A tile is not one of its tile set's.
    NoSuchTile {
        /// The tile.
        tile: u32,
    },
    /// A layer has tiles but no tile set.
    TilesWithoutTileset,
    /// A project's level has both its layers and an `externalRelPath`, or
    /// neither, or is saved apart in a project that does not save its
    /// levels apart, or the other way round.
    ExternalMismatch,
    /// An `.ldtkl` file has no layers.
    NoLayers,
    /// An `.ldtkl` file's level is not one the project saves apart.
    NoSuchLevel {
        /// The level's `iid`.
        iid: String,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Version { found } => write!(
                f,
                "LDtk {found} is not supported: save the project with LDtk {VERSION}"
            ),
            Self::MultiWorlds => write!(f, "a project with Multi-Worlds is not supported"),
            Self::WrongType { expected } => write!(f, "expected {expected}"),
            Self::MissingMember { name } => write!(f, "`{name}` is missing"),
            Self::OutOfRange { min, max } => write!(f, "must be from {min} to {max}"),
            Self::NotAFraction => write!(f, "must be a number from 0 to 1"),
            Self::WrongLength { expected } => write!(f, "must have {expected} items"),
            Self::UnknownValue { found } => write!(f, "{found:?} is not a value it may take"),
            Self::BadColor => write!(f, "a colour is `#rrggbb`"),
            Self::DuplicateUid { uid } => write!(f, "another definition has the uid {uid}"),
            Self::NoSuchTileset { uid } => write!(f, "the project has no tile set of uid {uid}"),
            Self::NoSuchLayer { uid } => write!(f, "the project has no layer of uid {uid}"),
            Self::NoSuchEntity { uid } => write!(f, "the project has no entity of uid {uid}"),
            Self::KindMismatch => write!(f, "the layer's type is not its definition's"),
            Self::TooManyCells => write!(f, "too many cells or tiles"),
            Self::NoSuchIntGridValue { value } => {
                write!(f, "the layer defines no IntGrid value {value}")
            }
            Self::NoSuchTile { tile } => write!(f, "the tile set has no tile {tile}"),
            Self::TilesWithoutTileset => write!(f, "the layer has tiles but no tile set"),
            Self::ExternalMismatch => write!(
                f,
                "a level has its layers or an `externalRelPath`, one of them, as its project's `externalLevels` says"
            ),
            Self::NoLayers => write!(f, "a level file has its layers"),
            Self::NoSuchLevel { iid } => {
                write!(f, "the project saves no level of iid {iid:?} apart")
            }
        }
    }
}

/// Reads an LDtk project: JSON, parsed within `limits`, then checked as the
/// module says.
///
/// # Errors
///
/// [`CodecError::Json`] if the text is not JSON, and [`CodecError::Ldtk`],
/// naming where, if it is not a project of LDtk 1.5.x that this reader
/// takes.
pub fn parse_project(text: &str, limits: Limits) -> Result<Project, CodecError> {
    read_project(&json::parse(text, limits)?, limits)
}

/// Reads an external level file, `.ldtkl`, of `project`: JSON, parsed
/// within `limits`, then checked as the module says, against the
/// project's definitions.
///
/// # Errors
///
/// [`CodecError::Json`] if the text is not JSON, and [`CodecError::Ldtk`],
/// naming where, if it is not one of the project's levels saved apart,
/// with its layers.
pub fn parse_level(text: &str, project: &Project, limits: Limits) -> Result<Level, CodecError> {
    let value = json::parse(text, limits)?;
    let root = At::Root;
    let level = level(&value, &root, &Defs::of(project), limits, false)?;
    if level.layers.is_none() {
        return Err(At::Member(&root, "layerInstances").error(ErrorKind::NoLayers));
    }
    if !project
        .levels
        .iter()
        .any(|known| known.iid == level.iid && known.external_path.is_some())
    {
        return Err(At::Member(&root, "iid").error(ErrorKind::NoSuchLevel {
            iid: level.iid.clone(),
        }));
    }
    Ok(level)
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
        CodecError::Ldtk {
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

/// An object's members, as LDtk writes them: those not asked for are
/// ignored.
#[derive(Clone, Copy)]
struct Object<'v, 'a> {
    members: &'v [(String, Value)],
    at: &'a At<'a>,
}

impl<'v, 'a> Object<'v, 'a> {
    fn of(value: &'v Value, at: &'a At<'a>) -> Result<Self, CodecError> {
        let members = value.as_object().ok_or_else(|| {
            at.error(ErrorKind::WrongType {
                expected: "an object",
            })
        })?;
        Ok(Self { members, at })
    }

    /// The member `name`, if there is one.
    fn get(&self, name: &str) -> Option<&'v Value> {
        self.members
            .iter()
            .find(|(member, _)| member == name)
            .map(|(_, value)| value)
    }

    /// The member `name`, which must be there.
    fn require(&self, name: &'static str) -> Result<&'v Value, CodecError> {
        self.get(name)
            .ok_or_else(|| self.at.error(ErrorKind::MissingMember { name }))
    }

    fn string(&self, name: &'static str) -> Result<String, CodecError> {
        string(self.require(name)?, &At::Member(self.at, name))
    }

    /// A string, `null`, or nothing at all.
    fn optional_string(&self, name: &'static str) -> Result<Option<String>, CodecError> {
        match self.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => string(value, &At::Member(self.at, name)).map(Some),
        }
    }

    fn boolean(&self, name: &'static str) -> Result<bool, CodecError> {
        self.require(name)?.as_bool().ok_or_else(|| {
            At::Member(self.at, name).error(ErrorKind::WrongType {
                expected: "true or false",
            })
        })
    }

    fn whole(&self, name: &'static str, min: i64, max: i64) -> Result<i64, CodecError> {
        whole(self.require(name)?, &At::Member(self.at, name), min, max)
    }

    fn unsigned(&self, name: &'static str, min: u32) -> Result<u32, CodecError> {
        let number = self.whole(name, i64::from(min), i64::from(u32::MAX))?;
        Ok(u32::try_from(number).expect("whole checked the range"))
    }

    fn signed(&self, name: &'static str) -> Result<i32, CodecError> {
        let number = self.whole(name, i64::from(i32::MIN), i64::from(i32::MAX))?;
        Ok(i32::try_from(number).expect("whole checked the range"))
    }

    /// A uid: a whole number, or `null`, or nothing at all.
    fn optional_uid(&self, name: &'static str) -> Result<Option<i64>, CodecError> {
        match self.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => whole(value, &At::Member(self.at, name), i64::MIN, i64::MAX).map(Some),
        }
    }

    fn fraction(&self, name: &'static str) -> Result<f32, CodecError> {
        fraction(self.require(name)?, &At::Member(self.at, name))
    }

    fn array(&self, name: &'static str) -> Result<&'v [Value], CodecError> {
        array(self.require(name)?, &At::Member(self.at, name))
    }

    /// Two whole numbers that fit an `i32`.
    fn point(&self, name: &'static str) -> Result<[i32; 2], CodecError> {
        let at = At::Member(self.at, name);
        let [x, y] = pair(self.require(name)?, &at)?;
        let coordinate = |value: &Value, index: usize| {
            let number = whole(
                value,
                &At::Item(&at, index),
                i64::from(i32::MIN),
                i64::from(i32::MAX),
            )?;
            Ok::<i32, CodecError>(i32::try_from(number).expect("whole checked the range"))
        };
        Ok([coordinate(x, 0)?, coordinate(y, 1)?])
    }
}

fn array<'v>(value: &'v Value, at: &At<'_>) -> Result<&'v [Value], CodecError> {
    value
        .as_array()
        .ok_or_else(|| at.error(ErrorKind::WrongType { expected: "a list" }))
}

fn string(value: &Value, at: &At<'_>) -> Result<String, CodecError> {
    value.as_str().map(str::to_string).ok_or_else(|| {
        at.error(ErrorKind::WrongType {
            expected: "a string",
        })
    })
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

/// A number from 0 to 1.
fn fraction(value: &Value, at: &At<'_>) -> Result<f32, CodecError> {
    let number = value.as_f64().ok_or_else(|| {
        at.error(ErrorKind::WrongType {
            expected: "a number",
        })
    })?;
    if (0.0..=1.0).contains(&number) {
        Ok(number as f32)
    } else {
        Err(at.error(ErrorKind::NotAFraction))
    }
}

/// A list of exactly two items.
fn pair<'v>(value: &'v Value, at: &At<'_>) -> Result<[&'v Value; 2], CodecError> {
    match array(value, at)? {
        [first, second] => Ok([first, second]),
        _ => Err(at.error(ErrorKind::WrongLength { expected: 2 })),
    }
}

/// A colour `#rrggbb`, as red, green and blue.
fn color(value: &Value, at: &At<'_>) -> Result<[u8; 3], CodecError> {
    let text = value.as_str().ok_or_else(|| {
        at.error(ErrorKind::WrongType {
            expected: "a string",
        })
    })?;
    let digits = text
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| at.error(ErrorKind::BadColor))?;
    let channel = |index: usize| {
        u8::from_str_radix(&digits[index..index + 2], 16).expect("two hexadecimal digits")
    };
    Ok([channel(0), channel(2), channel(4)])
}

/// Whether `version` is 1.5.x: three dot-separated numbers, the first 1
/// and the second 5.
fn readable(version: &str) -> bool {
    let mut parts = version.split('.');
    let numbers = (parts.next(), parts.next(), parts.next(), parts.next());
    matches!(numbers, (Some("1"), Some("5"), Some(patch), None)
        if !patch.is_empty() && patch.bytes().all(|b| b.is_ascii_digit()))
}

/// The definitions a level is checked against.
/// The uids of a list of definitions, sorted, with each one's place: so that
/// finding one, and finding two of one uid, takes no more than a sort, and
/// no check is quadratic in a crafted project's size.
struct Uids(Vec<(i64, usize)>);

impl Uids {
    fn of<T>(items: &[T], uid: impl Fn(&T) -> i64) -> Self {
        let mut uids: Vec<(i64, usize)> = items
            .iter()
            .enumerate()
            .map(|(index, item)| (uid(item), index))
            .collect();
        uids.sort_unstable();
        Self(uids)
    }

    /// The place of the later of two items of one uid, and the uid, if two
    /// share one.
    fn duplicate(&self) -> Option<(usize, i64)> {
        self.0
            .windows(2)
            .find(|pair| pair[0].0 == pair[1].0)
            .map(|pair| (pair[1].1, pair[1].0))
    }

    /// The place of the item of `uid`, if there is one.
    fn find(&self, uid: i64) -> Option<usize> {
        self.0
            .binary_search_by_key(&uid, |&(uid, _)| uid)
            .ok()
            .map(|found| self.0[found].1)
    }
}

/// The definitions a level is checked against, indexed.
struct Defs<'p> {
    tilesets: &'p [TilesetDef],
    tileset_uids: Uids,
    layers: &'p [LayerDef],
    layer_uids: Uids,
    /// Each layer definition's IntGrid values, sorted.
    int_grid_values: Vec<Vec<i64>>,
    entity_uids: Uids,
    /// Whether the project saves its levels apart.
    external_levels: bool,
}

impl<'p> Defs<'p> {
    fn new(
        tilesets: &'p [TilesetDef],
        layers: &'p [LayerDef],
        entities: &[EntityDef],
        external_levels: bool,
    ) -> Self {
        let int_grid_values = layers
            .iter()
            .map(|layer| {
                let mut values: Vec<i64> = layer
                    .int_grid_values
                    .iter()
                    .map(|value| value.value)
                    .collect();
                values.sort_unstable();
                values
            })
            .collect();
        Self {
            tilesets,
            tileset_uids: Uids::of(tilesets, |tileset| tileset.uid),
            layers,
            layer_uids: Uids::of(layers, |layer| layer.uid),
            int_grid_values,
            entity_uids: Uids::of(entities, |entity| entity.uid),
            external_levels,
        }
    }

    fn of(project: &'p Project) -> Self {
        Self::new(
            &project.tilesets,
            &project.layers,
            &project.entities,
            project.external_levels,
        )
    }

    fn tileset(&self, uid: i64) -> Option<&'p TilesetDef> {
        self.tileset_uids
            .find(uid)
            .map(|index| &self.tilesets[index])
    }
}

/// Fails if two of `uids` are one, naming the later's `uid` in `list`.
fn unique(uids: &Uids, list: &At<'_>) -> Result<(), CodecError> {
    match uids.duplicate() {
        Some((index, uid)) => {
            Err(At::Member(&At::Item(list, index), "uid").error(ErrorKind::DuplicateUid { uid }))
        }
        None => Ok(()),
    }
}

fn read_project(value: &Value, limits: Limits) -> Result<Project, CodecError> {
    let root = At::Root;
    let top = Object::of(value, &root)?;
    // The version first: another version's members may differ, and are not
    // supported rather than wrong.
    let json_version = top.string("jsonVersion")?;
    if !readable(&json_version) {
        return Err(At::Member(&root, "jsonVersion").error(ErrorKind::Version {
            found: json_version,
        }));
    }
    if !top.array("worlds")?.is_empty() {
        return Err(At::Member(&root, "worlds").error(ErrorKind::MultiWorlds));
    }
    let image_at = At::Member(&root, "imageExportMode");
    let image_export = match top.string("imageExportMode")?.as_str() {
        "None" => ImageExport::None,
        "OneImagePerLayer" => ImageExport::OneImagePerLayer,
        "OneImagePerLevel" => ImageExport::OneImagePerLevel,
        "LayersAndLevels" => ImageExport::LayersAndLevels,
        other => {
            return Err(image_at.error(ErrorKind::UnknownValue {
                found: other.to_string(),
            }));
        }
    };

    let defs_at = At::Member(&root, "defs");
    let defs = Object::of(top.require("defs")?, &defs_at)?;
    let tilesets_at = At::Member(&defs_at, "tilesets");
    let items = defs.array("tilesets")?;
    let mut tilesets = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        tilesets.push(tileset(item, &At::Item(&tilesets_at, index), limits)?);
    }
    let tileset_uids = Uids::of(&tilesets, |tileset| tileset.uid);
    unique(&tileset_uids, &tilesets_at)?;
    let layers_at = At::Member(&defs_at, "layers");
    let items = defs.array("layers")?;
    let mut layers = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        layers.push(layer_def(
            item,
            &At::Item(&layers_at, index),
            &tileset_uids,
        )?);
    }
    unique(&Uids::of(&layers, |layer| layer.uid), &layers_at)?;
    let entities_at = At::Member(&defs_at, "entities");
    let items = defs.array("entities")?;
    let mut entities = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        entities.push(entity_def(item, &At::Item(&entities_at, index))?);
    }
    unique(&Uids::of(&entities, |entity| entity.uid), &entities_at)?;

    let external_levels = top.boolean("externalLevels")?;
    let checks = Defs::new(&tilesets, &layers, &entities, external_levels);
    let levels_at = At::Member(&root, "levels");
    let items = top.array("levels")?;
    let mut levels = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        levels.push(level(
            item,
            &At::Item(&levels_at, index),
            &checks,
            limits,
            true,
        )?);
    }

    Ok(Project {
        json_version,
        external_levels,
        backup_on_save: top.boolean("backupOnSave")?,
        simplified_export: top.boolean("simplifiedExport")?,
        image_export,
        export_tiled: top.boolean("exportTiled")?,
        tilesets,
        layers,
        entities,
        levels,
    })
}

/// Checks that a grid of `columns` × `rows` is at most `max_entries`.
fn grid_fits(columns: u32, rows: u32, limits: Limits, at: &At<'_>) -> Result<u64, CodecError> {
    let cells = u64::from(columns) * u64::from(rows);
    if usize::try_from(cells).is_ok_and(|cells| cells <= limits.max_entries) {
        Ok(cells)
    } else {
        Err(at.error(ErrorKind::TooManyCells))
    }
}

fn tileset(value: &Value, at: &At<'_>, limits: Limits) -> Result<TilesetDef, CodecError> {
    let members = Object::of(value, at)?;
    let columns = members.unsigned("__cWid", 0)?;
    let rows = members.unsigned("__cHei", 0)?;
    let count = grid_fits(columns, rows, limits, at)?;
    let data_at = At::Member(at, "customData");
    let items = members.array("customData")?;
    let mut custom_data = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = At::Item(&data_at, index);
        let entry = Object::of(item, &at)?;
        let tile = entry.unsigned("tileId", 0)?;
        if u64::from(tile) >= count {
            return Err(At::Member(&at, "tileId").error(ErrorKind::NoSuchTile { tile }));
        }
        custom_data.push(CustomData {
            tile,
            data: entry.string("data")?,
        });
    }
    Ok(TilesetDef {
        uid: members.whole("uid", i64::MIN, i64::MAX)?,
        identifier: members.string("identifier")?,
        rel_path: members.optional_string("relPath")?,
        embedded: members.optional_string("embedAtlas")?.is_some(),
        width: members.unsigned("pxWid", 0)?,
        height: members.unsigned("pxHei", 0)?,
        tile_size: members.unsigned("tileGridSize", 1)?,
        spacing: members.unsigned("spacing", 0)?,
        padding: members.unsigned("padding", 0)?,
        columns,
        rows,
        custom_data,
    })
}

fn layer_kind(value: &Value, at: &At<'_>) -> Result<LayerKind, CodecError> {
    match string(value, at)?.as_str() {
        "IntGrid" => Ok(LayerKind::IntGrid),
        "Entities" => Ok(LayerKind::Entities),
        "Tiles" => Ok(LayerKind::Tiles),
        "AutoLayer" => Ok(LayerKind::AutoLayer),
        other => Err(at.error(ErrorKind::UnknownValue {
            found: other.to_string(),
        })),
    }
}

fn layer_def(value: &Value, at: &At<'_>, tilesets: &Uids) -> Result<LayerDef, CodecError> {
    let members = Object::of(value, at)?;
    let tileset = members.optional_uid("tilesetDefUid")?;
    if let Some(uid) = tileset
        && tilesets.find(uid).is_none()
    {
        return Err(At::Member(at, "tilesetDefUid").error(ErrorKind::NoSuchTileset { uid }));
    }
    let values_at = At::Member(at, "intGridValues");
    let items = members.array("intGridValues")?;
    let mut int_grid_values = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = At::Item(&values_at, index);
        let entry = Object::of(item, &at)?;
        int_grid_values.push(IntGridValue {
            value: entry.whole("value", 1, i64::MAX)?,
            identifier: entry.optional_string("identifier")?,
        });
    }
    Ok(LayerDef {
        uid: members.whole("uid", i64::MIN, i64::MAX)?,
        identifier: members.string("identifier")?,
        kind: layer_kind(members.require("type")?, &At::Member(at, "type"))?,
        grid_size: members.unsigned("gridSize", 1)?,
        tileset,
        int_grid_values,
    })
}

fn entity_def(value: &Value, at: &At<'_>) -> Result<EntityDef, CodecError> {
    let members = Object::of(value, at)?;
    Ok(EntityDef {
        uid: members.whole("uid", i64::MIN, i64::MAX)?,
        identifier: members.string("identifier")?,
        width: members.unsigned("width", 0)?,
        height: members.unsigned("height", 0)?,
        pivot: [members.fraction("pivotX")?, members.fraction("pivotY")?],
    })
}

/// A level of a project's `levels` (`in_project`), or of an `.ldtkl` file.
fn level(
    value: &Value,
    at: &At<'_>,
    defs: &Defs<'_>,
    limits: Limits,
    in_project: bool,
) -> Result<Level, CodecError> {
    let members = Object::of(value, at)?;
    let external_path = members.optional_string("externalRelPath")?;
    let layers_at = At::Member(at, "layerInstances");
    let layers = match members.get("layerInstances") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let items = array(value, &layers_at)?;
            let mut layers = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                layers.push(layer(item, &At::Item(&layers_at, index), defs, limits)?);
            }
            Some(layers)
        }
    };
    // In a project, a level saved apart has a path and no layers, and every
    // level is saved apart or none is; its `.ldtkl` has the layers, and may
    // keep its path or not, which `parse_level` leaves alone.
    let apart = layers.is_none();
    if in_project && (apart != external_path.is_some() || apart != defs.external_levels) {
        return Err(at.error(ErrorKind::ExternalMismatch));
    }
    Ok(Level {
        identifier: members.string("identifier")?,
        iid: members.string("iid")?,
        uid: members.whole("uid", i64::MIN, i64::MAX)?,
        world: [
            members.whole("worldX", i64::MIN, i64::MAX)?,
            members.whole("worldY", i64::MIN, i64::MAX)?,
        ],
        width: members.unsigned("pxWid", 1)?,
        height: members.unsigned("pxHei", 1)?,
        background: color(members.require("__bgColor")?, &At::Member(at, "__bgColor"))?,
        external_path,
        layers,
        fields: fields(&members, at)?,
    })
}

fn fields(members: &Object<'_, '_>, at: &At<'_>) -> Result<Vec<FieldInstance>, CodecError> {
    let fields_at = At::Member(at, "fieldInstances");
    let items = members.array("fieldInstances")?;
    let mut fields = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = At::Item(&fields_at, index);
        let field = Object::of(item, &at)?;
        fields.push(FieldInstance {
            identifier: field.string("__identifier")?,
            kind: field.string("__type")?,
            value: field.require("__value")?.clone(),
        });
    }
    Ok(fields)
}

fn layer(
    value: &Value,
    at: &At<'_>,
    defs: &Defs<'_>,
    limits: Limits,
) -> Result<LayerInstance, CodecError> {
    let members = Object::of(value, at)?;
    let layer_def = members.whole("layerDefUid", i64::MIN, i64::MAX)?;
    let Some(def_index) = defs.layer_uids.find(layer_def) else {
        return Err(At::Member(at, "layerDefUid").error(ErrorKind::NoSuchLayer { uid: layer_def }));
    };
    let definition = &defs.layers[def_index];
    let values = &defs.int_grid_values[def_index];
    let kind = layer_kind(members.require("__type")?, &At::Member(at, "__type"))?;
    if kind != definition.kind {
        return Err(At::Member(at, "__type").error(ErrorKind::KindMismatch));
    }
    let columns = members.unsigned("__cWid", 0)?;
    let rows = members.unsigned("__cHei", 0)?;
    let cells = grid_fits(columns, rows, limits, at)?;
    let tileset_uid = members.optional_uid("__tilesetDefUid")?;
    let tileset = match tileset_uid {
        Some(uid) => Some(defs.tileset(uid).ok_or_else(|| {
            At::Member(at, "__tilesetDefUid").error(ErrorKind::NoSuchTileset { uid })
        })?),
        None => None,
    };

    let csv_at = At::Member(at, "intGridCsv");
    let items = members.array("intGridCsv")?;
    let expected = if kind == LayerKind::IntGrid { cells } else { 0 };
    if items.len() as u64 != expected {
        return Err(csv_at.error(ErrorKind::WrongLength { expected }));
    }
    let mut int_grid = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = At::Item(&csv_at, index);
        let value = whole(item, &at, 0, i64::MAX)?;
        if value != 0 && values.binary_search(&value).is_err() {
            return Err(at.error(ErrorKind::NoSuchIntGridValue { value }));
        }
        int_grid.push(value);
    }

    let grid_tiles = tiles(&members, at, "gridTiles", tileset)?;
    let auto_layer_tiles = tiles(&members, at, "autoLayerTiles", tileset)?;

    let entities_at = At::Member(at, "entityInstances");
    let items = members.array("entityInstances")?;
    let mut entities = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        entities.push(entity(item, &At::Item(&entities_at, index), defs)?);
    }

    Ok(LayerInstance {
        identifier: members.string("__identifier")?,
        kind,
        layer_def,
        columns,
        rows,
        grid_size: members.unsigned("__gridSize", 1)?,
        opacity: members.fraction("__opacity")?,
        offset: [
            members.signed("__pxTotalOffsetX")?,
            members.signed("__pxTotalOffsetY")?,
        ],
        tileset: tileset_uid,
        visible: members.boolean("visible")?,
        int_grid,
        grid_tiles,
        auto_layer_tiles,
        entities,
    })
}

/// The tiles of the member `name` of a layer, of `tileset`.
fn tiles(
    members: &Object<'_, '_>,
    at: &At<'_>,
    name: &'static str,
    tileset: Option<&TilesetDef>,
) -> Result<Vec<TileInstance>, CodecError> {
    let list_at = At::Member(at, name);
    let items = members.array(name)?;
    let Some(tileset) = tileset else {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        return Err(list_at.error(ErrorKind::TilesWithoutTileset));
    };
    let mut tiles = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = At::Item(&list_at, index);
        let members = Object::of(item, &at)?;
        let tile = members.unsigned("t", 0)?;
        if u64::from(tile) >= tileset.tile_count() {
            return Err(At::Member(&at, "t").error(ErrorKind::NoSuchTile { tile }));
        }
        let flip = members.whole("f", 0, 3)?;
        tiles.push(TileInstance {
            tile,
            px: members.point("px")?,
            src: members.point("src")?,
            flip: u8::try_from(flip).expect("whole checked the range"),
            alpha: members.fraction("a")?,
        });
    }
    Ok(tiles)
}

fn entity(value: &Value, at: &At<'_>, defs: &Defs<'_>) -> Result<EntityInstance, CodecError> {
    let members = Object::of(value, at)?;
    let entity_def = members.whole("defUid", i64::MIN, i64::MAX)?;
    if defs.entity_uids.find(entity_def).is_none() {
        return Err(At::Member(at, "defUid").error(ErrorKind::NoSuchEntity { uid: entity_def }));
    }
    let pivot_at = At::Member(at, "__pivot");
    let [x, y] = pair(members.require("__pivot")?, &pivot_at)?;
    Ok(EntityInstance {
        identifier: members.string("__identifier")?,
        iid: members.string("iid")?,
        entity_def,
        px: members.point("px")?,
        cell: members.point("__grid")?,
        pivot: [
            fraction(x, &At::Item(&pivot_at, 0))?,
            fraction(y, &At::Item(&pivot_at, 1))?,
        ],
        width: members.unsigned("width", 0)?,
        height: members.unsigned("height", 0)?,
        fields: fields(&members, at)?,
    })
}
