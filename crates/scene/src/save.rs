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

//! Saves (roadmap M9.4, ADR-0068): the entities that have a [`SaveId`] and
//! the resources registered as saved, turned into a save file's payload
//! ([`capture`]) and back into the world ([`restore`]); the file written
//! atomically, beside a backup ([`write_file`]), and read back, from the
//! backup when the file is damaged ([`read_file`]); and the migrations from
//! older versions of the payload ([`migrate`]).
//!
//! The file's bytes are `nadir-codec`'s ([`ksav`]): a header with the
//! game's version, the time and a CRC-32, and a KBJ payload of
//! [`SaveData`].

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use nadir_codec::json::Value;
use nadir_codec::ksav::{self, SaveData, SavedEntity};
use nadir_codec::{CodecError, Limits};
use nadir_core::FastHashMap;
use nadir_core::log::warn;
use nadir_ecs::{Component, Entity, World};

use crate::prefab::Made;
use crate::{
    FieldInfo, FromValue, Layer, PrefabError, PrefabLibrary, Reflect, ReflectContext, ReflectError,
    ReflectErrorKind, ReflectKind, TypeRegistry,
};

crate::reflect_newtype! {
    /// An entity's id in saves, which stays the same from one run to the
    /// next, unlike its [`Entity`]. Saves store only entities that have one
    /// ([`capture`]), and an [`Entity`] in a saved value as its entity's id.
    /// [`SaveIds`] gives out new ones.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct SaveId(pub u64);
}

impl Component for SaveId {}

/// The prefab an entity was spawned of: [`PrefabLibrary::spawn`] and
/// [`PrefabLibrary::spawn_level`] give it to every entity they spawn. A save
/// writes it as the entity's `prefab`, and a load spawns the entity of that
/// prefab again, with its saved components applied over the prefab's, so
/// that what only the prefab gives, such as a sprite, need not be saved.
///
/// It is not reflected, and not in the [`TypeRegistry`]: prefabs, levels
/// and the world hash do not see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnedFrom(pub String);

impl Component for SpawnedFrom {}

/// The world's counter of save ids, a resource: [`SaveIds::assign`] gives
/// each entity to be saved an id no other has had. A save keeps where it
/// is, and a load puts it back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveIds {
    next: u64,
}

impl SaveIds {
    /// A counter that gives out 1 first.
    pub fn new() -> Self {
        Self::starting_at(1)
    }

    /// A counter that gives out `next` first.
    pub fn starting_at(next: u64) -> Self {
        Self { next }
    }

    /// The id given out next.
    pub fn next_id(&self) -> u64 {
        self.next
    }

    /// A new id: the one after the last given out. At `u64::MAX` it stays
    /// there, so that the ids from then on are one, which [`capture`]
    /// refuses.
    pub fn assign(&mut self) -> SaveId {
        let id = SaveId(self.next);
        self.next = self.next.saturating_add(1);
        id
    }
}

impl Default for SaveIds {
    fn default() -> Self {
        Self::new()
    }
}

/// Which entity has which save id, both ways: what a [`ReflectContext`]
/// holds to write an [`Entity`] as its save id and to read one back
/// ([`ReflectContext::with_save_ids`]). [`capture`] and [`restore`] give
/// their context one of their own while they run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveIdMap {
    ids: FastHashMap<Entity, u64>,
    entities: FastHashMap<u64, Entity>,
}

impl SaveIdMap {
    /// No entities.
    pub fn new() -> Self {
        Self::default()
    }

    /// The map of every entity in `world` that has a [`SaveId`]; none if
    /// the world has not registered the type.
    ///
    /// # Errors
    ///
    /// [`SaveError::DuplicateId`] if two entities have one id.
    pub fn of_world(world: &World) -> Result<Self, SaveError> {
        let mut map = Self::new();
        if !world.is_registered::<SaveId>() {
            return Ok(map);
        }
        for (entity, &SaveId(id)) in world.read::<SaveId>().iter() {
            if let Some(other) = map.insert(entity, id) {
                let mut entities = [other, entity];
                entities.sort_by_key(|entity| (entity.index(), entity.generation()));
                return Err(SaveError::DuplicateId { id, entities });
            }
        }
        Ok(map)
    }

