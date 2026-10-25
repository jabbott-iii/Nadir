//! [`TypeRegistry`]: the reflected component types, by name.

use std::any::TypeId;

use nadir_codec::json::Value;
use nadir_core::FastHashMap;
use nadir_ecs::{Commands, Component, Entity, World};

use crate::{FromValue, Reflect, ReflectContext, ReflectError};

type ConstructFn = fn(&Value, &mut ReflectContext<'_>) -> Result<Box<dyn Reflect>, ReflectError>;
type InsertFn =
    fn(&mut Commands<'_>, Entity, &Value, &mut ReflectContext<'_>) -> Result<(), ReflectError>;
type InspectFn = fn(&World, Entity, &mut dyn FnMut(&dyn Reflect)) -> bool;
type InspectMutFn = fn(&World, Entity, &mut dyn FnMut(&mut dyn Reflect)) -> bool;

/// What the registry knows of one component type.
pub struct Registration {
    name: &'static str,
    type_id: TypeId,
    construct: ConstructFn,
    register_in: fn(&mut World),
    insert: InsertFn,
    inspect: InspectFn,
    inspect_mut: InspectMutFn,
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
