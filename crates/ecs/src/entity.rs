//! [`Entity`]: a thing in the world, by its slot and generation.

use std::fmt;
use std::num::NonZeroU32;

use nadir_core::GenIndex;

/// A thing in the world. It is only an identifier: its data are its
/// components, kept by type in the world's storages.
///
/// An entity is a slot number plus a generation. When an entity is
/// despawned and its slot is reused, the new entity has another
/// generation, so the old `Entity` stops being alive and reads none of the
/// new one's components.
///
/// It is not stable across runs: saves refer to entities by a `SaveId`
/// component instead (`docs/subsystems/ecs.md`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Entity(GenIndex);

impl Entity {
    /// The entity of an arena index.
    pub(crate) const fn new(id: GenIndex) -> Self {
        Self(id)
    }

    /// The arena index of the entity.
    pub(crate) const fn id(self) -> GenIndex {
        self.0
    }

    /// The slot number, which a later entity may reuse.
    pub const fn index(self) -> u32 {
        self.0.index()
    }

    /// The generation, which tells this entity from others in its slot.
    pub const fn generation(self) -> NonZeroU32 {
        self.0.generation()
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Entity({}v{})", self.index(), self.generation())
    }
}

#[cfg(test)]
pub(crate) fn entity(index: u32, generation: u32) -> Entity {
    let generation = NonZeroU32::new(generation).expect("generations start at 1");
    Entity::new(GenIndex::new(index, generation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entity_shows_its_slot_and_generation() {
        let e = entity(7, 3);
        assert_eq!((e.index(), e.generation().get()), (7, 3));
        assert_eq!(format!("{e:?}"), "Entity(7v3)");
        assert_ne!(e, entity(7, 4));
    }
}
