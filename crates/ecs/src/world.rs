//! [`World`]: the entities, their components by type, and the resources.

use std::any::{Any, TypeId, type_name};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::VecDeque;

use nadir_core::log::debug;
use nadir_core::{FastHashMap, GenArena};

use crate::events::EventClears;
use crate::system::current_system;
use crate::{Commands, Entity, SparseSet};

/// A type that entities can have as a component. Implement it for each
/// component type, and register the type with [`World::register`] before
/// using it:
///
/// ```
/// use nadir_ecs::{Component, World};
///
/// struct Health(i32);
/// impl Component for Health {}
///
/// let mut world = World::new();
/// world.register::<Health>();
/// let player = world.spawn().with(Health(10)).id();
/// assert_eq!(world.read::<Health>().get(player).map(|h| h.0), Some(10));
/// ```
pub trait Component: 'static {}

/// The components of type `T`, borrowed to read: [`World::read`].
pub type Read<'a, T> = Ref<'a, SparseSet<T>>;
/// The components of type `T`, borrowed to change: [`World::write`].
pub type Write<'a, T> = RefMut<'a, SparseSet<T>>;
/// A resource, borrowed to read: [`World::resource`].
pub type Res<'a, R> = Ref<'a, R>;
/// A resource, borrowed to change: [`World::resource_mut`].
pub type ResMut<'a, R> = RefMut<'a, R>;

/// One component type's storage, and the inserts that commands queued
/// for it, in order.
pub(crate) struct Storage<T> {
    pub(crate) set: RefCell<SparseSet<T>>,
    pub(crate) pending: RefCell<VecDeque<(Entity, T)>>,
}

/// A [`Storage`] of any type, for what the world does to every storage.
trait ErasedStorage {
    fn as_any(&self) -> &dyn Any;
    /// Takes `entity`'s component away, if it has one.
    fn remove(&mut self, entity: Entity);
    /// Makes the oldest queued insert, if its entity is alive.
    fn apply_insert(&mut self, entities: &GenArena<()>);
    /// What its buffers hold before they must grow, for tests.
    #[cfg(test)]
    fn capacities(&self) -> [usize; 2];
}

impl<T: Component> ErasedStorage for Storage<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn remove(&mut self, entity: Entity) {
        self.set.get_mut().remove(entity);
    }

    fn apply_insert(&mut self, entities: &GenArena<()>) {
        let Some((entity, value)) = self.pending.get_mut().pop_front() else {
            return;
        };
        if entities.contains(entity.id()) {
            self.set.get_mut().insert(entity, value);
        } else {
            debug!(
                "a `{}` was not added to {entity:?}, which was despawned first",
                type_name::<T>()
            );
        }
    }

    #[cfg(test)]
    fn capacities(&self) -> [usize; 2] {
        [
            self.set.borrow().capacity(),
            self.pending.borrow().capacity(),
        ]
    }
}

/// A structural change that commands queued, in the order they were made.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Op {
    /// The oldest queued insert of this component type.
    Insert(TypeId),
    Remove(TypeId, Entity),
    Despawn(Entity),
}

/// The entities, their components by type, and the resources: the state
/// that systems share.
///
/// Components and resources are borrowed at run time, as with a `RefCell`.
/// Borrowing something that is already borrowed mutably, or borrowing it
/// mutably while it is borrowed, panics with a message that names the type
/// and the running system. So does asking for a component type that is not
/// registered, or a resource there is none of.
///
/// Systems change the world's structure (spawning, despawning, adding and
/// removing components) through [`World::commands`], which the schedule
/// applies after each tick and frame. Setup code and tests can use
/// [`World::spawn`] and [`World::despawn`] directly.
pub struct World {
    entities: RefCell<GenArena<()>>,
    storages: FastHashMap<TypeId, Box<dyn ErasedStorage>>,
    resources: FastHashMap<TypeId, RefCell<Box<dyn Any>>>,
    commands: RefCell<Vec<Op>>,
    /// How to clear each event type's buffers.
    pub(crate) events: Vec<EventClears>,
}