    /// Gives `entity` the save id `id`. Returns the entity that had `id`
    /// before, which no longer has it. An id `entity` had before is
    /// forgotten.
    pub fn insert(&mut self, entity: Entity, id: u64) -> Option<Entity> {
        if let Some(old) = self.ids.insert(entity, id) {
            self.entities.remove(&old);
        }
        let other = self.entities.insert(id, entity)?;
        if other == entity {
            return None;
        }
        self.ids.remove(&other);
        Some(other)
    }

    /// The save id of `entity`, if it has one.
    pub fn id(&self, entity: Entity) -> Option<u64> {
        self.ids.get(&entity).copied()
    }

    /// The entity of save id `id`, if one has it.
    pub fn entity(&self, id: u64) -> Option<Entity> {
        self.entities.get(&id).copied()
    }

    /// How many entities it has.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether it has none.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Every entity and its id, by id.
    fn by_id(&self) -> Vec<(u64, Entity)> {
        let mut entities: Vec<_> = self.ids.iter().map(|(&e, &id)| (id, e)).collect();
        entities.sort_unstable_by_key(|&(id, _)| id);
        entities
    }
}

/// An [`Entity`] is written as its entity's save id, through the context's
/// [`SaveIdMap`] ([`ReflectContext::save_ids`]), as a `u64` is (ADR-0020).
/// One without an id is written as `null`, with a warning, as only entities
/// with one are saved; for a hash ([`ReflectContext::hashing`]), as its
/// index and generation, `[index, generation]`. Reading one needs the map,
/// and an id it has.
impl Reflect for Entity {
    fn type_name(&self) -> &'static str {
        "Entity"
    }

    fn kind(&self) -> ReflectKind {
        ReflectKind::Primitive
    }

    fn fields(&self) -> &'static [FieldInfo] {
        &[]
    }

    fn field(&self, _index: usize) -> Option<&dyn Reflect> {
        None
    }

    fn field_mut(&mut self, _index: usize) -> Option<&mut dyn Reflect> {
        None
    }

    fn to_value(&self, cx: &mut ReflectContext<'_>) -> Value {
        if let Some(id) = cx.save_ids().and_then(|ids| ids.id(*self)) {
            return ksav::id_value(id);
        }
        if cx.is_hashing() {
            return Value::Array(vec![
                Value::Int(i64::from(self.index())),
                Value::Int(i64::from(self.generation().get())),
            ]);
        }
        cx.warn(
            "an entity without a `SaveId` is written as null: only entities with one are saved",
        );
        Value::Null
    }

    fn apply_value(
        &mut self,
        value: &Value,
        cx: &mut ReflectContext<'_>,
    ) -> Result<(), ReflectError> {
        *self = Self::from_value(value, cx)?;
        Ok(())
    }
}

impl FromValue for Entity {
    const TYPE_NAME: &'static str = "Entity";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        if cx.save_ids().is_none() {
            return Err(cx.error(ReflectErrorKind::NoContext {
                needs: "the save ids of the entities, to read an entity",
            }));
        }
        let id = u64::from_value(value, cx)?;
        match cx.save_ids().and_then(|ids| ids.entity(id)) {
            Some(entity) => Ok(entity),
            None => Err(cx.error(ReflectErrorKind::UnknownEntity { id })),
        }
    }
}

/// Turns a save's payload of an older version into the next version's, in
/// place. A migration need not check the payload: [`ksav::read`] checks it
/// once the last has run.
pub type Migration = fn(&mut Value);

/// The engine's migrations of the payload, in order: the one at index `i`
/// turns version `i + 1` into `i + 2`. None yet, as version 1 is the only
/// one; each version shipped keeps a fixture its tests load
/// (`crates/scene/tests/saves/`).
pub const MIGRATIONS: &[Migration] = &[];

const _: () = assert!(
    MIGRATIONS.len() + 1 == ksav::VERSION as usize,
    "each version before ksav::VERSION needs its migration"
);

/// Brings `payload`, of `version`, to the newest version `migrations`
/// know, `migrations.len() + 1`, running each from `version` on, in order.
/// [`read_file`] runs the engine's, [`MIGRATIONS`].
///
/// # Errors
///
/// [`SaveError::Version`] if `version` is 0 or newer than the newest.
pub fn migrate(
    payload: &mut Value,
    version: u16,
    migrations: &[Migration],
) -> Result<(), SaveError> {
    let newest = migrations.len() + 1;
    let from = usize::from(version);
    if from == 0 || from > newest {
        return Err(SaveError::Version {
            found: version,
            newest,
        });
    }
    for migration in &migrations[from - 1..] {
        migration(payload);
    }
    Ok(())
}

