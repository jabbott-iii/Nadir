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

//! Saves (roadmap M9.4, ADR-0068), as the scene spec's tests ask: a world
//! written to a file and read back into another; a damaged file that falls
//! back to its backup; the version 1 fixture, `tests/saves/v1.ksav`, loaded,
//! and written again by today's writer; the migrations' order; and a load
//! that fails, which changes nothing. The reader's fuzz target is
//! `cargo xtask fuzz-smoke --target ksav`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use nadir_codec::json::{self, Value};
use nadir_codec::ksav::{self, DataErrorKind, ErrorKind};
use nadir_codec::{CodecError, Limits, prefab};
use nadir_ecs::{Component, Entity, World};
use nadir_scene::{
    FromValue, Migration, PrefabError, PrefabLibrary, Reflect, ReflectContext, ReflectErrorKind,
    SaveData, SaveError, SaveId, SaveIdMap, SaveIds, SavedEntity, SpawnedFrom, TypeRegistry,
    backup_path, capture, migrate, read_file, restore, temp_path, write_file,
};

nadir_scene::reflect_struct! {
    /// Saved: hit points.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Health {
        current: i32,
        max: i32,
    }
}

nadir_scene::reflect_struct! {
    /// Saved: another entity, which may be gone.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Follow {
        target: Option<Entity>,
    }
}

nadir_scene::reflect_struct! {
    /// Not saved: what only a prefab gives.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Look {
        shade: u8,
    }
}

nadir_scene::reflect_struct! {
    /// Not saved: transient, rebuilt after a load.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Speed {
        value: f32,
    }
}

nadir_scene::reflect_struct! {
    /// A saved resource.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Score {
        points: u32,
        best: u32,
    }
}

impl Component for Health {}
impl Component for Follow {}
impl Component for Look {}
impl Component for Speed {}

/// The fixture's game version and time, which `make_fixture.py` gives it.
const FIXTURE_VERSION: &str = "0.4.0";
const FIXTURE_TIME: i64 = 1_795_000_000;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/saves/v1.ksav")
}

fn types() -> TypeRegistry {
    let mut registry = TypeRegistry::new();
    registry.register_saved_component::<Health>();
    registry.register_saved_component::<Follow>();
    registry.register_component::<Look>();
    registry.register_component::<Speed>();
    registry.register_saved_resource::<Score>();
    registry
}

/// A world of the test's types, with a `Score` and a `SaveIds`.
fn a_world(registry: &TypeRegistry) -> World {
    let mut world = World::new();
    registry.register_components(&mut world);
    world.insert_resource(Score::default());
    world.insert_resource(SaveIds::new());
    world
}

fn library() -> PrefabLibrary {
    let text = r#"{ "name": "slime", "components": {
        "Look": { "shade": 3 }, "Health": { "current": 12, "max": 12 } } }"#;
    let value = json::parse(text, Limits::default()).expect("the test's JSON");
    let file = prefab::read(&value).expect("a prefab");
    let mut library = PrefabLibrary::new();
    library.insert("data/prefabs/slime.kbj", file);
    library
}

fn object(members: &[(&str, Value)]) -> Value {
    Value::Object(
        members
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect(),
    )
}

/// The fixture's world: a hero of no prefab with a speed that is not saved,
/// following a slime of a prefab, which was hurt; an entity whose target
/// is gone, with the largest id; and scenery with no id. Returns the hero,
/// the slime and the last.
fn the_fixtures_world(registry: &TypeRegistry, library: &PrefabLibrary) -> (World, [Entity; 3]) {
    let mut world = a_world(registry);
    let mut cx = ReflectContext::new("test");
    let entities = {
        let mut commands = world.commands();
        let hurt = [("Health".to_string(), object(&[("current", Value::Int(2))]))];
        let slime = library
            .spawn("slime", &hurt, registry, &mut commands, &mut cx)
            .expect("the slime spawns");
        commands.insert(slime, SaveId(3));
        let hero = commands
            .spawn()
            .with(SaveId(1))
            .with(Health {
                current: 7,
                max: 10,
            })
            .with(Follow {
                target: Some(slime),
            })
            .with(Speed { value: 90.0 })
            .id();
        let last = commands
            .spawn()
            .with(SaveId(u64::MAX))
            .with(Follow { target: None })
            .id();
        commands.spawn().with(Look { shade: 9 });
        [hero, slime, last]
    };
    world.apply_commands();
    *world.resource_mut::<Score>() = Score {
        points: 40,
        best: 120,
    };
    *world.resource_mut::<SaveIds>() = SaveIds::starting_at(5);
    (world, entities)
}

