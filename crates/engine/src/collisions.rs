//! [`Collisions`]: the entities' colliders, for movement, triggers and
//! queries, and the trigger events.

use nadir_collision::{CollisionWorld, Contact, TileGrid, Triggers, move_and_slide};
use nadir_core::Vec2;
use nadir_ecs::{Entity, Events, SparseSet};

use crate::{Collider, Transform, Velocity};

/// A collider began to overlap a trigger, in this tick. The system
/// `nadir::triggers` sends it, in the tick stage `PostMovement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerEnter {
    /// The entity with the trigger.
    pub trigger: Entity,
    /// The entity whose collider it detects.
    pub other: Entity,
}

/// A collider stopped overlapping a trigger, in this tick, or one of the
/// two left: lost its collider or its `Transform`, or was despawned. The
/// system `nadir::triggers` sends it, in the tick stage `PostMovement`.
/// Either entity may be dead by then: check with `World::is_alive`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerExit {
    /// The entity with the trigger.
    pub trigger: Entity,
    /// The entity whose collider it detected.
    pub other: Entity,
}

/// A slot of no collider.
const NONE: u32 = u32::MAX;

/// The colliders of the entities that have a [`Collider`] and a
/// [`Transform`], in a [`CollisionWorld`]: a resource that the engine
/// rebuilds each tick, in the system `nadir::movement`.
///
/// Systems query it through [`Collisions::world`], whose answers are
/// collider indices: [`Collisions::entity`] turns them into entities, and
/// [`Collisions::index`] an entity into the index to `ignore` in a cast.
/// The tile grid is set here too.
///
/// What it holds during a tick:
///
/// - From `nadir::movement` on, the colliders of this tick, each moved as
///   its entity moved.
/// - `nadir::triggers` copies every collider's position from its
///   `Transform` before it looks for overlaps, so a system that moves an
///   entity in `Movement` after the engine's movement, or in
///   `PostMovement` before `nadir::triggers`, moves its collider too.
/// - After that, and until the next tick's `nadir::movement` (in the
///   `Input` and `Gameplay` stages, and in frames), it is as the last
///   tick left it. An entity despawned since still answers, and one
///   spawned since is missing: check what [`Collisions::entity`] gives
///   with `World::is_alive`.
pub struct Collisions {
    world: CollisionWorld,
    /// Each collider's entity, by index: `[0]` for the last rebuild, `[1]`
    /// for the one before, which the exits of the last tick's pairs are
    /// looked up in.
    entities: [Vec<Entity>; 2],
    /// Each entity's collider index, by the entity's index, for the same
    /// two rebuilds. A slot may be stale: it counts only when `entities`
    /// gives back the same entity.
    slots: [Vec<u32>; 2],
    triggers: Triggers,
    /// What the last move touched. Nothing reads it yet.
    contacts: Vec<Contact>,
}

impl Collisions {
    /// No colliders and an empty tile grid, hashing in cells of
    /// [`CollisionWorld::DEFAULT_CELL_SIZE`].
    pub fn new() -> Self {
        Self::with_world(CollisionWorld::new())
    }

    /// No colliders and an empty tile grid, hashing in cells of
    /// `cell_size` pixels: use at least twice the size of a typical
    /// collider. Insert it as a resource to replace the engine's.
    ///
    /// # Panics
    ///
    /// If `cell_size` is not a finite number above 0.
    pub fn with_cell_size(cell_size: f32) -> Self {
        Self::with_world(CollisionWorld::with_cell_size(cell_size))
    }

    fn with_world(world: CollisionWorld) -> Self {
        Self {
            world,
            entities: [Vec::new(), Vec::new()],
            slots: [Vec::new(), Vec::new()],
            triggers: Triggers::new(),
            contacts: Vec::new(),
        }
    }

    /// The collision world, for queries and casts.
    pub fn world(&self) -> &CollisionWorld {
        &self.world
    }

    /// The tile grid.
    pub fn grid(&self) -> &TileGrid {
        self.world.grid()
    }

    /// The tile grid, to change tiles.
    pub fn grid_mut(&mut self) -> &mut TileGrid {
        self.world.grid_mut()
    }

    /// Replaces the tile grid.
    pub fn set_grid(&mut self, grid: TileGrid) {
        self.world.set_grid(grid);
    }

    /// The entity of collider `index`, as of the last rebuild. It may have
    /// been despawned since (see [`Collisions`]).
    ///
    /// # Panics
    ///
    /// If there is no collider `index`.
    pub fn entity(&self, index: u32) -> Entity {
        match self.entities[0].get(index as usize) {
            Some(&entity) => entity,
            None => panic!(
                "there is no collider {index}: the world has {}",
                self.entities[0].len()
            ),
        }
    }

