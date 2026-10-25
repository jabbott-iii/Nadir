//! ECS benchmarks: joins over 5,000 entities, and spawn and despawn churn.

use std::hint::black_box;

use nadir_core::{Pcg32, Vec2};
use nadir_ecs::{Component, World, for_each_mut_with};

use crate::harness::Bencher;

/// How many entities the joins walk.
const ENTITIES: usize = 5_000;

/// How many entities a round of churn spawns and despawns.
const CHURN: usize = 100;

struct Position(Vec2);
impl Component for Position {}

struct Velocity(Vec2);
impl Component for Velocity {}

struct Speed(f32);
impl Component for Speed {}

/// A world of [`ENTITIES`] entities, all with a position, four in five with
/// a velocity, and three in five with a speed: the same every time.
fn world() -> World {
    let mut world = World::new();
    world.register::<Position>();
    world.register::<Velocity>();
    world.register::<Speed>();
    let mut rng = Pcg32::new(0x6563_7321, 0);
    for index in 0..ENTITIES {
        let position = Vec2::new(rng.next_f32() * 512.0, rng.next_f32() * 384.0);
        let velocity = Vec2::new(rng.next_f32() * 2.0 - 1.0, rng.next_f32() * 2.0 - 1.0);
        let mut entity = world.spawn().with(Position(position));
        if index % 5 != 0 {
            entity = entity.with(Velocity(velocity * 60.0));
        }
        if index % 5 < 3 {
            entity.with(Speed(0.5 + rng.next_f32()));
        }
    }
    world
}

/// `ecs_join2_5k`: moves every entity that has a velocity, as a movement
/// system does: positions driven, velocities looked up.
pub(crate) fn join2(bencher: &mut Bencher) {
    let world = world();
    bencher.iter(|| {
        let velocity = world.read::<Velocity>();
        let mut position = world.write::<Position>();
        let mut moved = 0u32;
        for_each_mut_with(&mut position, &velocity, |_, position, velocity| {
            position.0 += velocity.0 * black_box(1.0 / 60.0);
            moved += 1;
        });
        moved
    });
}

/// `ecs_join3_5k`: as `ecs_join2_5k`, scaled by a third component.
pub(crate) fn join3(bencher: &mut Bencher) {
    let world = world();
    bencher.iter(|| {
        let velocity = world.read::<Velocity>();
        let speed = world.read::<Speed>();
        let mut position = world.write::<Position>();
        let mut moved = 0u32;
        for (entity, position) in position.iter_mut() {
            if let (Some(velocity), Some(speed)) = (velocity.get(entity), speed.get(entity)) {
                position.0 += velocity.0 * speed.0 * black_box(1.0 / 60.0);
                moved += 1;
            }
        }
        moved
    });
}

/// `ecs_spawn_despawn`: a round of churn through commands: spawn [`CHURN`]
/// entities with two components, apply, despawn them, apply.
pub(crate) fn spawn_despawn(bencher: &mut Bencher) {
    let mut world = world();
    let mut spawned = Vec::with_capacity(CHURN);
    bencher.iter(|| {
        let mut commands = world.commands();
        for index in 0..CHURN {
            let entity = commands
                .spawn()
                .with(Position(Vec2::splat(index as f32)))
                .with(Velocity(Vec2::ONE))
                .id();
            spawned.push(entity);
        }
        world.apply_commands();
        let mut commands = world.commands();
        for entity in spawned.drain(..) {
            commands.despawn(entity);
        }
        world.apply_commands();
        world.entity_count()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Config;

    #[test]
    fn the_world_has_its_shares_of_each_component() {
        let world = world();
        assert_eq!(world.entity_count(), ENTITIES);
        assert_eq!(world.read::<Position>().len(), ENTITIES);
        assert_eq!(world.read::<Velocity>().len(), ENTITIES * 4 / 5);
        assert_eq!(world.read::<Speed>().len(), ENTITIES * 3 / 5);
    }

    #[test]
    fn the_benchmarks_time_their_work() {
        for run in [join2, join3, spawn_despawn] {
            let mut bencher = Bencher::new(Config::fast());
            run(&mut bencher);
            let measurement = bencher.finish().unwrap();
            assert!(measurement.median_ns() > 0.0);
        }
    }
}
