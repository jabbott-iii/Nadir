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

(Since M4.6, `Facing` is the engine's own component, as here, which
`nadir::animate` mirrors sprites by: ADR-0024.)

Generated (built in M4.2; ADR-0020 records where this differs from the
first sketch):

```rust
pub trait Reflect: 'static {
    fn type_name(&self) -> &'static str;                   // "Health"
    fn kind(&self) -> ReflectKind;                         // Struct | Enum | Primitive | List | Option | Handle
    fn fields(&self) -> &'static [FieldInfo];              // name, type as written, doc comment as written
    fn field(&self, index: usize) -> Option<&dyn Reflect>; // struct fields; list and option elements
    fn field_mut(&mut self, index: usize) -> Option<&mut dyn Reflect>;
    fn to_value(&self, cx: &mut ReflectContext<'_>) -> Value;  // nadir_codec::json::Value
    fn apply_value(&mut self, v: &Value, cx: &mut ReflectContext<'_>) -> Result<(), ReflectError>;
}                                                          // missing fields keep their value
pub trait FromValue: Reflect + Sized {
    const TYPE_NAME: &'static str;
    fn from_value(v: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError>;
}
```

- **The context** (`ReflectContext::new(source)`) carries the source's
  name, the path to the field at hand (`cx.field(name, …)`,
  `cx.key(name, …)` for a name the data gives, `cx.index(i, …)`) and the
  warnings. Errors and warnings name the source and the path, as in:
  data/prefabs/slime.kbj: at `components.Health.max`: expected an integer,
  found a string. As built in M5.11 (ADR-0038), it may also hold:
  - the asset server, shared to write handles (`with_assets`), or exclusive
    with a `Scope` to read them, each loaded in that scope (`loading`); a
    context for a hash (`hashing`, `is_hashing`) never loads;
  - resources a type needs, found by their type (`with_resource`,
    `resource`): the facade's `Clips` and `CollisionLayers`;
  - another source for a part of the value (`with_source`), as each of a
    prefab's layers has its own file.

  `ReflectErrorKind` is `#[non_exhaustive]` since M5.11, which added
  `MissingField`, `NoContext`, `BadAssetPath` and `UnknownName`.
- **`FromValue`** is implemented per type. The macros make a struct from its
  `Default` with the data applied, so a struct needs `Default`; one with a
  `Color` field needs a hand-written one, since `Color` has none.
- **Built-in impls:** `bool`, the integers up to 64 bits (`i8` to `i64`,
  `isize`, `u8` to `u64`, `usize`), `f32`, `f64`, `String`, `Vec2`,
  `IVec2`, `Color`, `Rect`, `Option<T>`, `Vec<T>`, and an asset's
  `Handle<T>` (M5.11) as its asset's path, a string: written through the
  context's server, `null` with a warning for a handle the server does not
  know; read by checking the path is an asset path of one of `T`'s
  extensions and loading it in the context's scope, which needs a loading
  context. `Entity` references (as `SaveId`) come with saves (M9.4), and
  need the `SaveId` map in the context.
- **Encodings** (ADR-0020):
  - integers are JSON integers; a `u64` or `usize` too large for an `i64` is
    a string of digits;
  - floats are numbers, and integers are read as floats too; non-finite
    floats become `null` in text, so they do not survive it;
  - `Vec2` and `IVec2`: `[x, y]`;
  - `Color`: `"#rrggbb"`, or `"#rrggbbaa"` when not opaque;
  - `Rect`: `{"min": [x, y], "max": [x, y]}`; applying one sets only the
    corners it names;
  - `Option<T>`: `null` or the value, so `Some(None)` reads back as `None`;
  - `Vec<T>`: an array, which replaces the list once every element is read.
- **Unknown fields** in data produce a warning naming the file, the type and
  the field. They are never silently ignored: each is logged and kept in
  the context.
- **Supported shapes:**
  - named-field structs, and unit structs (written as `{}`: tags)
  - unit-only enums, written as the variant's name
  - newtype structs (`reflect_newtype!`), written as the value they wrap
- **Not supported:** generics, enums with data, and raw identifiers
  (`r#type`) as field names. Write those `Reflect` impls by hand when
  needed; the switch trigger is in the dependency policy, row 9.
