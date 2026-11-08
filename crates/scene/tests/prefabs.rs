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

//! Prefabs, levels' entities and asset handles (roadmap M5.11, ADR-0038):
//! prefab files written by hand as JSON, encoded as the cook encodes them,
//! and read through an asset server from memory.

use std::collections::HashMap;

use nadir_assets::{
    Asset, AssetError, AssetPath, AssetServer, DecodeCtx, FinalizeCtx, Handle, OnFailure, Scope,
    Source,
};
use nadir_codec::json::{self, Value};
use nadir_codec::{Limits, kbj, level};
use nadir_ecs::{Component, World};
use nadir_scene::{
    FromValue, MAX_EXTENDS, PrefabError, PrefabLibrary, Reflect, ReflectContext, ReflectErrorKind,
    TypeRegistry,
};

nadir_scene::reflect_struct! {
    /// A test component: a struct with an integer, a list and an option.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Health {
        current: i32,
        max: i32,
        drops: Vec<i32>,
        shield: Option<Shield>,
    }
}

nadir_scene::reflect_struct! {
    /// A struct inside an option, which merges.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Shield {
        strength: i32,
        color: String,
    }
}

nadir_scene::reflect_struct! {
    /// A tag only a base has.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    struct Hostile;
}

/// A text asset, for handles.
#[derive(Debug, PartialEq)]
struct Note(String);

impl Asset for Note {
    type Decoded = String;
    const EXTENSIONS: &'static [&'static str] = &["txt"];

    fn decode(bytes: &[u8], _: &DecodeCtx<'_>) -> Result<String, AssetError> {
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn finalize(text: String, _: &mut FinalizeCtx<'_>) -> Result<Note, AssetError> {
        Ok(Note(text))
    }
}

nadir_scene::reflect_newtype! {
    /// A component that is an asset handle.
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Look(Handle<Note>);
}

impl Component for Health {}
impl Component for Hostile {}
impl Component for Look {}

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

/// The KBJ of `text`, JSON, as the cook writes a data file.
fn kbj(text: &str) -> Vec<u8> {
    let value = json::parse(text, Limits::default()).expect("the test's JSON");
    kbj::encode(&value, Limits::default()).expect("encodable")
}

/// A server of the prefabs `prefabs` (name, JSON), as cooked, and of a
/// note, `notes/hello.txt`.
fn a_server(prefabs: &[(&str, &str)]) -> AssetServer {
    let mut files: HashMap<String, Vec<u8>> = prefabs
        .iter()
        .map(|(name, text)| (PrefabLibrary::path(name), kbj(text)))
        .collect();
    files.insert("notes/hello.txt".to_string(), b"hello".to_vec());
    AssetServer::with_source(Memory(files), OnFailure::Nothing)
}

/// The test's types.
fn types() -> TypeRegistry {
    let mut registry = TypeRegistry::new();
    registry.register_component::<Health>();
    registry.register_component::<Hostile>();
    registry.register_component::<Look>();
    registry
}

/// A world that holds the test's types, and its registry.
fn a_world() -> (World, TypeRegistry) {
    let registry = types();
    let mut world = World::new();
    registry.register_components(&mut world);
    (world, registry)
}

const ENEMY: &str = r#"{ "name": "enemy", "components": {
    "Health": { "current": 5, "max": 10, "drops": [1, 2], "shield": { "strength": 1, "color": "grey" } },
    "Hostile": {} } }"#;
const SLIME: &str = r#"{ "name": "slime", "extends": "enemy", "components": {
    "Health": { "max": 12, "drops": [7], "shield": { "strength": 4 } } } }"#;