/// The payload of a save of `world`: each entity with a [`SaveId`], by
/// id, with the prefab it was spawned of ([`SpawnedFrom`]) and its
/// components of the types registered as saved
/// ([`TypeRegistry::register_saved_component`]), in the registry's order;
/// each resource registered as saved
/// ([`TypeRegistry::register_saved_resource`]) that the world has; and
/// where the world's [`SaveIds`] is, or, if it has none, the id after the
/// largest saved. A [`SaveId`] is the entity's id, never one of its
/// components.
///
/// The values are written with `cx`, given a [`SaveIdMap`] of the world
/// while it runs: give it the asset server for handles
/// ([`ReflectContext::with_assets`]) and the resources the types need
/// ([`ReflectContext::with_resource`]). An [`Entity`] without a
/// [`SaveId`] is written as `null`, with a warning. The same world gives
/// the same payload.
///
/// # Errors
///
/// [`SaveError::DuplicateId`] if two entities have one id.
///
/// # Panics
///
/// If a saved type is not registered with `world`, or its components or
/// resource are borrowed mutably elsewhere, as any read of them would.
pub fn capture(
    world: &World,
    registry: &TypeRegistry,
    cx: &mut ReflectContext<'_>,
) -> Result<SaveData, SaveError> {
    let ids = SaveIdMap::of_world(world)?;
    let order = ids.by_id();
    let next_id = if world.contains_resource::<SaveIds>() {
        world.resource::<SaveIds>().next_id()
    } else {
        order
            .last()
            .map_or(1, |&(largest, _)| largest.saturating_add(1))
    };
    let outer = cx.replace_save_ids(Some(ids));
    let mut data = SaveData::new(next_id);
    cx.field("globals", |cx| {
        for registration in registry.resources() {
            if let Some(value) = cx.key(registration.name(), |cx| registration.write(world, cx)) {
                data.globals.push((registration.name().to_string(), value));
            }
        }
    });
    let spawned_from = world.is_registered::<SpawnedFrom>();
    let saved: Vec<_> = registry
        .iter()
        .filter(|registration| {
            registration.is_saved() && registration.type_id() != std::any::TypeId::of::<SaveId>()
        })
        .collect();
    cx.field("entities", |cx| {
        for (index, &(id, entity)) in order.iter().enumerate() {
            let mut saved_entity = SavedEntity::new(id);
            if spawned_from {
                saved_entity.prefab = world
                    .read::<SpawnedFrom>()
                    .get(entity)
                    .map(|from| from.0.clone());
            }
            cx.index(index, |cx| {
                cx.field("components", |cx| {
                    for registration in &saved {
                        let mut value = None;
                        cx.key(registration.name(), |cx| {
                            registration.inspect(world, entity, &mut |component| {
                                value = Some(component.to_value(cx));
                            });
                        });
                        if let Some(value) = value {
                            saved_entity
                                .components
                                .push((registration.name().to_string(), value));
                        }
                    }
                });
            });
            data.entities.push(saved_entity);
        }
    });
    cx.replace_save_ids(outer);
    Ok(data)
}