- **`TypeRegistry`:** components register with
  `app.register_component::<Health>()` (the facade's, M4.4; the registry's
  own is `register_component::<T: Component + FromValue>()`; the registry is
  a world resource), which records:
  - the name (`T::TYPE_NAME`; two types with one name panic)
  - constructors from `Value` (`Registration::from_value`)
  - an insert-into-world function (`insert`, through `Commands`;
    `TypeRegistry::register_components` registers the types with a world)
  - inspector hooks (`inspect` and `inspect_mut`, which hold the type's
    borrow while their callback runs)
  - a walk of the type's components in storage order (`each`, M4.7), under
    `TypeRegistry::hash_world(world, cx)`: a 64-bit FNV-1a of every
    registered type's components, by entity, through `to_value`. Zeros of
    either sign hash as one, and so do all NaNs. Determinism tests compare
    it tick after tick (ADR-0025).

  A hash leaves out what the engine rewrites each frame: the context is
  for a hash (`ReflectContext::hashing`), and the facade's `Sprite` leaves
  out its texture, uv, size, pivot and `flip.x`, which `nadir::animate`
  writes (ADR-0038; a per-field marker waits for M9.4's `saved` flag).

  `xtask` cannot find unregistered components. Each prefab is checked
  against the registry when it loads (`PrefabLibrary::load` takes the
  registry, and `PrefabLibrary::check` checks one at will), and a spawn
  checks a level's overrides too; the debug-build check of every prefab
  file at startup is deferred (ADR-0038).
- **The engine's components** (the facade's, M4.4, ADR-0022):
  - reflected: `Transform` as `{"position": [x, y]}`, `Velocity` as
    `[x, y]` in pixels a second, `Player` as `{}`, `SaveId` as an
    integer, and `Facing` (M4.6) as its variant's name;
  - reflected by hand (M5.11, ADR-0038, `crates/engine/src/reflected.rs`):
    - `Sprite` as its fields, `{"texture": "atlas/sprites_0.qoi", "uv":
      {...}, "size": [w, h], "pivot": [x, y], "rotation": 0.0, "color":
      "#ffffff", "layer": 0, "sort": "YSort", "flip": {"x": false, "y":
      false}}`, `sort` also `{"Fixed": n}`; read, it may name a frame of a
      cooked atlas instead, `{"atlas": "atlas/sprites.kbj", "sprite":
      "sprites/hero", "frame": 0}`, which gives its texture, uv, size and
      pivot (`Sprite::from_atlas`), the fields beside it applying over
      them; it needs a `texture` or an `atlas`, and a `sprite` or `frame`
      without its `atlas` is an error, since a sprite keeps no atlas: a
      layer that changes the frame names the atlas and sprite again;
    - `Collider` as `{"shape": {"Circle": {"radius": 6}}, "offset": [x,
      y], "layer": ["World"], "mask": "all", "kind": "Solid"}`, the shape
      also `{"Aabb": {"half": [x, y]}}`, a mask `"all"` or a list of the
      names `App::collision_layers` gives the bits, and bit numbers for
      the rest; read, it needs its `shape`, the rest defaulting as
      `Collider::solid` has them;
    - `Animator` as `{"clip": "enemies/slime/idle"}`, its clip's name in
      the facade's `Clips` (`Clips::find`); its time and finished flag are
      transient, and a read one starts its clip (ADR-0024). A clip with no
      name is written as `null`, with a warning, and for a hash as its
      handle's index and generation;
  - not yet: `Parent` (an `Entity`, M9.4);
  - never in data: `PrevTransform`, which the engine gives every drawn
    entity.

## Prefabs

```jsonc
// assets/data/prefabs/slime.jsonc
{
  "name": "slime",                     // its file's stem
  "extends": "enemy_base",            // single inheritance, resolved at load, cycle-checked
  "components": {
    "Sprite":   { "atlas": "atlas/enemies.kbj", "sprite": "enemies/slime", "frame": 0 },
    "Animator": { "clip": "enemies/slime/idle" },   // a clip of Clips: the tag `idle` of `enemies/slime.json`
    "Health":   { "current": 12, "max": 12 },
    "Collider": { "shape": { "Circle": { "radius": 6 } }, "layer": ["Enemy"], "mask": ["World", "Player"] }
  }
}
```

As built in M5.11 (ADR-0038):

- Cooked to KBJ, `data/prefabs/slime.kbj` (M5.5, ADR-0035), and read with
  `nadir_codec::prefab` (M5.10): `name`, `extends` and `components`, no
  `null` component, so a derived prefab cannot remove its base's.
- `PrefabLibrary`, a world resource of every `App`, loads a prefab and the
  prefabs it extends (`load`, through the asset server, before it
  returns), and checks their components against the registry, at most 16
  of them (`MAX_EXTENDS`): an unknown component, a cycle
  (`slime -> enemy_base -> slime`), a longer chain, a missing base and a
  file whose prefab is named otherwise are errors (`PrefabError`).
- `PrefabLibrary::spawn(name, overrides, registry, commands, cx)` makes
  each component's value from its layers: the furthest base's, then each
  derived prefab's applied onto it, then the override's
  (`Registration::insert_layers`), so a field none names keeps its
  default and a component one alone has comes from it; an override may add
  a component the prefab lacks. Every component is made before the entity
  is spawned, so a spawn that fails leaves no entity: an unknown
  component (`PrevTransform` among them) names the file that gave it, the
  prefab and the component, and a value that does not fit names its file
  and its place: `components.<Type>.<field>` of a prefab's file, or
  `entities[<i>].overrides.<Type>.<field>` of a level's.
- The context gives what the values need: the facade's `loading_context`
  holds the asset server, with the scope handles load in, the `Clips` and
  the `CollisionLayers`. `spawn` takes the library, the registry and the
  commands as arguments, so the ECS needs no access to resources from
  `Commands`.

## Levels

- `.klvl` (cooked from LDtk) contains:
  - tile layers, each of a tile set, a sprite of an atlas: a tile or none
    in each cell, with its flips, row by row; chunking is the renderer's
    (ADR-0038)
  - the collision grid
  - tall-tile sprites
  - entity instances (prefab name, position, field overrides)
  - level metadata (music, ambience, camera bounds)
- As built in M5.11, `Level` is the level file as an asset (`.klvl`,
  `nadir_codec::level`, M5.10), and `PrefabLibrary::spawn_level` spawns its
  entities through their prefabs, with their overrides, after
  `PrefabLibrary::load_level` loads them, and returns each with its
  position; the context loads their handles in `Scope::Level`. Every
  entity is made before any is spawned, so a level that fails spawns none.
- `load_level(name)` (M5.6) is a state transition:
  1. Unload the previous level's asset scope.
  2. Request the new assets and show the loading state until they're done.
  3. Spawn the entities.
  4. Hand the tile layers to `render`'s chunk cache and the grid to
     `collision`. As built (M4.5, ADR-0023), that is: add each layer to the
     facade's `TileLayers` resource and draw it with a `TileLayerDraw`, and
     set the grid with `Collisions::set_grid`. nadir-scene cannot depend on
     nadir-render, so the facade does the first.

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
