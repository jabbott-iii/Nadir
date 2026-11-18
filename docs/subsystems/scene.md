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
    fn variants(&self) -> &'static [&'static str] { &[] }  // an enum's, by name (M7.4, ADR-0062)
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
  context. An `Entity` (M9.4, ADR-0068) as its entity's `SaveId`, through
  the context's `SaveIdMap` (`ReflectContext::with_save_ids`, which saves
  give it): `null`, with a warning, for one without an id; for a hash,
  `[index, generation]`; read back through the map, an id it lacks
  `ReflectErrorKind::UnknownEntity`.
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
    borrow while their callback runs), and `remove`, which takes an
    entity's component away through commands (M7.4, ADR-0062)
  - a walk of the type's components in storage order (`each`, M4.7), under
    `TypeRegistry::hash_world(world, cx)`: a 64-bit FNV-1a of every
    registered type's components, by entity, through `to_value`. Zeros of
    either sign hash as one, and so do all NaNs. Determinism tests compare
    it tick after tick (ADR-0025).

  A hash leaves out what the engine rewrites each frame: the context is
  for a hash (`ReflectContext::hashing`), and the facade's `Sprite` leaves
  out its texture, uv, size, pivot and `flip.x`, which `nadir::animate`
  writes (ADR-0038; M9.4's `saved` flag is per type, so a per-field marker
  is still to come).

  `xtask` cannot find unregistered components. Each prefab is checked
  against the registry when it loads (`PrefabLibrary::load` takes the
  registry, and `PrefabLibrary::check` checks one at will), and a spawn
  checks a level's overrides too; the debug-build check of every prefab
  file at startup is deferred (ADR-0038).
