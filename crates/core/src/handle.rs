//! Generational handles: [`GenIndex`], the [`GenArena`] that hands them out,
//! and the typed [`Handle`] wrapper.
//!
//! A generational index is a slot number plus a generation counter. When a
//! slot is reused its generation changes, so an index to the old occupant
//! stops resolving instead of silently pointing at the new one.

use std::any::type_name;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::num::NonZeroU32;

/// A slot index plus the generation of the value stored there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GenIndex {
    index: u32,
    generation: NonZeroU32,
}

impl GenIndex {
    /// Creates an index from its parts.
    ///
    /// Arenas hand out their own indices; this exists for code that stores
    /// the parts separately, and for tests.
    pub const fn new(index: u32, generation: NonZeroU32) -> Self {
        Self { index, generation }
    }

    /// Returns the slot number.
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Returns the generation.
    pub const fn generation(self) -> NonZeroU32 {
        self.generation
    }
}

struct Slot<T> {
    /// The generation of the occupant or, if the slot is free, of its next
    /// occupant.
    generation: NonZeroU32,
    /// `Some` while the slot is occupied.
    value: Option<T>,
    /// The next slot on the free list. Only meaningful while this slot is on
    /// it.
    next_free: Option<u32>,
}

/// A container that stores values in reusable slots and identifies each one
/// by a [`GenIndex`].
///
/// - Insert, remove and lookup are O(1).
/// - An index stops resolving as soon as its value is removed, and never
///   resolves again: every `(index, generation)` pair is handed out at most
///   once. A slot whose generation counter is exhausted is retired
///   permanently instead of wrapping around.
/// - Iteration visits values in slot order, which depends only on the
///   history of inserts and removes.
/// - Freed slots are reused before the arena grows, so a steady-state
///   insert/remove pattern does not allocate.
pub struct GenArena<T> {
    slots: Vec<Slot<T>>,
    /// Head of the list of free slots; the most recently freed is reused
    /// first.
    free_head: Option<u32>,
    len: usize,
}

impl<T> GenArena<T> {
    /// Creates an empty arena.
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_head: None,
            len: 0,
        }
    }

    /// Creates an empty arena with room for `capacity` values before it
    /// allocates again.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: Vec::with_capacity(capacity),
            free_head: None,
            len: 0,
        }
    }

    /// Returns the number of values stored.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the arena stores no values.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Stores `value` and returns the index that identifies it.
    ///
    /// # Panics
    ///
    /// Panics if the arena would need more than `u32::MAX` slots.
    pub fn insert(&mut self, value: T) -> GenIndex {
        let (index, generation) = match self.free_head {
            Some(index) => {
                let slot = &mut self.slots[index as usize];
                self.free_head = slot.next_free.take();
                slot.value = Some(value);
                (index, slot.generation)
            }
            None => {
                let index =
                    u32::try_from(self.slots.len()).expect("GenArena is limited to u32::MAX slots");
                self.slots.push(Slot {
                    generation: NonZeroU32::MIN,
                    value: Some(value),
                    next_free: None,
                });
                (index, NonZeroU32::MIN)
            }
        };
        self.len += 1;
        GenIndex::new(index, generation)
    }

    /// Removes and returns the value at `index`, or `None` if `index` is
    /// stale.
    pub fn remove(&mut self, index: GenIndex) -> Option<T> {
        let slot = self.slots.get_mut(index.index as usize)?;
        if slot.generation != index.generation {
            return None;
        }
        let value = slot.value.take()?;
        // Free the slot for reuse under a new generation. If the generation
        // counter is exhausted the slot stays off the free list for good, so
        // the old index can never match a later occupant.
        if let Some(next_generation) = slot.generation.checked_add(1) {
            slot.generation = next_generation;
            slot.next_free = self.free_head;
            self.free_head = Some(index.index);
        }
        self.len -= 1;
        Some(value)
    }

    /// Returns the value at `index`, or `None` if `index` is stale.
    pub fn get(&self, index: GenIndex) -> Option<&T> {
        let slot = self.slots.get(index.index as usize)?;
        if slot.generation != index.generation {
            return None;
        }
        slot.value.as_ref()
    }

    /// Returns the value at `index` mutably, or `None` if `index` is stale.
    pub fn get_mut(&mut self, index: GenIndex) -> Option<&mut T> {
        let slot = self.slots.get_mut(index.index as usize)?;
        if slot.generation != index.generation {
            return None;
        }
        slot.value.as_mut()
    }

    /// Returns `true` if `index` still identifies a stored value.
    pub fn contains(&self, index: GenIndex) -> bool {
        self.get(index).is_some()
    }

    /// Iterates over the stored values and their indices, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (GenIndex, &T)> {
        self.slots.iter().zip(0u32..).filter_map(|(slot, index)| {
            let value = slot.value.as_ref()?;
            Some((GenIndex::new(index, slot.generation), value))
        })
    }

    /// Iterates mutably over the stored values and their indices, in slot
    /// order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (GenIndex, &mut T)> {
        self.slots
            .iter_mut()
            .zip(0u32..)
            .filter_map(|(slot, index)| {
                let value = slot.value.as_mut()?;
                Some((GenIndex::new(index, slot.generation), value))
            })
    }
}

