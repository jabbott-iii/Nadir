# nadir-scene

Reflection for Nadir: macros that define a type together with the code that
walks its fields by name and turns it into a JSON value and back, a registry
of reflected component types, and a hash of a world's components.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Games use it through
[nadir_engine](https://crates.io/crates/nadir_engine), which re-exports it as
`nadir::scene`; depend on it directly to use it on its own. The engine keeps
a `TypeRegistry` in its world, and a game adds its own components to it with
`App::register_component`.

## Status

Version 0.2: the API will change before 1.0.

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
  `PrefabLibrary::spawn_level` spawns through their prefabs.

A whole level, its tile layers, grid and tall tiles with its entities, is
loaded by the facade's `App::load_level`, which uses these. Not yet:
entity references and saves with M9.4; game states with M9.1.

## Usage

```toml
[dependencies]
nadir-scene = "0.2"
# For `Component` and `World`, to use the registry.
nadir-ecs = "0.2"
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
    ("armour".to_string(), Value::Int(3)),
]);
let mut cx = ReflectContext::new("slime.jsonc");
let health = Health::from_value(&data, &mut cx).expect("a Health");
assert_eq!(health, Health { current: 0, max: 12 });
assert_eq!(cx.warnings()[0].field, "armour");

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
needs `nadir-codec = "0.2"` too:

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

## Documentation

- API reference: https://docs.rs/nadir-scene
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/scene.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
