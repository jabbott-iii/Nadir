//! [`Triggers`]: which triggers began and stopped overlapping what, tick
//! by tick.

use crate::geometry;
use crate::{ColliderKind, CollisionWorld};

/// A trigger and a collider it overlaps, by the ids the caller gave them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TriggerPair {
    /// The trigger's id.
    pub trigger: u64,
    /// The id of the collider it overlaps.
    pub other: u64,
}

/// The pairs of a trigger and a collider it overlaps, from one
/// [`update`](Self::update) to the next: which began, which ended.
///
/// A trigger detects another collider, solid or trigger, when its mask has
/// the other's layer and their shapes overlap. Pairs are by id, sorted,
/// so the same ticks give the same pairs in the same order; and as ids
/// last across rebuilds, a pair that goes on overlapping is not reported
/// again. A collider that leaves the world ends its pairs.
///
/// The buffers are kept, so a steady number of pairs allocates nothing.
#[derive(Debug, Default)]
pub struct Triggers {
    current: Vec<TriggerPair>,
    previous: Vec<TriggerPair>,
    entered: Vec<TriggerPair>,
    exited: Vec<TriggerPair>,
}

impl Triggers {
    /// No pairs yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Finds the pairs that overlap in `world` now, and compares them with
    /// the last update's.
    pub fn update(&mut self, world: &CollisionWorld) {
        std::mem::swap(&mut self.current, &mut self.previous);
        self.current.clear();
        for trigger in 0..world.len() as u32 {
            let collider = world.collider(trigger);
            if collider.kind != ColliderKind::Trigger {
                continue;
            }
            let placed = world.placed(trigger);
            let bounds = collider.bounds(world.position(trigger));
            world.candidates(bounds, |other| {
                if other != trigger
                    && collider.mask.intersects(world.collider(other).layer)
                    && geometry::overlaps(placed, world.placed(other))
                {
                    self.current.push(TriggerPair {
                        trigger: world.id(trigger),
                        other: world.id(other),
                    });
                }
            });
        }
        self.current.sort_unstable();
        self.current.dedup();

        self.entered.clear();
        self.exited.clear();
        let (mut now, mut before) = (0, 0);
        while now < self.current.len() || before < self.previous.len() {
            match (self.current.get(now), self.previous.get(before)) {
                (Some(a), Some(b)) if a == b => {
                    now += 1;
                    before += 1;
                }
                (Some(&a), Some(b)) if a < *b => {
                    self.entered.push(a);
                    now += 1;
                }
                (Some(&a), None) => {
                    self.entered.push(a);
                    now += 1;
                }
                (_, Some(&b)) => {
                    self.exited.push(b);
                    before += 1;
                }
                (None, None) => break,
            }
        }
    }

    /// The pairs that began to overlap in the last update, sorted.
    pub fn entered(&self) -> &[TriggerPair] {
        &self.entered
    }

    /// The pairs that stopped overlapping in the last update, sorted.
    pub fn exited(&self) -> &[TriggerPair] {
        &self.exited
    }

    /// The pairs that overlap as of the last update, sorted.
    pub fn overlapping(&self) -> &[TriggerPair] {
        &self.current
    }

    /// The capacity of each buffer, to check that a steady tick keeps
    /// them.
    #[cfg(test)]
    pub(crate) fn capacities(&self) -> [usize; 4] {
        [
            self.current.capacity(),
            self.previous.capacity(),
            self.entered.capacity(),
            self.exited.capacity(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use nadir_core::Vec2;

    use super::*;
    use crate::{Collider, LayerMask, Shape};

    fn pair(trigger: u64, other: u64) -> TriggerPair {
        TriggerPair { trigger, other }
    }

    #[test]
    fn pairs_enter_once_and_exit_once() {
        let zone = Collider::trigger(Shape::Circle { radius: 10.0 });
        let player = Collider::solid(Shape::Aabb { half: Vec2::ONE });
        let ghost = Collider {
            layer: LayerMask::bit(5),
            ..player
        };
        let picky = Collider {
            mask: LayerMask::bit(0),
            ..zone
        };
        let mut world = CollisionWorld::new();
        let mut triggers = Triggers::new();
        let tick = |world: &mut CollisionWorld, triggers: &mut Triggers, x: f32| {
            world.rebuild([
                (7, Vec2::ZERO, zone),
                (9, Vec2::new(x, 0.0), player),
                (3, Vec2::new(x, 0.0), ghost),
                (4, Vec2::new(0.0, 100.0), picky),
            ]);
            triggers.update(world);
        };

        tick(&mut world, &mut triggers, 50.0);
        assert!(triggers.entered().is_empty() && triggers.overlapping().is_empty());
        tick(&mut world, &mut triggers, 5.0);
        assert_eq!(triggers.entered(), [pair(7, 3), pair(7, 9)]);
        tick(&mut world, &mut triggers, 4.0);
        assert!(triggers.entered().is_empty() && triggers.exited().is_empty());
        assert_eq!(triggers.overlapping(), [pair(7, 3), pair(7, 9)]);
        // The player and the ghost leave the world: their pairs end.
        world.rebuild([(7, Vec2::ZERO, zone)]);
        triggers.update(&world);
        assert_eq!(triggers.exited(), [pair(7, 3), pair(7, 9)]);
        assert!(triggers.overlapping().is_empty());
    }
}
