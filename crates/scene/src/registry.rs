//! [`TypeRegistry`]: the reflected component types, by name.

use std::any::TypeId;

use nadir_codec::json::Value;
use nadir_core::FastHashMap;
use nadir_core::hash::fnv1a64;
use nadir_ecs::{Commands, Component, Entity, World};

use crate::{FromValue, Reflect, ReflectContext, ReflectError};

type ConstructFn = fn(&Value, &mut ReflectContext<'_>) -> Result<Box<dyn Reflect>, ReflectError>;
type InsertFn =
    fn(&mut Commands<'_>, Entity, &Value, &mut ReflectContext<'_>) -> Result<(), ReflectError>;
type InspectFn = fn(&World, Entity, &mut dyn FnMut(&dyn Reflect)) -> bool;
type InspectMutFn = fn(&World, Entity, &mut dyn FnMut(&mut dyn Reflect)) -> bool;
type EachFn = fn(&World, &mut dyn FnMut(Entity, &dyn Reflect));

/// What the registry knows of one component type.
pub struct Registration {
    name: &'static str,
    type_id: TypeId,
    construct: ConstructFn,
    register_in: fn(&mut World),
    insert: InsertFn,
    inspect: InspectFn,
    inspect_mut: InspectMutFn,
    each: EachFn,
}

impl Registration {
    /// The type's name, which data uses: `Health`.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The type.
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Makes a value of the type from `value`.
    ///
    /// # Errors
    ///
    /// If `value` does not fit the type.
    pub fn from_value(
        &self,
        value: &Value,
        cx: &mut ReflectContext<'_>,
    ) -> Result<Box<dyn Reflect>, ReflectError> {
        (self.construct)(value, cx)
    }

    /// Makes a value of the type from `value`, and gives it to `entity`
    /// through `commands`: it is added when they are applied.
    ///
    /// # Errors
    ///
    /// If `value` does not fit the type; nothing is queued then.
    pub fn insert(
        &self,
        commands: &mut Commands<'_>,
        entity: Entity,
        value: &Value,
        cx: &mut ReflectContext<'_>,
    ) -> Result<(), ReflectError> {
        (self.insert)(commands, entity, value, cx)
    }

    /// Calls `f` with `entity`'s component of this type, to read it.
    /// Returns whether it has one. The type's components stay borrowed
    /// while `f` runs.
    pub fn inspect(&self, world: &World, entity: Entity, f: &mut dyn FnMut(&dyn Reflect)) -> bool {
        (self.inspect)(world, entity, f)
    }

    /// Calls `f` with `entity`'s component of this type, to change it.
    /// Returns whether it has one. The type's components stay borrowed
    /// mutably while `f` runs.
    pub fn inspect_mut(
        &self,
        world: &World,
        entity: Entity,
        f: &mut dyn FnMut(&mut dyn Reflect),
    ) -> bool {
        (self.inspect_mut)(world, entity, f)
    }

    /// Calls `f` with every component of this type in `world`, and its
    /// entity, in the order of the type's storage: the order its inserts
    /// and removes left it in. The type's components stay borrowed while
    /// `f` runs.
    pub fn each(&self, world: &World, f: &mut dyn FnMut(Entity, &dyn Reflect)) {
        (self.each)(world, f);
    }
}

/// The reflected component types, by the names data gives them: what
/// prefabs, levels, saves and the inspector look components up in.
pub struct TypeRegistry {
    registrations: Vec<Registration>,
    by_name: FastHashMap<&'static str, usize>,
}

impl TypeRegistry {
    /// No types.
    pub fn new() -> Self {
        Self {
            registrations: Vec::new(),
            by_name: FastHashMap::default(),
        }
    }

    /// Adds the component type `T`, under its reflected name. Adding it
    /// again does nothing.
    ///
    /// # Panics
    ///
    /// If another type has the same name: data could not tell them apart.
    pub fn register_component<T: Component + FromValue>(&mut self) {
        if let Some(&at) = self.by_name.get(T::TYPE_NAME) {
            assert!(
                self.registrations[at].type_id == TypeId::of::<T>(),
                "two component types are named `{}`: rename one of them",
                T::TYPE_NAME
            );
            return;
        }
        self.by_name.insert(T::TYPE_NAME, self.registrations.len());
        self.registrations.push(Registration {
            name: T::TYPE_NAME,
            type_id: TypeId::of::<T>(),
            construct: construct::<T>,
            register_in: World::register::<T>,
            insert: insert::<T>,
            inspect: inspect::<T>,
            inspect_mut: inspect_mut::<T>,
            each: each::<T>,
        });
    }

    /// The type named `name`.
    pub fn get(&self, name: &str) -> Option<&Registration> {
        self.by_name.get(name).map(|&at| &self.registrations[at])
    }

    /// Every type, in the order they were added.
    pub fn iter(&self) -> impl Iterator<Item = &Registration> {
        self.registrations.iter()
    }

    /// Registers every type with `world`, so that it can hold them.
    pub fn register_components(&self, world: &mut World) {
        for registration in &self.registrations {
            (registration.register_in)(world);
        }
    }

    /// A 64-bit FNV-1a hash of every component of every type here, in
    /// `world`: the same for two worlds whose components of these types
    /// have the same values, on the same entities, in the same storage
    /// order.
    ///
    /// It hashes, for each type in the order they were added, its name,
    /// then each component in its storage's order (see
    /// [`Registration::each`]): its entity's index and generation, and its
    /// [`Reflect::to_value`] with `cx`. Floats count by their bits, except
    /// that zeros of either sign count as one, and so do all NaNs: the sign
    /// of a zero from `min`, `max` or `clamp`, and a NaN's bits, can differ
    /// from one processor to another. Components of types not here, and
    /// resources, are not in it.
    ///
    /// The bytes hashed, little endian throughout, which stored hashes
    /// depend on:
    ///
    /// - for each type: its name's length as a `u64` and the name; then for
    ///   each component a 1 byte, the entity's index and generation as
    ///   `u32`s, and its value; then a 0 byte;
    /// - a value: a tag byte, then its contents: null 0; a bool 1 and a
    ///   byte of 0 or 1; an integer 2 and an `i64`; a float 3 and its `f64`
    ///   bits (0 for either zero, `0x7ff8_0000_0000_0000` for any NaN); a
    ///   string 4, its length as a `u64` and its bytes; an array 5, its
    ///   count as a `u64` and its items; an object 6, its count as a `u64`,
    ///   and for each member its key's length as a `u64`, the key and the
    ///   value.
    ///
    /// It is for tests and tools, such as determinism checks: it builds
    /// each component's [`Value`], which allocates.
    ///
    /// # Panics
    ///
    /// If a type here is not registered with `world`, or its components
    /// are borrowed mutably elsewhere, as any read of them would.
    pub fn hash_world(&self, world: &World, cx: &mut ReflectContext<'_>) -> u64 {
        let mut bytes = Vec::new();
        for registration in &self.registrations {
            put_len(&mut bytes, registration.name.len());
            bytes.extend_from_slice(registration.name.as_bytes());
            registration.each(world, &mut |entity, component| {
                bytes.push(1);
                bytes.extend_from_slice(&entity.index().to_le_bytes());
                bytes.extend_from_slice(&entity.generation().get().to_le_bytes());
                encode(&component.to_value(cx), &mut bytes);
            });
            bytes.push(0);
        }
        fnv1a64(&bytes)
    }
}

/// Writes a length or a count, as a `u64`, little endian.
fn put_len(bytes: &mut Vec<u8>, len: usize) {
    bytes.extend_from_slice(&(len as u64).to_le_bytes());
}

/// The bits that every NaN is hashed as.
const ANY_NAN: u64 = 0x7ff8_0000_0000_0000;

/// Writes `value` in a form that tells any two values apart, but for the
/// sign of a zero and the bits of a NaN: a tag byte, then its contents,
/// with every length and count before what it counts (see
/// [`TypeRegistry::hash_world`]). For hashing only: nothing reads it back.
fn encode(value: &Value, bytes: &mut Vec<u8>) {
    match value {
        Value::Null => bytes.push(0),
        Value::Bool(value) => bytes.extend_from_slice(&[1, u8::from(*value)]),
        Value::Int(value) => {
            bytes.push(2);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        Value::Float(value) => {
            let bits = if value.is_nan() {
                ANY_NAN
            } else if *value == 0.0 {
                0
            } else {
                value.to_bits()
            };
            bytes.push(3);
            bytes.extend_from_slice(&bits.to_le_bytes());
        }
        Value::String(value) => {
            bytes.push(4);
            put_len(bytes, value.len());
            bytes.extend_from_slice(value.as_bytes());
        }
        Value::Array(items) => {
            bytes.push(5);
            put_len(bytes, items.len());
            for item in items {
                encode(item, bytes);
            }
        }
        Value::Object(members) => {
            bytes.push(6);
            put_len(bytes, members.len());
            for (key, member) in members {
                put_len(bytes, key.len());
                bytes.extend_from_slice(key.as_bytes());
                encode(member, bytes);
            }
        }
    }
}

impl Default for TypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn construct<T: FromValue>(
    value: &Value,
    cx: &mut ReflectContext<'_>,
) -> Result<Box<dyn Reflect>, ReflectError> {
    Ok(Box::new(T::from_value(value, cx)?))
}

fn insert<T: Component + FromValue>(
    commands: &mut Commands<'_>,
    entity: Entity,
    value: &Value,
    cx: &mut ReflectContext<'_>,
) -> Result<(), ReflectError> {
    commands.insert(entity, T::from_value(value, cx)?);
    Ok(())
}

fn inspect<T: Component + Reflect>(
    world: &World,
    entity: Entity,
    f: &mut dyn FnMut(&dyn Reflect),
) -> bool {
    match world.read::<T>().get(entity) {
        Some(component) => {
            f(component);
            true
        }
        None => false,
    }
}

fn each<T: Component + Reflect>(world: &World, f: &mut dyn FnMut(Entity, &dyn Reflect)) {
    for (entity, component) in world.read::<T>().iter() {
        f(entity, component);
    }
}

fn inspect_mut<T: Component + Reflect>(
    world: &World,
    entity: Entity,
    f: &mut dyn FnMut(&mut dyn Reflect),
) -> bool {
    match world.write::<T>().get_mut(entity) {
        Some(component) => {
            f(component);
            true
        }
        None => false,
    }
}
