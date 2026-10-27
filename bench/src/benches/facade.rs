//! The facade's benchmark: a whole tick of the engine for 5,000 colliding
//! entities.

use std::hint::black_box;
use std::time::Duration;

use nadir::ecs::{Component, FixedStage, World};
use nadir::{App, Collider, Collisions, Config, TickStats, Transform, Velocity};
use nadir_collision::ColliderKind;
use nadir_core::Vec2;
use nadir_core::time::Tick;

use super::collision::scene;
use crate::harness::Bencher;

/// A frame one tick long: it runs one tick.
const TICK: Duration = Duration::from_nanos(16_666_667);

/// How often a body turns: once in this many ticks, each at its own time.
const TURN_TICKS: u64 = 120;

/// Ticks run before the harness's own warm-up, so that the crowd has met
/// its walls and neighbours, as a game's has.
const SETTLE_TICKS: usize = 60;

/// A body's number, which says when it turns.
#[derive(Clone, Copy)]
struct Body(u32);

impl Component for Body {}

/// Turns each body by 2.4 radians, once every [`TURN_TICKS`] at its own
/// time, so that the crowd keeps moving rather than piling up.
fn turn(world: &World, tick: &Tick) {
    let bodies = world.read::<Body>();
    let mut velocities = world.write::<Velocity>();
    let (sin, cos) = 2.4f32.sin_cos();
    for (entity, &Body(index)) in bodies.iter() {
        if (tick.index + u64::from(index)).is_multiple_of(TURN_TICKS)
            && let Some(velocity) = velocities.get_mut(entity)
        {
            let Vec2 { x, y } = velocity.0;
            velocity.0 = Vec2::new(x * cos - y * sin, x * sin + y * cos);
        }
    }
}

/// The app of [`tick`]: `collision_slide_5k`'s crowd and walls as
/// entities, a tenth of them triggers, with no sprites to draw.
fn crowd() -> App {
    let (grid, bodies) = scene();
    let mut app = App::new(Config::default()).add_fixed_system(FixedStage::Gameplay, "turn", turn);
    let world = app.world_mut();
    world.register::<Body>();
    world.resource_mut::<Collisions>().set_grid(grid);
    for (index, &(_, at, collider, velocity)) in (0u32..).zip(&bodies) {
        let kind = if index.is_multiple_of(10) {
            ColliderKind::Trigger
        } else {
            ColliderKind::Solid
        };
        world
            .spawn()
            .with(Transform::at(at))
            .with(Velocity(velocity * 60.0))
            .with(Collider(nadir_collision::Collider { kind, ..collider }))
            .with(Body(index));
    }
    app
}

/// `facade_tick_5k`: a frame of one tick, through `App::update`, of
/// 5,000 entities moving 2 to 4 px among walls, a tenth of them
/// triggers: the engine's systems (rebuild, `move_and_slide` for each,
/// triggers and their events) and the ECS around them. Nothing is drawn.
/// The M4 exit budget is 4 ms a tick for 5,000 entities on the min-spec
/// machine.
pub(crate) fn tick(bencher: &mut Bencher) {
    let mut app = crowd();
    for _ in 0..SETTLE_TICKS {
        app.update(TICK, &[]);
    }
    bencher.iter(|| {
        app.update(black_box(TICK), &[]);
        app.world().resource::<TickStats>().ticks
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benches::collision::COLLIDERS;
    use crate::harness::Config;

    #[test]
    fn a_frame_of_the_benchmark_runs_one_tick_of_5k_colliders() {
        let mut app = crowd();
        let before: Vec<Vec2> = app
            .world()
            .read::<Transform>()
            .iter()
            .map(|(_, transform)| transform.position)
            .collect();
        app.update(TICK, &[]);
        let world = app.world();
        assert_eq!(world.resource::<TickStats>().ticks, 1);
        assert_eq!(world.resource::<Collisions>().world().len(), COLLIDERS);
        let moved = world
            .read::<Transform>()
            .iter()
            .zip(&before)
            .filter(|((_, transform), before)| transform.position != **before)
            .count();
        assert!(moved > COLLIDERS * 2 / 3, "{moved} moved");
    }

    #[test]
    fn the_benchmark_times_its_work() {
        let mut bencher = Bencher::new(Config::fast());
        tick(&mut bencher);
        let measurement = bencher.finish().unwrap();
        assert!(measurement.median_ns() > 0.0);
    }
}