/// Puts the save `data` into `world`, through its commands, and returns
/// the entities it spawns, in the save's order. Load the prefabs it names
/// first ([`PrefabLibrary::load_save`]).
///
/// Every entity of the save is spawned anew, alive at once, and given a
/// [`SaveIdMap`] in `cx` while the values are read, so that an [`Entity`]
/// in them is the new entity of its id. One of a prefab is made of it as
/// [`PrefabLibrary::spawn`] makes one, its saved components applied over
/// the prefab's values, and keeps its [`SpawnedFrom`]: a component the
/// prefab gives comes back, even one of a saved type that the entity had
/// lost. One of none is made of its saved components alone. Each gets its
/// [`SaveId`]. Every resource
/// of the save replaces the world's, which must have one, at once. The
/// world's [`SaveIds`], if it has one, goes on from the save's.
///
/// Every value is made before anything changes, so a save that fails
/// changes nothing, but for the entities spawned for it, which are
/// despawned with the commands. Otherwise, the entities that had a
/// [`SaveId`] before are despawned, and the new ones get their components,
/// when the commands are applied: a load gives the saved entities new
/// [`Entity`]s, so hold their [`SaveId`]s across one. Entities without a
/// [`SaveId`], such as a level's scenery, stay.
///
/// `cx` gives what the values need, as for [`PrefabLibrary::spawn`]: the
/// asset server and the scope to load handles in, and the resources the
/// types need, which must not be among those the save replaces.
///
/// # Errors
///
/// [`SaveError::DuplicateId`] if two entities of `data` have one id (a
/// save read from a file has none); [`SaveError::UnknownResource`] and
/// [`SaveError::NoResource`] for a resource that is not registered or that
/// the world has not got; [`SaveError::UnknownComponent`] for a component
/// of an entity of no prefab that the registry has not got;
/// [`SaveError::Prefab`] for an entity whose prefab fails, as
/// [`PrefabLibrary::spawn`] says; and [`SaveError::Reflect`] for a value
/// that does not fit its type, at `entities[<i>].components.<Type>` or
/// `globals.<Type>` under `cx`'s source.
///
/// # Panics
///
/// If a type of the save, [`SaveId`] or [`SpawnedFrom`] is not registered
/// with `world` ([`TypeRegistry::register_components`] registers them), or
/// a resource it replaces is borrowed.
pub fn restore(
    data: &SaveData,
    world: &World,
    registry: &TypeRegistry,
    library: &PrefabLibrary,
    cx: &mut ReflectContext<'_>,
) -> Result<Vec<Entity>, SaveError> {
    let mut commands = world.commands();
    let mut ids = SaveIdMap::new();
    let mut spawned = Vec::with_capacity(data.entities.len());
    let mut duplicate = None;
    for saved in &data.entities {
        let entity = commands.spawn().id();
        spawned.push(entity);
        if let Some(other) = ids.insert(entity, saved.id) {
            duplicate.get_or_insert(SaveError::DuplicateId {
                id: saved.id,
                entities: [other, entity],
            });
        }
    }
    let outer = cx.replace_save_ids(Some(ids));
    let made = match duplicate {
        Some(error) => Err(error),
        None => make(data, world, registry, library, cx),
    };
    cx.replace_save_ids(outer);
    let (globals, entities) = match made {
        Ok(made) => made,
        Err(error) => {
            for entity in spawned {
                commands.despawn(entity);
            }
            return Err(error);
        }
    };

    if world.is_registered::<SaveId>() {
        for &entity in world.read::<SaveId>().entities() {
            commands.despawn(entity);
        }
    }
    for ((saved, &entity), components) in data.entities.iter().zip(&spawned).zip(entities) {
        commands.insert(entity, SaveId(saved.id));
        for (registration, value) in components {
            registration.insert_made(&mut commands, entity, value);
        }
        if let Some(prefab) = &saved.prefab {
            commands.insert(entity, SpawnedFrom(prefab.clone()));
        }
    }
    for (registration, value) in globals {
        registration.replace(world, value);
    }
    if world.contains_resource::<SaveIds>() {
        *world.resource_mut::<SaveIds>() = SaveIds::starting_at(data.next_id);
    }
    Ok(spawned)
}

/// A save's resources and entities' components, made.
type MadeSave<'r> = (
    Vec<(&'r crate::ResourceRegistration, Box<dyn std::any::Any>)>,
    Vec<Vec<Made<'r>>>,
);

/// Every value of `data`, made with `cx`, which has the save's ids.
fn make<'r>(
    data: &SaveData,
    world: &World,
    registry: &'r TypeRegistry,
    library: &PrefabLibrary,
    cx: &mut ReflectContext<'_>,
) -> Result<MadeSave<'r>, SaveError> {
    let mut globals = Vec::with_capacity(data.globals.len());
    for (name, value) in &data.globals {
        let Some(registration) = registry.resource(name) else {
            return Err(SaveError::UnknownResource { name: name.clone() });
        };
        if !registration.is_in(world) {
            return Err(SaveError::NoResource { name: name.clone() });
        }
        let made = cx
            .field("globals", |cx| {
                cx.key(name, |cx| registration.make(value, cx))
            })
            .map_err(SaveError::Reflect)?;
        globals.push((registration, made));
    }
    let mut entities = Vec::with_capacity(data.entities.len());
    cx.field("entities", |cx| {
        for (index, saved) in data.entities.iter().enumerate() {
            let components = cx.index(index, |cx| match &saved.prefab {
                Some(prefab) => library
                    .make(prefab, &saved.components, "components", registry, cx)
                    .map_err(SaveError::Prefab),
                None => make_components(saved, registry, cx),
            })?;
            entities.push(components);
        }
        Ok::<(), SaveError>(())
    })?;
    Ok((globals, entities))
}

