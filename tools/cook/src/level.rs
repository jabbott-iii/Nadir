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

//! An LDtk project's levels as level files, `levels/<name>.klvl`, by
//! ADR-0038's conventions (`docs/subsystems/assets-and-codecs.md`, "LDtk
//! levels"): the tile sets its levels draw, checked against their images
//! and read for their animated and tall tiles; then each level's tile
//! layers, its `Collision` grid, its tall tiles and its entities, written
//! as KBJ and read back as the game reads it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use nadir_assets::AssetPath;
use nadir_codec::json::{self, Value};
use nadir_codec::ldtk::{FieldInstance, LayerInstance, LayerKind, Level, TilesetDef};
use nadir_codec::{Limits, kbj, level, prefab};

use crate::CookError;

/// The IntGrid layer whose cells that are not 0 are the grid's solid ones.
pub(crate) const COLLISION: &str = "Collision";
/// The collision layer of the grid's solid cells.
const WORLD: &str = "World";
/// The sort layer of the tile layers, beneath every y-sorted sprite.
const TILE_LAYER: i64 = 0;
/// The sort layer of the tall tiles, the y-sorted sprites'.
const TALL_LAYER: i64 = 1;
/// The flip that multiplies a cell's flips in a level file.
const FLIP: i64 = 65536;
/// The most cells a level's tile layers and grid hold together: 2^23, so
/// that a small crafted project, of many large layers each with a tile,
/// cannot make the cook hold more than some 300 MiB of them.
pub(crate) const MAX_CELLS: u64 = 1 << 23;

/// An error at `at` in the file `path` (as found): `at` a member path, or
/// empty for the file as a whole.
pub(crate) fn fault(path: &str, at: &str, reason: impl fmt::Display) -> CookError {
    CookError::Config {
        path: path.to_string(),
        reason: if at.is_empty() {
            reason.to_string()
        } else {
            format!("at `{at}`: {reason}")
        },
    }
}

/// An LDtk identifier in snake_case, as prefabs and level files are named:
/// `EnemyBase` → `enemy_base`, `Level_0` → `level_0`, `HTTPGate` →
/// `http_gate`. An uppercase letter starts a word after a lowercase letter
/// or a digit, or before a lowercase letter after an uppercase one.
pub(crate) fn snake_case(identifier: &str) -> String {
    let chars: Vec<char> = identifier.chars().collect();
    let mut name = String::with_capacity(identifier.len() + 4);
    for (index, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let before = index.checked_sub(1).map(|before| chars[before]);
            let after = chars.get(index + 1);
            let starts = match before {
                Some(b) if b.is_ascii_lowercase() || b.is_ascii_digit() => true,
                Some(b) if b.is_ascii_uppercase() => after.is_some_and(char::is_ascii_lowercase),
                _ => false,
            };
            if starts {
                name.push('_');
            }
            name.push(c.to_ascii_lowercase());
        } else {
            name.push(c);
        }
    }
    name
}

/// The path of the level file of the level `identifier`, if its name in
/// snake_case is a prefab's sort of name and makes an asset path.
pub(crate) fn file_name(identifier: &str) -> Option<String> {
    let name = snake_case(identifier);
    let path = format!("levels/{name}.klvl");
    (prefab::is_name(&name) && AssetPath::new(&path).is_ok_and(|checked| checked.as_str() == path))
        .then_some(path)
}

/// `relative`, a path LDtk wrote relative to the folder `folder` of the
/// source folder, as an asset path: `..` may only climb to the source
/// folder itself.
///
/// # Errors
///
/// Why it is not one.
pub(crate) fn resolve(folder: &str, relative: &str) -> Result<String, String> {
    let mut parts: Vec<&str> = folder.split('/').filter(|part| !part.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "" => {
                return Err(format!(
                    "`{relative}` is not a path relative to the project"
                ));
            }
            "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!(
                        "`{relative}` leaves the source folder, where the cook finds what it cooks"
                    ));
                }
            }
            part => parts.push(part),
        }
    }
    let joined = parts.join("/");
    AssetPath::new(&joined)
        .map(|path| path.as_str().to_string())
        .map_err(|error| format!("`{relative}` cannot be an asset: {error}"))
}

