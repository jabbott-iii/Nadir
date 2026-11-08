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

//! Reflection for the Nadir engine: walking a value's fields by name, and
//! turning it into a [`Value`] and back.
//!
//! This is the scene layer of Nadir, and its own replacement for serde and
//! runtime reflection (ADR-0007): declarative macros define a type and its
//! impls together, with no procedural macros. So far it has reflection
//! (roadmap M4.2), the world hash (M4.7), and asset handles in data,
//! prefabs with `extends` and a level's entities (M5.11, ADR-0038).
//! Entity references and saves come with M9.4, and game states with M9.1.
//! `docs/subsystems/scene.md` has the design.
//!
//! The facade crate, `nadir_engine`, re-exports this crate as
//! `nadir::scene`. Every `App` keeps a [`TypeRegistry`] as a world
//! resource, with the engine's reflected components in it (`Transform`,
//! `Velocity`, `Player`, `SaveId`, `Facing`, `Sprite`, `Collider` and
//! `Animator`), and a [`PrefabLibrary`]; `App::register_component` adds a
//! game's, and `App::world_hash` is [`TypeRegistry::hash_world`] of that
//! registry, with a context for a hash. This crate depends on `nadir-core`,
//! `nadir-codec`, `nadir-ecs` and `nadir-assets`.
//!
//! # The parts
//!
//! - **The traits.** [`Reflect`] walks a value: its
//!   [`type_name`](Reflect::type_name), [`kind`](Reflect::kind)
//!   ([`ReflectKind`]), [`fields`](Reflect::fields) ([`FieldInfo`]: name,
//!   type and doc comment), [`field`](Reflect::field) and
//!   [`field_mut`](Reflect::field_mut), and turns it into a [`Value`]
//!   ([`Reflect::to_value`]) and back ([`Reflect::apply_value`]).
//!   [`FromValue`] makes a new value from data.
//! - **The macros.** [`reflect_struct!`] defines a struct with named fields,
//!   or a unit struct; [`reflect_enum!`] an enum of unit variants;
//!   [`reflect_newtype!`] a struct that wraps one value. Each writes the
//!   type and both impls.
//! - **The built-in impls:** `bool`, `i8` to `i64`, `isize`, `u8` to `u64`,
//!   `usize`, `f32`, `f64`, `String`, `Option<T>`, `Vec<T>`,
//!   `nadir-core`'s `Vec2`, `IVec2`, `Color` and `Rect`, and an asset's
//!   `Handle<T>`, written as its asset's path.
//! - **The context.** [`ReflectContext`] goes with every read and write:
//!   it names the source, keeps the path to the field at hand, and
//!   collects the [`ReflectWarning`]s ([`ReflectContext::warnings`]). It
//!   may hold the asset server, to write handles
//!   ([`ReflectContext::with_assets`]) or to read them, loading each in a
//!   scope ([`ReflectContext::loading`]); resources a type needs, such as
//!   the facade's clips ([`ReflectContext::with_resource`]); and whether
//!   the values are for a hash ([`ReflectContext::hashing`]).
//!   [`ReflectContext::field`], [`ReflectContext::key`],
//!   [`ReflectContext::index`], [`ReflectContext::with_source`],
//!   [`ReflectContext::expected`], [`ReflectContext::unknown_field`] and
//!   [`ReflectContext::warn`] are for impls written by hand.
//! - **Errors.** [`ReflectError`], with a [`ReflectErrorKind`].
//! - **The registry.** [`TypeRegistry`] holds component types by name:
//!   [`TypeRegistry::register_component`], [`TypeRegistry::get`],
//!   [`TypeRegistry::iter`], [`TypeRegistry::register_components`] (with a
//!   world) and [`TypeRegistry::hash_world`]. Each type's
//!   [`Registration`] makes it from data ([`Registration::from_value`]),
//!   gives it to an entity through commands ([`Registration::insert`]),
//!   reads or changes an entity's ([`Registration::inspect`],
//!   [`Registration::inspect_mut`]), and walks every one in a world
//!   ([`Registration::each`]). [`Registration::insert_layers`] makes one
//!   value of several [`Layer`]s, a prefab's base's, its own and an
//!   override, applied in order.
//! - **Prefabs.** [`PrefabLibrary`] loads prefabs, cooked to
//!   `data/prefabs/<name>.kbj`, with the prefabs they extend
//!   ([`PrefabLibrary::load`], at most [`MAX_EXTENDS`] of them, with no
//!   cycle, each component a type of the registry), and spawns entities of
//!   them through commands
//!   ([`PrefabLibrary::spawn`]): each component's value is its base's,
//!   then its own, then the override's. A spawn that fails leaves no
//!   entity; [`PrefabError`] says why, naming the file. It keeps each
//!   prefab it read, which later loads use; [`PrefabLibrary::remove`] takes
//!   one out, so that the next load reads its file again.
//! - **Levels.** [`Level`] is the cooked level file as an asset
//!   ([`level`] re-exports its types), and
//!   [`PrefabLibrary::spawn_level`] spawns its entities, each with its
//!   position, after [`PrefabLibrary::load_level`] loads their prefabs.
//!   Its tile layers and grid are the facade's, whose `load_level` loads a
//!   whole level (M5.6).
//! - **[`Value`]** is `nadir-codec`'s JSON tree, re-exported. `nadir-codec`
//!   reads JSONC text and KBJ into one.
//!
//! # Example
//!
//! A component, an enum field and a list, read from data, inserted by
//! name, and walked by field:
//!
//! ```
//! use nadir_core::Vec2;
//! use nadir_ecs::{Component, World};
//! use nadir_scene::{Reflect, ReflectContext, ReflectKind, TypeRegistry, Value};
//!
//! nadir_scene::reflect_enum! {
//!     #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
//!     pub enum Mood { #[default] Calm, Angry }
//! }
//!
//! nadir_scene::reflect_struct! {
//!     /// A creature that wanders between points.
//!     #[derive(Clone, Debug, Default, PartialEq)]
//!     pub struct Wanderer {
//!         /// How it feels now.
//!         pub mood: Mood,
//!         /// Where it goes, in order.
//!         pub path: Vec<Vec2>,
//!         pub speed: f32,
//!     }
//! }
//! impl Component for Wanderer {}
//!
//! let mut registry = TypeRegistry::new();
//! registry.register_component::<Wanderer>();
//! let mut world = World::new();
//! registry.register_components(&mut world);
//!
//! let point = |x, y| Value::Array(vec![Value::Int(x), Value::Int(y)]);
//! let data = Value::Object(vec![
//!     ("mood".to_string(), Value::String("Angry".to_string())),
//!     ("path".to_string(), Value::Array(vec![point(0, 0), point(16, 8)])),
//! ]);
//! let mut cx = ReflectContext::new("example");
//! let goblin = world.spawn().id();
//! let registration = registry.get("Wanderer").expect("registered");
//! registration.insert(&mut world.commands(), goblin, &data, &mut cx)?;
//! world.apply_commands();
//!
//! let wanderers = world.read::<Wanderer>();
//! let wanderer = wanderers.get(goblin).expect("inserted");
//! assert_eq!(wanderer.mood, Mood::Angry);
//! assert_eq!(wanderer.path, [Vec2::new(0.0, 0.0), Vec2::new(16.0, 8.0)]);
//! assert_eq!(wanderer.speed, 0.0, "a field the data leaves out keeps its default");
//!
//! // What an inspector sees.
//! let names: Vec<_> = wanderer.fields().iter().map(|field| field.name).collect();
//! assert_eq!(names, ["mood", "path", "speed"]);
//! assert_eq!(wanderer.fields()[0].doc, " How it feels now.\n");
//! assert_eq!(wanderer.field(1).map(|path| path.kind()), Some(ReflectKind::List));
//! # Ok::<(), nadir_scene::ReflectError>(())
//! ```
//!
//! # Encodings
//!
//! How the built-in types are written (ADR-0020):
//!
//! - integers are JSON integers; a `u64` or `usize` too large for an `i64`
//!   is a string of its digits;
//! - floats are numbers, and integers are read as floats too. A
//!   non-finite float is written as `null` in JSON text, so it does not
//!   survive text; KBJ keeps it;
//! - `Vec2` and `IVec2` are `[x, y]`;
//! - `Color` is `"#rrggbb"`, or `"#rrggbbaa"` when it is not opaque;
//! - `Rect` is `{"min": [x, y], "max": [x, y]}`;
//! - `Option<T>` is `null` or the value, so `Some(None)` reads back as
//!   `None`;
//! - `Vec<T>` is an array, which replaces the whole list;
//! - a struct is an object of its fields, and a unit struct is `{}`;
//! - an enum is its variant's name, and a newtype is the value it wraps.
//!
//! # Using it well
//!
//! - **Errors** name the source given to [`ReflectContext::new`], the path
//!   to the field, and what is wrong, as in
//!   ``slime.jsonc: at `max`: expected an integer, found a string``. When
//!   [`Reflect::apply_value`] fails, what it applied before the error stays
//!   applied; [`Registration::insert`] queues nothing.
//! - **Unknown fields** are warnings, never ignored in silence: each is
//!   logged with `nadir_core::log::warn!` and kept in the context.
//! - **Missing fields** keep their value. [`FromValue`] for a struct starts
//!   from its `Default`, so the macros' structs need one. `Color` has none,
//!   so a struct with a `Color` field needs a `Default` written by hand.
//! - **The macros do not cover** generics, enums whose variants carry data,
//!   or raw identifiers (`r#type`) as field names. Write the impls by hand
//!   for those.
//! - **Names.** Data names a component by its type's name as written, its
//!   [`FromValue::TYPE_NAME`]. Registering two types with one name panics.
//! - **Borrows.** [`Registration::inspect`], [`Registration::inspect_mut`]
//!   and [`Registration::each`] hold the type's components borrowed while
//!   their callback runs, as [`World::read`](nadir_ecs::World::read) and
//!   [`World::write`](nadir_ecs::World::write) do: do not borrow that type
//!   again inside it.
//! - **Determinism.** [`TypeRegistry::hash_world`] is a 64-bit FNV-1a of
//!   every registered type's components, by entity, in storage order. Zeros
//!   of either sign hash as one, and so do all NaNs, whose bits can differ
//!   from one processor to another. Components of types not in the
//!   registry, and resources, are not in it. Its documentation gives the
//!   bytes it hashes.
//! - **Allocation.** [`Reflect::to_value`] builds a [`Value`], which
//!   allocates, and reading one can too. Reflection is for loading,
//!   saving, tools and tests, not for every frame.

mod builtin;
pub mod level;
mod macros;
pub mod prefab;
mod reflect;
mod registry;
#[cfg(test)]
mod tests;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use level::Level;
pub use nadir_codec::json::Value;
pub use prefab::{MAX_EXTENDS, Prefab, PrefabAsset, PrefabError, PrefabLibrary};
pub use reflect::{
    FieldInfo, FromValue, Reflect, ReflectContext, ReflectError, ReflectErrorKind, ReflectKind,
    ReflectWarning,
};
pub use registry::{Layer, Registration, TypeRegistry};
