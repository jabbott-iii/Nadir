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

//! Saves in the game (roadmap M9.8, ADR-0078): the [`Saves`] resource,
//! whose slots are save files in the game's data folder, written and read
//! with nadir-scene's [`write_file`] and [`read_file`] between ticks; a
//! load that loads its level first, then restores the save over it; and
//! the [`SaveEvent`]s that say how each went.

use std::fs;
use std::path::{Path, PathBuf};

use nadir_assets::{AssetServer, Scope};
use nadir_codec::Limits;
use nadir_codec::json::Value;
use nadir_core::log::{info, warn};
use nadir_core::time::Tick;
use nadir_ecs::{Entity, Events, World};
use nadir_scene::{
    GameState, LevelId, LoadedSave, PrefabLibrary, SaveData, SaveId, StateKind, StateMachine,
    Transition, TypeRegistry, backup_path, capture, read_file, restore, write_file,
};

use crate::level::level_path;
use crate::{CurrentLevel, Parent, loading_context, reflect_context};

/// The longest name a save slot may have, in bytes.
const MAX_SLOT_NAME: usize = 64;

/// The game's save slots: a resource of every [`App`](crate::App).
///
/// A slot is a save file, `<slot>.ksav`, in the folder [`Saves::folder`]
/// gives: [`App::run`](crate::App::run) makes it `saves` in the game's
/// data folder (on Linux `~/.local/share/<game>/saves`), unless the game
/// gave one before. A slot's name is 1 to 64 lowercase ASCII letters,
/// digits, `_` and `-`, such as `slot1` or `autosave`.
///
/// A system asks for a save or a load, and the engine makes it between
/// ticks, as it makes the game's state transitions
/// ([`StateMachine`]), so that a tick runs to its end on the world it
/// started with:
///
/// - [`Saves::save`] writes the world, as nadir-scene's [`capture`] takes
///   it, at the end of the tick that asked, or of the frame stages, before
///   the transition they queued: so a system that saves and goes to the main
///   menu in one tick saves the world it played in. Each entity with a
///   [`SaveId`] is saved, with its components of the types registered as
///   saved ([`App::register_saved_component`](crate::App::register_saved_component)),
///   and so is each resource registered as saved, and the name of the level
///   the world holds ([`CurrentLevel`]).
/// - [`Saves::load`] reads the slot there, and goes to
///   [`GameState::Loading`] of the save's level. The level loads as any
///   does, then the game enters [`GameState::InGame`], and its enter
///   systems run, spawning what a new game spawns. Then the save is
///   restored over the world, as nadir-scene's [`restore`] does: every
///   entity with a [`SaveId`] is despawned, and the save's entities spawn
///   anew. A level's entity has the id the cook gave it, so a level's
///   entity that the save lacks, one killed before the save, stays gone;
///   one that it has is made of its prefab, then the level's overrides of
///   it, then its saved components. A save of no level is restored at once,
///   between ticks.
///
/// The outcome of each comes as a [`SaveEvent`], to the readers of the next
/// tick and of its frame.
///
/// A load gives the saved entities new [`Entity`]s: a game that holds an
/// entity across one holds its [`SaveId`] instead, or finds it again by a
/// component. [`Saves::loading`] tells the enter systems of
/// [`GameState::InGame`] that a load will replace what they spawn.
///
/// The writes go through a temporary file and keep the save before as a
/// backup, and a damaged file is read from its backup, with
/// [`SaveEvent::Loaded`]'s `from_backup` saying why ([`write_file`],
/// [`read_file`]). A save or load is made on the main thread, in the frame
/// that asks: a save waits for the disk to flush its file.
#[derive(Debug)]
pub struct Saves {
    folder: Option<PathBuf>,
    game_version: String,
    request: Option<Request>,
    waiting: Option<Waiting>,
    outbox: Vec<SaveEvent>,
}

/// A save or load asked for, to be made between ticks.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Request {
    Save(String),
    Load(String),
}

/// A load read, waiting for its level to load.
#[derive(Debug)]
struct Waiting {
    slot: String,
    path: PathBuf,
    level: String,
    save: LoadedSave,
}

