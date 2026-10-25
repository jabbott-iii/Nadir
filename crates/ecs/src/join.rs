//! Iterating several component types: the common shapes.
//!
//! Iteration is driven from one storage, and the others are looked up by
//! entity. That needs no `unsafe`, and it is fast when the driving storage
//! is the smaller one.

use crate::{Entity, SparseSet};

/// The entities that have both an `A` and a `B`, with both: read-only. It
/// walks the smaller of the two sets and looks each entity up in the
/// other, so its order is the smaller set's.
pub fn join2<'a, A, B>(a: &'a SparseSet<A>, b: &'a SparseSet<B>) -> Join2<'a, A, B> {
    Join2 {
        a,
        b,
        a_drives: a.len() <= b.len(),
        next: 0,
    }
}

/// The iterator of [`join2`].
pub struct Join2<'a, A, B> {
    a: &'a SparseSet<A>,
    b: &'a SparseSet<B>,
    a_drives: bool,
    /// The next dense slot of the driving set.
    next: usize,
}

impl<'a, A, B> Iterator for Join2<'a, A, B> {
    type Item = (Entity, &'a A, &'a B);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let at = self.next;
            self.next += 1;
            if self.a_drives {
                let (entity, a) = self.a.at(at)?;
                if let Some(b) = self.b.get(entity) {
                    return Some((entity, a, b));
                }
            } else {
                let (entity, b) = self.b.at(at)?;
                if let Some(a) = self.a.get(entity) {
                    return Some((entity, a, b));
                }
            }
        }
    }
}

/// Calls `f` with every entity that has both an `A` and a `B`, its `A` to
/// change and its `B` to read, in `a`'s order.
pub fn for_each_mut_with<A, B>(
    a: &mut SparseSet<A>,
    b: &SparseSet<B>,
    mut f: impl FnMut(Entity, &mut A, &B),
) {
    for (entity, a) in a.iter_mut() {
        if let Some(b) = b.get(entity) {
            f(entity, a, b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::entity;

    #[test]
    fn a_join_finds_the_entities_with_both_from_either_side() {
        let (mut small, mut big) = (SparseSet::new(), SparseSet::new());
        for index in 0..6 {
            big.insert(entity(index, 1), index * 10);
        }
        small.insert(entity(4, 1), 'e');
        small.insert(entity(9, 1), 'x');
        small.insert(entity(1, 1), 'b');
        let from_small: Vec<_> = join2(&small, &big).collect();
        assert_eq!(
            from_small,
            [(entity(4, 1), &'e', &40), (entity(1, 1), &'b', &10)]
        );
        let from_big: Vec<_> = join2(&big, &small).map(|(e, _, _)| e).collect();
        assert_eq!(
            from_big,
            [entity(4, 1), entity(1, 1)],
            "in the smaller set's order"
        );
    }

    #[test]
    fn each_entity_with_both_is_changed_once() {
        let (mut position, mut velocity) = (SparseSet::new(), SparseSet::new());
        for index in 0..4 {
            position.insert(entity(index, 1), 0.0f32);
        }
        velocity.insert(entity(2, 1), 2.5f32);
        velocity.insert(entity(3, 1), -1.0f32);
        for_each_mut_with(&mut position, &velocity, |_, p, v| *p += v);
        let moved: Vec<f32> = position.iter().map(|(_, &p)| p).collect();
        assert_eq!(moved, [0.0, 0.0, 2.5, -1.0]);
    }
}
