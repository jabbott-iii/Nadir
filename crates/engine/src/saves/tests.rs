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

//! The saves of an app: a game saved and loaded through its level, a
//! backup read when the file is damaged, and the saves and loads that
//! fail.

use std::fs;
use std::time::Duration;

use nadir_codec::level::MIN_ID;
use nadir_core::Vec2;
use nadir_core::time::Frame;
use nadir_ecs::{Component, FixedStage, FrameStage};
use nadir_scene::SaveIds;

use super::*;
use crate::level::tests::app as level_app;
use crate::{App, Collider, Config, Facing, Player, Transform, Velocity, unload_level};

/// A frame of one tick.
const TICK: Duration = Duration::from_nanos(16_666_667);

nadir_scene::reflect_struct! {
    /// A game's component that saves keep.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    struct Health {
        current: i32,
    }
}

impl Component for Health {}

nadir_scene::reflect_newtype! {
    /// A game's resource that saves keep.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    struct Gold(i32);
}

/// The save events the game's systems heard: `tick` or `frame`, and the
/// event.
#[derive(Default)]
struct Heard(Vec<(&'static str, SaveEvent)>);

fn hear_tick(world: &World, _tick: &Tick) {
    let events = world.resource::<Events<SaveEvent>>().tick_events().to_vec();
    let mut heard = world.resource_mut::<Heard>();
    heard
        .0
        .extend(events.into_iter().map(|event| ("tick", event)));
}

fn hear_frame(world: &World, _frame: &Frame) {
    let events = world
        .resource::<Events<SaveEvent>>()
        .frame_events()
        .to_vec();
    let mut heard = world.resource_mut::<Heard>();
    heard
        .0
        .extend(events.into_iter().map(|event| ("frame", event)));
}

/// What the game's enter system of `InGame` saw of [`Saves::loading`],
/// each time the game entered from a loading.
#[derive(Default)]
struct Entered(Vec<Option<String>>);

/// As the game enters from a loading: spawns a new game's hero, of a save
/// id the world gives out, at (1, 2), standing, facing right, at full
/// health.
fn spawn_hero(world: &World, transition: &Transition) {
    if !matches!(transition.from, Some(GameState::Loading(_))) {
        return;
    }
    let loading = world.resource::<Saves>().loading().map(str::to_string);
    world.resource_mut::<Entered>().0.push(loading);
    let id = world.resource_mut::<SaveIds>().assign();
    world
        .commands()
        .spawn()
        .with(id)
        .with(Player)
        .with(Transform::at(Vec2::new(1.0, 2.0)))
        .with(Velocity(Vec2::ZERO))
        .with(Facing::Right)
        .with(Health { current: 10 });
}

/// A game that plays the level `meadow` (`level.rs`'s), with a hero, its
/// health and its gold saved, its slots in a folder of its own for the
/// test `test`: the app and the folder.
fn game(test: &str) -> (App, PathBuf) {
    let folder = std::env::temp_dir().join(format!("nadir-saves-{test}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    let app = level_app(true)
        .register_saved_component::<Health>()
        .register_saved_resource::<Gold>()
        .insert_resource(Gold(0))
        .insert_resource(Heard::default())
        .insert_resource(Entered::default())
        .insert_resource(StateMachine::new(GameState::Loading(LevelId::new(
            "meadow",
        ))))
        .on_enter(StateKind::InGame, "spawn_hero", spawn_hero)
        .add_fixed_system(FixedStage::Gameplay, "hear_tick", hear_tick)
        .add_frame_system(FrameStage::Ui, "hear_frame", hear_frame);
    app.world().resource_mut::<Saves>().set_folder(&folder);
    (app, folder)
}

/// Runs a frame, then frames until the game is in `InGame` with no load
/// waiting: through a level's loading.
fn play(app: &mut App) {
    app.update(TICK, &[]);
    for _ in 0..8 {
        let world = app.world();
        let playing = world.resource::<StateMachine>().is(StateKind::InGame)
            && world.resource::<Saves>().loading().is_none();
        if playing {
            return;
        }
        app.update(TICK, &[]);
    }
    panic!("the game did not get to InGame");
}

/// What the game heard since this was last asked.
fn heard(app: &App) -> Vec<(&'static str, SaveEvent)> {
    std::mem::take(&mut app.world().resource_mut::<Heard>().0)
}

/// The entity of `id`, if one has it.
fn of_id(app: &App, id: u64) -> Option<Entity> {
    let ids = app.world().read::<SaveId>();
    ids.iter()
        .find(|(_, saved)| saved.0 == id)
        .map(|(entity, _)| entity)
}

/// The players.
fn heroes(app: &App) -> Vec<Entity> {
    app.world().read::<Player>().entities().to_vec()
}

fn saves(app: &App) -> nadir_ecs::ResMut<'_, Saves> {
    app.world().resource_mut::<Saves>()
}

#[test]
fn a_game_saved_and_loaded_comes_back_over_its_level() {
    let (mut app, folder) = game("round_trip");
    play(&mut app);
    assert_eq!(app.world().resource::<Entered>().0, [None]);
    let level = app.world().resource::<CurrentLevel>().entities().to_vec();
    let [tall, stone, moved] = level[..] else {
        panic!("the tall tile and two stones: {level:?}");
    };
    // The level's entities have the ids the level gives them.
    assert_eq!(of_id(&app, MIN_ID + 1), Some(stone));
    assert_eq!(of_id(&app, MIN_ID + 2), Some(moved));
    assert!(!app.world().read::<SaveId>().contains(tall));

    // The hero, hurt and turned, with a child that follows them; a stone
    // pushed and the other broken; gold found.
    let &[hero] = &heroes(&app)[..] else {
        panic!("one hero");
    };
    let child_id = app.world().resource_mut::<SaveIds>().assign();
    let child = app
        .world_mut()
        .spawn()
        .with(child_id)
        .with(Parent {
            entity: hero,
            offset: Vec2::new(0.0, -8.0),
        })
        .with(Transform::default())
        .id();
    {
        let world = app.world();
        world
            .write::<Health>()
            .get_mut(hero)
            .expect("health")
            .current = 4;
        world
            .write::<Transform>()
            .get_mut(hero)
            .expect("a place")
            .position = Vec2::new(30.0, 40.0);
        *world.write::<Facing>().get_mut(hero).expect("a facing") = Facing::Left;
        world
            .write::<Transform>()
            .get_mut(moved)
            .expect("a place")
            .position = Vec2::new(12.0, 12.0);
        world.resource_mut::<Gold>().0 = 7;
    }
    app.world_mut().despawn(stone);
    let next_id = app.world().resource::<SaveIds>().next_id();

    saves(&app).save("slot1");
    app.update(TICK, &[]);
    let path = saves(&app).path("slot1").expect("a slot's file");
    assert_eq!(path, folder.join("slot1.ksav"));
    assert!(path.is_file() && saves(&app).exists("slot1"));
    assert!(!saves(&app).exists("slot2"));
    // The tick after, and its frame, hear of it.
    app.update(TICK, &[]);
    let saved = SaveEvent::Saved {
        slot: "slot1".to_string(),
    };
    assert_eq!(heard(&app), [("tick", saved.clone()), ("frame", saved)]);

    // Play on: the hero walks away, healed, the child gone, the gold spent.
    {
        let world = app.world();
        world
            .write::<Health>()
            .get_mut(hero)
            .expect("health")
            .current = 9;
        world
            .write::<Transform>()
            .get_mut(hero)
            .expect("a place")
            .position = Vec2::new(99.0, 99.0);
        world.resource_mut::<Gold>().0 = 0;
    }
    app.world_mut().despawn(child);

    saves(&app).load("slot1");
    play(&mut app);
    // The enter system ran before the save was restored, and knew of it.
    assert_eq!(
        app.world().resource::<Entered>().0,
        [None, Some("slot1".to_string())]
    );
    let loaded = SaveEvent::Loaded {
        slot: "slot1".to_string(),
        from_backup: None,
    };
    assert_eq!(heard(&app), [("tick", loaded.clone()), ("frame", loaded)]);

    // One hero, the saved one, in a new entity: its saved components, and
    // not the velocity, which saves do not keep.
    let world = app.world();
    let &[hero] = &heroes(&app)[..] else {
        panic!("one hero: {:?}", heroes(&app));
    };
    assert_eq!(
        world.read::<Transform>().get(hero).map(|t| t.position),
        Some(Vec2::new(30.0, 40.0))
    );
    assert_eq!(
        world.read::<Health>().get(hero),
        Some(&Health { current: 4 })
    );
    assert_eq!(world.read::<Facing>().get(hero), Some(&Facing::Left));
    assert!(!world.read::<Velocity>().contains(hero));
    // The child is back, and follows the new hero.
    let child = of_id(&app, child_id.0).expect("the child");
    assert_eq!(
        world.read::<Parent>().get(child),
        Some(&Parent {
            entity: hero,
            offset: Vec2::new(0.0, -8.0)
        })
    );
    // The broken stone stays broken; the pushed one is where it was pushed,
    // with its prefab's collider and the level's override of its offset.
    assert_eq!(of_id(&app, MIN_ID + 1), None);
    let moved = of_id(&app, MIN_ID + 2).expect("the pushed stone");
    assert_eq!(
        world.read::<Transform>().get(moved).map(|t| t.position),
        Some(Vec2::new(12.0, 12.0))
    );
    let colliders = world.read::<Collider>();
    let collider = colliders.get(moved).expect("its prefab's collider");
    assert_eq!(collider.0.offset, Vec2::new(0.0, -4.0));
    drop(colliders);
    assert_eq!(*world.resource::<Gold>(), Gold(7));
    assert_eq!(world.resource::<SaveIds>().next_id(), next_id);
    // The level holds its tall tile, made again, and the restored stone.
    let level = world.resource::<CurrentLevel>().entities().to_vec();
    assert_eq!(level.len(), 2, "{level:?}");
    assert_eq!(level[1], moved);

    // So the next level's load takes the stone away with it.
    unload_level(app.world_mut());
    assert!(!app.world().is_alive(moved));
    assert!(app.world().is_alive(hero));
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_damaged_save_is_read_from_its_backup_and_says_why() {
    let (mut app, folder) = game("backup");
    play(&mut app);
    app.world().resource_mut::<Gold>().0 = 1;
    saves(&app).save("slot1");
    app.update(TICK, &[]);
    app.world().resource_mut::<Gold>().0 = 2;
    saves(&app).save("slot1");
    app.update(TICK, &[]);
    app.update(TICK, &[]);
    let path = saves(&app).path("slot1").expect("the file");
    let mut bytes = fs::read(&path).expect("the save");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&path, bytes).expect("damage it");
    heard(&app);

    saves(&app).load("slot1");
    play(&mut app);
    // The save before the last.
    assert_eq!(*app.world().resource::<Gold>(), Gold(1));
    let events = heard(&app);
    let [
        (
            "tick",
            SaveEvent::Loaded {
                slot,
                from_backup: Some(why),
            },
        ),
        _,
    ] = &events[..]
    else {
        panic!("a load from the backup: {events:?}");
    };
    assert_eq!(slot, "slot1");
    assert!(why.contains("damaged: its CRC-32"), "{why}");
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_save_or_load_that_cannot_be_made_says_why_and_changes_nothing() {
    let (mut app, folder) = game("failures");
    play(&mut app);
    let hero = heroes(&app)[0];

    // No save in the slot.
    saves(&app).load("nothing");
    app.update(TICK, &[]);
    app.update(TICK, &[]);
    let events = heard(&app);
    let [
        (
            "tick",
            SaveEvent::LoadFailed {
                slot,
                missing: true,
                ..
            },
        ),
        _,
    ] = &events[..]
    else {
        panic!("a load of nothing: {events:?}");
    };
    assert_eq!(slot, "nothing");
    assert!(app.world().resource::<StateMachine>().is(StateKind::InGame));
    assert_eq!(heroes(&app), [hero]);

    // A name that is not a slot's, to save to or to load.
    for name in ["Slot1", "", "../slot", &"a".repeat(65)] {
        assert_eq!(saves(&app).path(name), None, "{name:?}");
        saves(&app).save(name);
        app.update(TICK, &[]);
        app.update(TICK, &[]);
        let events = heard(&app);
        let [("tick", SaveEvent::SaveFailed { reason, .. }), _] = &events[..] else {
            panic!("a save to {name:?}: {events:?}");
        };
        assert!(reason.contains("is not a slot's name"), "{reason}");
    }
    assert!(!folder.exists(), "nothing written");

    // A child saved whose parent is not.
    let unsaved = app.world_mut().spawn().with(Transform::default()).id();
    let id = app.world().resource_mut::<SaveIds>().assign();
    let _child = app
        .world_mut()
        .spawn()
        .with(id)
        .with(Parent {
            entity: unsaved,
            offset: Vec2::ZERO,
        })
        .id();
    saves(&app).save("slot1");
    app.update(TICK, &[]);
    app.update(TICK, &[]);
    let events = heard(&app);
    let [("tick", SaveEvent::SaveFailed { reason, .. }), _] = &events[..] else {
        panic!("a save of an unsaved parent: {events:?}");
    };
    assert!(
        reason.contains("has a `Parent` whose entity has no `SaveId`, or is despawned"),
        "{reason}"
    );
    assert!(!folder.join("slot1.ksav").exists());

    // An app that runs without `App::run` has no folder.
    let mut bare = App::new(Config::default())
        .insert_resource(Heard::default())
        .add_fixed_system(FixedStage::Gameplay, "hear_tick", hear_tick)
        .add_frame_system(FrameStage::Ui, "hear_frame", hear_frame);
    assert_eq!(saves(&bare).folder(), None);
    saves(&bare).save("slot1");
    bare.update(TICK, &[]);
    bare.update(TICK, &[]);
    let events = heard(&bare);
    let [("tick", SaveEvent::SaveFailed { reason, .. }), _] = &events[..] else {
        panic!("a save with no folder: {events:?}");
    };
    assert!(reason.contains("no folder"), "{reason}");
}

#[test]
fn a_load_whose_loading_the_game_leaves_fails_and_no_save_is_made_while_loading() {
    let (app, folder) = game("left");
    let mut app =
        app.add_frame_system_in(StateKind::Loading, FrameStage::Ui, "save", |world, _| {
            world.resource_mut::<Saves>().save("during");
        });
    play(&mut app);
    saves(&app).save("slot1");
    app.update(TICK, &[]);
    app.update(TICK, &[]);
    heard(&app);

    saves(&app).load("slot1");
    // The tick reads the save, and the game goes to the level's loading,
    // whose frame asks for a save, which is refused.
    app.update(TICK, &[]);
    assert_eq!(saves(&app).loading(), Some("slot1"));
    assert!(!folder.join("during.ksav").exists());
    // The game goes to its main menu before the level loads.
    app.world()
        .resource_mut::<StateMachine>()
        .set(GameState::MainMenu);
    app.update(TICK, &[]);
    assert_eq!(saves(&app).loading(), None);
    let events = heard(&app);
    let reasons: Vec<String> = events
        .iter()
        .filter(|(at, _)| *at == "tick")
        .map(|(_, event)| match event {
            SaveEvent::SaveFailed { slot, reason } => format!("save {slot}: {reason}"),
            SaveEvent::LoadFailed { slot, reason, .. } => format!("load {slot}: {reason}"),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        reasons,
        [
            "save during: the game is loading a level",
            "load slot1: the game went to MainMenu before the level `meadow` loaded",
        ]
    );
    assert!(
        app.world()
            .resource::<StateMachine>()
            .is(StateKind::MainMenu)
    );
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_save_of_no_level_is_restored_between_ticks() {
    let folder = std::env::temp_dir().join(format!("nadir-saves-no-level-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    let mut app = App::new(Config {
        version: "2.1.0".to_string(),
        ..Config::default()
    })
    .insert_resource(Heard::default())
    .add_fixed_system(FixedStage::Gameplay, "hear_tick", hear_tick)
    .add_frame_system(FrameStage::Ui, "hear_frame", hear_frame);
    saves(&app).set_folder(&folder);
    let id = app.world().resource_mut::<SaveIds>().assign();
    let hero = app
        .world_mut()
        .spawn()
        .with(id)
        .with(Player)
        .with(Transform::at(Vec2::new(3.0, 4.0)))
        .id();
    saves(&app).save("slot1");
    app.update(TICK, &[]);
    let save = read_file(&folder.join("slot1.ksav"), Limits::default()).expect("the save");
    assert_eq!(save.header.game_version, "2.1.0");
    assert_eq!(saved_level(&save.data), Ok(None));
    app.world_mut().despawn(hero);

    saves(&app).load("slot1");
    app.update(TICK, &[]);
    // Restored after the tick that asked, with no loading.
    assert!(app.world().resource::<StateMachine>().is(StateKind::Boot));
    let &[hero] = &heroes(&app)[..] else {
        panic!("one hero");
    };
    assert_eq!(
        app.world()
            .read::<Transform>()
            .get(hero)
            .map(|t| t.position),
        Some(Vec2::new(3.0, 4.0))
    );
    app.update(TICK, &[]);
    let loaded = SaveEvent::Loaded {
        slot: "slot1".to_string(),
        from_backup: None,
    };
    assert_eq!(
        heard(&app),
        [
            (
                "tick",
                SaveEvent::Saved {
                    slot: "slot1".to_string()
                }
            ),
            (
                "frame",
                SaveEvent::Saved {
                    slot: "slot1".to_string()
                }
            ),
            ("tick", loaded.clone()),
            ("frame", loaded),
        ]
    );
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn the_engine_s_components_saves_keep_are_marked_saved() {
    let app = App::new(Config::default());
    let registry = app.world().resource::<TypeRegistry>();
    let saved: Vec<&str> = registry
        .iter()
        .filter(|registration| registration.is_saved())
        .map(|registration| registration.name())
        .collect();
    assert_eq!(saved, ["Transform", "Player", "Facing", "Parent"]);
    let resources: Vec<&str> = registry.resources().map(|r| r.name()).collect();
    assert_eq!(resources, ["SavedLevel"]);
}

#[test]
fn a_save_names_its_level_by_a_level_s_name_or_none() {
    let with = |level: Value| {
        let mut data = SaveData::new(1);
        data.globals.push(("SavedLevel".to_string(), level));
        data
    };
    assert_eq!(saved_level(&SaveData::new(1)), Ok(None));
    assert_eq!(saved_level(&with(Value::String(String::new()))), Ok(None));
    assert_eq!(
        saved_level(&with(Value::String("meadow_2".to_string()))),
        Ok(Some("meadow_2".to_string()))
    );
    for bad in [Value::String("../meadow".to_string()), Value::Int(3)] {
        let error = saved_level(&with(bad)).expect_err("not a level's name");
        assert!(
            error.starts_with("its level, `globals.SavedLevel`, is not a level's name"),
            "{error}"
        );
    }
}