/// How a save or a load went: an event of every [`App`](crate::App)
/// ([`Events`]), sent in the tick after the one that asked
/// ([`Saves`]), so that both that tick's readers and the frame's see it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SaveEvent {
    /// The slot was written.
    Saved {
        /// The slot.
        slot: String,
    },
    /// The slot was read, its level loaded, and the save restored.
    Loaded {
        /// The slot.
        slot: String,
        /// Why the slot's file could not be read, when its backup was read
        /// instead: tell the player that their last save was lost, and the
        /// one before it loaded.
        from_backup: Option<String>,
    },
    /// The slot was not written: the save before it, if any, is as it was.
    SaveFailed {
        /// The slot.
        slot: String,
        /// Why.
        reason: String,
    },
    /// The slot was not loaded. If its level had loaded, the world is that
    /// level as a new game has it.
    LoadFailed {
        /// The slot.
        slot: String,
        /// Why.
        reason: String,
        /// Whether the slot has no save: neither a file nor a backup. A
        /// "Continue" with nothing to continue, say.
        missing: bool,
    },
}

nadir_scene::reflect_newtype! {
    /// The name of the level the world held when it was saved, empty if
    /// none: a resource of every [`App`](crate::App), registered as saved,
    /// which [`Saves::save`] sets before it saves, so that a load knows
    /// which level to load first.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct SavedLevel(pub String);
}

impl Saves {
    /// Slots in no folder yet, saved as the game's version `game_version`,
    /// which each save file's header keeps.
    pub(crate) fn new(game_version: &str) -> Self {
        Self {
            folder: None,
            game_version: game_version.to_string(),
            request: None,
            waiting: None,
            outbox: Vec::new(),
        }
    }

    /// The folder of the slots: none until [`App::run`](crate::App::run) or
    /// the game sets one, and saves and loads fail until then.
    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    /// Keeps the slots in `folder`, which is made when a save first needs
    /// it: before [`App::run`](crate::App::run), so that it does not set
    /// its own, or in a test or a headless run, which `run` does not reach.
    pub fn set_folder(&mut self, folder: impl Into<PathBuf>) {
        self.folder = Some(folder.into());
    }

    /// Keeps the slots in `folder` unless the game gave a folder already:
    /// [`App::run`](crate::App::run)'s.
    pub(crate) fn default_folder(&mut self, folder: PathBuf) {
        if self.folder.is_none() {
            self.folder = Some(folder);
        }
    }

    /// The file of the slot `slot`: `<folder>/<slot>.ksav`. `None` if there
    /// is no folder or `slot` is not a slot's name.
    pub fn path(&self, slot: &str) -> Option<PathBuf> {
        slot_path(self.folder.as_deref(), slot).ok()
    }

    /// Whether the slot `slot` has a save to load: its file or its backup
    /// is there. A load of it may still fail, if neither reads.
    pub fn exists(&self, slot: &str) -> bool {
        self.path(slot)
            .is_some_and(|path| path.is_file() || backup_path(&path).is_file())
    }

    /// Asks for the world to be saved in the slot `slot` at the end of this
    /// tick, or of the frame stages, in place of any save or load asked for
    /// before it there. A [`SaveEvent`] says how it went.
    pub fn save(&mut self, slot: &str) {
        self.request = Some(Request::Save(slot.to_string()));
    }

    /// Asks for the slot `slot` to be loaded at the end of this tick, or of
    /// the frame stages, in place of any save or load asked for before it
    /// there: its level loads, then the save is restored over it
    /// ([`Saves`]). A [`SaveEvent`] says how it went.
    pub fn load(&mut self, slot: &str) {
        self.request = Some(Request::Load(slot.to_string()));
    }

    /// The slot whose load waits for its level, from the transition to the
    /// level's [`GameState::Loading`] until the save is restored: the enter
    /// systems of [`GameState::InGame`] see it, and may leave out what the
    /// save brings back.
    pub fn loading(&self) -> Option<&str> {
        self.waiting.as_ref().map(|waiting| waiting.slot.as_str())
    }
}