    /// The index of `entity`'s collider as of the last rebuild, or `None`
    /// if it had none then.
    pub fn index(&self, entity: Entity) -> Option<u32> {
        let index = *self.slots[0].get(entity.index() as usize)?;
        (self.entities[0].get(index as usize) == Some(&entity)).then_some(index)
    }

    /// Replaces the colliders with those of the entities that have a
    /// `Collider` and a `Transform`, each at its entity's position.
    pub(crate) fn rebuild(
        &mut self,
        colliders: &SparseSet<Collider>,
        transforms: &SparseSet<Transform>,
    ) {
        self.entities.swap(0, 1);
        self.slots.swap(0, 1);
        let entities = &mut self.entities[0];
        let slots = &mut self.slots[0];
        entities.clear();
        // In the colliders' order, which their insert and remove history
        // fixes: the order the movers move in.
        let colliders = colliders.iter().filter_map(|(entity, collider)| {
            let transform = transforms.get(entity)?;
            let at = entity.index() as usize;
            if slots.len() <= at {
                slots.resize(at + 1, NONE);
            }
            slots[at] = u32::try_from(entities.len()).unwrap_or(NONE);
            entities.push(entity);
            Some((bits(entity), transform.position, collider.0))
        });
        self.world.rebuild(colliders);
    }

    /// Moves every collider whose entity has a `Velocity` by its velocity
    /// for `dt` seconds, sliding along what it meets, in index order: each
    /// sees where the ones before it went. Writes their `Transform`s.
    pub(crate) fn move_colliders(
        &mut self,
        velocities: &SparseSet<Velocity>,
        transforms: &mut SparseSet<Transform>,
        dt: f32,
    ) {
        for (index, &entity) in (0u32..).zip(&self.entities[0]) {
            let Some(velocity) = velocities.get(entity) else {
                continue;
            };
            let delta = velocity.0 * dt;
            let Some(transform) = transforms.get_mut(entity) else {
                continue;
            };
            if delta == Vec2::ZERO {
                continue;
            }
            let end = move_and_slide(
                &self.world,
                index,
                transform.position,
                delta,
                &mut self.contacts,
            );
            self.world.set_position(index, end);
            transform.position = end;
        }
    }

    /// Moves every collider to its entity's position, where they differ.
    pub(crate) fn sync(&mut self, transforms: &SparseSet<Transform>) {
        for (index, &entity) in (0u32..).zip(&self.entities[0]) {
            if let Some(transform) = transforms.get(entity)
                && self.world.position(index) != transform.position
            {
                self.world.set_position(index, transform.position);
            }
        }
    }

    /// Finds the trigger pairs that began and ended since the last call,
    /// and sends them as events.
    pub(crate) fn update_triggers(
        &mut self,
        enter: &mut Events<TriggerEnter>,
        exit: &mut Events<TriggerExit>,
    ) {
        self.triggers.update(&self.world);
        for pair in self.triggers.entered() {
            if let (Some(trigger), Some(other)) =
                (self.resolve(0, pair.trigger), self.resolve(0, pair.other))
            {
                enter.send(TriggerEnter { trigger, other });
            }
        }
        // The pairs that ended were found among the last rebuild's
        // colliders.
        for pair in self.triggers.exited() {
            if let (Some(trigger), Some(other)) =
                (self.resolve(1, pair.trigger), self.resolve(1, pair.other))
            {
                exit.send(TriggerExit { trigger, other });
            }
        }
    }

    /// The entity whose collider had `id` in rebuild `which`: 0 for the
    /// last, 1 for the one before.
    fn resolve(&self, which: usize, id: u64) -> Option<Entity> {
        // The low half of an id is its entity's index.
        let index = *self.slots[which].get(id as u32 as usize)?;
        let entity = *self.entities[which].get(index as usize)?;
        // In a tick, a pair's ids come from the rebuild it is looked up in,
        // which wrote this slot for this very entity. A `Collisions` put in
        // mid-tick, between the movement and the triggers, breaks that:
        // its pair is dropped rather than given to the wrong entity.
        (bits(entity) == id).then_some(entity)
    }
}

impl Default for Collisions {
    fn default() -> Self {
        Self::new()
    }
}

/// A collider's id: its entity's generation and index, which tell it from
/// every other entity there ever was.
fn bits(entity: Entity) -> u64 {
    (u64::from(entity.generation().get()) << 32) | u64::from(entity.index())
}
