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

//! Levels in the world (ADR-0038, `docs/subsystems/scene.md`, "Levels"):
//! [`load_level`] reads a cooked level file, `levels/<name>.klvl`, and its
//! prefabs, and makes its tile layers, its collision grid, its tall tiles
//! and its entities; [`CurrentLevel`] keeps what it made, so that the next
//! load removes the level whole; and the engine's `nadir::extract_level`
//! draws its tile layers each frame, its background the clear colour.

use std::fmt;

use nadir_assets::{AssetError, AssetServer, FinalizeCtx, Handle, Scope};
use nadir_collision::TileGrid;
use nadir_core::log::info;
use nadir_core::time::{Frame, Stopwatch};
use nadir_core::{Color, IVec2, UVec2, Vec2};
use nadir_ecs::{Entity, World};
use nadir_render::{
    Atlas, Flip, RenderFrame, Texture, Tile, TileLayer, TileLayerDraw, TileLayers, TileSet,
};
use nadir_scene::level::{Level, LevelFile};
use nadir_scene::{PrefabError, PrefabLibrary, TypeRegistry};

use crate::{ClipError, Clips, CollisionLayers, Collisions, Sprite, Transform, loading_context};

/// The level the world holds, if any: what [`load_level`] made of it. A
/// resource of every [`App`](crate::App), empty until a level loads.
#[derive(Debug, Default)]
pub struct CurrentLevel {
    level: Option<Loaded>,
}

/// What a level made.
#[derive(Debug)]
struct Loaded {
    name: String,
    background: Color,
    /// Its tall tiles' entities, then its prefabs', in the order they
    /// spawned.
    entities: Vec<Entity>,
    /// Its tile layers, in [`TileLayers`], drawn in this order.
    layers: Vec<TileLayerDraw>,
    /// The atlases whose clips it holds in [`Clips`], by their pages.
    atlases: Vec<Vec<Handle<Texture>>>,
    /// Whether it set the collision grid.
    grid: bool,
    /// The clear colour it replaced, which it puts back when it goes.
    clear: Color,
}

impl Loaded {
    /// Its entities and tile layers counted, for the log.
    fn counts(&self) -> String {
        let count = |n: usize, one: &str, many: &str| match n {
            1 => format!("1 {one}"),
            n => format!("{n} {many}"),
        };
        format!(
            "{}, {}",
            count(self.entities.len(), "entity", "entities"),
            count(self.layers.len(), "tile layer", "tile layers")
        )
    }
}

impl CurrentLevel {
    /// The level's name, as [`load_level`] was given it, if one is loaded.
    pub fn name(&self) -> Option<&str> {
        self.level.as_ref().map(|level| level.name.as_str())
    }

    /// The level's background, the frames' clear colour while it is
    /// loaded.
    pub fn background(&self) -> Option<Color> {
        self.level.as_ref().map(|level| level.background)
    }

    /// The entities the level spawned, its tall tiles' then its prefabs',
    /// which the next load despawns. Empty if none is loaded.
    pub fn entities(&self) -> &[Entity] {
        self.level
            .as_ref()
            .map_or(&[], |level| level.entities.as_slice())
    }

    /// The level's tile layers, in [`TileLayers`], as the engine draws them
    /// each frame, the first beneath. Empty if none is loaded.
    pub fn tile_layers(&self) -> &[TileLayerDraw] {
        self.level
            .as_ref()
            .map_or(&[], |level| level.layers.as_slice())
    }

    /// The path of the level's file, if one is loaded.
    #[cfg(feature = "dev")]
    pub(crate) fn path(&self) -> Option<String> {
        self.name().and_then(level_path)
    }

    /// Knows the atlas of the level's that was known by `old`, its pages,
    /// by `new` from now on, as [`Clips`] knows it after a reload
    /// ([`Clips::reload_atlas`]), so that the level lets go of it there.
    #[cfg(feature = "dev")]
    pub(crate) fn replace_atlas_pages(&mut self, old: &[Handle<Texture>], new: &[Handle<Texture>]) {
        if let Some(level) = &mut self.level {
            for pages in &mut level.atlases {
                if pages.as_slice() == old {
                    *pages = new.to_vec();
                }
            }
        }
    }
}

/// Why a level could not be loaded. The previous level is gone then, and
/// nothing of the new one is in the world but the prefabs it read, which
/// the [`PrefabLibrary`] keeps.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum LevelError {
    /// The name is not a level's: lowercase letters, digits and `_`, as the
    /// cook names level files.
    BadName {
        /// The name.
        name: String,
    },
    /// The level file, or an atlas of its tile sets, could not be loaded.
    Load {
        /// Its path.
        path: String,
        /// Why.
        error: AssetError,
    },
    /// A tile set of the level is not a tile set of its atlas, or not of
    /// the level's size.
    Tileset {
        /// The atlas.
        atlas: String,
        /// The tile set's sprite in it.
        sprite: String,
        /// What is wrong.
        reason: &'static str,
    },
    /// The clips of an atlas of the level could not be added.
    Clips {
        /// The atlas.
        atlas: String,
        /// Why.
        error: ClipError,
    },
    /// The level's grid is on a collision layer the game has not named
    /// ([`App::collision_layers`](crate::App::collision_layers)).
    UnknownLayer {
        /// The layer's name.
        name: String,
    },
    /// A prefab of the level's entities could not be loaded, or an entity
    /// could not be spawned.
    Prefab(PrefabError),
}

impl fmt::Display for LevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadName { name } => write!(
                f,
                "`{name}` is not a level's name: lowercase letters, digits and `_`"
            ),
            Self::Load { path, error } => write!(f, "`{path}` could not be loaded: {error}"),
            Self::Tileset {
                atlas,
                sprite,
                reason,
            } => write!(
                f,
                "the tile set `{sprite}` of `{atlas}` is not the level's: {reason}"
            ),
            Self::Clips { atlas, error } => {
                write!(f, "the clips of `{atlas}` could not be added: {error}")
            }
            Self::UnknownLayer { name } => write!(
                f,
                "the level's grid is on the collision layer `{name}`, which the game has not named"
            ),
            Self::Prefab(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LevelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Load { error, .. } => Some(error),
            Self::Clips { error, .. } => Some(error),
            Self::Prefab(error) => Some(error),
            _ => None,
        }
    }
}

/// The path of the level file of the level `name`, if it is a level's
/// name.
fn level_path(name: &str) -> Option<String> {
    let named = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    named.then(|| format!("levels/{name}.klvl"))
}

