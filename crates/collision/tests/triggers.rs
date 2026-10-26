//! `Triggers` against brute force, tick after tick, as colliders move,
//! leave and arrive, in a different order each tick.

mod common;

use std::cell::Cell;

use common::{coord, depth, motion};
use nadir_collision::{Collider, ColliderKind, CollisionWorld, TriggerPair, Triggers};
use nadir_core::testing::check_sized;
use nadir_core::{Pcg32, Vec2};

/// One tick's change to the colliders.
#[derive(Debug)]
enum Tick {
    /// Rebuild from this list, in this order.
    Rebuild(Vec<(u64, Vec2, Collider)>),
    /// Move these colliders, by index, without a rebuild.
    Move(Vec<(u32, Vec2)>),
}

fn ticks(rng: &mut Pcg32, size: u32) -> Vec<Tick> {
    let mut next_id = 0u64;
    let mut fresh = |rng: &mut Pcg32| {
        next_id += 1;
        let collider = Collider {
            kind: if rng.range(0..3u32) == 0 {
                ColliderKind::Trigger
            } else {
                ColliderKind::Solid
            },
            ..common::collider(rng, 40.0)
        };
        (
            next_id * 3,
            Vec2::new(coord(rng, 0.0, 160.0), coord(rng, 0.0, 160.0)),
            collider,
        )
    };
    let mut alive: Vec<(u64, Vec2, Collider)> = (0..size / 3).map(|_| fresh(rng)).collect();
    let mut ticks = vec![Tick::Rebuild(alive.clone())];
    for _ in 0..2 + size / 4 {
        if rng.range(0..3u32) == 0 && !alive.is_empty() {
            let moves = (0..alive.len() / 2 + 1)
                .map(|_| {
                    let index = rng.range(0..alive.len() as u32);
                    (index, Vec2::new(motion(rng, 12.0), motion(rng, 12.0)))
                })
                .collect();
            ticks.push(Tick::Move(moves));
            continue;
        }
        for entry in &mut alive {
            entry.1 += Vec2::new(motion(rng, 10.0), motion(rng, 10.0));
        }
        alive.retain(|_| rng.range(0..8u32) != 0);
        for _ in 0..rng.range(0..3u32) {
            let entry = fresh(rng);
            alive.push(entry);
        }
        rng.shuffle(&mut alive);
        ticks.push(Tick::Rebuild(alive.clone()));
    }
    ticks
}

/// Every overlapping (trigger, other) pair of `world`, by brute force.
fn pairs(world: &CollisionWorld) -> Vec<TriggerPair> {
    let mut pairs = Vec::new();
    for trigger in 0..world.len() as u32 {
        let t = world.collider(trigger);
        if t.kind != ColliderKind::Trigger {
            continue;
        }
        for other in 0..world.len() as u32 {
            let o = world.collider(other);
            let at = |index, collider: &Collider| collider.center(world.position(index));
            if other != trigger
                && t.mask.intersects(o.layer)
                && depth(at(trigger, t), t.shape, at(other, o), o.shape) > 0.0
            {
                pairs.push(TriggerPair {
                    trigger: world.id(trigger),
                    other: world.id(other),
                });
            }
        }
    }
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// The pairs of `a` that are not in `b`, both sorted.
fn minus(a: &[TriggerPair], b: &[TriggerPair]) -> Vec<TriggerPair> {
    a.iter()
        .filter(|pair| b.binary_search(pair).is_err())
        .copied()
        .collect()
}

#[test]
fn triggers_report_exactly_the_pairs_that_begin_and_end() {
    let (entered, exited) = (Cell::new(0usize), Cell::new(0usize));
    check_sized("trigger pairs against brute force", 200, ticks, |ticks| {
        let mut world = CollisionWorld::with_cell_size(32.0);
        let mut triggers = Triggers::new();
        let mut before: Vec<TriggerPair> = Vec::new();
        for (number, tick) in ticks.iter().enumerate() {
            match tick {
                Tick::Rebuild(colliders) => world.rebuild(colliders.iter().copied()),
                Tick::Move(moves) => {
                    for &(index, by) in moves {
                        world.set_position(index, world.position(index) + by);
                    }
                }
            }
            triggers.update(&world);
            let now = pairs(&world);
            let expected = (minus(&now, &before), minus(&before, &now));
            let found = (triggers.entered().to_vec(), triggers.exited().to_vec());
            if triggers.overlapping() != now || found != expected {
                return Err(format!(
                    "tick {number}: overlapping {:?}, not {now:?}; entered and exited {found:?}, not {expected:?}",
                    triggers.overlapping()
                ));
            }
            entered.set(entered.get() + found.0.len());
            exited.set(exited.get() + found.1.len());
            before = now;
        }
        // When everything leaves, every pair that began ends.
        world.clear();
        triggers.update(&world);
        if triggers.exited() != before || !triggers.overlapping().is_empty() {
            return Err(format!(
                "after clearing, exited {:?}, not {before:?}",
                triggers.exited()
            ));
        }
        exited.set(exited.get() + before.len());
        Ok(())
    });
    // The property is not satisfied by reporting nothing.
    assert!(
        entered.get() > 100 && entered.get() == exited.get(),
        "{entered:?} entered, {exited:?} exited"
    );
}