/// Whether `name` is a slot's: 1 to [`MAX_SLOT_NAME`] lowercase ASCII
/// letters, digits, `_` and `-`, so that it is a file's name on every
/// system, the same whatever case the file system ignores.
fn is_slot_name(name: &str) -> bool {
    (1..=MAX_SLOT_NAME).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// The file of the slot `slot` in `folder`, or why there is none.
fn slot_path(folder: Option<&Path>, slot: &str) -> Result<PathBuf, String> {
    if !is_slot_name(slot) {
        return Err(format!(
            "{slot:?} is not a slot's name: 1 to {MAX_SLOT_NAME} lowercase letters, digits, `_` \
             and `-`"
        ));
    }
    let folder = folder.ok_or_else(|| {
        "the saves have no folder: `App::run` gives them one, or `Saves::set_folder`".to_string()
    })?;
    Ok(folder.join(format!("{slot}.ksav")))
}

/// The engine's system `nadir::saves`, first in each tick: sends the
/// outcomes of the saves and loads made since the last tick, as
/// [`SaveEvent`]s.
pub(crate) fn send_events(world: &World, _tick: &Tick) {
    let mut saves = world.resource_mut::<Saves>();
    if saves.outbox.is_empty() {
        return;
    }
    let mut events = world.resource_mut::<Events<SaveEvent>>();
    for event in saves.outbox.drain(..) {
        events.send(event);
    }
}

/// Makes the save or load asked for, if any: between ticks, after a tick
/// or the frame stages, before the transition they queued.
pub(crate) fn make_requested(world: &mut World) {
    let Some(request) = world.resource_mut::<Saves>().request.take() else {
        return;
    };
    let outcome = match request {
        Request::Save(slot) => Some(match save(world, &slot) {
            Ok(path) => {
                info!("saved the slot `{slot}` in {}", path.display());
                SaveEvent::Saved { slot }
            }
            Err(reason) => {
                warn!("the slot `{slot}` was not saved: {reason}");
                SaveEvent::SaveFailed { slot, reason }
            }
        }),
        Request::Load(slot) => load(world, slot),
    };
    if let Some(outcome) = outcome {
        world.resource_mut::<Saves>().outbox.push(outcome);
    }
}

/// Saves `world` in the slot `slot`, and returns its file.
fn save(world: &World, slot: &str) -> Result<PathBuf, String> {
    if world.resource::<StateMachine>().is(StateKind::Loading) {
        return Err("the game is loading a level".to_string());
    }
    let path = slot_path(world.resource::<Saves>().folder(), slot)?;
    check_parents(world)?;
    let level = world
        .resource::<CurrentLevel>()
        .name()
        .unwrap_or_default()
        .to_string();
    world.resource_mut::<SavedLevel>().0 = level;
    let source = path.display().to_string();
    let data = {
        let registry = world.resource::<TypeRegistry>();
        reflect_context(world, &source, |mut cx| capture(world, &registry, &mut cx))
            .map_err(|error| error.to_string())?
    };
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)
            .map_err(|error| format!("{} cannot be made: {error}", folder.display()))?;
    }
    let version = world.resource::<Saves>().game_version.clone();
    write_file(&path, &data, &version).map_err(|error| error.to_string())?;
    Ok(path)
}

/// Fails if a saved entity's [`Parent`] is an entity without a
/// [`SaveId`], or one despawned: the save would write it as `null`, and
/// could not be loaded.
fn check_parents(world: &World) -> Result<(), String> {
    let ids = world.read::<SaveId>();
    let parents = world.read::<Parent>();
    for (child, parent) in parents.iter() {
        if let Some(id) = ids.get(child)
            && !ids.contains(parent.entity)
        {
            return Err(format!(
                "the entity of save id {}, which is saved, has a `Parent` whose entity has no \
                 `SaveId`, or is despawned: give the parent one, or the child none",
                id.0
            ));
        }
    }
    Ok(())
}

/// Reads the slot `slot`, then goes to the loading of its level, or, for a
/// save of no level, restores it at once: the outcome then, or `None` if
/// the load waits for its level.
fn load(world: &mut World, slot: String) -> Option<SaveEvent> {
    let failed = |slot: String, reason: String, missing: bool| {
        warn!("the slot `{slot}` was not loaded: {reason}");
        Some(SaveEvent::LoadFailed {
            slot,
            reason,
            missing,
        })
    };
    if world.resource::<StateMachine>().is(StateKind::Loading) {
        return failed(slot, "the game is loading a level".to_string(), false);
    }
    let path = match slot_path(world.resource::<Saves>().folder(), &slot) {
        Ok(path) => path,
        Err(reason) => return failed(slot, reason, false),
    };
    let save = match read_file(&path, Limits::default()) {
        Ok(save) => save,
        Err(error) => return failed(slot, error.to_string(), error.is_missing()),
    };
    let level = match saved_level(&save.data) {
        Ok(level) => level,
        Err(reason) => return failed(slot, reason, false),
    };
    match level {
        Some(level) => {
            info!("loading the slot `{slot}`: its level `{level}` first");
            world
                .resource_mut::<StateMachine>()
                .set(GameState::Loading(LevelId::new(level.clone())));
            world.resource_mut::<Saves>().waiting = Some(Waiting {
                slot,
                path,
                level,
                save,
            });
            None
        }
        None => Some(restored(world, slot, &path, save)),
    }
}