/// Loads the level `name` into `world`, the world of an
/// [`App`](crate::App), in place of the one it holds (ADR-0038):
///
/// 1. The previous level goes, as [`unload_level`] removes it.
/// 2. `levels/<name>.klvl` and its prefabs are read before this returns,
///    the prefabs checked against the [`TypeRegistry`]. The level's assets
///    load in [`Scope::Level`]; its textures stream in as any do, so they
///    show a frame or more later. The [`PrefabLibrary`] keeps the prefabs,
///    which later loads use without reading them again, as it keeps any.
/// 3. Its tile sets' atlases are read, and their clips added to the
///    [`Clips`], which hold an atlas the game added too only once
///    ([`Clips::add_atlas`]).
/// 4. Its tall tiles spawn as sprites (`TileSet::block_uv`), each with its
///    feet at its place, then its entities spawn through their prefabs, each
///    with its [`Transform`] at its place in the level.
/// 5. Its tile layers go into [`TileLayers`], and the engine draws them
///    each frame, in their order, in front of the clear colour, which is the
///    level's background. Its grid replaces the [`Collisions`]' grid, on
///    the collision layer the grid names; a level with none leaves an empty
///    grid.
///
/// It logs how long it took.
///
/// # Errors
///
/// A [`LevelError`]: the name is not a level's; the level file or an atlas
/// is missing or malformed; a tile set is not its atlas's; an atlas's
/// clips cannot be added; the grid's collision layer is not named; or a
/// prefab is missing, has a component the registry has not got, or a value
/// that does not fit. Then the previous level is gone, and nothing of the
/// new one is left in the world but the prefabs it read.
///
/// # Panics
///
/// If `world` lacks one of an [`App`](crate::App)'s resources, or one of
/// them is borrowed elsewhere.
pub fn load_level(world: &mut World, name: &str) -> Result<(), LevelError> {
    let stopwatch = Stopwatch::start();
    unload_level(world);
    let Some(path) = level_path(name) else {
        return Err(LevelError::BadName {
            name: name.to_string(),
        });
    };
    let mut made = Loaded {
        name: name.to_string(),
        background: Color::BLACK,
        entities: Vec::new(),
        layers: Vec::new(),
        atlases: Vec::new(),
        grid: false,
        // With no level loaded, the clear colour is the game's.
        clear: world.resource::<RenderFrame>().clear,
    };
    match make(world, &path, &mut made) {
        Ok(()) => {
            let ms = stopwatch.elapsed().as_secs_f64() * 1000.0;
            info!("loaded the level `{name}` in {ms:.1} ms: {}", made.counts());
            world.resource_mut::<CurrentLevel>().level = Some(made);
            Ok(())
        }
        Err(error) => {
            remove(world, made);
            Err(error)
        }
    }
}

/// Removes the level `world` holds, if any, and returns whether there was
/// one: despawns the entities it spawned, takes its tile layers out of
/// [`TileLayers`], lets go of its atlases' clips in [`Clips`]
/// ([`Clips::release_atlas`]), empties the grid it set, puts back the
/// clear colour it replaced, and unloads [`Scope::Level`]. The prefabs it
/// read stay in the [`PrefabLibrary`].
///
/// # Panics
///
/// As [`load_level`].
pub fn unload_level(world: &mut World) -> bool {
    let Some(level) = world.resource_mut::<CurrentLevel>().level.take() else {
        return false;
    };
    remove(world, level);
    true
}

/// Makes the level `world` holds again, from its files as they are now,
/// in place of what it made, and returns whether there was one: the hot
/// reload's, under `dev` (M5.8). Unlike [`load_level`], it keeps the
/// handles a game may hold of the level:
///
/// - it unloads no scope, so its assets keep their handles, and a file
///   loaded again gives the value it has now;
/// - its tile layers keep theirs in [`TileLayers`], in their order: a
///   layer more is added, and one fewer is removed;
/// - its atlases' clips keep theirs in [`Clips`]: each atlas is held again
///   before the last load's hold goes.
///
/// Its entities are spawned anew, so their [`Entity`] ids change, and what
/// happened to them since the level loaded is undone. Its grid and its
/// background are its new files'; the clear colour it puts back when it
/// goes is the one from before its load. It logs how long it took.
///
/// # Errors
///
/// As [`load_level`]: then the level is as it was. Of its new files, the
/// prefabs they read stay in the library, and the assets they loaded stay
/// loaded, in [`Scope::Level`], until the level goes.
#[cfg(feature = "dev")]
pub(crate) fn reload_level(world: &mut World) -> Result<bool, LevelError> {
    let Some(old) = world.resource_mut::<CurrentLevel>().level.take() else {
        return Ok(false);
    };
    let stopwatch = Stopwatch::start();
    let path = level_path(&old.name).expect("a level loaded by its name");
    let mut made = Loaded {
        name: old.name.clone(),
        background: Color::BLACK,
        entities: Vec::new(),
        layers: Vec::new(),
        atlases: Vec::new(),
        grid: false,
        clear: old.clear,
    };
    if let Err(error) = make(world, &path, &mut made) {
        // `make` sets the grid last, so a failure left the old one.
        take_apart(world, &made);
        world.resource_mut::<CurrentLevel>().level = Some(old);
        return Err(error);
    }
    for &entity in &old.entities {
        world.despawn(entity);
    }
    {
        let mut layers = world.resource_mut::<TileLayers>();
        for (index, draw) in made.layers.iter_mut().enumerate() {
            let Some(kept) = old.layers.get(index) else {
                continue;
            };
            // A layer the game removed keeps the handle it was added under.
            if layers.get(kept.tiles).is_some()
                && let Some(layer) = layers.remove(draw.tiles)
                && let Some(slot) = layers.get_mut(kept.tiles)
            {
                *slot = layer;
                draw.tiles = kept.tiles;
            }
        }
        for gone in old.layers.iter().skip(made.layers.len()) {
            layers.remove(gone.tiles);
        }
    }
    {
        let mut clips = world.resource_mut::<Clips>();
        for pages in &old.atlases {
            clips.release_atlas(pages);
        }
    }
    let ms = stopwatch.elapsed().as_secs_f64() * 1000.0;
    info!(
        "reloaded the level `{}` in {ms:.1} ms: {}",
        made.name,
        made.counts()
    );
    world.resource_mut::<CurrentLevel>().level = Some(made);
    Ok(true)
}

/// Removes what `level` made, and unloads [`Scope::Level`].
fn remove(world: &mut World, level: Loaded) {
    take_apart(world, &level);
    if level.grid {
        world
            .resource_mut::<Collisions>()
            .set_grid(TileGrid::empty());
    }
    world.resource_mut::<RenderFrame>().clear = level.clear;
    world
        .resource_mut::<AssetServer>()
        .unload_scope(Scope::Level);
}