- **The engine's components** (the facade's, M4.4, ADR-0022):
  - reflected: `Transform` as `{"position": [x, y]}`, `Velocity` as
    `[x, y]` in pixels a second, `Player` as `{}`, `SaveId` (nadir-scene's
    since M9.4, which the facade re-exports) as an integer, and `Facing`
    (M4.6) as its variant's name;
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
  - not yet: `Parent`, whose `Entity` saves can write since M9.4: M9.7's;
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
  file whose prefab is named otherwise are errors (`PrefabError`). It
  keeps each prefab it read; `remove` takes one out, so that the next
  `load` reads its file again (M5.8).
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
- `load_level(name)` is the facade's, as nadir-scene cannot depend on
  nadir-render. As built in M5.6 (ADR-0038), `App::load_level`, or
  `nadir::load_level` on a world:
  1. Removes the previous level (`nadir::unload_level`): it despawns the
     entities it spawned, takes its tile layers out of `TileLayers` and
     the clips it added out of `Clips`, empties the grid, puts back the
     clear colour it replaced, and unloads `Scope::Level`.
  2. Reads `levels/<name>.klvl` and its prefabs before it returns, the
     prefabs checked against the registry, in `Scope::Level`.
  3. Reads its tile sets' atlases, and adds their clips to `Clips`, but
     those of an atlas the game added already: `Clips` knows an atlas by
     its pages.
  4. Spawns its tall tiles, each a `Sprite` of its block
     (`TileSet::block_uv`) with its feet at its place, then its entities
     through their prefabs, each with its `Transform` at its place.
  5. Adds each tile layer to the facade's `TileLayers`, which the engine's
     `nadir::extract_level` draws each frame, in frame `Extract`, before the
     game's, its background the clear colour, with no allocation once the
     frame's list has room (M4.5, ADR-0023). It sets the grid with
     `Collisions::set_grid`, on the collision layer it names, which the
     game must have named (`App::collision_layers`); a level with none
     leaves an empty grid.
  It logs how long it took. `CurrentLevel`, a resource, says what it made.
  A `LevelError` says why a level did not load: the previous one is gone
  then, and nothing of the new one is in the world but the prefabs it
  read, which the `PrefabLibrary` keeps. Its textures stream
  in as any do, a frame or more later; the game state `Loading` (M9.1,
  below) loads a level behind a loading screen and waits for them. The clips of an atlas
  only a prefab names, not a tile set, are not added: the cook lists only
  tile sets' atlases among a level's assets. It does not play the level's
  `music` or `ambience`, nor use its camera bounds, yet: the cook writes
  none of them (the roadmap's backlog, ADR-0051). A game's music comes
  from the facade's `MusicArea`s (`docs/subsystems/audio.md`).
- **Reloaded, under `dev`** (M5.8, ADR-0043): the facade's hot reload
  makes the current level again once its file, an atlas, or a prefab was
  reloaded, from its files as they are then. Unlike a load, it keeps the
  handles a game may hold: it unloads no scope, its tile layers keep
  their handles in `TileLayers` (a layer more is added, one fewer
  removed), and its atlases' clips keep theirs, each atlas held again
  before the last hold goes. Its entities are spawned anew, with new ids,
  and what happened to them since the load is undone. A level that cannot
  be made again stays as it was. A prefab reloaded replaces the one of its
  name in the `PrefabLibrary`, so a level made again, and the game's next
  spawn, read it; the entities spawned of it outside the level stay as
  they were. Under `dev`, before a level's prefabs are read, those whose
  files the asset server no longer holds, as after the last load
  unloaded `Scope::Level`, are removed from the library
  (`PrefabLibrary::remove`), so that the level reads them, and the hot
  reload watches them, again; without `dev` the library keeps them.

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

As built in M9.4 (ADR-0068; `crates/scene/src/save.rs`, the bytes in
`nadir_codec::ksav`, `docs/subsystems/assets-and-codecs.md`):

- **The file**, version 1: a 24-byte header, `"KSAV" | version u16 |
  game_version_len u16 | crc32 u32 | saved_at i64 | payload_size u32`,
  the game's version, then the payload's KBJ. The time is in seconds since
  1970 UTC, and the CRC-32 covers every byte after it. The payload, a
  `SaveData`, has `next_id` beside the sketch's members: the counter, so
  that no id is given out twice.
- **The types:** `SaveId(u64)` (moved from the facade, which re-exports it
  as `nadir::SaveId`); `SaveIds`, the world's counter, a resource
  (`assign`, `next_id`); `SaveIdMap`, which entity has which id
  (`of_world`, `insert`, `id`, `entity`); and `SpawnedFrom(String)`, which
  `PrefabLibrary::spawn` and `spawn_level` give every entity they spawn.
  It is not in the registry, so prefabs, levels, the inspector and the
  world hash do not see it. `TypeRegistry::register_components` registers
  `SaveId` and `SpawnedFrom` with the world.
- **What is saved:** components registered with
  `TypeRegistry::register_saved_component` (`Registration::is_saved`),
  and resources registered with `register_saved_resource`, as `globals`;
  `register_component` stays unsaved. A `SaveId` is the entity's `id`,
  never one of its components.
- **`capture(world, registry, cx)`** gives the payload: each entity with a
  `SaveId`, by id, its `SpawnedFrom` as its `prefab`, its saved components
  in the registry's order; the saved resources the world has; the
  world's `SaveIds`, or the largest id plus one. The same world gives the
  same bytes. Two entities of one id are `SaveError::DuplicateId`.
- **`restore(data, world, registry, library, cx)`** spawns each saved
  entity anew, through commands, and returns them in the save's order:
  - each is alive at once, so that the context's `SaveIdMap` holds the
    new entities while the values are read;
  - one of a prefab is made as `PrefabLibrary::spawn` makes one, its saved
    components applied over the prefab's, and keeps its `SpawnedFrom`, so
    a component the prefab gives comes back, even one of a saved type the
    entity had lost; one of none, of its saved components alone. Load the
    prefabs first (`PrefabLibrary::load_save`);
  - every value is made before anything changes, so a load that fails
    changes nothing but the entities it reserved, despawned with the
    commands; an unknown component or resource is an error, as in
    prefabs, and a game fixes up an old save's `SaveData` before it;
  - then the entities that had a `SaveId` are despawned and the new ones
    get their components and ids, when the commands are applied, and
    the saved resources replace the world's, at once (the world must have
    them); the world's `SaveIds`, if any, goes on from the save's. A load
    gives the saved entities new `Entity`s, and entities without a
    `SaveId`, such as a level's scenery, stay.
- **The atomic write** is `write_file(path, data, game_version)`: it reads
  its bytes back, as `read_file` would, before it writes any, so that a
  save that would not read is never written; then the three steps above
  (`temp_path`, `backup_path`). `read_file(path, limits)` falls back to the
  backup on any failure of the file (damaged, cut short, missing, a
  version it cannot migrate), with a warning, and `LoadedSave::from_backup`
  says why; if neither reads, `SaveError::Unreadable`, and
  `SaveError::is_missing` is an empty slot.
- **Migrations:** `Migration` is `fn(&mut Value)`; `MIGRATIONS` has none
  yet (index `i` turns version `i + 1` into `i + 2`, checked against
  `ksav::VERSION` when it compiles); `migrate(payload, version, list)`
  runs those from the file's version on. Version 1's fixture,
  `crates/scene/tests/saves/v1.ksav`, is written by `make_fixture.py`
  beside it and never again.
- **Not yet** (M9.7, the facade): a `SaveIds` in every `App`, a slot in
  the user data folder, the engine's components marked saved or not,
  `Parent` reflected, and ids for a level's entities that do not change
  between runs.