impl<T> Default for GenArena<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A [`GenIndex`] tagged with the type of thing it refers to, so that a
/// handle to one kind of resource cannot be passed where another is
/// expected.
///
/// A handle is a plain identifier: it is `Copy`, it does not own or borrow
/// the `T`, and it is `Send + Sync` whatever `T` is.
pub struct Handle<T> {
    id: GenIndex,
    marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    /// Wraps an index as a handle to a `T`.
    pub const fn new(id: GenIndex) -> Self {
        Self {
            id,
            marker: PhantomData,
        }
    }

    /// Returns the untyped index.
    pub const fn id(self) -> GenIndex {
        self.id
    }
}

// These are written by hand because deriving them would add a `T: Trait`
// bound that a handle does not need.

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for Handle<T> {}

impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Handle<{}>({}v{})",
            type_name::<T>(),
            self.id.index,
            self.id.generation
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Pcg32;
    use crate::testing::check_sized;
    use std::collections::hash_map::DefaultHasher;

    fn generation(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).expect("test generations are non-zero")
    }

    #[test]
    fn gen_index_exposes_its_parts() {
        let index = GenIndex::new(7, generation(3));
        assert_eq!(index.index(), 7);
        assert_eq!(index.generation(), generation(3));
        assert_eq!(index, GenIndex::new(7, generation(3)));
        assert_ne!(index, GenIndex::new(7, generation(4)));
        assert_ne!(index, GenIndex::new(8, generation(3)));
    }

    #[test]
    fn insert_get_remove() {
        let mut arena = GenArena::new();
        assert!(arena.is_empty());
        let a = arena.insert("a");
        let b = arena.insert("b");
        assert_eq!(arena.len(), 2);
        assert_eq!((a.index(), b.index()), (0, 1));
        assert_eq!(arena.get(a), Some(&"a"));
        assert_eq!(arena.get(b), Some(&"b"));
        assert!(arena.contains(a));

        assert_eq!(arena.remove(a), Some("a"));
        assert_eq!(arena.len(), 1);
        assert_eq!(arena.get(a), None);
        assert!(!arena.contains(a));
        assert_eq!(arena.remove(a), None);
        assert_eq!(arena.len(), 1);
        assert_eq!(arena.get(b), Some(&"b"));
    }

    #[test]
    fn get_mut_changes_the_stored_value() {
        let mut arena = GenArena::with_capacity(4);
        let a = arena.insert(1);
        *arena.get_mut(a).expect("just inserted") += 10;
        assert_eq!(arena.get(a), Some(&11));
        arena.remove(a);
        assert_eq!(arena.get_mut(a), None);
    }

    #[test]
    fn a_reused_slot_gets_a_new_generation() {
        let mut arena = GenArena::new();
        let old = arena.insert("old");
        arena.remove(old);
        let new = arena.insert("new");
        assert_eq!(new.index(), old.index());
        assert_eq!(new.generation(), generation(2));
        assert_eq!(arena.get(old), None);
        assert_eq!(arena.get(new), Some(&"new"));
    }

    #[test]
    fn the_most_recently_freed_slot_is_reused_first() {
        let mut arena = GenArena::new();
        let ids: Vec<_> = (0..4).map(|n| arena.insert(n)).collect();
        arena.remove(ids[1]);
        arena.remove(ids[3]);
        assert_eq!(arena.insert(10).index(), 3);
        assert_eq!(arena.insert(11).index(), 1);
        assert_eq!(arena.insert(12).index(), 4);
    }

    #[test]
    fn forged_indices_do_not_resolve() {
        let mut arena = GenArena::new();
        let a = arena.insert(1);
        // Out of bounds.
        assert_eq!(arena.get(GenIndex::new(99, generation(1))), None);
        assert_eq!(arena.remove(GenIndex::new(99, generation(1))), None);
        // A free slot, guessed at the generation its next occupant will get.
        arena.remove(a);
        let guess = GenIndex::new(0, generation(2));
        assert_eq!(arena.get(guess), None);
        assert_eq!(arena.remove(guess), None);
        assert_eq!(arena.len(), 0);
        // The guess did not corrupt the free list.
        assert_eq!(arena.insert(2), guess);
        assert_eq!(arena.insert(3).index(), 1);
    }

    #[test]
    fn iteration_is_in_slot_order_and_skips_free_slots() {
        let mut arena = GenArena::new();
        let ids: Vec<_> = (0..5).map(|n| arena.insert(n * 10)).collect();
        arena.remove(ids[1]);
        arena.remove(ids[3]);
        let seen: Vec<_> = arena.iter().map(|(id, &value)| (id, value)).collect();
        assert_eq!(seen, [(ids[0], 0), (ids[2], 20), (ids[4], 40)]);

        for (_, value) in arena.iter_mut() {
            *value += 1;
        }
        let values: Vec<_> = arena.iter().map(|(_, &value)| value).collect();
        assert_eq!(values, [1, 21, 41]);
        assert_eq!(GenArena::<u8>::default().iter().count(), 0);
    }

    #[test]
    fn a_slot_with_an_exhausted_generation_is_retired() {
        let mut arena = GenArena::new();
        let first = arena.insert("first");
        // Fast-forward slot 0 to its last generation.
        arena.slots[0].generation = NonZeroU32::MAX;
        assert_eq!(arena.get(first), None);
        let last = GenIndex::new(0, NonZeroU32::MAX);
        assert_eq!(arena.get(last), Some(&"first"));

        assert_eq!(arena.remove(last), Some("first"));
        assert_eq!(arena.len(), 0);
        assert_eq!(arena.get(last), None);
        assert_eq!(arena.remove(last), None);
        assert_eq!(arena.iter().count(), 0);

        // The retired slot is never handed out again.
        for round in 0..10 {
            let a = arena.insert("a");
            let b = arena.insert("b");
            assert_ne!(a.index(), 0, "round {round}");
            assert_ne!(b.index(), 0, "round {round}");
            assert_eq!(arena.get(last), None);
            arena.remove(a);
            arena.remove(b);
        }
        assert_eq!(arena.slots.len(), 3);
    }

    #[test]
    fn handles_compare_copy_and_hash_by_id() {
        // Deliberately not `Clone`, `PartialEq`, `Hash` or `Debug`.
        struct Texture;

        fn hash_of(value: impl Hash) -> u64 {
            let mut hasher = DefaultHasher::new();
            value.hash(&mut hasher);
            hasher.finish()
        }

        let id = GenIndex::new(3, generation(2));
        let handle: Handle<Texture> = Handle::new(id);
        let copy = handle;
        assert_eq!(handle, copy);
        assert_eq!(handle.id(), id);
        assert_ne!(handle, Handle::new(GenIndex::new(4, generation(2))));
        assert_eq!(hash_of(handle), hash_of(id));
        let text = format!("{handle:?}");
        assert!(
            text.starts_with("Handle<") && text.ends_with("Texture>(3v2)"),
            "{text}"
        );
    }

    #[test]
    fn handles_are_send_and_sync_for_any_type() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Handle<std::rc::Rc<u8>>>();
    }

    /// The reference model: every index the arena has ever handed out, and
    /// whether its value is still stored.
    #[derive(Default)]
    struct Model {
        live: Vec<(GenIndex, u32)>,
        dead: Vec<GenIndex>,
    }

    impl Model {
        fn check(&self, arena: &GenArena<u32>) -> Result<(), String> {
            if arena.len() != self.live.len() || arena.is_empty() != self.live.is_empty() {
                return Err(format!(
                    "len is {}, model has {}",
                    arena.len(),
                    self.live.len()
                ));
            }
            for &(id, value) in &self.live {
                if arena.get(id) != Some(&value) || !arena.contains(id) {
                    return Err(format!(
                        "live {id:?} resolves to {:?}, expected {value}",
                        arena.get(id)
                    ));
                }
            }
            for &id in &self.dead {
                if arena.get(id).is_some() || arena.contains(id) {
                    return Err(format!(
                        "removed {id:?} resolves again, to {:?}",
                        arena.get(id)
                    ));
                }
            }
            let mut expected = self.live.clone();
            expected.sort_by_key(|&(id, _)| id.index());
            let actual: Vec<_> = arena.iter().map(|(id, &value)| (id, value)).collect();
            if actual != expected {
                return Err(format!("iteration gave {actual:?}, expected {expected:?}"));
            }
            Ok(())
        }
    }

    /// One step of the model test. A `pick` chooses among whatever indices
    /// exist when the step runs, so a list of operations is meaningful
    /// whatever came before it.
    #[derive(Clone, Copy, Debug)]
    enum Op {
        Insert { value: u32 },
        Remove { pick: u32 },
        Set { pick: u32, value: u32 },
        UseStale { pick: u32 },
    }

    /// Two operations per unit of size. A shorter list is a prefix of a
    /// longer one from the same case, so shrinking finds the shortest
    /// failing prefix.
    fn random_ops(rng: &mut Pcg32, size: u32) -> Vec<Op> {
        // Bias toward growth in some cases and toward churn in others.
        let insert_weight = rng.range(2..7u32);
        (0..size * 2)
            .map(|_| match rng.range(0..10u32) {
                kind if kind < insert_weight => Op::Insert {
                    value: rng.next_u32(),
                },
                kind if kind < 8 => Op::Remove {
                    pick: rng.next_u32(),
                },
                8 => Op::Set {
                    pick: rng.next_u32(),
                    value: rng.next_u32(),
                },
                _ => Op::UseStale {
                    pick: rng.next_u32(),
                },
            })
            .collect()
    }

    /// Applies `ops` to an arena and to the model, comparing them after
    /// every step.
    fn run_ops(ops: &[Op]) -> Result<(), String> {
        fn pick_from<T>(list: &[T], pick: u32) -> Option<usize> {
            (!list.is_empty()).then(|| pick as usize % list.len())
        }

        let mut arena = GenArena::new();
        let mut model = Model::default();
        for (step, &op) in ops.iter().enumerate() {
            match op {
                Op::Insert { value } => {
                    let id = arena.insert(value);
                    if model.live.iter().any(|&(live, _)| live == id) || model.dead.contains(&id) {
                        return Err(format!(
                            "step {step}: insert handed out {id:?} a second time"
                        ));
                    }
                    model.live.push((id, value));
                }
                Op::Remove { pick } => {
                    if let Some(slot) = pick_from(&model.live, pick) {
                        let (id, value) = model.live.swap_remove(slot);
                        let removed = arena.remove(id);
                        if removed != Some(value) {
                            return Err(format!(
                                "step {step}: remove({id:?}) gave {removed:?}, expected {value}"
                            ));
                        }
                        model.dead.push(id);
                    }
                }
                Op::Set { pick, value } => {
                    if let Some(slot) = pick_from(&model.live, pick) {
                        let id = model.live[slot].0;
                        match arena.get_mut(id) {
                            Some(stored) => *stored = value,
                            None => {
                                return Err(format!("step {step}: get_mut({id:?}) found nothing"));
                            }
                        }
                        model.live[slot].1 = value;
                    }
                }
                // Stale indices must be inert for every operation.
                Op::UseStale { pick } => {
                    if let Some(slot) = pick_from(&model.dead, pick) {
                        let id = model.dead[slot];
                        if arena.remove(id).is_some() || arena.get_mut(id).is_some() {
                            return Err(format!("step {step}: stale {id:?} still resolves"));
                        }
                    }
                }
            }
            model
                .check(&arena)
                .map_err(|message| format!("step {step}: {message}"))?;
        }
        Ok(())
    }

    #[test]
    fn matches_a_reference_model() {
        check_sized("gen arena model", 200, random_ops, |ops| run_ops(ops));
    }
}