/// The level the save `data` names ([`SavedLevel`]): `None` for a save of
/// no level, or one written without it.
fn saved_level(data: &SaveData) -> Result<Option<String>, String> {
    let name = <SavedLevel as nadir_scene::FromValue>::TYPE_NAME;
    match data.globals.iter().find(|(global, _)| global == name) {
        None => Ok(None),
        Some((_, Value::String(level))) if level.is_empty() => Ok(None),
        Some((_, Value::String(level))) if level_path(level).is_some() => Ok(Some(level.clone())),
        Some((_, value)) => Err(format!(
            "its level, `globals.{name}`, is not a level's name: {}",
            nadir_codec::json::to_string_compact(value)
        )),
    }
}

/// After a transition: restores the load that waits for its level, if the
/// game entered [`GameState::InGame`] from that level's loading, after its
/// enter systems; or lets go of it, as failed, if the game went elsewhere.
pub(crate) fn transitioned(world: &mut World, transition: &Transition) {
    let Some(level) = world
        .resource::<Saves>()
        .waiting
        .as_ref()
        .map(|waiting| waiting.level.clone())
    else {
        return;
    };
    let of_level =
        |state: &GameState| matches!(state, GameState::Loading(id) if id.as_str() == level);
    if of_level(&transition.to) {
        return;
    }
    let Some(waiting) = world.resource_mut::<Saves>().waiting.take() else {
        return;
    };
    let outcome = match (&transition.from, &transition.to) {
        (Some(from), GameState::InGame) if of_level(from) => {
            restored(world, waiting.slot, &waiting.path, waiting.save)
        }
        _ => {
            let reason = format!(
                "the game went to {} before the level `{level}` loaded",
                transition.to
            );
            warn!("the slot `{}` was not loaded: {reason}", waiting.slot);
            SaveEvent::LoadFailed {
                slot: waiting.slot,
                reason,
                missing: false,
            }
        }
    };
    world.resource_mut::<Saves>().outbox.push(outcome);
}

/// Restores `save`, of the slot `slot` read from `path`: the outcome.
fn restored(world: &mut World, slot: String, path: &Path, save: LoadedSave) -> SaveEvent {
    let LoadedSave {
        data, from_backup, ..
    } = save;
    match restore_now(world, path, data) {
        Ok(count) => {
            info!("loaded the slot `{slot}`: {count} entities");
            SaveEvent::Loaded {
                slot,
                from_backup: from_backup.map(|error| error.to_string()),
            }
        }
        Err(reason) => {
            warn!("the slot `{slot}` was not loaded: {reason}");
            SaveEvent::LoadFailed {
                slot,
                reason,
                missing: false,
            }
        }
    }
}

/// Puts the save `data`, read from `path`, into `world`, with the
/// level's overrides of its entities under their saved values; returns how
/// many entities it spawned.
fn restore_now(world: &mut World, path: &Path, mut data: SaveData) -> Result<usize, String> {
    let source = path.display().to_string();
    let scope = {
        let current = world.resource::<CurrentLevel>();
        for entity in &mut data.entities {
            if let Some(overrides) = current.overrides_of(entity.id) {
                let saved = std::mem::take(&mut entity.components);
                entity.components = overrides.to_vec();
                entity.components.extend(saved);
            }
        }
        // A save's assets go with its level, or stay, with none.
        if current.name().is_some() {
            Scope::Level
        } else {
            Scope::Global
        }
    };
    {
        let mut server = world.resource_mut::<AssetServer>();
        let registry = world.resource::<TypeRegistry>();
        world
            .resource_mut::<PrefabLibrary>()
            .load_save(&mut server, &registry, &data, scope)
            .map_err(|error| error.to_string())?;
    }
    let spawned = {
        let library = world.resource::<PrefabLibrary>();
        let registry = world.resource::<TypeRegistry>();
        loading_context(world, &source, scope, |mut cx| {
            restore(&data, world, &registry, &library, &mut cx)
        })
        .map_err(|error| error.to_string())?
    };
    world.apply_commands();
    let restored: Vec<(u64, Entity)> = data
        .entities
        .iter()
        .map(|entity| entity.id)
        .zip(spawned.iter().copied())
        .collect();
    crate::level::adopt_restored(world, &restored);
    Ok(spawned.len())
}

#[cfg(test)]
mod tests;