/// The components of `saved`, an entity of no prefab, made of its values
/// alone.
fn make_components<'r>(
    saved: &SavedEntity,
    registry: &'r TypeRegistry,
    cx: &mut ReflectContext<'_>,
) -> Result<Vec<Made<'r>>, SaveError> {
    let source = cx.source().to_string();
    let outer = cx.at();
    let mut made = Vec::with_capacity(saved.components.len());
    for (name, value) in &saved.components {
        let Some(registration) = registry.get(name) else {
            return Err(SaveError::UnknownComponent {
                id: saved.id,
                component: name.clone(),
            });
        };
        let layer = Layer {
            source: &source,
            at: format!("{outer}.components.{name}"),
            value,
        };
        let value = registration
            .make_layers(&[layer], cx)
            .map_err(SaveError::Reflect)?;
        made.push((registration, value));
    }
    Ok(made)
}

/// A save read from its file by [`read_file`]: its header, and its payload,
/// migrated to the newest version and checked.
#[derive(Debug)]
#[non_exhaustive]
pub struct LoadedSave {
    /// The file's header: the version it was written in, the game's
    /// version and when.
    pub header: ksav::Header,
    /// The save.
    pub data: SaveData,
    /// Why the file itself could not be read, when this came from its
    /// backup: tell the player that the last save was lost.
    pub from_backup: Option<SaveError>,
}

/// The backup of the save file `path`: `slot1.ksav.bak` for `slot1.ksav`.
pub fn backup_path(path: &Path) -> PathBuf {
    with_suffix(path, ".bak")
}

/// The file [`write_file`] writes the save file `path` to first:
/// `slot1.ksav.tmp` for `slot1.ksav`.
pub fn temp_path(path: &Path) -> PathBuf {
    with_suffix(path, ".tmp")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Writes `data` to the save file `path`, of the game's `game_version`, at
/// the time it is now, so that a crash at any point leaves a save that
/// reads: first to [`temp_path`], flushed to the disk
/// ([`File::sync_all`]); then the save that was at `path`, if any, is
/// renamed [`backup_path`], replacing the backup before it; then the new
/// file is renamed `path`. A rename replaces its target on every OS.
///
/// The file is read back from its bytes, as [`read_file`] would, before
/// any is written: a save that would not read is never written.
///
/// # Errors
///
/// [`SaveError::Codec`] if `data` cannot be written as a save file, or
/// would not read back with the default [`Limits`]: a prefab's name that is
/// not one, say; [`SaveError::Io`] naming the file of the step that failed.
/// A failure before the last rename leaves the save before at `path` or
/// its backup.
pub fn write_file(path: &Path, data: &SaveData, game_version: &str) -> Result<(), SaveError> {
    let limits = Limits::default();
    let codec = |error| SaveError::Codec {
        path: path.to_path_buf(),
        error,
    };
    let bytes = ksav::encode(game_version, now_utc(), &data.to_value(), limits).map_err(codec)?;
    let (_, payload) = ksav::parse(&bytes, limits).map_err(codec)?;
    ksav::read(&payload).map_err(codec)?;

    let temp = temp_path(path);
    if let Err(error) = write_synced(&temp, &bytes) {
        // What was written is of no use; the next save truncates it anyway.
        let _ = fs::remove_file(&temp);
        return Err(SaveError::Io { path: temp, error });
    }
    match fs::rename(path, backup_path(path)) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(SaveError::Io {
                path: path.to_path_buf(),
                error,
            });
        }
    }
    fs::rename(&temp, path).map_err(|error| SaveError::Io { path: temp, error })
}

/// Writes `bytes` to the file `path`, replacing it, and flushes it to the
/// disk.
fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// The time it is now, in seconds since 1970-01-01 00:00:00 UTC.
fn now_utc() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => i64::try_from(before.duration().as_secs()).map_or(i64::MIN, |s| -s),
    }
}