/// Whether `layer` is drawn: it is shown, and has tiles.
pub(crate) fn drawn(layer: &LayerInstance) -> bool {
    layer.visible && !(layer.grid_tiles.is_empty() && layer.auto_layer_tiles.is_empty())
}

/// A tile set that a level draws: the atlas sprite its image cooks to, and
/// what its tiles' custom data makes of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tileset {
    /// Its atlas file, `atlas/<group>.kbj`.
    pub(crate) atlas: String,
    /// Its sprite in the atlas: its image's path without `.png`.
    pub(crate) sprite: String,
    pub(crate) columns: u16,
    pub(crate) rows: u16,
    /// A tile's side, in pixels.
    pub(crate) tile: u32,
    /// Its animated tiles, by first tile: the tile, its frames and their
    /// milliseconds.
    pub(crate) animations: Vec<(u16, u16, u16)>,
    /// Its tall tiles: each block's columns and rows, by its top-left tile.
    pub(crate) tall: BTreeMap<u32, (u16, u16)>,
}

/// What a tile's custom data makes of it.
enum Custom {
    Animated { frames: u16, ms: u16 },
    Tall { columns: u16, rows: u16 },
}

/// A whole number from `min` to `max`.
fn whole(value: &Value, min: i64, max: i64) -> Option<u16> {
    value
        .as_i64()
        .filter(|number| (min..=max).contains(number))
        .and_then(|number| u16::try_from(number).ok())
}

/// Reads one tile's custom data: `{"frames": N, "ms": M}` or `{"tall": [C, R]}`.
fn custom(text: &str) -> Option<Custom> {
    let value = json::parse(text, Limits::default()).ok()?;
    match value.as_object()? {
        [(a, frames), (b, ms)] | [(b, ms), (a, frames)] if a == "frames" && b == "ms" => {
            Some(Custom::Animated {
                frames: whole(frames, 2, i64::from(u16::MAX))?,
                ms: whole(ms, 1, i64::from(u16::MAX))?,
            })
        }
        [(key, block)] if key == "tall" => match block.as_array()? {
            [columns, rows] => Some(Custom::Tall {
                columns: whole(columns, 1, i64::from(u16::MAX))?,
                rows: whole(rows, 1, i64::from(u16::MAX))?,
            }),
            _ => None,
        },
        _ => None,
    }
}