## Game states

```rust
pub enum GameState { Boot, Loading(LevelId), MainMenu, InGame, Paused }
```

- A `StateMachine` resource holds the state and any queued transition.
- Systems can be registered `on_enter(state)`, `on_exit(state)`, or "only
  while in state X". This is implemented as a run condition the schedule
  checks before calling the system.
- Transitions happen between ticks, never mid-tick.

As built in M9.1 (ADR-0067):

- **The types** (`crates/scene/src/state.rs`): `LevelId` is a level's
  name, as `load_level` takes it. `GameState` is `Clone` and `Eq`, not
  `Copy`, for its `LevelId`; `Boot` is its default, and it displays as its
  variant's name, `Loading(field)` with the level. `StateKind` is its
  variant without the level: run conditions and enter and exit systems are
  by kind, so `Loading` of any level is one kind.
- **`StateMachine`**: `new(first)` (default `Boot`), `state()`,
  `is(kind)`, `queued()` and `set(next)`, which queues: a later `set`
  replaces the queued one, at debug level. It never changes by itself.
- **Run conditions**: `StateKind::condition()` is a `RunCondition`, the
  schedule's `fn(&World) -> bool`, one function per kind; it asks the state
  the machine is in, not the queued one.
- **Enter and exit systems** are `fn(&World, &Transition)`, `Transition {
  from: Option<GameState>, to: GameState }`, held by `StateSystems`
  (`on_enter`, `on_exit`, `describe`). `StateSystems::apply(&mut World)`
  makes at most one transition:
  - its first call runs the enter systems of the state the machine started
    in, from `None`, and leaves a transition queued before it for the next
    call;
  - after that it takes the queued transition, and drops one to the state
    the machine is in;
  - the exit systems of the kind left run, in the order added, while the
    machine is still in it, then their commands apply; the machine goes to
    the new state; its kind's enter systems run, then their commands
    apply; the transition is logged;
  - a transition these systems queue waits for the next call;
  - each runs through `nadir_ecs::run_system`, named in borrow panics and
    timed for the profiler.
- **In the facade** (`crates/engine/src/states.rs`): every `App` has a
  `StateMachine` in `Boot`, and `App::add_fixed_system_in`,
  `App::add_frame_system_in`, `App::on_enter` and `App::on_exit`.
  `App::update` makes a transition as the frame starts (the first frame's
  start enters the first state), after each tick, and after the frame
  stages:
  - `Loading(level)`: no tick runs and the clock is not told the time, so
    the tick after the loading is the one after the last before it; the
    frame stages run, with the frame's real `dt`, and the engine's
    `nadir::extract_sprites` and `nadir::extract_level` do not, so the
    game's frame systems of `StateKind::Loading` draw a loading screen on
    the clear colour. Once a frame's stages have run in the state, the
    next frame's start loads the level (`load_level`, which unloads the
    last level and its `Scope::Level`); the frames after wait until the
    asset server has no load pending, where the frames are drawn and so
    finish loads (`App::run` with a device, `App::offscreen`), and not at
    all where they are not; then the engine queues `InGame` and makes it.
    A `LevelError`, or without `dev` a failed texture of the level's tile
    sets, atlases or entities' sprites, is logged, the game stays in the
    state, and `App::run` ends with `Error::Level` (its dialog says why,
    a pack's opening error included), as does `Offscreen::frame`.
  - `Paused`: the ticks come, latched and recorded as any, but
    `nadir::movement`, `nadir::parent_offsets` and `nadir::triggers` do
    not run (nor, since M9.5, `nadir::navigation`), and `RenderFrame::time` leaves out the time spent paused, so
    animators and animated tiles stand still and go on without a jump.
    `nadir::snapshot` runs, so that nothing is drawn between two places.
  - The others are the game's to give a meaning to. A system added
    without a state runs in every state.
- **Determinism:** a transition queued in a tick is made after it, at a
  tick the simulation decides, and the loading takes no tick, so a replay
  meets the same states at the same ticks with or without a window. One
  queued by a frame system is made at a frame boundary, which is not.

## Tests

- **Reflection:**
  - the macro generates correct field lists
  - Value round-trip for every built-in type
  - unknown-field warnings
- **Prefabs:**
  - inheritance and override order
  - a cycle is a clear error
- **Saves** (`crates/scene/tests/saves.rs`, M9.4):
  - write/read round-trip
  - a corrupted main file falls back to `.bak`
  - migrations from stored fixtures
  - fuzz the reader (`cargo xtask fuzz-smoke --target ksav`)
- **States:** enter/exit ordering; no transition mid-tick.