/// Reads the save file `path`, within `limits`: its header, then its
/// payload, migrated from the file's version by [`MIGRATIONS`] and checked
/// ([`ksav::read`]). If the file cannot be read (it is damaged, cut short,
/// missing, or not a save the engine can migrate), its backup
/// ([`backup_path`]) is read instead, with a warning in the log, and
/// [`LoadedSave::from_backup`] says why.
///
/// # Errors
///
/// When the file cannot be read and has no backup, its error:
/// [`SaveError::Io`] ([`SaveError::is_missing`] if there is no save), or
/// [`SaveError::Codec`]. When neither can be read,
/// [`SaveError::Unreadable`] with both errors.
pub fn read_file(path: &Path, limits: Limits) -> Result<LoadedSave, SaveError> {
    let main_error = match read_one(path, limits) {
        Ok((header, data)) => {
            return Ok(LoadedSave {
                header,
                data,
                from_backup: None,
            });
        }
        Err(error) => error,
    };
    let backup = backup_path(path);
    match read_one(&backup, limits) {
        Ok((header, data)) => {
            warn!(
                "{main_error}; the backup {} was read instead",
                backup.display()
            );
            Ok(LoadedSave {
                header,
                data,
                from_backup: Some(main_error),
            })
        }
        Err(backup_error) if backup_error.is_missing() => Err(main_error),
        Err(backup_error) => Err(SaveError::Unreadable {
            main: Box::new(main_error),
            backup: Box::new(backup_error),
        }),
    }
}

/// Reads one save file, as [`read_file`] says, without its backup.
fn read_one(path: &Path, limits: Limits) -> Result<(ksav::Header, SaveData), SaveError> {
    let codec = |error| SaveError::Codec {
        path: path.to_path_buf(),
        error,
    };
    let bytes = fs::read(path).map_err(|error| SaveError::Io {
        path: path.to_path_buf(),
        error,
    })?;
    let (header, mut payload) = ksav::parse(&bytes, limits).map_err(codec)?;
    migrate(&mut payload, header.version, MIGRATIONS)?;
    let data = ksav::read(&payload).map_err(codec)?;
    Ok((header, data))
}

/// Why a save could not be written, read or put into the world.
#[derive(Debug)]
#[non_exhaustive]
pub enum SaveError {
    /// A file could not be read, written or renamed.
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        error: io::Error,
    },
    /// A save file is damaged, cut short, of a version not supported, or
    /// its payload is not a save's; or a save cannot be written as one.
    Codec {
        /// The file.
        path: PathBuf,
        /// What is wrong.
        error: CodecError,
    },
    /// A payload's version is 0, or newer than the migrations know.
    Version {
        /// The version.
        found: u16,
        /// The newest the migrations know.
        newest: usize,
    },
    /// Two entities have one save id.
    DuplicateId {
        /// The id.
        id: u64,
        /// The two entities.
        entities: [Entity; 2],
    },
    /// A component of a saved entity of no prefab is of no type the
    /// registry has.
    UnknownComponent {
        /// The entity's save id.
        id: u64,
        /// The component's name.
        component: String,
    },
    /// A saved resource is of no type the registry has as a resource.
    UnknownResource {
        /// The resource's name.
        name: String,
    },
    /// A saved resource is of a type the world has none of, to replace.
    NoResource {
        /// The resource's name.
        name: String,
    },
    /// An entity of a prefab could not be made of it.
    Prefab(PrefabError),
    /// A saved value does not fit its type.
    Reflect(ReflectError),
    /// Neither a save file nor its backup could be read.
    Unreadable {
        /// Why the file could not.
        main: Box<SaveError>,
        /// Why its backup could not.
        backup: Box<SaveError>,
    },
}

impl SaveError {
    /// Whether this is a file that is not there: the slot is empty.
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Io { error, .. } if error.kind() == io::ErrorKind::NotFound)
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Codec { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Version { found, newest } => write!(
                f,
                "version {found} of a save cannot be migrated: the migrations go from 1 to {newest}"
            ),
            Self::DuplicateId { id, entities } => write!(
                f,
                "{:?} and {:?} both have the save id {id}",
                entities[0], entities[1]
            ),
            Self::UnknownComponent { id, component } => write!(
                f,
                "the saved entity {id} has a component `{component}`, which is not registered"
            ),
            Self::UnknownResource { name } => {
                write!(f, "the saved resource `{name}` is not registered as saved")
            }
            Self::NoResource { name } => write!(
                f,
                "the world has no resource `{name}` for the saved one to replace"
            ),
            Self::Prefab(error) => write!(f, "{error}"),
            Self::Reflect(error) => write!(f, "{error}"),
            Self::Unreadable { main, backup } => {
                write!(f, "{main}; and its backup: {backup}")
            }
        }
    }
}

impl std::error::Error for SaveError {}