/// Takes out what `level` made but its grid: despawns its entities, takes
/// its tile layers out of [`TileLayers`], and lets go of its atlases' clips
/// in [`Clips`].
fn take_apart(world: &mut World, level: &Loaded) {
    for &entity in &level.entities {
        world.despawn(entity);
    }
    {
        let mut layers = world.resource_mut::<TileLayers>();
        for draw in &level.layers {
            layers.remove(draw.tiles);
        }
    }
    let mut clips = world.resource_mut::<Clips>();
    for pages in &level.atlases {
        clips.release_atlas(pages);
    }
}

/// A tile set of the level: its texture's tiles, and a tile's size in
/// pixels.
#[derive(Clone, Copy)]
struct Set {
    tileset: TileSet,
    tile: UVec2,
}

/// The cell at `index` of a grid `width` cells wide, row by row.
fn cell(index: usize, width: u32) -> IVec2 {
    let width = width.max(1) as usize;
    // The reader bounds a level's sides by 2^24 pixels, and so its cells.
    IVec2::new((index % width) as i32, (index / width) as i32)
}

fn point([x, y]: [i32; 2]) -> Vec2 {
    Vec2::new(x as f32, y as f32)
}

/// Makes the level of `path` in `world`, noting in `made` what it makes, so
/// that it can be removed if a later step fails.
fn make(world: &mut World, path: &str, made: &mut Loaded) -> Result<(), LevelError> {
    let load = |path: &str| {
        let path = path.to_string();
        move |error| LevelError::Load { path, error }
    };
    let file: LevelFile = {
        let mut server = world.resource_mut::<AssetServer>();
        let handle = server
            .load_now_in::<Level>(path, Scope::Level, &mut FinalizeCtx::new())
            .map_err(load(path))?;
        server
            .get(handle)
            .map(|level| level.file().clone())
            .ok_or_else(|| load(path)(AssetError::NotFound))?
    };
    made.background = Color::rgb(file.background[0], file.background[1], file.background[2]);

    // Its prefabs, read and checked before anything is made.
    #[cfg(feature = "dev")]
    forget_unloaded_prefabs(world, &file);
    {
        let mut server = world.resource_mut::<AssetServer>();
        let registry = world.resource::<TypeRegistry>();
        world
            .resource_mut::<PrefabLibrary>()
            .load_level(&mut server, &registry, &file, Scope::Level)
            .map_err(LevelError::Prefab)?;
    }

    // Its grid, on the collision layer it names.
    let grid = match &file.grid {
        Some(entry) => {
            let mask = world
                .resource::<CollisionLayers>()
                .mask(&[entry.layer.as_str()])
                .ok_or_else(|| LevelError::UnknownLayer {
                    name: entry.layer.clone(),
                })?;
            let [width, height] = entry.size;
            let mut grid = TileGrid::new(width, height, entry.cell as f32, point(entry.origin));
            grid.layer = mask;
            for (index, &solid) in entry.solid.iter().enumerate() {
                if solid {
                    grid.set_solid(cell(index, width), true);
                }
            }
            grid
        }
        None => TileGrid::empty(),
    };

    // Its tile sets, and their atlases' clips.
    let mut sets = Vec::with_capacity(file.tilesets.len());
    for entry in &file.tilesets {
        let fault = |reason| LevelError::Tileset {
            atlas: entry.atlas.clone(),
            sprite: entry.sprite.clone(),
            reason,
        };
        let (atlas, pages) = {
            let mut server = world.resource_mut::<AssetServer>();
            let handle = server
                .load_now_in::<Atlas>(&entry.atlas, Scope::Level, &mut FinalizeCtx::new())
                .map_err(load(&entry.atlas))?;
            let atlas = server
                .get(handle)
                .cloned()
                .ok_or_else(|| load(&entry.atlas)(AssetError::NotFound))?;
            let pages = atlas.load_pages(&mut server, Scope::Level);
            (atlas, pages)
        };
        let sprite = atlas
            .sprite(&entry.sprite)
            .ok_or_else(|| fault("its atlas has no sprite of that name"))?;
        let page = sprite
            .frames()
            .first()
            .and_then(|frame| pages.get(frame.page()).copied());
        let (Some(tileset), Some(tile)) = (
            page.and_then(|texture| sprite.tile_set(texture)),
            sprite.tile_size(),
        ) else {
            return Err(fault("the sprite is not a tile set"));
        };
        if (tileset.columns, tileset.rows) != (entry.columns, entry.rows) {
            return Err(fault(
                "its tiles across and down are not the level's: cook the level again",
            ));
        }
        let held = world
            .resource_mut::<Clips>()
            .add_atlas(&atlas, &pages)
            .map_err(|error| LevelError::Clips {
                atlas: entry.atlas.clone(),
                error,
            })?;
        // An atlas of no clip is not held, and is not let go of either.
        if !held.is_empty() {
            made.atlases.push(pages);
        }
        sets.push(Set { tileset, tile });
    }
    // The reader checked that each layer and tall tile names a tile set of
    // the file.
    let set = |index: u32| {
        usize::try_from(index)
            .ok()
            .and_then(|index| sets.get(index).copied())
            .ok_or(LevelError::Tileset {
                atlas: String::new(),
                sprite: String::new(),
                reason: "the level names a tile set it has not got",
            })
    };

    // Its tall tiles, as the y-sorted sprites of their blocks.
    for tall in &file.tall {
        let Set { tileset, tile } = set(tall.tileset)?;
        let size = UVec2::new(u32::from(tall.size[0]), u32::from(tall.size[1]));
        let mut sprite = Sprite::new(
            tileset.texture,
            Vec2::new((size.x * tile.x) as f32, (size.y * tile.y) as f32),
        );
        sprite.uv = tileset.block_uv(tall.tile, size);
        sprite.layer = tall.layer;
        let entity = world
            .spawn()
            .with(Transform::at(point(tall.feet)))
            .with(sprite)
            .id();
        made.entities.push(entity);
    }

    // Its entities, through their prefabs, all made before any is spawned.
    let spawned = {
        let library = world.resource::<PrefabLibrary>();
        let registry = world.resource::<TypeRegistry>();
        let mut commands = world.commands();
        loading_context(world, path, Scope::Level, |mut cx| {
            library.spawn_level(&file, &registry, &mut commands, &mut cx)
        })
        .map_err(LevelError::Prefab)?
    };
    world.apply_commands();
    {
        let mut transforms = world.write::<Transform>();
        for &(entity, position) in &spawned {
            let at = point([position.x, position.y]);
            match transforms.get_mut(entity) {
                Some(transform) => transform.position = at,
                None => {
                    transforms.insert(entity, Transform::at(at));
                }
            }
            made.entities.push(entity);
        }
    }

    // Its tile layers.
    {
        let mut layers = world.resource_mut::<TileLayers>();
        for entry in &file.layers {
            let Set { tileset, .. } = set(entry.tileset)?;
            let animations = &file.tilesets[entry.tileset as usize];
            let [width, height] = entry.size;
            let mut layer = TileLayer::new(
                tileset,
                UVec2::new(width, height),
                entry.cell as f32,
                point(entry.origin),
            );
            for (index, at) in entry.cells.iter().enumerate() {
                let Some(at) = at else {
                    continue;
                };
                let mut tile = match animations.animation(at.tile) {
                    Some(animation) => Tile::animated(at.tile, animation.frames, animation.millis),
                    None => Tile::new(at.tile),
                };
                tile.flip = Flip {
                    x: at.flip_x,
                    y: at.flip_y,
                };
                layer.set(cell(index, width), Some(tile));
            }
            made.layers.push(TileLayerDraw {
                tiles: layers.add(layer),
                layer: entry.layer,
                depth: entry.depth,
            });
        }
    }

    world.resource_mut::<Collisions>().set_grid(grid);
    made.grid = true;
    Ok(())
}