/// Checks the tile set `def`, `defs.tilesets[index]` of the project `path`,
/// against its image, `image`, of `size` pixels, which its sidecar cuts
/// into tiles of `tiles`, and reads its tiles' custom data. The image cooks
/// to the sprite `sprite` of the atlas `atlas`.
///
/// # Errors
///
/// [`CookError::Config`], naming the project and the member, for a tile
/// set with spacing or padding, of another size than its image, of other
/// tiles than its sidecar's, of more than [`level::MAX_TILES`] tiles, or
/// with custom data the cook does not read or that does not fit it.
pub(crate) fn tileset(
    path: &str,
    index: usize,
    def: &TilesetDef,
    (atlas, sprite): (String, String),
    size: (u32, u32),
    tiles: (u32, u32),
) -> Result<Tileset, CookError> {
    let at = format!("defs.tilesets[{index}]");
    let named = &def.identifier;
    for (gap, member) in [(def.spacing, "spacing"), (def.padding, "padding")] {
        if gap != 0 {
            return Err(fault(
                path,
                &format!("{at}.{member}"),
                format!(
                    "the tile set `{named}` has {member}: the cook's tile sets are a grid of tiles with none"
                ),
            ));
        }
    }
    if (def.width, def.height) != size {
        let member = if def.width != size.0 {
            "pxWid"
        } else {
            "pxHei"
        };
        return Err(fault(
            path,
            &format!("{at}.{member}"),
            format!(
                "the tile set `{named}` is {} × {}, and its image `{sprite}.png` is {} × {}: open the project in LDtk to bring it up to date",
                def.width, def.height, size.0, size.1
            ),
        ));
    }
    if tiles != (def.tile_size, def.tile_size) {
        return Err(fault(
            path,
            &format!("{at}.tileGridSize"),
            format!(
                "the tile set `{named}` has tiles of {} pixels, and the sidecar of `{sprite}.png` cuts it into tiles of {} × {}",
                def.tile_size, tiles.0, tiles.1
            ),
        ));
    }
    if (def.columns, def.rows) != (size.0 / def.tile_size, size.1 / def.tile_size) {
        let member = if def.columns != size.0 / def.tile_size {
            "__cWid"
        } else {
            "__cHei"
        };
        return Err(fault(
            path,
            &format!("{at}.{member}"),
            format!(
                "the tile set `{named}` is {} × {} tiles, and its image holds {} × {}",
                def.columns,
                def.rows,
                size.0 / def.tile_size,
                size.1 / def.tile_size
            ),
        ));
    }
    let count = def.tile_count();
    let (Ok(columns), Ok(rows)) = (u16::try_from(def.columns), u16::try_from(def.rows)) else {
        return Err(fault(
            path,
            &at,
            format!("a tile set has at most {} tiles", level::MAX_TILES),
        ));
    };
    if count > u64::from(level::MAX_TILES) {
        return Err(fault(
            path,
            &at,
            format!(
                "a tile set has at most {} tiles, and `{named}` has {count}",
                level::MAX_TILES
            ),
        ));
    }

    let mut animations: Vec<(u16, u16, u16, usize)> = Vec::new();
    let mut tall = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (entry, data) in def.custom_data.iter().enumerate() {
        let at = format!("{at}.customData[{entry}].data");
        let tile = data.tile;
        if !seen.insert(tile) {
            return Err(fault(
                path,
                &at,
                format!("tile {tile} has custom data twice"),
            ));
        }
        let Some(what) = custom(&data.data) else {
            return Err(fault(
                path,
                &at,
                r#"a tile's custom data is `{"frames": 4, "ms": 250}`, an animation of its tile and the 3 after it, each shown 250 ms, or `{"tall": [1, 2]}`, a tall tile of the block of 1 × 2 tiles from it"#,
            ));
        };
        let (column, row) = (tile % def.columns, tile / def.columns);
        match what {
            Custom::Animated { frames, ms } => {
                if u64::from(tile) + u64::from(frames) > count {
                    return Err(fault(
                        path,
                        &at,
                        format!(
                            "the animation of tile {tile} has {frames} frames, past the tile set's last tile"
                        ),
                    ));
                }
                // `tile` < `count` ≤ `MAX_TILES`, which fits a `u16`.
                let first = u16::try_from(tile).expect("a tile of at most 65535 tiles");
                animations.push((first, frames, ms, entry));
            }
            Custom::Tall { columns, rows } => {
                if column + u32::from(columns) > def.columns || row + u32::from(rows) > def.rows {
                    return Err(fault(
                        path,
                        &at,
                        format!(
                            "the block of {columns} × {rows} tiles from tile {tile} goes past the tile set's edge"
                        ),
                    ));
                }
                tall.insert(tile, (columns, rows));
            }
        }
    }
    animations.sort_unstable();
    for pair in animations.windows(2) {
        let ((first, frames, ..), (next, .., entry)) = (pair[0], pair[1]);
        if u32::from(next) < u32::from(first) + u32::from(frames) {
            return Err(fault(
                path,
                &format!("{at}.customData[{entry}].data"),
                format!(
                    "the animation of tile {next} starts inside that of tile {first}, of {frames} frames"
                ),
            ));
        }
    }
    Ok(Tileset {
        atlas,
        sprite,
        columns,
        rows,
        tile: def.tile_size,
        animations: animations
            .into_iter()
            .map(|(tile, frames, ms, _)| (tile, frames, ms))
            .collect(),
        tall,
    })
}

