//! [`Commands`]: structural changes, deferred until the schedule applies
//! them.

use std::any::TypeId;

use crate::world::Op;
use crate::{Component, Entity, World};

/// A system's structural changes: spawning and despawning entities, and
/// adding and removing components. They are made in the order they were
/// queued when the world applies them ([`World::apply_commands`]), which
/// the schedule does after each tick and after the frame, never while
/// systems iterate.
///
/// ```
/// use nadir_ecs::{Component, World};
///
/// struct Position(f32);
/// impl Component for Position {}
///
/// let mut world = World::new();
/// world.register::<Position>();
/// let mut commands = world.commands();
/// let shot = commands.spawn().with(Position(4.0)).id();
/// // The entity exists at once; its components come with the commands.
/// assert!(world.is_alive(shot) && world.read::<Position>().get(shot).is_none());
/// world.apply_commands();
/// assert_eq!(world.read::<Position>().get(shot).map(|p| p.0), Some(4.0));
/// ```
///
/// A component's value waits in its own type's queue, and the queues are
/// reused, so a steady stream of commands allocates nothing once they have
/// grown.
pub struct Commands<'w> {
    world: &'w World,
}

impl<'w> Commands<'w> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self { world }
    }

    /// Spawns an entity, to add components to. The entity is alive at once;
    /// its components are added when the commands are applied.
    pub fn spawn(&mut self) -> EntityCommands<'w> {
        EntityCommands {
            world: self.world,
            entity: self.world.reserve(),
        }
    }

    /// The commands of `entity`, to add or remove its components.
    pub fn entity(&mut self, entity: Entity) -> EntityCommands<'w> {
        EntityCommands {
            world: self.world,
            entity,
        }
    }

    /// Gives `entity` the component `value`, or replaces the one it has. If
    /// the entity is despawned first, nothing happens.
    ///
    /// # Panics
    ///
    /// If `T` is not registered.
    pub fn insert<T: Component>(&mut self, entity: Entity, value: T) {
        insert(self.world, entity, value);
    }

    /// Takes `entity`'s component of type `T` away, if it has one.
    ///
    /// # Panics
    ///
    /// If `T` is not registered.
    pub fn remove<T: Component>(&mut self, entity: Entity) {
        remove::<T>(self.world, entity);
    }

    /// Despawns `entity`, with all its components. Despawning a dead entity
    /// does nothing.
    pub fn despawn(&mut self, entity: Entity) {
        self.world.queue(Op::Despawn(entity));
    }
}

/// The commands of one entity: [`Commands::spawn`] and [`Commands::entity`].
pub struct EntityCommands<'w> {
    world: &'w World,
    entity: Entity,
}

impl EntityCommands<'_> {
    /// Gives the entity the component `value`, as [`Commands::insert`].
    pub fn with<T: Component>(self, value: T) -> Self {
        insert(self.world, self.entity, value);
        self
    }

    /// Takes the entity's component of type `T` away, as
    /// [`Commands::remove`].
    pub fn without<T: Component>(self) -> Self {
        remove::<T>(self.world, self.entity);
        self
    }

    /// The entity.
    pub fn id(&self) -> Entity {
        self.entity
    }
}

fn insert<T: Component>(world: &World, entity: Entity, value: T) {
    world
        .storage::<T>()
        .pending
        .borrow_mut()
        .push_back((entity, value));
    world.queue(Op::Insert(TypeId::of::<T>()));
}

fn remove<T: Component>(world: &World, entity: Entity) {
    world.storage::<T>();
    world.queue(Op::Remove(TypeId::of::<T>(), entity));
}