/// Removes from the [`PrefabLibrary`] each prefab the entities of `file`
/// spawn, and each base it extends as the library has it, whose file the
/// asset server no longer holds, as when the last load of a level unloaded
/// it: so that the level reads it from its file again, and the hot reload
/// watches that file again (M5.8). A prefab the game inserted from
/// another source is kept.
#[cfg(feature = "dev")]
fn forget_unloaded_prefabs(world: &World, file: &LevelFile) {
    let server = world.resource::<AssetServer>();
    let loaded = |path: &str| {
        server
            .assets::<nadir_scene::PrefabAsset>()
            .and_then(|assets| assets.handle(path))
            .is_some()
    };
    let mut library = world.resource_mut::<PrefabLibrary>();
    for entity in &file.entities {
        let mut next = Some(entity.prefab.clone());
        // A chain longer than `load` takes, or a cycle, is its to report.
        for _ in 0..=nadir_scene::MAX_EXTENDS {
            let Some(name) = next.take() else {
                break;
            };
            let Some(prefab) = library.get(&name) else {
                break;
            };
            next = prefab.extends().map(str::to_string);
            let path = PrefabLibrary::path(&name);
            if prefab.source() == path && !loaded(&path) {
                library.remove(&name);
            }
        }
    }
}

/// Draws the level's tile layers into the [`RenderFrame`], in their order,
/// before the game's own, and makes its background the clear colour: the
/// engine's `nadir::extract_level`, in frame `Extract`. It allocates
/// nothing once the frame's list has room for them.
pub(crate) fn extract_level(world: &World, _frame: &Frame) {
    let current = world.resource::<CurrentLevel>();
    let Some(level) = &current.level else {
        return;
    };
    let mut out = world.resource_mut::<RenderFrame>();
    out.clear = level.background;
    out.tile_layers.extend_from_slice(&level.layers);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use nadir_assets::{AssetPath, OnFailure, Source};
    use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
    use nadir_codec::{Limits, json, kbj};
    use nadir_collision::{LayerMask, Shape};

    use super::*;
    use crate::{App, Collider, Config};

    /// Files in memory, by path.
    struct Memory(HashMap<String, Vec<u8>>);

    impl Source for Memory {
        fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
            self.0
                .get(path.as_str())
                .cloned()
                .ok_or(AssetError::NotFound)
        }
    }

    /// The atlas `atlas/tiles.kbj`: a flame of two frames, whose clip is
    /// `tiles/flame/burn`, and the tile set `tiles/ground`, 2 × 1 tiles of
    /// 16 on a page of its own.
    const ATLAS: &str = r#"{ "version": 2,
        "pages": [ { "path": "atlas/tiles_0.qoi", "w": 64, "h": 64 },
                   { "path": "atlas/tiles/ground.tiles.qoi", "w": 32, "h": 16 } ],
        "sprites": [
          { "name": "tiles/flame",
            "frames": [ { "page": 0, "x": 1, "y": 1, "w": 8, "h": 8, "ox": 0, "oy": 0, "sw": 8, "sh": 8 },
                        { "page": 0, "x": 11, "y": 1, "w": 8, "h": 8, "ox": 0, "oy": 0, "sw": 8, "sh": 8 } ],
            "pivot": [4.0, 8.0],
            "clips": [ { "name": "burn", "loop": true, "frames": [[0, 100], [1, 100]] } ] },
          { "name": "tiles/ground",
            "frames": [ { "page": 1, "x": 0, "y": 0, "w": 32, "h": 16, "ox": 0, "oy": 0, "sw": 32, "sh": 16 } ],
            "tiles": [16, 16] } ] }"#;

    /// The level `meadow`: a layer of 4 × 3 cells, its first tile animated
    /// and its second mirrored; a grid; a tall tile; two stones, the second
    /// with its collider's offset overridden.
    const MEADOW: &str = r#"{ "version": 1, "size": [64, 48], "background": [10, 20, 30],
        "assets": ["atlas/tiles.kbj"],
        "tilesets": [ { "atlas": "atlas/tiles.kbj", "sprite": "tiles/ground", "columns": 2, "rows": 1,
                        "animations": [ { "tile": 0, "frames": 2, "ms": 200 } ] } ],
        "layers": [ { "tileset": 0, "size": [4, 3], "cell": 16, "origin": [0, 0], "layer": 0, "depth": 0,
                      "cells": [1, 65538, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2] } ],
        "grid": { "size": [4, 3], "cell": 16, "origin": [0, 0], "layer": "World",
                  "solid": [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1] },
        "tall": [ { "tileset": 0, "tile": 1, "size": [1, 1], "feet": [24, 32], "layer": 1 } ],
        "entities": [ { "prefab": "stone", "position": [40, 16], "overrides": {} },
                      { "prefab": "stone", "position": [8, 40],
                        "overrides": { "Collider": { "offset": [0, -4] } } } ] }"#;

    /// The level `pond`: nothing but its size and background.
    const POND: &str = r#"{ "version": 1, "size": [16, 16], "background": [1, 2, 3], "assets": [],
        "tilesets": [], "layers": [], "tall": [], "entities": [] }"#;

    /// The level `haunted`, of a ghost whose prefab has a component no one
    /// registered.
    const HAUNTED: &str = r#"{ "version": 1, "size": [16, 16], "background": [1, 2, 3], "assets": [],
        "tilesets": [], "layers": [], "tall": [],
        "entities": [ { "prefab": "ghost", "position": [0, 0], "overrides": {} } ] }"#;

    fn kbj(text: &str) -> Vec<u8> {
        let value = json::parse(text, Limits::default()).expect("the test's JSON");
        kbj::encode(&value, Limits::default()).expect("KBJ")
    }

    fn page(width: u32, height: u32) -> Vec<u8> {
        let header = Header {
            width,
            height,
            channels: Channels::Rgba,
            color_space: ColorSpace::Srgb,
        };
        qoi::encode(
            &header,
            &[90, 140, 70, 255].repeat((width * height) as usize),
        )
        .expect("a valid image")
    }

    /// The files above, by path, with the prefabs of the stone and the
    /// ghost.
    fn files() -> HashMap<String, Vec<u8>> {
        HashMap::from([
            ("atlas/tiles.kbj".to_string(), kbj(ATLAS)),
            ("atlas/tiles_0.qoi".to_string(), page(64, 64)),
            ("atlas/tiles/ground.tiles.qoi".to_string(), page(32, 16)),
            ("levels/meadow.klvl".to_string(), kbj(MEADOW)),
            ("levels/pond.klvl".to_string(), kbj(POND)),
            ("levels/haunted.klvl".to_string(), kbj(HAUNTED)),
            (
                "data/prefabs/stone.kbj".to_string(),
                kbj(r#"{ "name": "stone", "components": {
                    "Collider": { "shape": { "Aabb": { "half": [4, 4] } },
                                  "layer": ["World"], "mask": ["World"] } } }"#),
            ),
            (
                "data/prefabs/ghost.kbj".to_string(),
                kbj(r#"{ "name": "ghost", "components": { "Health": { "max": 3 } } }"#),
            ),
        ])
    }

    /// An app whose assets are the files above, with the collision layer
    /// `World` named if `named`.
    fn app(named: bool) -> App {
        let app = App::new(Config::default()).insert_resource(AssetServer::with_source(
            Memory(files()),
            OnFailure::Nothing,
        ));
        if named {
            app.collision_layers(&["World"])
        } else {
            app
        }
    }

    #[test]
    fn a_level_loads_its_tile_layers_grid_tall_tiles_and_entities() {
        let mut app = app(true);
        app.load_level("meadow").expect("the meadow");
        let world = app.world();
        let current = world.resource::<CurrentLevel>();
        assert_eq!(current.name(), Some("meadow"));
        assert_eq!(current.background(), Some(Color::rgb(10, 20, 30)));

        // The layer, its tiles as the file has them.
        let &[draw] = current.tile_layers() else {
            panic!("one layer: {:?}", current.tile_layers());
        };
        assert_eq!((draw.layer, draw.depth), (0, 0));
        let layers = world.resource::<TileLayers>();
        let layer = layers.get(draw.tiles).expect("the layer");
        assert_eq!(layer.size(), UVec2::new(4, 3));
        assert_eq!(layer.get(IVec2::new(0, 0)), Some(Tile::animated(0, 2, 200)));
        let mut mirrored = Tile::new(1);
        mirrored.flip.x = true;
        assert_eq!(layer.get(IVec2::new(1, 0)), Some(mirrored));
        assert_eq!(layer.get(IVec2::new(2, 0)), None);
        assert_eq!(layer.get(IVec2::new(3, 2)), Some(Tile::new(1)));
        let tileset = layer.tileset();
        assert_eq!((tileset.columns, tileset.rows), (2, 1));

        // The grid, on `World`.
        let collisions = world.resource::<Collisions>();
        let grid = collisions.grid();
        assert_eq!(grid.layer, LayerMask::bit(0));
        assert!(grid.is_solid(IVec2::new(0, 0)) && grid.is_solid(IVec2::new(3, 2)));
        assert!(!grid.is_solid(IVec2::new(1, 0)));

        // The tall tile first, then the stones, each at its place.
        let entities = current.entities();
        assert_eq!(entities.len(), 3);
        let transforms = world.read::<Transform>();
        let at = |index: usize| transforms.get(entities[index]).map(|t| t.position);
        assert_eq!(at(0), Some(Vec2::new(24.0, 32.0)));
        assert_eq!(at(1), Some(Vec2::new(40.0, 16.0)));
        assert_eq!(at(2), Some(Vec2::new(8.0, 40.0)));
        let sprites = world.read::<Sprite>();
        let tall = sprites.get(entities[0]).expect("the tall tile's sprite");
        assert_eq!((tall.size, tall.layer), (Vec2::new(16.0, 16.0), 1));
        assert_eq!(tall.uv, tileset.block_uv(1, UVec2::new(1, 1)));
        assert_eq!(tall.texture, tileset.texture);
        let colliders = world.read::<Collider>();
        let stone = colliders.get(entities[1]).expect("a stone's collider");
        assert_eq!(
            stone.0.shape,
            Shape::Aabb {
                half: Vec2::splat(4.0)
            }
        );
        assert_eq!(stone.0.offset, Vec2::ZERO);
        let moved = colliders.get(entities[2]).expect("a stone's collider");
        assert_eq!(moved.0.offset, Vec2::new(0.0, -4.0));
        assert!(world.resource::<Clips>().find("tiles/flame/burn").is_some());
        drop((current, layers, collisions, transforms, sprites, colliders));

        // Each frame draws the layer, in front of the level's background.
        app.update(Duration::ZERO, &[]);
        let frame = app.world().resource::<RenderFrame>();
        assert_eq!(frame.tile_layers, [draw]);
        assert_eq!(frame.clear, Color::rgb(10, 20, 30));
    }

    #[test]
    fn a_second_level_replaces_the_first_whole() {
        let mut app = app(true);
        // The game's clear colour, which the levels' backgrounds replace.
        let game = Color::rgb(7, 8, 9);
        app.world().resource_mut::<RenderFrame>().clear = game;
        app.load_level("meadow").expect("the meadow");
        let (entities, draws) = {
            let current = app.world().resource::<CurrentLevel>();
            (current.entities().to_vec(), current.tile_layers().to_vec())
        };
        let atlas = app
            .world()
            .resource_mut::<AssetServer>()
            .load_in::<Atlas>("atlas/tiles.kbj", Scope::Level);

        app.load_level("pond").expect("the pond");
        let world = app.world();
        assert_eq!(world.resource::<CurrentLevel>().name(), Some("pond"));
        // The meadow's entities, layers, clips, grid and assets are gone.
        assert!(entities.iter().all(|&entity| !world.is_alive(entity)));
        assert_eq!(world.entity_count(), 0);
        let layers = world.resource::<TileLayers>();
        assert!(draws.iter().all(|draw| layers.get(draw.tiles).is_none()));
        assert!(layers.is_empty());
        assert!(world.resource::<Clips>().is_empty());
        assert!(!world.resource::<Collisions>().grid().is_solid(IVec2::ZERO));
        assert_eq!(world.resource::<AssetServer>().state(atlas), None);
        drop(layers);

        // Its own background, and no tile layer, from the next frame.
        app.update(Duration::ZERO, &[]);
        let frame = app.world().resource::<RenderFrame>();
        assert!(frame.tile_layers.is_empty());
        assert_eq!(frame.clear, Color::rgb(1, 2, 3));
        drop(frame);
        // With no level, the game's clear colour is back.
        assert!(unload_level(app.world_mut()));
        assert_eq!(app.world().resource::<CurrentLevel>().name(), None);
        assert!(!unload_level(app.world_mut()));
        app.update(Duration::ZERO, &[]);
        assert_eq!(app.world().resource::<RenderFrame>().clear, game);
    }

    #[test]
    fn an_atlas_the_game_added_keeps_its_clips_when_a_level_goes() {
        let mut app = app(true);
        {
            let world = app.world();
            let mut server = world.resource_mut::<AssetServer>();
            let handle = server
                .load_now::<Atlas>("atlas/tiles.kbj", &mut FinalizeCtx::new())
                .expect("the atlas");
            let atlas = server.get(handle).cloned().expect("the atlas");
            let pages = atlas.load_pages(&mut server, Scope::Global);
            world
                .resource_mut::<Clips>()
                .add_atlas(&atlas, &pages)
                .expect("the clips");
        }
        let burn = app.world().resource::<Clips>().find("tiles/flame/burn");
        assert!(burn.is_some());
        // The level does not add them again, nor take them when it goes.
        app.load_level("meadow").expect("the meadow");
        app.load_level("pond").expect("the pond");
        assert_eq!(
            app.world().resource::<Clips>().find("tiles/flame/burn"),
            burn
        );
    }

    #[test]
    fn an_atlas_the_game_adds_after_a_level_keeps_its_clips_when_it_goes() {
        let mut app = app(true);
        app.load_level("meadow").expect("the meadow");
        let burn = app.world().resource::<Clips>().find("tiles/flame/burn");
        assert!(burn.is_some());
        // The game adds the atlas the level added: the same clips, held by
        // both, which stay when the level goes.
        {
            let world = app.world();
            let mut server = world.resource_mut::<AssetServer>();
            let handle = server
                .load_now::<Atlas>("atlas/tiles.kbj", &mut FinalizeCtx::new())
                .expect("the atlas");
            let atlas = server.get(handle).cloned().expect("the atlas");
            let pages = atlas.load_pages(&mut server, Scope::Global);
            let added = world
                .resource_mut::<Clips>()
                .add_atlas(&atlas, &pages)
                .expect("the clips");
            assert_eq!(added.first().copied(), burn);
        }
        app.load_level("pond").expect("the pond");
        assert_eq!(
            app.world().resource::<Clips>().find("tiles/flame/burn"),
            burn
        );
    }

    #[test]
    fn a_level_that_cannot_load_says_why_and_leaves_nothing() {
        let mut app = app(true);
        app.load_level("meadow").expect("the meadow");
        let error = app.load_level("haunted").expect_err("an unknown component");
        assert!(
            matches!(
                &error,
                LevelError::Prefab(PrefabError::UnknownComponent { prefab, component, .. })
                    if prefab == "ghost" && component == "Health"
            ),
            "{error}"
        );
        let world = app.world();
        assert_eq!(world.resource::<CurrentLevel>().name(), None);
        assert_eq!(world.entity_count(), 0);
        assert!(world.resource::<TileLayers>().is_empty());
        // But for the prefabs it read, which the library keeps.
        assert!(world.resource::<PrefabLibrary>().get("ghost").is_some());

        assert_eq!(
            app.load_level("nowhere"),
            Err(LevelError::Load {
                path: "levels/nowhere.klvl".to_string(),
                error: AssetError::NotFound,
            })
        );
        assert_eq!(
            app.load_level("../meadow"),
            Err(LevelError::BadName {
                name: "../meadow".to_string()
            })
        );
        // A grid on a layer the game has not named.
        let mut unnamed = self::app(false);
        assert_eq!(
            unnamed.load_level("meadow"),
            Err(LevelError::UnknownLayer {
                name: "World".to_string()
            })
        );
        assert_eq!(unnamed.world().entity_count(), 0);
    }

    /// A level made again by the hot reload, under `dev` (M5.8).
    #[cfg(feature = "dev")]
    mod reloaded {
        use std::path::Path;
        use std::sync::{Arc, Mutex, PoisonError};

        use super::*;
        use crate::hot_reload::{HotReload, Reloads};

        type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;

        /// The hot reload of `app`, its asset half: no shaders are watched.
        fn hot(app: &App) -> HotReload {
            HotReload::start(app.world(), Path::new(""))
        }

        /// The files above, which a test changes.
        struct Changing(Files);

        impl Source for Changing {
            fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
                let files = self.0.lock().unwrap_or_else(PoisonError::into_inner);
                files
                    .get(path.as_str())
                    .cloned()
                    .ok_or(AssetError::NotFound)
            }
        }

        /// The meadow, loaded, and its files.
        fn meadow() -> (App, Files) {
            let files = Arc::new(Mutex::new(files()));
            let source = Changing(Arc::clone(&files));
            let mut app = App::new(Config::default())
                .insert_resource(AssetServer::with_source(source, OnFailure::Nothing))
                .collision_layers(&["World"]);
            app.load_level("meadow").expect("the meadow");
            (app, files)
        }

        /// Writes `text` as the KBJ file at `path`, and reloads it as the
        /// watcher would.
        fn rewrite(app: &App, files: &Files, path: &str, text: &str) {
            files
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(path.to_string(), kbj(text));
            let mut server = app.world().resource_mut::<AssetServer>();
            assert_eq!(server.reload(path), 1, "{path} is loaded once");
            server.finish_all_loads(&mut FinalizeCtx::new());
        }

        /// The meadow with each `(from, to)` of `edits` made in its file.
        fn edited(edits: &[(&str, &str)]) -> String {
            edits.iter().fold(MEADOW.to_string(), |text, (from, to)| {
                assert!(text.contains(from), "the meadow has {from}");
                text.replace(from, to)
            })
        }

        #[test]
        fn a_level_made_again_keeps_its_handles_and_spawns_its_entities_anew() {
            let (mut app, files) = meadow();
            let (entities, draw) = {
                let current = app.world().resource::<CurrentLevel>();
                (current.entities().to_vec(), current.tile_layers()[0])
            };
            let (burn, atlas) = {
                let world = app.world();
                let server = world.resource::<AssetServer>();
                let atlas = server
                    .assets::<Atlas>()
                    .and_then(|it| it.handle("atlas/tiles.kbj"));
                (world.resource::<Clips>().find("tiles/flame/burn"), atlas)
            };
            assert!(burn.is_some() && atlas.is_some());

            // The first stone moved, the last cell of the layer emptied,
            // and a second layer, of one cell.
            let second = r#", { "tileset": 0, "size": [1, 1], "cell": 16, "origin": [0, 0],
                    "layer": 2, "depth": 1, "cells": [1] } ],"#;
            let text = edited(&[
                (r#""position": [40, 16]"#, r#""position": [44, 20]"#),
                ("0, 0, 0, 0, 2] } ],", &format!("0, 0, 0, 0, 0] }}{second}")),
            ]);
            rewrite(&app, &files, "levels/meadow.klvl", &text);
            assert_eq!(reload_level(app.world_mut()), Ok(true));

            let world = app.world();
            let current = world.resource::<CurrentLevel>();
            // Its first layer under its handle, the second new.
            let draws = current.tile_layers();
            assert_eq!(draws.len(), 2);
            assert_eq!(draws[0], draw);
            assert_ne!(draws[1].tiles, draw.tiles);
            let layers = world.resource::<TileLayers>();
            assert_eq!(layers.len(), 2);
            let layer = layers.get(draw.tiles).expect("the first layer");
            assert_eq!(layer.get(IVec2::new(3, 2)), None);
            assert_eq!(layer.get(IVec2::new(1, 0)).map(|tile| tile.index), Some(1));
            // Its entities spawned anew, the stone at its new place.
            assert!(entities.iter().all(|&entity| !world.is_alive(entity)));
            assert_eq!(world.entity_count(), 3);
            let stone = current.entities()[1];
            let at = world.read::<Transform>().get(stone).map(|it| it.position);
            assert_eq!(at, Some(Vec2::new(44.0, 20.0)));
            // Its clips and its assets under their handles.
            assert_eq!(world.resource::<Clips>().find("tiles/flame/burn"), burn);
            let server = world.resource::<AssetServer>();
            assert!(atlas.is_some_and(|it| server.state(it).is_some()));
            drop((current, layers, server));

            // Made again as it first was: the second layer goes.
            rewrite(&app, &files, "levels/meadow.klvl", MEADOW);
            assert_eq!(reload_level(app.world_mut()), Ok(true));
            let world = app.world();
            assert_eq!(world.resource::<CurrentLevel>().tile_layers(), [draw]);
            assert_eq!(world.resource::<TileLayers>().len(), 1);
            assert_eq!(world.entity_count(), 3);

            // The atlas is held once, by the level: its clips go with it.
            assert!(unload_level(app.world_mut()));
            assert!(app.world().resource::<Clips>().is_empty());
            assert!(app.world().resource::<TileLayers>().is_empty());
            assert_eq!(reload_level(app.world_mut()), Ok(false));
        }

        #[test]
        fn a_level_that_cannot_be_made_again_stays_as_it_was() {
            let (mut app, files) = meadow();
            let before = {
                let current = app.world().resource::<CurrentLevel>();
                (current.entities().to_vec(), current.tile_layers().to_vec())
            };
            let burn = app.world().resource::<Clips>().find("tiles/flame/burn");
            // A stone of a prefab no one cooked: nothing is made.
            let text = edited(&[(
                r#""prefab": "stone", "position": [40, 16]"#,
                r#""prefab": "boulder", "position": [40, 16]"#,
            )]);
            rewrite(&app, &files, "levels/meadow.klvl", &text);
            let error = reload_level(app.world_mut()).expect_err("no boulder");
            assert!(
                matches!(&error, LevelError::Prefab(PrefabError::Load { path, .. })
                    if path == "data/prefabs/boulder.kbj"),
                "{error}"
            );
            assert_eq!(app.world().entity_count(), 3);
            // A stone whose value does not fit, once its atlas is held again
            // and its tall tile spawned.
            let text = edited(&[("[0, -4]", r#""up""#)]);
            rewrite(&app, &files, "levels/meadow.klvl", &text);
            let error = reload_level(app.world_mut()).expect_err("a bad offset");
            assert!(matches!(&error, LevelError::Prefab(_)), "{error}");

            let world = app.world();
            let current = world.resource::<CurrentLevel>();
            assert_eq!(current.name(), Some("meadow"));
            assert_eq!(
                (current.entities().to_vec(), current.tile_layers().to_vec()),
                before
            );
            assert!(before.0.iter().all(|&entity| world.is_alive(entity)));
            assert_eq!(world.entity_count(), 3);
            assert_eq!(world.resource::<TileLayers>().len(), 1);
            assert_eq!(world.resource::<Clips>().find("tiles/flame/burn"), burn);
            assert!(world.resource::<Collisions>().grid().is_solid(IVec2::ZERO));
            drop((world, current));

            // Fixed, it is made again; the failure held nothing.
            rewrite(&app, &files, "levels/meadow.klvl", MEADOW);
            assert_eq!(reload_level(app.world_mut()), Ok(true));
            assert!(unload_level(app.world_mut()));
            assert!(app.world().resource::<Clips>().is_empty());
        }

        #[test]
        fn a_level_loaded_again_reads_the_prefabs_its_last_load_unloaded() {
            let (mut app, files) = meadow();
            // The last load's prefab files go with its scope; the stone is
            // edited meanwhile, its box larger.
            app.load_level("pond").expect("the pond");
            files.lock().unwrap_or_else(PoisonError::into_inner).insert(
                "data/prefabs/stone.kbj".to_string(),
                kbj(r#"{ "name": "stone", "components": {
                    "Collider": { "shape": { "Aabb": { "half": [6, 6] } },
                                  "layer": ["World"], "mask": ["World"] } } }"#),
            );
            app.load_level("meadow").expect("the meadow again");
            let world = app.world();
            let stone = world.resource::<CurrentLevel>().entities()[1];
            let shape = world.read::<Collider>().get(stone).map(|it| it.0.shape);
            assert_eq!(
                shape,
                Some(Shape::Aabb {
                    half: Vec2::splat(6.0)
                })
            );
            // Its file is loaded, and so watched, again.
            let mut server = world.resource_mut::<AssetServer>();
            assert_eq!(server.reload("data/prefabs/stone.kbj"), 1);
        }

        #[test]
        fn an_atlas_that_gains_its_first_clip_keeps_it_through_the_level() {
            // The meadow's atlas, its flame of no clip.
            let still = ATLAS.replace(
                r#""pivot": [4.0, 8.0],
            "clips": [ { "name": "burn", "loop": true, "frames": [[0, 100], [1, 100]] } ] },"#,
                r#""pivot": [4.0, 8.0] },"#,
            );
            assert_ne!(still, ATLAS);
            let files = Arc::new(Mutex::new(files()));
            files
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert("atlas/tiles.kbj".to_string(), kbj(&still));
            let source = Changing(Arc::clone(&files));
            let mut app = App::new(Config::default())
                .insert_resource(AssetServer::with_source(source, OnFailure::Nothing))
                .collision_layers(&["World"]);
            app.load_level("meadow").expect("the meadow");
            assert!(app.world().resource::<Clips>().is_empty());

            // The flame gains its clip: none of the atlas's was added, so
            // the level made again adds it.
            rewrite(&app, &files, "atlas/tiles.kbj", ATLAS);
            {
                let world = app.world();
                let mut server = world.resource_mut::<AssetServer>();
                let handle = server
                    .assets::<Atlas>()
                    .and_then(|it| it.handle("atlas/tiles.kbj"));
                let atlas = handle
                    .and_then(|it| server.get(it))
                    .cloned()
                    .expect("the atlas");
                let pages = atlas.load_pages(&mut server, Scope::Level);
                assert_eq!(
                    world.resource_mut::<Clips>().reload_atlas(&atlas, &pages),
                    None
                );
            }
            assert_eq!(reload_level(app.world_mut()), Ok(true));
            let burn = app.world().resource::<Clips>().find("tiles/flame/burn");
            assert!(
                burn.is_some(),
                "the clip is there after the level is made again"
            );
            assert_eq!(reload_level(app.world_mut()), Ok(true));
            assert_eq!(
                app.world().resource::<Clips>().find("tiles/flame/burn"),
                burn
            );
            // Held once, by the level.
            assert!(unload_level(app.world_mut()));
            assert!(app.world().resource::<Clips>().is_empty());
        }

        #[test]
        fn a_level_due_waits_until_no_load_is_pending() {
            let (mut app, files) = meadow();
            let mut hot = hot(&app);
            let longer = ATLAS.replace("[[0, 100], [1, 100]]", "[[0, 100], [1, 300]]");
            rewrite(&app, &files, "atlas/tiles.kbj", &longer);
            // A load read, and not made yet.
            app.world()
                .resource_mut::<AssetServer>()
                .load::<Texture>("atlas/elsewhere.qoi");
            hot.assets(app.world_mut());
            hot.assets(app.world_mut());
            let reloads = *app.world().resource::<Reloads>();
            assert_eq!((reloads.atlases, reloads.levels), (1, 0));
            // Made: the level is made again, once.
            app.world()
                .resource_mut::<AssetServer>()
                .finish_all_loads(&mut FinalizeCtx::new());
            hot.assets(app.world_mut());
            hot.assets(app.world_mut());
            assert_eq!(app.world().resource::<Reloads>().levels, 1);
        }

        #[test]
        fn a_prefab_reloaded_reads_its_new_base_and_one_named_otherwise_is_refused() {
            let files = Arc::new(Mutex::new(files()));
            let source = Changing(Arc::clone(&files));
            let mut app = App::new(Config::default())
                .insert_resource(AssetServer::with_source(source, OnFailure::Nothing));
            {
                let world = app.world();
                let mut server = world.resource_mut::<AssetServer>();
                let registry = world.resource::<TypeRegistry>();
                world
                    .resource_mut::<PrefabLibrary>()
                    .load(&mut server, &registry, "stone", Scope::Global)
                    .expect("the stone");
            }
            let mut hot = hot(&app);

            // The stone extends a pebble now, which no one read yet.
            files.lock().unwrap_or_else(PoisonError::into_inner).insert(
                "data/prefabs/pebble.kbj".to_string(),
                kbj(r#"{ "name": "pebble", "components": {} }"#),
            );
            let extends = r#"{ "name": "stone", "extends": "pebble", "components": {} }"#;
            rewrite(&app, &files, "data/prefabs/stone.kbj", extends);
            hot.assets(app.world_mut());
            {
                let world = app.world();
                let library = world.resource::<PrefabLibrary>();
                let chain = library.chain("stone").map(|it| it.len());
                assert_eq!(chain, Ok(2), "the pebble is read");
                assert_eq!(world.resource::<Reloads>().prefabs, 1);
            }

            // Its file names another prefab now: the stone stays.
            let renamed = r#"{ "name": "boulder", "components": {} }"#;
            rewrite(&app, &files, "data/prefabs/stone.kbj", renamed);
            hot.assets(app.world_mut());
            let world = app.world();
            let library = world.resource::<PrefabLibrary>();
            assert_eq!(
                library.get("stone").and_then(|it| it.extends()),
                Some("pebble")
            );
            assert!(library.get("boulder").is_none());
            assert_eq!(world.resource::<Reloads>().prefabs, 1);
        }

        #[test]
        fn an_atlas_on_more_pages_is_let_go_of_by_its_new_ones() {
            let (mut app, files) = meadow();
            let burn = app.world().resource::<Clips>().find("tiles/flame/burn");
            // The atlas gains a page, which the flame's second frame is on.
            files
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert("atlas/tiles_1.qoi".to_string(), page(64, 64));
            let text = ATLAS
                .replace(
                    r#"{ "path": "atlas/tiles/ground.tiles.qoi", "w": 32, "h": 16 } ],"#,
                    r#"{ "path": "atlas/tiles/ground.tiles.qoi", "w": 32, "h": 16 },
                       { "path": "atlas/tiles_1.qoi", "w": 64, "h": 64 } ],"#,
                )
                .replace(
                    r#"{ "page": 0, "x": 11, "y": 1,"#,
                    r#"{ "page": 2, "x": 11, "y": 1,"#,
                );
            assert_ne!(text, ATLAS);
            let mut hot = hot(&app);
            rewrite(&app, &files, "atlas/tiles.kbj", &text);
            // The hot reload finds its clips again with its pages, the new
            // one loaded, and makes the level again once it is made.
            hot.assets(app.world_mut());
            app.world()
                .resource_mut::<AssetServer>()
                .finish_all_loads(&mut FinalizeCtx::new());
            hot.assets(app.world_mut());
            let world = app.world();
            assert_eq!(
                *world.resource::<Reloads>(),
                Reloads {
                    started: true,
                    atlases: 1,
                    levels: 1,
                    ..Reloads::default()
                }
            );
            let server = world.resource::<AssetServer>();
            let page = server
                .assets::<Texture>()
                .and_then(|it| it.handle("atlas/tiles_1.qoi"));
            let clips = world.resource::<Clips>();
            let frames = burn.and_then(|it| clips.get(it)).map(crate::Clip::frames);
            assert_eq!(frames.map(|it| Some(it[1].texture)), Some(page));
            assert_eq!(clips.find("tiles/flame/burn"), burn);
            drop((server, clips));
            // The level held it once, through its loads: it goes with it.
            assert!(unload_level(app.world_mut()));
            assert!(app.world().resource::<Clips>().is_empty());
        }
    }
}