/// A level to cook: the file it is in, with where, its name, and the level
/// with its layers.
#[derive(Clone, Debug)]
pub(crate) struct LevelSource {
    /// The file that holds its layers, as found: the project, or its level
    /// file.
    pub(crate) found: String,
    /// Where it is in that file, with a dot after it: `levels[0].` in a
    /// project, nothing in a level file.
    pub(crate) at: String,
    /// Its level file's path, `levels/<name>.klvl`.
    pub(crate) output: String,
    /// The level, with its layers.
    pub(crate) level: Level,
}

/// What a project's levels cook to.
pub(crate) struct Cooked {
    /// The level files, by path below the output folder, in order.
    pub(crate) files: Vec<(String, Vec<u8>)>,
    /// What to tell the user.
    pub(crate) warnings: Vec<String>,
    pub(crate) entities: usize,
}

/// Cooks `levels`, which draw the tile sets `tilesets` (by uid) and spawn
/// the prefabs `prefabs` (by name).
///
/// # Errors
///
/// [`CookError::Config`], naming the file and the member, for what
/// ADR-0038 refuses: a drawn layer with an opacity below 1, a tile with an
/// alpha below 1, two tiles in one cell, a tile off its layer's grid or of
/// another size, a flipped tall tile, an offset or a second `Collision`
/// layer or one that is not an IntGrid, an entity whose prefab is not
/// there, and a field that overrides nothing or that version 1 cannot
/// write. [`CookError::Encode`] if the level file cannot be written as KBJ,
/// or the game's reader refuses it.
pub(crate) fn cook(
    levels: &[LevelSource],
    tilesets: &BTreeMap<i64, Tileset>,
    prefabs: &BTreeSet<String>,
) -> Result<Cooked, CookError> {
    let mut cooked = Cooked {
        files: Vec::with_capacity(levels.len()),
        warnings: Vec::new(),
        entities: 0,
    };
    for source in levels {
        let (value, entities) = one(source, tilesets, prefabs)?;
        if !source.level.fields.is_empty() {
            cooked.warnings.push(format!(
                "`{}`: the fields of the level `{}` are not cooked: a level file has no place for them yet",
                source.found, source.level.identifier
            ));
        }
        let path = &source.output;
        let bytes = kbj::encode(&value, Limits::default()).map_err(|error| CookError::Encode {
            path: path.clone(),
            error,
        })?;
        // The game's reader takes it, or the cook refuses it: a level past
        // the reader's size, say.
        level::parse(&bytes, Limits::default()).map_err(|error| CookError::Encode {
            path: path.clone(),
            error,
        })?;
        cooked.entities += entities;
        cooked.files.push((path.clone(), bytes));
    }
    cooked.files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(cooked)
}

fn int(value: impl Into<i64>) -> Value {
    Value::Int(value.into())
}

fn pair(a: impl Into<i64>, b: impl Into<i64>) -> Value {
    Value::Array(vec![int(a), int(b)])
}

