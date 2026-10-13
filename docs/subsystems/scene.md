# Subsystem: `nadir-scene`

Reflection, prefabs, levels, saves and game states. This is where our
"serde + reflection" replacement lives (ADR-0007).

## Reflection via `macro_rules!`

Rust has no runtime reflection. Saves, prefabs, the inspector and settings
all need to walk fields by name. Rather than proc macros (which would
realistically need `syn`/`quote`), a declarative macro defines the struct
**and** its reflection impl together:

```rust
nadir_scene::reflect_struct! {
    /// Hit points of a damageable entity.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Health {
        pub current: i32,
        pub max: i32,
    }
}

nadir_scene::reflect_enum! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Facing { #[default] Down, Up, Left, Right }
}
```

Generated:

```rust
pub trait Reflect: 'static {
    fn type_name(&self) -> &'static str;
    fn kind(&self) -> ReflectKind;                         // Struct | Enum | Primitive | List | Option | Handle
    fn fields(&self) -> &'static [FieldInfo];              // name, type name, doc string
    fn field(&self, index: usize) -> Option<&dyn Reflect>;
    fn field_mut(&mut self, index: usize) -> Option<&mut dyn Reflect>;
    fn to_value(&self) -> Value;                           // nadir_codec::json::Value
    fn apply_value(&mut self, v: &Value) -> Result<(), ReflectError>;   // missing fields keep current value
}
pub trait FromValue: Sized { fn from_value(v: &Value) -> Result<Self, ReflectError>; } // via Default + apply_value
```

- **Built-in impls:** `bool`, all integers, `f32`, `f64`, `String`, `Vec2`,
  `IVec2`, `Color`, `Rect`, `Option<T>`, `Vec<T>`, `Handle<T>` (as an asset
  path string), `Entity` references (as `SaveId`).
- **Unknown fields** in data produce a warning naming the file, the type and
  the field. They are never silently ignored.
- **Supported shapes:**
  - named-field structs
  - unit-only enums
  - newtype structs (`reflect_newtype!`)
- **Not supported:** generics, and enums with data. Write those `Reflect`
  impls by hand when needed; the switch trigger is in the dependency policy,
  row 9.
- **`TypeRegistry`:** components register with
  `app.register_component::<Health>()`, which records:
  - the name
  - constructors from `Value`
  - an insert-into-world function
  - inspector hooks

  `xtask` cannot find unregistered components, but a debug-build startup
  check walks prefab files and reports unknown component names.

## Prefabs

```jsonc
// assets/data/prefabs/slime.jsonc
{
  "name": "slime",
  "extends": "enemy_base",            // single inheritance, resolved at load, cycle-checked
  "components": {
    "Sprite":   { "atlas": "atlas/enemies", "region": "slime_idle_0" },
    "Animator": { "clip": "anim/slime_idle" },
    "Health":   { "current": 12, "max": 12 },
    "Collider": { "shape": { "Circle": { "radius": 6 } }, "layer": ["Enemy"], "mask": ["World", "Player"] }
  }
}
```

- Cooked to KBJ, then loaded into `PrefabLibrary`.
- `spawn_prefab(commands, "slime", overrides)` applies component values in
  order: base, then derived, then per-instance overrides from the level.

## Levels

- `.klvl` (cooked from LDtk) contains:
  - tile layers (chunked indices + atlas)
  - the collision grid
  - tall-tile sprites
  - entity instances (prefab name, position, field overrides)
  - level metadata (music, ambience, camera bounds)
- `load_level(name)` is a state transition:
  1. Unload the previous level's asset scope.
  2. Request the new assets and show the loading state until they're done.
  3. Spawn the entities.
  4. Hand the tile layers to `render`'s chunk cache and the grid to
     `collision`.

## Saves

- Format `.ksav`:
  - header: magic `KSAV`, format version `u16`, game version string, UTC
    timestamp, payload CRC32
  - payload: KBJ, written through reflection

  ```
  { "globals": {resource name: value}, "entities": [ { "id": SaveId, "prefab": name?, "components": {..} } ] }
  ```
- **What gets saved:** only entities with a `SaveId`, and only components
  registered as `saved`. Transient components (velocity, animation time) are
  rebuilt on load.
- **Atomic write:**
  1. Write `slot1.ksav.tmp` and flush (`File::sync_all`).
  2. Rename the current `slot1.ksav` to `slot1.ksav.bak`.
  3. Rename the temp file to `slot1.ksav`.

  On load, if the main file fails its CRC or parse, fall back to `.bak` and
  tell the player.
- **Migrations:** `fn migrate_v1_to_v2(&mut Value)` functions, applied in
  order, with fixture tests for every version ever shipped.

## Game states

```rust
pub enum GameState { Boot, Loading(LevelId), MainMenu, InGame, Paused }
```

- A `StateMachine` resource holds the state and any queued transition.
- Systems can be registered `on_enter(state)`, `on_exit(state)`, or "only
  while in state X". This is implemented as a run condition the schedule
  checks before calling the system.
- Transitions happen between ticks, never mid-tick.

## Tests

- **Reflection:**
  - the macro generates correct field lists
  - Value round-trip for every built-in type
  - unknown-field warnings
- **Prefabs:**
  - inheritance and override order
  - a cycle is a clear error
- **Saves:**
  - write/read round-trip
  - a corrupted main file falls back to `.bak`
  - migrations from stored fixtures
  - fuzz the reader
- **States:** enter/exit ordering; no transition mid-tick.