impl World {
    /// An empty world.
    pub fn new() -> Self {
        Self {
            entities: RefCell::new(GenArena::new()),
            storages: FastHashMap::default(),
            resources: FastHashMap::default(),
            commands: RefCell::new(Vec::new()),
            events: Vec::new(),
        }
    }

    /// Makes `T` a component type of this world. Registering it again does
    /// nothing.
    pub fn register<T: Component>(&mut self) {
        self.storages.entry(TypeId::of::<T>()).or_insert_with(|| {
            Box::new(Storage::<T> {
                set: RefCell::new(SparseSet::new()),
                pending: RefCell::new(VecDeque::new()),
            })
        });
    }

    /// Whether `T` is a registered component type.
    pub fn is_registered<T: Component>(&self) -> bool {
        self.storages.contains_key(&TypeId::of::<T>())
    }

    /// Borrows the components of type `T`, to read them.
    ///
    /// # Panics
    ///
    /// If `T` is not registered, or its components are borrowed mutably.
    pub fn read<T: Component>(&self) -> Read<'_, T> {
        match self.storage::<T>().set.try_borrow() {
            Ok(set) => set,
            Err(_) => conflict("component", type_name::<T>(), "being changed elsewhere"),
        }
    }

    /// Borrows the components of type `T`, to change them.
    ///
    /// # Panics
    ///
    /// If `T` is not registered, or its components are borrowed.
    pub fn write<T: Component>(&self) -> Write<'_, T> {
        match self.storage::<T>().set.try_borrow_mut() {
            Ok(set) => set,
            Err(_) => conflict("component", type_name::<T>(), "borrowed elsewhere"),
        }
    }

    /// Adds `resource`, or replaces the one of its type.
    pub fn insert_resource<R: 'static>(&mut self, resource: R) {
        self.resources
            .insert(TypeId::of::<R>(), RefCell::new(Box::new(resource)));
    }

    /// Whether there is a resource of type `R`.
    pub fn contains_resource<R: 'static>(&self) -> bool {
        self.resources.contains_key(&TypeId::of::<R>())
    }

    /// Borrows the resource of type `R`.
    ///
    /// # Panics
    ///
    /// If there is none, or it is borrowed mutably.
    pub fn resource<R: 'static>(&self) -> Res<'_, R> {
        match self.resource_cell::<R>().try_borrow() {
            Ok(value) => Ref::map(value, downcast_ref::<R>),
            Err(_) => conflict("resource", type_name::<R>(), "being changed elsewhere"),
        }
    }

    /// Borrows the resource of type `R` mutably.
    ///
    /// # Panics
    ///
    /// If there is none, or it is borrowed.
    pub fn resource_mut<R: 'static>(&self) -> ResMut<'_, R> {
        match self.resource_cell::<R>().try_borrow_mut() {
            Ok(value) => RefMut::map(value, downcast_mut::<R>),
            Err(_) => conflict("resource", type_name::<R>(), "borrowed elsewhere"),
        }
    }

    /// The commands of a system: structural changes, made when the
    /// schedule applies them, after each tick and after the frame.
    pub fn commands(&self) -> Commands<'_> {
        Commands::new(self)
    }

    /// Whether `entity` is alive: spawned, and not despawned since.
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.entities.borrow().contains(entity.id())
    }

    /// How many entities are alive.
    pub fn entity_count(&self) -> usize {
        self.entities.borrow().len()
    }

    /// Spawns an entity with no components, to add them to.
    pub fn spawn(&mut self) -> EntityMut<'_> {
        let entity = Entity::new(self.entities.get_mut().insert(()));
        EntityMut {
            world: self,
            entity,
        }
    }

    /// Despawns `entity`, with all its components. Returns whether it was
    /// alive; despawning a dead entity does nothing.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if self.entities.get_mut().remove(entity.id()).is_none() {
            debug!("{entity:?} was despawned already");
            return false;
        }
        for storage in self.storages.values_mut() {
            storage.remove(entity);
        }
        true
    }

    /// Makes every queued command, in the order they were queued.
    pub fn apply_commands(&mut self) {
        // By index, so that the queue keeps its allocation.
        for at in 0..self.commands.get_mut().len() {
            match self.commands.get_mut()[at] {
                Op::Insert(type_id) => {
                    if let Some(storage) = self.storages.get_mut(&type_id) {
                        storage.apply_insert(self.entities.get_mut());
                    }
                }
                Op::Remove(type_id, entity) => {
                    if let Some(storage) = self.storages.get_mut(&type_id) {
                        storage.remove(entity);
                    }
                }
                Op::Despawn(entity) => {
                    self.despawn(entity);
                }
            }
        }
        self.commands.get_mut().clear();
    }

    /// Reserves a new entity, alive at once, for a command.
    pub(crate) fn reserve(&self) -> Entity {
        Entity::new(self.entities.borrow_mut().insert(()))
    }

    /// Queues a command.
    pub(crate) fn queue(&self, op: Op) {
        self.commands.borrow_mut().push(op);
    }

    /// The storage of `T`.
    ///
    /// # Panics
    ///
    /// If `T` is not registered.
    pub(crate) fn storage<T: Component>(&self) -> &Storage<T> {
        let Some(storage) = self.storages.get(&TypeId::of::<T>()) else {
            panic!(
                "the component `{}` is not registered (system `{}`): call \
                 `World::register::<{0}>()` first",
                type_name::<T>(),
                current_system()
            );
        };
        storage
            .as_any()
            .downcast_ref::<Storage<T>>()
            .expect("a storage is kept under its own type")
    }

    /// The resource of type `R`, with nothing else borrowing it.
    pub(crate) fn resource_get_mut<R: 'static>(&mut self) -> Option<&mut R> {
        self.resources
            .get_mut(&TypeId::of::<R>())
            .map(|cell| downcast_mut::<R>(cell.get_mut()))
    }

    fn resource_cell<R: 'static>(&self) -> &RefCell<Box<dyn Any>> {
        match self.resources.get(&TypeId::of::<R>()) {
            Some(cell) => cell,
            None => panic!(
                "there is no resource of type `{}` (system `{}`): insert one with \
                 `World::insert_resource`",
                type_name::<R>(),
                current_system()
            ),
        }
    }

    /// The commands queued and not yet applied, for tests.
    #[cfg(test)]
    pub(crate) fn queued(&self) -> usize {
        self.commands.borrow().len()
    }

    /// What the world's buffers hold before they must grow, for tests: the
    /// command queue's, each storage's in a fixed order, then each event
    /// type's. (The entity arena reuses freed slots; `nadir-core` tests
    /// that.)
    #[cfg(test)]
    pub(crate) fn capacities(&self) -> Vec<usize> {
        let mut storages: Vec<_> = self.storages.iter().collect();
        storages.sort_by_key(|(type_id, _)| **type_id);
        let mut all = vec![self.commands.borrow().capacity()];
        for (_, storage) in storages {
            all.extend(storage.capacities());
        }
        for clears in &self.events {
            all.extend((clears.capacities)(self));
        }
        all
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

fn downcast_ref<R: 'static>(value: &Box<dyn Any>) -> &R {
    value
        .downcast_ref::<R>()
        .expect("a resource is kept under its own type")
}

fn downcast_mut<R: 'static>(value: &mut Box<dyn Any>) -> &mut R {
    value
        .downcast_mut::<R>()
        .expect("a resource is kept under its own type")
}

/// Panics for a borrow that conflicts with another.
fn conflict(kind: &str, name: &str, how: &str) -> ! {
    panic!(
        "the {kind} `{name}` is {how} (system `{}`): end the other borrow first",
        current_system()
    )
}

/// A newly spawned entity, to add components to: [`World::spawn`].
pub struct EntityMut<'w> {
    world: &'w mut World,
    entity: Entity,
}

impl EntityMut<'_> {
    /// Gives the entity the component `value`.
    ///
    /// # Panics
    ///
    /// If `T` is not registered.
    pub fn with<T: Component>(self, value: T) -> Self {
        self.world.write::<T>().insert(self.entity, value);
        self
    }

    /// The entity.
    pub fn id(&self) -> Entity {
        self.entity
    }
}