#[test]
fn base_derived_and_override_apply_in_that_order_onto_one_value() {
    let mut server = a_server(&[("enemy", ENEMY), ("slime", SLIME)]);
    let mut library = PrefabLibrary::new();
    library
        .load(&mut server, &types(), "slime", Scope::Global)
        .expect("slime and its base load");
    let names: Vec<&str> = library
        .chain("slime")
        .expect("a chain")
        .iter()
        .map(|prefab| prefab.name())
        .collect();
    assert_eq!(names, ["enemy", "slime"], "the base first");

    let (mut world, registry) = a_world();
    let overrides = vec![(
        "Health".to_string(),
        json::parse(r#"{ "current": 3 }"#, Limits::default()).expect("JSON"),
    )];
    let mut cx = ReflectContext::new("levels/field.klvl").loading(&mut server, Scope::Level);
    let entity = library
        .spawn(
            "slime",
            &overrides,
            &registry,
            &mut world.commands(),
            &mut cx,
        )
        .expect("a slime");
    drop(cx);
    world.apply_commands();
    let health = world.read::<Health>().get(entity).cloned();
    assert_eq!(
        health,
        Some(Health {
            // The override's.
            current: 3,
            // The slime's.
            max: 12,
            // A list is replaced whole.
            drops: vec![7],
            // An option's value merges: the slime's strength, the base's color.
            shield: Some(Shield {
                strength: 4,
                color: "grey".to_string(),
            }),
        })
    );
    // A component only the base has.
    assert!(world.read::<Hostile>().get(entity).is_some());
}

#[test]
fn a_cycle_of_one_two_or_three_is_an_error_naming_the_chain() {
    let a_to_b = r#"{ "name": "a", "extends": "b", "components": {} }"#;
    let b_to_a = r#"{ "name": "b", "extends": "a", "components": {} }"#;
    let b_to_c = r#"{ "name": "b", "extends": "c", "components": {} }"#;
    let c_to_a = r#"{ "name": "c", "extends": "a", "components": {} }"#;
    let mut library = PrefabLibrary::new();
    let two = library.load(
        &mut a_server(&[("a", a_to_b), ("b", b_to_a)]),
        &types(),
        "a",
        Scope::Global,
    );
    assert_eq!(
        two,
        Err(PrefabError::Cycle {
            chain: vec!["a".into(), "b".into(), "a".into()]
        })
    );
    let mut library = PrefabLibrary::new();
    let three = library.load(
        &mut a_server(&[("a", a_to_b), ("b", b_to_c), ("c", c_to_a)]),
        &types(),
        "b",
        Scope::Global,
    );
    let error = three.expect_err("a cycle");
    assert_eq!(
        error.to_string(),
        "b -> c -> a -> b: a prefab cannot extend itself, through others or not"
    );
    // One extending itself is refused by its file's reader.
    let mut library = PrefabLibrary::new();
    let one = library.load(
        &mut a_server(&[("a", r#"{ "name": "a", "extends": "a", "components": {} }"#)]),
        &types(),
        "a",
        Scope::Global,
    );
    assert!(
        matches!(&one, Err(PrefabError::Load { path, error: AssetError::Decode(_) }) if path == "data/prefabs/a.kbj"),
        "{one:?}"
    );
    // A chain of prefabs inserted by hand is checked when it is followed.
    let mut library = PrefabLibrary::new();
    for (name, base) in [("x", "y"), ("y", "x")] {
        let text = format!(r#"{{ "name": "{name}", "extends": "{base}", "components": {{}} }}"#);
        let file = nadir_codec::prefab::read(&json::parse(&text, Limits::default()).expect("JSON"))
            .expect("a prefab");
        library.insert(format!("{name}.jsonc"), file);
    }
    assert!(matches!(
        library.chain("x"),
        Err(PrefabError::Cycle { chain }) if chain == ["x", "y", "x"]
    ));
}

/// A chain of `links` prefabs extending the next: `p0` extends `p1` and
/// so on, `p<links>` extending none.
fn chain_of(links: usize) -> Vec<(String, String)> {
    (0..=links)
        .map(|index| {
            let name = format!("p{index}");
            let text = if index < links {
                format!(
                    r#"{{ "name": "{name}", "extends": "p{}", "components": {{}} }}"#,
                    index + 1
                )
            } else {
                format!(r#"{{ "name": "{name}", "components": {{ "Hostile": {{}} }} }}"#)
            };
            (name, text)
        })
        .collect()
}

#[test]
fn a_chain_of_16_bases_loads_and_one_more_is_an_error() {
    let longest = chain_of(MAX_EXTENDS);
    let files: Vec<(&str, &str)> = longest
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();
    let mut library = PrefabLibrary::new();
    library
        .load(&mut a_server(&files), &types(), "p0", Scope::Global)
        .expect("16 bases");
    assert_eq!(library.chain("p0").expect("a chain").len(), MAX_EXTENDS + 1);

    let past = chain_of(MAX_EXTENDS + 1);
    let files: Vec<(&str, &str)> = past.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
    let mut library = PrefabLibrary::new();
    match library.load(&mut a_server(&files), &types(), "p0", Scope::Global) {
        Err(PrefabError::TooLong { chain }) => {
            assert_eq!(chain.len(), MAX_EXTENDS + 2);
            assert_eq!(chain.last().map(String::as_str), Some("p17"));
        }
        other => panic!("expected a chain too long, got {other:?}"),
    }
}

#[test]
fn a_missing_base_or_prefab_is_an_error() {
    let mut library = PrefabLibrary::new();
    let missing = library.load(
        &mut a_server(&[("slime", SLIME)]),
        &types(),
        "slime",
        Scope::Global,
    );
    assert_eq!(
        missing,
        Err(PrefabError::Load {
            path: "data/prefabs/enemy.kbj".to_string(),
            error: AssetError::NotFound,
        })
    );
    // The derived prefab stays loaded; following its chain names the base.
    assert_eq!(
        library.chain("slime"),
        Err(PrefabError::MissingBase {
            prefab: "slime".into(),
            base: "enemy".into()
        })
    );
    assert_eq!(
        library.chain("bat").map(|chain| chain.len()),
        Err(PrefabError::NotLoaded { name: "bat".into() })
    );
    // A file whose prefab is named otherwise.
    let mut library = PrefabLibrary::new();
    let renamed = library.load(
        &mut a_server(&[("bat", ENEMY)]),
        &types(),
        "bat",
        Scope::Global,
    );
    assert_eq!(
        renamed,
        Err(PrefabError::NameMismatch {
            path: "data/prefabs/bat.kbj".into(),
            name: "enemy".into()
        })
    );
}

#[test]
fn a_prefab_removed_is_read_again_by_the_next_load() {
    let mut server = a_server(&[("enemy", ENEMY), ("slime", SLIME)]);
    let mut library = PrefabLibrary::new();
    library
        .load(&mut server, &types(), "slime", Scope::Level)
        .expect("slime and its base load");
    // The level goes, and its files with it: the library keeps its copies.
    server.unload_scope(Scope::Level);
    assert!(library.get("enemy").is_some());

    let removed = library.remove("enemy").expect("the base was loaded");
    assert_eq!(removed.name(), "enemy");
    assert_eq!(library.remove("enemy"), None);
    assert!(library.get("enemy").is_none());
    assert_eq!(
        library.chain("slime").map(|chain| chain.len()),
        Err(PrefabError::MissingBase {
            prefab: "slime".into(),
            base: "enemy".into()
        })
    );
    // Loaded again, it is read from its file, which is loaded again too.
    library
        .load(&mut server, &types(), "slime", Scope::Level)
        .expect("slime's base reads again");
    assert_eq!(library.chain("slime").map(|chain| chain.len()), Ok(2));
    let path = PrefabLibrary::path("enemy");
    let handle = server
        .assets::<nadir_scene::PrefabAsset>()
        .and_then(|it| it.handle(&path));
    assert!(handle.is_some_and(|it| server.state(it).is_some()));
}

#[test]
fn an_unknown_component_is_reported_and_the_spawn_leaves_no_entity() {
    let ghost = r#"{ "name": "ghost", "extends": "enemy", "components": { "PrevTransform": {} } }"#;
    let wraith = r#"{ "name": "wraith", "extends": "ghost", "components": {} }"#;
    let mut server = a_server(&[("enemy", ENEMY), ("ghost", ghost), ("wraith", wraith)]);
    let mut library = PrefabLibrary::new();
    let (mut world, registry) = a_world();
    let expected = PrefabError::UnknownComponent {
        source: "data/prefabs/ghost.kbj".into(),
        prefab: "ghost".into(),
        component: "PrevTransform".into(),
    };
    // Reported when it loads, and for a prefab that extends it, as its own.
    assert_eq!(
        library.load(&mut server, &registry, "ghost", Scope::Global),
        Err(expected.clone())
    );
    assert_eq!(
        library.load(&mut server, &registry, "wraith", Scope::Global),
        Err(expected.clone())
    );
    assert_eq!(library.check("ghost", &registry), Err(expected.clone()));
    assert_eq!(
        expected.to_string(),
        "data/prefabs/ghost.kbj: the prefab `ghost` has the component `PrevTransform`, which is not a reflected type the game registered"
    );
    let mut cx = ReflectContext::new("test").loading(&mut server, Scope::Level);
    let spawned = library.spawn("ghost", &[], &registry, &mut world.commands(), &mut cx);
    assert_eq!(spawned, Err(expected.clone()));
    let spawned = library.spawn("wraith", &[], &registry, &mut world.commands(), &mut cx);
    assert_eq!(spawned, Err(expected));
    // An override's unknown component is named with its source.
    let spawned = library.spawn(
        "enemy",
        &[("Armour".to_string(), Value::Object(Vec::new()))],
        &registry,
        &mut world.commands(),
        &mut cx,
    );
    assert!(
        matches!(&spawned, Err(PrefabError::UnknownComponent { source, component, .. }) if source == "test" && component == "Armour"),
        "{spawned:?}"
    );
    drop(cx);
    world.apply_commands();
    assert_eq!(world.entity_count(), 0, "no entity was spawned");
}

#[test]
fn a_value_that_does_not_fit_names_its_file_and_field() {
    let bad = r#"{ "name": "slime", "extends": "enemy", "components": { "Health": { "max": "twelve" } } }"#;
    let mut server = a_server(&[("enemy", ENEMY), ("slime", bad)]);
    let mut library = PrefabLibrary::new();
    library
        .load(&mut server, &types(), "slime", Scope::Global)
        .expect("the files load: their values are read at spawn");
    let (mut world, registry) = a_world();
    let mut cx = ReflectContext::new("levels/field.klvl").loading(&mut server, Scope::Level);
    let error = library
        .spawn("slime", &[], &registry, &mut world.commands(), &mut cx)
        .expect_err("a string is no integer");
    assert_eq!(
        error.to_string(),
        "data/prefabs/slime.kbj: at `components.Health.max`: expected an integer, found a string"
    );
    drop(cx);
    world.apply_commands();
    assert_eq!(world.entity_count(), 0);
}

#[test]
fn a_handle_writes_its_path_and_reads_back_to_the_same_handle() {
    let mut server = a_server(&[]);
    let note = server.load::<Note>("notes/hello.txt");
    let mut cx = ReflectContext::new("test").with_assets(&server);
    assert_eq!(
        note.to_value(&mut cx),
        Value::String("notes/hello.txt".into())
    );
    drop(cx);

    let path = Value::String("Notes/Hello.txt".into());
    let mut cx = ReflectContext::new("test").loading(&mut server, Scope::Level);
    let read = Handle::<Note>::from_value(&path, &mut cx).expect("a handle");
    assert_eq!(read, note, "the same path is the same handle");
    // A bad path, or one of another type's extension, names its field.
    for bad in ["../notes/hello.txt", "notes/hello.qoi"] {
        let error = cx
            .field("note", |cx| {
                Handle::<Note>::from_value(&Value::String(bad.into()), cx)
            })
            .expect_err("not a note's path");
        assert_eq!(error.path, "note", "{bad}");
        assert!(
            matches!(&error.kind, ReflectErrorKind::BadAssetPath { path, .. } if path == bad),
            "{error}"
        );
    }
    drop(cx);
    // Reading needs a server to load with.
    let mut cx = ReflectContext::new("test").with_assets(&server);
    let error = Handle::<Note>::from_value(&path, &mut cx).expect_err("no server to load with");
    assert!(
        matches!(error.kind, ReflectErrorKind::NoContext { .. }),
        "{error}"
    );
    // A handle the server does not know is written as null.
    let other = AssetServer::with_source(Memory(HashMap::new()), OnFailure::Nothing);
    let mut cx = ReflectContext::new("test").with_assets(&other);
    assert_eq!(note.to_value(&mut cx), Value::Null);
}

#[test]
fn a_level_s_entities_spawn_through_their_prefabs_in_the_level_scope() {
    let lamp = r#"{ "name": "lamp", "components": { "Look": "notes/hello.txt", "Hostile": {} } }"#;
    let mut server = a_server(&[("enemy", ENEMY), ("slime", SLIME), ("lamp", lamp)]);
    let level = level::parse(
        &kbj(
            r#"{ "version": 1, "size": [64, 48], "background": [0, 0, 0], "assets": [],
                 "tilesets": [], "layers": [], "tall": [],
                 "entities": [ { "prefab": "lamp", "position": [8, 16], "overrides": {} },
                               { "prefab": "slime", "position": [40, -2],
                                 "overrides": { "Health": { "current": 1 } } } ] }"#,
        ),
        Limits::default(),
    )
    .expect("a level file");
    let mut library = PrefabLibrary::new();
    library
        .load_level(&mut server, &types(), &level, Scope::Level)
        .expect("the level's prefabs load");
    let (mut world, registry) = a_world();
    let mut cx = ReflectContext::new("levels/test.klvl").loading(&mut server, Scope::Level);
    let spawned = library
        .spawn_level(&level, &registry, &mut world.commands(), &mut cx)
        .expect("the level's entities");
    drop(cx);
    world.apply_commands();
    let positions: Vec<(i32, i32)> = spawned.iter().map(|(_, at)| (at.x, at.y)).collect();
    assert_eq!(positions, [(8, 16), (40, -2)]);
    let (lamp, slime) = (spawned[0].0, spawned[1].0);
    assert_eq!(
        world
            .read::<Health>()
            .get(slime)
            .map(|h| (h.current, h.max)),
        Some((1, 12))
    );
    let note = world.read::<Look>().get(lamp).expect("a look").0;
    server.finish_all_loads(&mut FinalizeCtx::new());
    assert_eq!(server.get(note), Some(&Note("hello".into())));
    // The lamp's note was loaded in the level's scope alone.
    server.unload_scope(Scope::Level);
    assert_eq!(server.state(note), None, "unloading the level unloads it");

    // An override that does not fit names the entity and the level.
    let bad = level::parse(
        &kbj(
            r#"{ "version": 1, "size": [64, 48], "background": [0, 0, 0], "assets": [],
                 "tilesets": [], "layers": [], "tall": [],
                 "entities": [ { "prefab": "enemy", "position": [0, 0], "overrides": {} },
                               { "prefab": "enemy", "position": [0, 0],
                                 "overrides": { "Health": { "max": [] } } } ] }"#,
        ),
        Limits::default(),
    )
    .expect("a level file");
    let mut library = PrefabLibrary::new();
    library
        .load_level(&mut server, &types(), &bad, Scope::Level)
        .expect("enemy loads");
    let (mut world, registry) = a_world();
    let mut cx = ReflectContext::new("levels/bad.klvl").loading(&mut server, Scope::Level);
    let error = library
        .spawn_level(&bad, &registry, &mut world.commands(), &mut cx)
        .expect_err("an array is no integer");
    assert_eq!(
        error.to_string(),
        "levels/bad.klvl: at `entities[1].overrides.Health.max`: expected an integer, found an array"
    );
    drop(cx);
    world.apply_commands();
    assert_eq!(world.entity_count(), 0, "a level that fails spawns none");

    // A prefab's value that does not fit names its own file and place.
    let bad_slime = r#"{ "name": "slime", "extends": "enemy", "components": { "Health": { "max": "twelve" } } }"#;
    let mut server = a_server(&[("enemy", ENEMY), ("slime", bad_slime)]);
    let level = level::parse(
        &kbj(
            r#"{ "version": 1, "size": [16, 16], "background": [0, 0, 0], "assets": [],
                 "tilesets": [], "layers": [], "tall": [],
                 "entities": [ { "prefab": "slime", "position": [0, 0], "overrides": {} } ] }"#,
        ),
        Limits::default(),
    )
    .expect("a level file");
    let mut library = PrefabLibrary::new();
    library
        .load_level(&mut server, &types(), &level, Scope::Level)
        .expect("the files load: their values are read at spawn");
    let (world, registry) = a_world();
    let mut cx = ReflectContext::new("levels/slimes.klvl").loading(&mut server, Scope::Level);
    let error = library
        .spawn_level(&level, &registry, &mut world.commands(), &mut cx)
        .expect_err("a string is no integer");
    assert_eq!(
        error.to_string(),
        "data/prefabs/slime.kbj: at `components.Health.max`: expected an integer, found a string"
    );
}
