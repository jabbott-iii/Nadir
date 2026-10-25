//! [`SparseSet`]: the storage of one component type.

use crate::Entity;

/// A free entry of [`SparseSet::sparse`].
const EMPTY: u32 = u32::MAX;

/// The components of one type, by entity.
///
/// The components sit in a dense array, so iterating them is a walk over
/// contiguous memory. A sparse array, indexed by entity slot, says where in
/// the dense array each entity's component is. Insert, remove and lookup
/// are O(1).
///
/// The order of iteration depends only on the history of inserts and
/// removes, which keeps replays deterministic. A remove moves the last
/// component into the removed one's place.
///
/// The arrays only grow, so a steady pattern of inserts and removes
/// allocates nothing.
pub struct SparseSet<T> {
    /// Entity slot to dense slot, or [`EMPTY`].
    sparse: Vec<u32>,
    /// Dense slot to entity.
    dense: Vec<Entity>,
    /// Dense slot to component, beside `dense`.
    data: Vec<T>,
}

impl<T> SparseSet<T> {
    /// An empty set.
    pub const fn new() -> Self {
        Self {
            sparse: Vec::new(),
            dense: Vec::new(),
            data: Vec::new(),
        }
    }

    /// The number of components.
    pub fn len(&self) -> usize {
        self.dense.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.dense.is_empty()
    }

    /// Gives `entity` the component `value`, and returns the one it had.
    ///
    /// The set does not know which entities are alive; the world does.
    /// Generations only grow, so it can still tell an older entity of the
    /// same slot from a newer one:
    ///
    /// - A component left in the slot by an older entity is dropped, and
    ///   `value` takes its place.
    /// - If a newer entity has the slot, `entity` is stale: `value` is
    ///   dropped, and the newer entity keeps its component.
    pub fn insert(&mut self, entity: Entity, value: T) -> Option<T> {
        let index = entity.index() as usize;
        if let Some(slot) = self.slot_of_index(index) {
            let stored = self.dense[slot];
            if stored.generation() > entity.generation() {
                return None;
            }
            let old = std::mem::replace(&mut self.data[slot], value);
            self.dense[slot] = entity;
            return (stored == entity).then_some(old);
        }
        if index >= self.sparse.len() {
            self.sparse.resize(index + 1, EMPTY);
        }
        let slot = u32::try_from(self.dense.len())
            .ok()
            .filter(|&slot| slot != EMPTY)
            .expect("a sparse set holds fewer than u32::MAX components");
        self.sparse[index] = slot;
        self.dense.push(entity);
        self.data.push(value);
        self.debug_check(slot as usize);
        None
    }

    /// Takes `entity`'s component away, and returns it.
    pub fn remove(&mut self, entity: Entity) -> Option<T> {
        let slot = self.slot(entity)?;
        self.sparse[entity.index() as usize] = EMPTY;
        self.dense.swap_remove(slot);
        let value = self.data.swap_remove(slot);
        // The last component moved into the freed place.
        if let Some(&moved) = self.dense.get(slot) {
            self.sparse[moved.index() as usize] = slot as u32;
            self.debug_check(slot);
        }
        Some(value)
    }

    /// Whether `entity` has a component here.
    pub fn contains(&self, entity: Entity) -> bool {
        self.slot(entity).is_some()
    }

    /// `entity`'s component.
    pub fn get(&self, entity: Entity) -> Option<&T> {
        self.slot(entity).map(|slot| &self.data[slot])
    }