/// Each entity of `world` with a save id, by id.
fn by_id(world: &World) -> SaveIdMap {
    SaveIdMap::of_world(world).expect("no id twice")
}

/// Checks that `world` holds the fixture's world, as a load makes it.
fn assert_is_the_fixtures_world(world: &World) {
    let ids = by_id(world);
    assert_eq!(ids.len(), 3);
    let [hero, slime, last] = [1, 3, u64::MAX].map(|id| ids.entity(id).expect("loaded"));
    let health = world.read::<Health>();
    assert_eq!(
        health.get(hero),
        Some(&Health {
            current: 7,
            max: 10
        })
    );
    assert_eq!(
        health.get(slime),
        Some(&Health {
            current: 2,
            max: 12
        })
    );
    assert_eq!(health.get(last), None);
    let follow = world.read::<Follow>();
    assert_eq!(
        follow.get(hero).map(|f| f.target),
        Some(Some(slime)),
        "the new slime"
    );
    assert_eq!(follow.get(last).map(|f| f.target), Some(None));
    // The prefab gave what was not saved, and is remembered.
    assert_eq!(world.read::<Look>().get(slime), Some(&Look { shade: 3 }));
    let from = world.read::<SpawnedFrom>();
    assert_eq!(from.get(slime), Some(&SpawnedFrom("slime".into())));
    assert_eq!(from.get(hero), None);
    // What is not saved is not there.
    assert_eq!(world.read::<Speed>().get(hero), None);
    assert_eq!(
        *world.resource::<Score>(),
        Score {
            points: 40,
            best: 120
        }
    );
    assert_eq!(world.resource::<SaveIds>().next_id(), 5);
}

/// A folder of this test's own, empty.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nadir-scene-saves-{test}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs() as i64
}