fn object(members: Vec<(&str, Value)>) -> Value {
    Value::Object(
        members
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

/// One level's file, as a value, and how many entities it has.
fn one(
    source: &LevelSource,
    tilesets: &BTreeMap<i64, Tileset>,
    prefabs: &BTreeSet<String>,
) -> Result<(Value, usize), CookError> {
    let level = &source.level;
    let found = source.found.as_str();
    // The tile sets by uid, in the order the level first draws them, and
    // each one's place in that order.
    let mut used = Used::default();
    let mut layers = Vec::new();
    let mut grid = None;
    let mut tall = Vec::new();
    let mut entities = Vec::new();
    let empty = Vec::new();
    let instances = level.layers.as_ref().unwrap_or(&empty);
    // What the grid and the drawn layers' cells will hold, before any is
    // made: the reader bounds each layer, not their sum.
    let mut cells: u64 = 0;
    for (index, layer) in instances.iter().enumerate().rev() {
        let holds = u64::from(layer.columns) * u64::from(layer.rows);
        let made = u64::from(layer.identifier == COLLISION) + u64::from(drawn(layer));
        cells = cells.saturating_add(holds.saturating_mul(made));
        if cells > MAX_CELLS {
            return Err(fault(
                found,
                &format!("{}layerInstances[{index}].__cWid", source.at),
                format!(
                    "the level's tile layers and grid hold more than {MAX_CELLS} cells together"
                ),
            ));
        }
    }
    // LDtk lists the top-most first: they are drawn from the bottom up.
    for (index, layer) in instances.iter().enumerate().rev() {
        let at = format!("{}layerInstances[{index}]", source.at);
        if layer.identifier == COLLISION {
            if grid.is_some() {
                return Err(fault(found, &at, "a level has one `Collision` layer"));
            }
            grid = Some(collision(found, &at, layer)?);
        }
        if drawn(layer) {
            draw(
                found,
                &at,
                layer,
                tilesets,
                &mut used,
                &mut layers,
                &mut tall,
            )?;
        }
        for (entry, entity) in layer.entities.iter().enumerate() {
            let at = format!("{at}.entityInstances[{entry}]");
            let name = snake_case(&entity.identifier);
            if !prefab::is_name(&name) {
                return Err(fault(
                    found,
                    &format!("{at}.__identifier"),
                    format!(
                        "the entity `{}` would spawn the prefab `{name}`, which is not a prefab's name: lowercase letters, digits and `_`",
                        entity.identifier
                    ),
                ));
            }
            if !prefabs.contains(&name) {
                return Err(fault(
                    found,
                    &format!("{at}.__identifier"),
                    format!(
                        "the entity `{}` spawns the prefab `{name}`, and there is no `data/prefabs/{name}.jsonc`",
                        entity.identifier
                    ),
                ));
            }
            let position = pair(
                i64::from(entity.px[0]) + i64::from(layer.offset[0]),
                i64::from(entity.px[1]) + i64::from(layer.offset[1]),
            );
            entities.push(object(vec![
                ("prefab", Value::String(name)),
                ("position", position),
                ("overrides", overrides(found, &at, &entity.fields)?),
            ]));
        }
    }

    let mut assets: Vec<String> = Vec::new();
    let mut sets = Vec::with_capacity(used.order.len());
    for uid in &used.order {
        let set = &tilesets[uid];
        assets.push(set.atlas.clone());
        let animations = set
            .animations
            .iter()
            .map(|&(tile, frames, ms)| {
                object(vec![
                    ("tile", int(tile)),
                    ("frames", int(frames)),
                    ("ms", int(ms)),
                ])
            })
            .collect();
        sets.push(object(vec![
            ("atlas", Value::String(set.atlas.clone())),
            ("sprite", Value::String(set.sprite.clone())),
            ("columns", int(set.columns)),
            ("rows", int(set.rows)),
            ("animations", Value::Array(animations)),
        ]));
    }
    assets.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assets.dedup();
    let [red, green, blue] = level.background;
    let entity_count = entities.len();
    let mut members = vec![
        ("version", int(level::VERSION)),
        ("size", pair(level.width, level.height)),
        (
            "background",
            Value::Array(vec![int(red), int(green), int(blue)]),
        ),
        (
            "assets",
            Value::Array(assets.into_iter().map(Value::String).collect()),
        ),
        ("tilesets", Value::Array(sets)),
        ("layers", Value::Array(layers)),
    ];
    if let Some(grid) = grid {
        members.push(("grid", grid));
    }
    members.push(("tall", Value::Array(tall)));
    members.push(("entities", Value::Array(entities)));
    Ok((object(members), entity_count))
}

/// The grid of the `Collision` layer `layer`, at `at` in `found`.
fn collision(found: &str, at: &str, layer: &LayerInstance) -> Result<Value, CookError> {
    if layer.kind != LayerKind::IntGrid {
        return Err(fault(
            found,
            &format!("{at}.__type"),
            "the `Collision` layer is an IntGrid, whose cells that are not 0 are solid",
        ));
    }
    if layer.offset != [0, 0] {
        let member = if layer.offset[0] != 0 {
            "__pxTotalOffsetX"
        } else {
            "__pxTotalOffsetY"
        };
        return Err(fault(
            found,
            &format!("{at}.{member}"),
            "the `Collision` layer has no offset: the grid starts at the level's corner",
        ));
    }
    let solid = layer
        .int_grid
        .iter()
        .map(|&value| int(i64::from(value != 0)))
        .collect();
    Ok(object(vec![
        ("size", pair(layer.columns, layer.rows)),
        ("cell", int(layer.grid_size)),
        ("origin", pair(0, 0)),
        ("layer", Value::String(WORLD.to_string())),
        ("solid", Value::Array(solid)),
    ]))
}

/// The tile sets a level draws, by uid, in the order it first draws them.
#[derive(Default)]
struct Used {
    order: Vec<i64>,
    places: BTreeMap<i64, usize>,
}

impl Used {
    /// The place of `uid` in the order, which it joins if it is new.
    fn place(&mut self, uid: i64) -> usize {
        *self.places.entry(uid).or_insert_with(|| {
            self.order.push(uid);
            self.order.len() - 1
        })
    }
}

/// Adds the tiles of the drawn layer `layer`, at `at` in `found`: its
/// cells to `layers`, if any is not empty, and its tall tiles to `tall`,
/// by cell, row by row; its tile set to `used`, if it is new.
fn draw(
    found: &str,
    at: &str,
    layer: &LayerInstance,
    tilesets: &BTreeMap<i64, Tileset>,
    used: &mut Used,
    layers: &mut Vec<Value>,
    tall: &mut Vec<Value>,
) -> Result<(), CookError> {
    let unknown = || {
        fault(
            found,
            &format!("{at}.__tilesetDefUid"),
            "its tile set is not one the cook found",
        )
    };
    let uid = layer.tileset.ok_or_else(unknown)?;
    let set = tilesets.get(&uid).ok_or_else(unknown)?;
    if layer.opacity < 1.0 {
        return Err(fault(
            found,
            &format!("{at}.__opacity"),
            "a layer's opacity must be 1: the cook draws every tile opaque",
        ));
    }
    let cell = layer.grid_size;
    if set.tile != cell {
        return Err(fault(
            found,
            &format!("{at}.__gridSize"),
            format!(
                "the layer's cells are {cell} pixels, and its tile set's tiles {}: they must be one size",
                set.tile
            ),
        ));
    }
    let index = used.place(uid);
    let (columns, rows) = (i64::from(layer.columns), i64::from(layer.rows));
    let side = i64::from(cell);
    let [x0, y0] = layer.offset.map(i64::from);
    // `columns` × `rows` is at most `max_entries`, as the reader checked,
    // and the level's cells at most `MAX_CELLS`, as `one` checked.
    let count = usize::try_from(columns * rows).expect("a layer's cells fit memory");
    let mut cells = vec![0i64; count];
    let mut taken = vec![false; count];
    let mut blocks: Vec<((i64, i64), Value)> = Vec::new();
    let tiles = layer
        .grid_tiles
        .iter()
        .enumerate()
        .map(|(entry, tile)| ("gridTiles", entry, tile))
        .chain(
            layer
                .auto_layer_tiles
                .iter()
                .enumerate()
                .map(|(entry, tile)| ("autoLayerTiles", entry, tile)),
        );
    for (member, entry, tile) in tiles {
        let at = format!("{at}.{member}[{entry}]");
        if tile.alpha < 1.0 {
            return Err(fault(
                found,
                &format!("{at}.a"),
                "a tile's opacity must be 1: the cook draws every tile opaque",
            ));
        }
        let [x, y] = tile.px.map(i64::from);
        let (column, row) = (x.div_euclid(side), y.div_euclid(side));
        if x % side != 0
            || y % side != 0
            || !(0..columns).contains(&column)
            || !(0..rows).contains(&row)
        {
            return Err(fault(
                found,
                &format!("{at}.px"),
                "a tile's corner is on a corner of one of its layer's cells",
            ));
        }
        let place = usize::try_from(row * columns + column).expect("a cell of the layer");
        if std::mem::replace(&mut taken[place], true) {
            return Err(fault(
                found,
                &at,
                format!(
                    "two tiles in the cell ({column}, {row}): the cook draws one a cell, so put the other on a layer of its own"
                ),
            ));
        }
        match set.tall.get(&tile.tile) {
            Some(&(width, height)) => {
                if tile.flip != 0 {
                    return Err(fault(
                        found,
                        &format!("{at}.f"),
                        "a tall tile cannot be flipped",
                    ));
                }
                let across = i64::from(width) * side;
                if across % 2 != 0 {
                    return Err(fault(
                        found,
                        &at,
                        "a tall tile's feet would fall between two pixels: its block is an odd number of pixels wide",
                    ));
                }
                // Its feet are the bottom middle of the block, whose
                // bottom-left cell is the tile's.
                let feet = pair(x0 + column * side + across / 2, y0 + (row + 1) * side);
                blocks.push((
                    (row, column),
                    object(vec![
                        ("tileset", int(i64::try_from(index).unwrap_or(i64::MAX))),
                        ("tile", int(tile.tile)),
                        ("size", pair(width, height)),
                        ("feet", feet),
                        ("layer", int(TALL_LAYER)),
                    ]),
                ));
            }
            None => cells[place] = 1 + i64::from(tile.tile) + FLIP * i64::from(tile.flip),
        }
    }
    blocks.sort_by_key(|&(place, _)| place);
    tall.extend(blocks.into_iter().map(|(_, block)| block));
    if cells.iter().any(|&cell| cell != 0) {
        let depth = i64::try_from(layers.len()).unwrap_or(i64::MAX);
        layers.push(object(vec![
            ("tileset", int(i64::try_from(index).unwrap_or(i64::MAX))),
            ("size", pair(columns, rows)),
            ("cell", int(cell)),
            ("origin", pair(x0, y0)),
            ("layer", int(TILE_LAYER)),
            ("depth", int(depth)),
            (
                "cells",
                Value::Array(cells.into_iter().map(Value::Int).collect()),
            ),
        ]));
    }
    Ok(())
}

/// Whether a field of LDtk's type `kind` can override a component's field:
/// a number, a truth, text, a file's path or an enum's value.
fn overridable(kind: &str) -> bool {
    matches!(
        kind,
        "Int" | "Float" | "Bool" | "String" | "Multilines" | "FilePath"
    ) || kind.starts_with("LocalEnum.")
        || kind.starts_with("ExternEnum.")
}

/// The overrides of an entity's fields, at `at` in `found`: each field
/// `Component__field` (with more `__` for nested fields) that has a value,
/// as `{"Component": {"field": value}}`. Components, and fields in each,
/// are in name order. A field with no value is checked too, but left out.
fn overrides(found: &str, at: &str, fields: &[FieldInstance]) -> Result<Value, CookError> {
    let mut paths: Vec<(Vec<&str>, usize)> = Vec::with_capacity(fields.len());
    for (entry, field) in fields.iter().enumerate() {
        let at = format!("{at}.fieldInstances[{entry}]");
        let path: Vec<&str> = field.identifier.split("__").collect();
        if path.len() < 2
            || !prefab::is_component_name(path[0])
            || path.iter().any(|part| part.is_empty())
        {
            return Err(fault(
                found,
                &format!("{at}.__identifier"),
                format!(
                    "the field `{}` overrides nothing: name it `Component__field`, as `Health__max`, to override that field of that component",
                    field.identifier
                ),
            ));
        }
        if !overridable(&field.kind) {
            return Err(fault(
                found,
                &format!("{at}.__type"),
                format!(
                    "a field of type `{}` cannot override a component in version 1: use an Int, Float, Bool, String, Multilines, FilePath or enum field",
                    field.kind
                ),
            ));
        }
        // LDtk's "no value", checked as the others are, so that a field
        // the cook cannot write is refused before an instance sets it: the
        // prefab's value stands.
        if !field.value.is_null() {
            paths.push((path, entry));
        }
    }
    paths.sort_unstable();
    // A field that another's path holds, or that holds another's, would
    // override it: sorted, such a pair is side by side.
    for pair in paths.windows(2) {
        let ((first, _), (second, entry)) = (&pair[0], &pair[1]);
        if second.starts_with(first) {
            return Err(fault(
                found,
                &format!("{at}.fieldInstances[{entry}].__identifier"),
                format!(
                    "the field `{}` overrides what `{}` does",
                    second.join("__"),
                    first.join("__")
                ),
            ));
        }
    }
    let mut root: Vec<(String, Value)> = Vec::new();
    for (path, entry) in paths {
        let (last, folders) = path.split_last().expect("two parts at least");
        let mut members = &mut root;
        // Sorted, the fields of one object are side by side: each goes in
        // the last member, or a new one.
        for part in folders {
            let open = matches!(members.last(), Some((key, Value::Object(_))) if key == part);
            if !open {
                members.push((part.to_string(), Value::Object(Vec::new())));
            }
            members = match members.last_mut() {
                Some((_, Value::Object(inner))) => inner,
                _ => unreachable!("the last member is the object just found or made"),
            };
        }
        members.push((last.to_string(), fields[entry].value.clone()));
    }
    Ok(Value::Object(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_named_in_snake_case() {
        for (identifier, name) in [
            ("Tree", "tree"),
            ("EnemyBase", "enemy_base"),
            ("Level_0", "level_0"),
            ("Big_Tree", "big_tree"),
            ("HTTPGate", "http_gate"),
            ("Level2B", "level2_b"),
            ("rock", "rock"),
            ("Été", "Été"),
        ] {
            assert_eq!(snake_case(identifier), name, "{identifier}");
        }
        assert_eq!(file_name("Field"), Some("levels/field.klvl".to_string()));
        assert_eq!(file_name("Été"), None);
        assert_eq!(file_name("Con"), None, "a Windows device name");
        assert_eq!(file_name(""), None);
    }

    #[test]
    fn paths_relative_to_a_project_stay_in_the_source_folder() {
        assert_eq!(
            resolve("levels", "../sprites/Tiles.png"),
            Ok("sprites/tiles.png".to_string())
        );
        assert_eq!(
            resolve("levels", "field/0000-Field.ldtkl"),
            Ok("levels/field/0000-field.ldtkl".to_string())
        );
        assert_eq!(resolve("", "./a/./b.png"), Ok("a/b.png".to_string()));
        for (folder, relative) in [
            ("levels", "../../outside.png"),
            ("", "../x.png"),
            ("levels", "/etc/x.png"),
            ("levels", "a//b.png"),
            ("levels", "C:/x.png"),
            ("levels", "a\\b.png"),
        ] {
            assert!(resolve(folder, relative).is_err(), "{relative}");
        }
    }

    #[test]
    fn custom_data_is_an_animation_or_a_tall_tile() {
        assert!(matches!(
            custom(r#"{"frames": 4, "ms": 250}"#),
            Some(Custom::Animated { frames: 4, ms: 250 })
        ));
        assert!(matches!(
            custom(r#"{ "ms": 600, "frames": 2 }"#),
            Some(Custom::Animated { frames: 2, ms: 600 })
        ));
        assert!(matches!(
            custom(r#"{"tall": [1, 2]}"#),
            Some(Custom::Tall {
                columns: 1,
                rows: 2
            })
        ));
        for text in [
            "",
            "tall",
            "[]",
            r#"{"frames": 1, "ms": 250}"#,
            r#"{"frames": 4, "ms": 0}"#,
            r#"{"frames": 4, "ms": 65536}"#,
            r#"{"frames": 4}"#,
            r#"{"frames": 4, "ms": 250, "loop": true}"#,
            r#"{"frames": 2.5, "ms": 250}"#,
            r#"{"tall": [0, 2]}"#,
            r#"{"tall": [1]}"#,
            r#"{"tall": 2}"#,
        ] {
            assert!(custom(text).is_none(), "{text}");
        }
    }
}