    /// `entity`'s component, to change it.
    pub fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
        self.slot(entity).map(|slot| &mut self.data[slot])
    }

    /// Every entity and its component, in storage order.
    pub fn iter(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.dense.iter().copied().zip(&self.data)
    }

    /// Every entity and its component, to change them, in storage order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, &mut T)> {
        self.dense.iter().copied().zip(&mut self.data)
    }

    /// The entities, in storage order.
    pub fn entities(&self) -> &[Entity] {
        &self.dense
    }

    /// The entity and component in dense slot `slot`.
    pub(crate) fn at(&self, slot: usize) -> Option<(Entity, &T)> {
        Some((*self.dense.get(slot)?, &self.data[slot]))
    }

    /// Where `entity`'s component is in the dense arrays.
    fn slot(&self, entity: Entity) -> Option<usize> {
        self.slot_of_index(entity.index() as usize)
            .filter(|&slot| self.dense[slot] == entity)
    }

    /// Where the component of the entity in slot `index` is, whatever its
    /// generation.
    fn slot_of_index(&self, index: usize) -> Option<usize> {
        match self.sparse.get(index) {
            Some(&slot) if slot != EMPTY => Some(slot as usize),
            _ => None,
        }
    }

    /// Checks, in debug builds, that the arrays agree about `slot`.
    fn debug_check(&self, slot: usize) {
        debug_assert_eq!(self.dense.len(), self.data.len());
        debug_assert_eq!(
            self.sparse[self.dense[slot].index() as usize] as usize,
            slot,
            "the sparse array points back at dense slot {slot}"
        );
    }

    /// What the arrays hold before they must grow, for tests: the sum of
    /// the three.
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.sparse.capacity() + self.dense.capacity() + self.data.capacity()
    }

    /// Checks every invariant: the dense arrays are as long as each other,
    /// and the sparse array points at each entity's slot and nowhere else.
    #[cfg(test)]
    pub(crate) fn check_invariants(&self) -> Result<(), String> {
        if self.dense.len() != self.data.len() {
            return Err(format!(
                "{} entities, {} components",
                self.dense.len(),
                self.data.len()
            ));
        }
        for (slot, entity) in self.dense.iter().enumerate() {
            let back = self.sparse.get(entity.index() as usize).copied();
            if back != Some(slot as u32) {
                return Err(format!(
                    "{entity:?} is in slot {slot}, but sparse says {back:?}"
                ));
            }
        }
        let used = self.sparse.iter().filter(|&&slot| slot != EMPTY).count();
        if used != self.dense.len() {
            return Err(format!(
                "{used} sparse entries for {} components",
                self.dense.len()
            ));
        }
        Ok(())
    }
}

impl<T> Default for SparseSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::entity;

    #[test]
    fn components_are_found_by_entity_and_replaced() {
        let mut set = SparseSet::new();
        let (a, b) = (entity(3, 1), entity(0, 1));
        assert_eq!(set.insert(a, "a"), None);
        assert_eq!(set.insert(b, "b"), None);
        assert_eq!(set.insert(a, "A"), Some("a"));
        assert_eq!(
            (set.get(a), set.get(b), set.len()),
            (Some(&"A"), Some(&"b"), 2)
        );
        *set.get_mut(b).expect("b's") = "B";
        let all: Vec<_> = set.iter().collect();
        assert_eq!(all, [(a, &"A"), (b, &"B")]);
        set.check_invariants().expect("consistent");
    }

    #[test]
    fn a_remove_moves_the_last_component_into_the_gap() {
        let mut set = SparseSet::new();
        let entities: Vec<_> = (0..4).map(|index| entity(index, 1)).collect();
        for (value, &e) in entities.iter().enumerate() {
            set.insert(e, value);
        }
        assert_eq!(set.remove(entities[1]), Some(1));
        assert_eq!(set.entities(), [entities[0], entities[3], entities[2]]);
        assert_eq!(set.get(entities[3]), Some(&3));
        assert_eq!(set.remove(entities[1]), None, "already gone");
        // Removing the last one moves nothing.
        assert_eq!(set.remove(entities[2]), Some(2));
        set.check_invariants().expect("consistent");
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn a_stale_entity_reads_nothing_of_its_slots_new_occupant() {
        let mut set = SparseSet::new();
        let (old, new) = (entity(5, 1), entity(5, 2));
        set.insert(new, 'n');
        assert_eq!((set.get(old), set.contains(old)), (None, false));
        assert_eq!(set.remove(old), None);
        assert_eq!(set.get(new), Some(&'n'));
        // A stale entity's insert leaves the new occupant's component.
        assert_eq!(set.insert(old, 'o'), None);
        assert_eq!((set.get(new), set.get(old)), (Some(&'n'), None));
        // A newer entity's insert replaces what an older one left.
        let newest = entity(5, 3);
        assert_eq!(set.insert(newest, 'x'), None);
        assert_eq!(
            (set.get(new), set.get(newest), set.len()),
            (None, Some(&'x'), 1)
        );
        set.check_invariants().expect("consistent");
    }

    #[test]
    fn zero_sized_tags_work_like_any_component() {
        struct Enemy;
        let mut set = SparseSet::new();
        set.insert(entity(2, 1), Enemy);
        assert!(set.contains(entity(2, 1)));
        assert!(set.remove(entity(2, 1)).is_some());
        assert!(set.is_empty());
    }
}