#[test]
fn a_world_saved_and_loaded_is_the_world_again() {
    let registry = types();
    let library = library();
    let (world, _) = the_fixtures_world(&registry, &library);
    let data = capture(&world, &registry, &mut ReflectContext::new("slot1.ksav")).unwrap();

    let dir = scratch("round-trip");
    let path = dir.join("slot1.ksav");
    let before = now();
    write_file(&path, &data, "1.2.3").unwrap();
    let after = now();
    assert!(!temp_path(&path).exists(), "the temporary file is renamed");
    assert!(!backup_path(&path).exists(), "no save was there before");
    let loaded = read_file(&path, Limits::default()).unwrap();
    assert_eq!(loaded.header.version, ksav::VERSION);
    assert_eq!(loaded.header.game_version, "1.2.3");
    assert!((before..=after).contains(&loaded.header.saved_at));
    assert!(loaded.from_backup.is_none());
    assert_eq!(loaded.data, data);

    // Into another world, whose saved entities make way and whose scenery
    // stays.
    let mut other = a_world(&registry);
    let scenery = other.spawn().with(Look { shade: 1 }).id();
    let old = other.spawn().with(SaveId(1)).with(Health::default()).id();
    let mut cx = ReflectContext::new("slot1.ksav");
    let spawned = restore(&loaded.data, &other, &registry, &library, &mut cx).unwrap();
    assert!(other.is_alive(old), "until the commands are applied");
    other.apply_commands();
    assert!(!other.is_alive(old));
    assert_eq!(other.read::<Look>().get(scenery), Some(&Look { shade: 1 }));
    assert_eq!(other.entity_count(), 4);
    let ids = by_id(&other);
    let order: Vec<_> = [1, 3, u64::MAX].map(|id| ids.entity(id).unwrap()).into();
    assert_eq!(spawned, order, "in the save's order");
    assert_is_the_fixtures_world(&other);
    // And it saves as the world it came from did.
    let again = capture(&other, &registry, &mut ReflectContext::new("again")).unwrap();
    assert_eq!(again, data);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_damaged_save_falls_back_to_its_backup() {
    let dir = scratch("backup");
    let path = dir.join("slot1.ksav");
    let backup = backup_path(&path);
    let limits = Limits::default();
    let missing = read_file(&path, limits).unwrap_err();
    assert!(missing.is_missing(), "an empty slot: {missing}");

    let first = SaveData::new(1);
    let second = SaveData::new(2);
    write_file(&path, &first, "1").unwrap();
    write_file(&path, &second, "2").unwrap();
    assert_eq!(read_file(&path, limits).unwrap().data, second);
    let good = fs::read(&path).unwrap();

    let reads_the_backup = |what: &str| {
        let loaded = read_file(&path, limits).unwrap_or_else(|error| panic!("{what}: {error}"));
        assert_eq!(loaded.data, first, "{what}");
        assert_eq!(loaded.header.game_version, "1", "{what}");
        loaded
            .from_backup
            .unwrap_or_else(|| panic!("{what}: not from the backup"))
    };

    let mut damaged = good.clone();
    *damaged.last_mut().unwrap() ^= 0x20;
    fs::write(&path, &damaged).unwrap();
    let why = reads_the_backup("a bit changed");
    assert!(
        matches!(
            why,
            SaveError::Codec {
                error: CodecError::Ksav {
                    kind: ErrorKind::Damaged { .. },
                    ..
                },
                ..
            }
        ),
        "{why}"
    );
    assert!(why.to_string().contains("the file is damaged"), "{why}");

    fs::write(&path, &good[..good.len() - 3]).unwrap();
    reads_the_backup("cut short");
    fs::write(&path, b"").unwrap();
    reads_the_backup("empty");
    // A crash between the two renames leaves no file, and its backup.
    fs::remove_file(&path).unwrap();
    assert!(reads_the_backup("missing").is_missing());

    fs::write(&path, &damaged).unwrap();
    fs::write(&backup, &good[..10]).unwrap();
    match read_file(&path, limits) {
        Err(SaveError::Unreadable { main, backup }) => {
            assert!(matches!(*main, SaveError::Codec { .. }), "{main}");
            assert!(matches!(*backup, SaveError::Codec { .. }), "{backup}");
        }
        other => panic!("expected neither to read, found {other:?}"),
    }
    // The next save goes on as before: it writes the file, and the damaged
    // one becomes the backup.
    write_file(&path, &first, "3").unwrap();
    assert_eq!(read_file(&path, limits).unwrap().data, first);
    assert_eq!(fs::read(&backup).unwrap(), damaged);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_version_1_fixture_loads() {
    let loaded = read_file(&fixture(), Limits::default()).unwrap();
    assert_eq!(loaded.header.version, 1);
    assert_eq!(loaded.header.game_version, FIXTURE_VERSION);
    assert_eq!(loaded.header.saved_at, FIXTURE_TIME);
    assert!(loaded.from_backup.is_none());
    let registry = types();
    let library = library();
    let world = a_world(&registry);
    let mut cx = ReflectContext::new("v1.ksav");
    restore(&loaded.data, &world, &registry, &library, &mut cx).unwrap();
    let mut world = world;
    world.apply_commands();
    assert_is_the_fixtures_world(&world);
}

#[test]
fn todays_writer_writes_the_version_1_fixture() {
    // While version 1 is the newest: once there is a version 2, this test
    // moves to its fixture, and version 1's is only loaded.
    assert_eq!(ksav::VERSION, 1);
    let registry = types();
    let (world, _) = the_fixtures_world(&registry, &library());
    let data = capture(&world, &registry, &mut ReflectContext::new("test")).unwrap();
    let bytes = ksav::encode(
        FIXTURE_VERSION,
        FIXTURE_TIME,
        &data.to_value(),
        Limits::default(),
    )
    .unwrap();
    assert!(
        bytes == fs::read(fixture()).unwrap(),
        "the writer no longer writes version 1 as it was shipped"
    );
}

#[test]
fn migrations_run_in_order_from_the_files_version() {
    // Two made-up versions after the first: version 2 renamed `next_id`,
    // and version 3 adds a member whose value depends on that rename.
    fn to_v2(payload: &mut Value) {
        if let Value::Object(members) = payload {
            for (name, _) in members.iter_mut() {
                if name == "next_id" {
                    *name = "next".to_string();
                }
            }
        }
    }
    fn to_v3(payload: &mut Value) {
        let renamed = payload.get("next").is_some();
        if let Value::Object(members) = payload {
            members.push(("renamed".to_string(), Value::Bool(renamed)));
        }
    }
    let migrations: &[Migration] = &[to_v2, to_v3];
    let v1 = || object(&[("next_id", Value::Int(4))]);

    let mut payload = v1();
    migrate(&mut payload, 1, migrations).unwrap();
    assert_eq!(
        payload,
        object(&[("next", Value::Int(4)), ("renamed", Value::Bool(true))])
    );
    // From version 2, the first is not run again.
    let mut payload = v1();
    migrate(&mut payload, 2, migrations).unwrap();
    assert_eq!(
        payload,
        object(&[("next_id", Value::Int(4)), ("renamed", Value::Bool(false))])
    );
    let mut payload = v1();
    migrate(&mut payload, 3, migrations).unwrap();
    assert_eq!(payload, v1(), "the newest needs none");
    for version in [0, 4] {
        assert!(matches!(
            migrate(&mut v1(), version, migrations),
            Err(SaveError::Version { found, newest: 3 }) if found == version
        ));
    }
    // The engine's: one for each version before the newest.
    assert_eq!(
        nadir_scene::MIGRATIONS.len() + 1,
        usize::from(ksav::VERSION)
    );
}

#[test]
fn a_load_that_fails_changes_nothing() {
    let registry = types();
    let library = library();
    let saved = SaveData::new(9);
    let hero = |components: Vec<(String, Value)>| {
        let mut entity = SavedEntity::new(1);
        entity.components = components;
        entity
    };
    let health = |value: Value| ("Health".to_string(), value);
    let fails = |data: &SaveData, world: &mut World| {
        let mut cx = ReflectContext::new("slot1.ksav");
        let before = world.entity_count();
        let error = restore(data, world, &registry, &library, &mut cx).unwrap_err();
        world.apply_commands();
        assert_eq!(world.entity_count(), before, "{error}");
        assert_eq!(by_id(world).len(), 1, "{error}");
        assert_eq!(world.resource::<Score>().points, 7, "{error}");
        error
    };
    let mut world = a_world(&registry);
    world
        .spawn()
        .with(SaveId(1))
        .with(Health { current: 3, max: 3 });
    world.resource_mut::<Score>().points = 7;

    // A value that does not fit, after a resource and an entity that do.
    let mut data = saved.clone();
    data.globals
        .push(("Score".into(), object(&[("points", Value::Int(1))])));
    data.entities.push(hero(vec![]));
    let mut slime = SavedEntity::new(2);
    slime.prefab = Some("slime".into());
    slime
        .components
        .push(health(object(&[("max", Value::String("x".into()))])));
    data.entities.push(slime);
    match fails(&data, &mut world) {
        SaveError::Prefab(PrefabError::Reflect(error)) => {
            assert_eq!(error.source, "slot1.ksav");
            assert_eq!(error.path, "entities[1].components.Health.max");
        }
        other => panic!("expected the prefab's value refused, found {other}"),
    }
    let mut data = saved.clone();
    data.entities.push(hero(vec![health(Value::Int(3))]));
    match fails(&data, &mut world) {
        SaveError::Reflect(error) => {
            assert_eq!(error.path, "entities[0].components.Health");
        }
        other => panic!("expected the value refused, found {other}"),
    }
    // An entity the save does not have.
    let mut data = saved.clone();
    data.entities.push(hero(vec![(
        "Follow".into(),
        object(&[("target", Value::Int(99))]),
    )]));
    match fails(&data, &mut world) {
        SaveError::Reflect(error) => {
            assert_eq!(error.kind, ReflectErrorKind::UnknownEntity { id: 99 });
            assert_eq!(error.path, "entities[0].components.Follow.target");
        }
        other => panic!("expected an unknown entity, found {other}"),
    }
    let mut data = saved.clone();
    data.entities
        .push(hero(vec![("Poisoned".into(), Value::Null)]));
    assert!(matches!(
        fails(&data, &mut world),
        SaveError::UnknownComponent { id: 1, ref component } if component == "Poisoned"
    ));
    let mut data = saved.clone();
    data.globals.push(("Weather".into(), Value::Null));
    assert!(matches!(
        fails(&data, &mut world),
        SaveError::UnknownResource { ref name } if name == "Weather"
    ));
    let mut data = saved.clone();
    let mut ghost = SavedEntity::new(5);
    ghost.prefab = Some("ghost".into());
    data.entities.push(ghost);
    assert!(matches!(
        fails(&data, &mut world),
        SaveError::Prefab(PrefabError::NotLoaded { ref name }) if name == "ghost"
    ));
    let mut data = saved.clone();
    data.entities.push(hero(vec![]));
    data.entities.push(hero(vec![]));
    assert!(matches!(
        fails(&data, &mut world),
        SaveError::DuplicateId { id: 1, .. }
    ));
    // A world without the resource to replace.
    let mut bare = World::new();
    registry.register_components(&mut bare);
    let mut data = saved.clone();
    data.globals.push(("Score".into(), object(&[])));
    let mut cx = ReflectContext::new("slot1.ksav");
    assert!(matches!(
        restore(&data, &bare, &registry, &library, &mut cx),
        Err(SaveError::NoResource { ref name }) if name == "Score"
    ));
    bare.apply_commands();
    assert_eq!(bare.entity_count(), 0);
}

#[test]
fn an_entity_is_written_as_its_save_id() {
    let registry = types();
    let mut world = a_world(&registry);
    let kept = world.spawn().with(SaveId(1 << 63)).id();
    let loose = world.spawn().id();
    world
        .spawn()
        .with(SaveId(2))
        .with(Follow { target: Some(kept) });
    world.spawn().with(SaveId(3)).with(Follow {
        target: Some(loose),
    });

    let ids = by_id(&world);
    let mut cx = ReflectContext::new("test").with_save_ids(ids.clone());
    assert_eq!(
        kept.to_value(&mut cx),
        Value::String((1u64 << 63).to_string())
    );
    assert_eq!(
        Entity::from_value(&Value::String((1u64 << 63).to_string()), &mut cx),
        Ok(kept)
    );
    assert_eq!(loose.to_value(&mut cx), Value::Null, "with a warning");
    let error = Entity::from_value(&Value::Int(8), &mut cx).unwrap_err();
    assert_eq!(error.kind, ReflectErrorKind::UnknownEntity { id: 8 });
    assert_eq!(
        error.to_string(),
        "test: no entity of the save has the save id 8"
    );
    let mut hashing = ReflectContext::new("hash").hashing();
    assert_eq!(
        loose.to_value(&mut hashing),
        Value::Array(vec![
            Value::Int(i64::from(loose.index())),
            Value::Int(i64::from(loose.generation().get()))
        ])
    );
    let error = Entity::from_value(&Value::Int(2), &mut ReflectContext::new("test")).unwrap_err();
    assert!(matches!(error.kind, ReflectErrorKind::NoContext { .. }));

    let data = capture(&world, &registry, &mut ReflectContext::new("test")).unwrap();
    let targets: Vec<_> = data
        .entities
        .iter()
        .map(|entity| (entity.id, entity.components.clone()))
        .collect();
    let follows = |target| vec![("Follow".to_string(), object(&[("target", target)]))];
    assert_eq!(
        targets,
        [
            (2, follows(Value::String((1u64 << 63).to_string()))),
            (3, follows(Value::Null)),
            (1 << 63, vec![]),
        ],
        "by id; an entity without one is gone"
    );
    // Without `SaveIds`, the counter goes on from the largest id.
    world.insert_resource(SaveIds::starting_at(1));
    assert_eq!(
        capture(&world, &registry, &mut ReflectContext::new("test"))
            .unwrap()
            .next_id,
        1
    );
    let mut fresh = World::new();
    registry.register_components(&mut fresh);
    fresh.spawn().with(SaveId(41));
    assert_eq!(
        capture(&fresh, &registry, &mut ReflectContext::new("test"))
            .unwrap()
            .next_id,
        42
    );

    world.spawn().with(SaveId(2));
    assert!(matches!(
        capture(&world, &registry, &mut ReflectContext::new("test")),
        Err(SaveError::DuplicateId { id: 2, .. })
    ));
}

#[test]
fn saves_keep_only_the_types_registered_as_saved() {
    let mut registry = TypeRegistry::new();
    registry.register_component::<Health>();
    assert!(!registry.get("Health").unwrap().is_saved());
    registry.register_saved_component::<Health>();
    assert!(registry.get("Health").unwrap().is_saved());
    registry.register_component::<Health>();
    assert!(registry.get("Health").unwrap().is_saved(), "still saved");
    registry.register_saved_resource::<Score>();
    registry.register_saved_resource::<Score>();
    let names: Vec<_> = registry
        .resources()
        .map(|resource| resource.name())
        .collect();
    assert_eq!(names, ["Score"]);
    assert!(
        registry.resource("Health").is_none(),
        "a component is not a resource"
    );
    assert!(
        registry.get("Score").is_none(),
        "a resource is not a component"
    );

    // `SaveId` is the entity's id, never a component, saved or not.
    registry.register_saved_component::<SaveId>();
    let mut world = World::new();
    registry.register_components(&mut world);
    world
        .spawn()
        .with(SaveId(6))
        .with(Health { current: 1, max: 2 });
    let data = capture(&world, &registry, &mut ReflectContext::new("test")).unwrap();
    assert_eq!(data.entities.len(), 1);
    assert_eq!(data.entities[0].components.len(), 1);
    assert_eq!(data.entities[0].components[0].0, "Health");
    assert!(data.globals.is_empty(), "the world has no Score");
}

#[test]
fn a_save_that_would_not_read_back_is_not_written() {
    let registry = types();
    let mut world = a_world(&registry);
    world
        .spawn()
        .with(SaveId(1))
        .with(SpawnedFrom("Not A Prefab".into()));
    let data = capture(&world, &registry, &mut ReflectContext::new("test")).unwrap();
    let dir = scratch("unreadable");
    let path = dir.join("slot1.ksav");
    match write_file(&path, &data, "1") {
        Err(SaveError::Codec {
            error:
                CodecError::KsavData {
                    at,
                    kind: DataErrorKind::BadPrefabName,
                },
            ..
        }) => assert_eq!(at, "entities[0].prefab"),
        other => panic!("expected the prefab's name refused, found {other:?}"),
    }
    assert!(!path.exists() && !temp_path(&path).exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_spawned_entity_knows_its_prefab() {
    let registry = types();
    let library = library();
    let mut world = a_world(&registry);
    let mut cx = ReflectContext::new("test");
    let slime = library
        .spawn("slime", &[], &registry, &mut world.commands(), &mut cx)
        .unwrap();
    world.apply_commands();
    assert_eq!(
        world.read::<SpawnedFrom>().get(slime),
        Some(&SpawnedFrom("slime".into()))
    );
}
