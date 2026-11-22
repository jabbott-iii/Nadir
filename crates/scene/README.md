# nadir-scene

Reflection for Nadir: macros that define a type together with the code that
walks its fields by name and turns it into a JSON value and back, a registry
of reflected component types, and a hash of a world's components. On it are
built prefabs, a level's entities, game states, and saves.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Games use it through
[nadir_engine](https://crates.io/crates/nadir_engine), which re-exports it as
`nadir::scene`; depend on it directly to use it on its own. The engine keeps
a `TypeRegistry` in its world, and a game adds its own components to it with
`App::register_component`.

## Status

Version 0.4: the API will change before 1.0.

Built today:

- the `Reflect` and `FromValue` traits, and the macros `reflect_struct!`,
  `reflect_enum!` and `reflect_newtype!`, which define a type with both;
- reflection for `bool`, the integers, `f32`, `f64`, `String`, `Option<T>`,
  `Vec<T>`, and nadir-core's `Vec2`, `IVec2`, `Color` and `Rect`;
- errors and warnings that name the source and the path to the field;
- `TypeRegistry`: component types by name, to make, insert and inspect
  them from data, and a 64-bit hash of every reflected component in a
  world, for determinism tests;
- asset handles in data, written as their assets' paths, and loaded in a
  scope when read;
- prefabs: `PrefabLibrary` loads `data/prefabs/<name>.kbj` and the prefabs
  each extends (at most 16, with no cycle), keeps them, until `remove`
  takes one out to be read again, and spawns entities of them, each
  component's value its base's, then its own, then an override's;
- `Level`, the cooked level file as an asset, whose entities
  `PrefabLibrary::spawn_level` spawns through their prefabs;
- game states: `GameState` (booting, loading a level, the main menu,
  playing, paused) in a `StateMachine` resource, run conditions by
  `StateKind`, and `StateSystems`, the systems that run on entering and
  leaving a kind of state, which makes a queued transition between ticks;
- saves: `capture` turns the entities with a `SaveId`, their components
  of the types registered as saved, and the saved resources into a save,
  which `write_file` writes atomically beside a backup, and `read_file`
  reads back, from the backup if the file is damaged, migrating older
  versions; `restore` puts it into a world, all or nothing, with an
  `Entity` in a value written as its entity's `SaveId`.

A whole level, its tile layers, grid and tall tiles with its entities, is
loaded by the facade's `App::load_level`, which uses these; its `App` makes
the state transitions after each tick and after the frame stages, and
gives `Loading` and `Paused` their meaning. Not yet: saving and loading a
game from the facade's `App`, with roadmap M9.7.

## Usage

```toml
[dependencies]
nadir-scene = "0.4"
# For `Component` and `World`, to use the registry.
nadir-ecs = "0.4"
```

A reflected struct is made from its `Default` with the data applied, so the
fields that the data leaves out keep their defaults. A field the type has
not got is a warning, never ignored in silence:

```rust
use nadir_scene::{FromValue, Reflect, ReflectContext, Value};

nadir_scene::reflect_struct! {
    /// Hit points of a damageable entity.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Health {
        pub current: i32,
        pub max: i32,
    }
}

let data = Value::Object(vec![
    ("max".to_string(), Value::Int(12)),
    ("armor".to_string(), Value::Int(3)),
]);
let mut cx = ReflectContext::new("slime.jsonc");
let health = Health::from_value(&data, &mut cx).expect("a Health");
assert_eq!(health, Health { current: 0, max: 12 });
assert_eq!(cx.warnings()[0].field, "armor");

// And back: every field, in order.
let written = health.to_value(&mut cx);
assert_eq!(written.get("current"), Some(&Value::Int(0)));

// Data of the wrong sort is an error that says where.
let bad = Value::Object(vec![("max".to_string(), Value::String("lots".into()))]);
let error = Health::from_value(&bad, &mut cx).expect_err("not an integer");
assert_eq!(
    error.to_string(),
    "slime.jsonc: at `max`: expected an integer, found a string"
);
```

The registry finds a component type by the name that data gives it:

```rust
use nadir_ecs::{Component, World};
use nadir_scene::{ReflectContext, TypeRegistry, Value};

nadir_scene::reflect_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Health {
        pub current: i32,
        pub max: i32,
    }
}
impl Component for Health {}

let mut registry = TypeRegistry::new();
registry.register_component::<Health>();
let mut world = World::new();
registry.register_components(&mut world);

let slime = world.spawn().id();
let data = Value::Object(vec![("current".to_string(), Value::Int(12))]);
let mut cx = ReflectContext::new("example");
let health = registry.get("Health").expect("registered");
health
    .insert(&mut world.commands(), slime, &data, &mut cx)
    .expect("data that fits");
world.apply_commands();
assert_eq!(world.read::<Health>().get(slime).map(|h| h.current), Some(12));

// The hash changes with any reflected value.
let before = registry.hash_world(&world, &mut cx);
world.write::<Health>().get_mut(slime).expect("a Health").current -= 1;
assert_ne!(registry.hash_world(&world, &mut cx), before);
```

`Value` is nadir-codec's JSON tree, re-exported here. nadir-codec's
`json::parse` reads JSONC text into one, and its `kbj` module reads and
writes the binary form.

A prefab's components apply over its base's. A game's prefabs come from the
asset server (`PrefabLibrary::load`); these two are read from JSON, which
needs `nadir-codec = "0.4"` too:

```rust
use nadir_codec::{Limits, json, prefab};
use nadir_ecs::{Component, World};
use nadir_scene::{PrefabLibrary, ReflectContext, TypeRegistry};

nadir_scene::reflect_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Health {
        pub current: i32,
        pub max: i32,
    }
}
impl Component for Health {}

let mut registry = TypeRegistry::new();
registry.register_component::<Health>();
let mut world = World::new();
registry.register_components(&mut world);

let file = |text: &str| prefab::read(&json::parse(text, Limits::default()).expect("JSON"));
let mut library = PrefabLibrary::new();
library.insert(
    "prefabs/enemy.jsonc",
    file(r#"{ "name": "enemy", "components": { "Health": { "current": 5, "max": 5 } } }"#)
        .expect("a prefab"),
);
library.insert(
    "prefabs/slime.jsonc",
    file(r#"{ "name": "slime", "extends": "enemy", "components": { "Health": { "max": 12 } } }"#)
        .expect("a prefab"),
);

let mut cx = ReflectContext::new("example");
let slime = library
    .spawn("slime", &[], &registry, &mut world.commands(), &mut cx)
    .expect("a slime");
world.apply_commands();
let health = world.read::<Health>().get(slime).cloned();
assert_eq!(health, Some(Health { current: 5, max: 12 }));
```

A transition waits for the end of the tick that queues it, and the exit
systems of the state left run before the enter systems of the next. A game
on the engine adds these with `App::add_fixed_system_in`, `App::on_exit`
and `App::on_enter`; on its own, the tick needs `nadir-core = "0.4"` too:

```rust
use nadir_core::time::Tick;
use nadir_ecs::{FixedStage, Schedule, World};
use nadir_scene::{GameState, StateKind, StateMachine, StateSystems};

#[derive(Default)]
struct Log(Vec<&'static str>);

let mut world = World::new();
world.insert_resource(StateMachine::new(GameState::InGame));
world.insert_resource(Log::default());
let mut schedule = Schedule::new();
schedule
    .add_fixed_if(
        FixedStage::Gameplay,
        "pause",
        |world, _| world.resource_mut::<StateMachine>().set(GameState::Paused),
        StateKind::InGame.condition(),
    )
    .add_fixed_if(
        FixedStage::Cleanup,
        "still_playing",
        |world, _| world.resource_mut::<Log>().0.push("still playing"),
        StateKind::InGame.condition(),
    );
let mut systems = StateSystems::new();
systems
    .on_exit(StateKind::InGame, "leave", |world, _| {
        world.resource_mut::<Log>().0.push("leave");
    })
    .on_enter(StateKind::Paused, "menu", |world, _| {
        world.resource_mut::<Log>().0.push("menu");
    });

systems.apply(&mut world); // the start: InGame's enter systems, none here
schedule.run_fixed(&mut world, &Tick { index: 0, dt: 1.0 / 60.0 });
systems.apply(&mut world); // between ticks
assert!(world.resource::<StateMachine>().is(StateKind::Paused));
assert_eq!(world.resource::<Log>().0, ["still playing", "leave", "menu"]);
```

## Documentation

- API reference: https://docs.rs/nadir-scene
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/scene.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
