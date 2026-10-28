//! The facade's benchmarks: a whole tick of the engine for 5,000 colliding
//! entities, and a frame of 10,000 animated sprites.

use std::hint::black_box;
use std::num::NonZeroU32;
use std::time::Duration;

use nadir::ecs::{Component, Events, FixedStage, FrameStage, World};
use nadir::{
    AnimationFinished, Animator, App, Clip, ClipFrame, Clips, Collider, Collisions, Config, Facing,
    Playback, Sprite, TickStats, Transform, Velocity,
};
use nadir_collision::ColliderKind;
use nadir_core::time::{Frame, Tick};
use nadir_core::{GenIndex, Handle, Rect, Vec2};
use nadir_render::{RenderFrame, Texture};

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

/// How many sprites animate in [`frame`].
const ANIMATED: u32 = 10_000;

/// A frame at 240 Hz, a quarter of a tick: about one in four runs a tick.
const QUARTER_TICK: Duration = Duration::from_nanos(4_166_667);

/// Frames of [`QUARTER_TICK`] run before the harness's own warm-up, so that
/// the clips are under way: 250 ms.
const SETTLE_FRAMES: usize = 60;

/// How often a clip ended, which [`replay`] restarted.
#[derive(Default)]
struct Replayed(usize);

/// Restarts each clip that ended, as a game replays an effect.
fn replay(world: &World, _frame: &Frame) {
    let events = world.resource::<Events<AnimationFinished>>();
    let mut animators = world.write::<Animator>();
    for ended in events.frame_events() {
        if let Some(animator) = animators.get_mut(ended.entity) {
            animator.restart();
        }
    }
    world.resource_mut::<Replayed>().0 += events.frame_events().len();
}

/// The app of [`frame`]: 10,000 sprites that stand still, each with an
/// animator and a facing. Three in four walk: a clip of 4 frames of
/// 100 ms that loops, each started at its own point. One in four plays a
/// clip of 2 frames of 20 ms once, which [`replay`] restarts when it ends.
/// No texture is loaded: nothing is drawn.
fn animated() -> App {
    let mut app = App::new(Config::default())
        .insert_resource(Replayed::default())
        .add_frame_system(FrameStage::Animate, "replay", replay);
    let sheet: Handle<Texture> = Handle::new(GenIndex::new(0, NonZeroU32::MIN));
    let frame = |index: u32, ms: u64| {
        let left = index as f32 / 8.0;
        let uv = Rect::new(Vec2::new(left, 0.0), Vec2::new(left + 0.125, 1.0));
        ClipFrame::new(sheet, uv, Duration::from_millis(ms))
    };
    let (walk, puff) = {
        let mut clips = app.world().resource_mut::<Clips>();
        let walk = (0..4).map(|index| frame(index, 100)).collect();
        let puff = (4..6).map(|index| frame(index, 20)).collect();
        (
            clips.add(Clip::new(walk, Playback::Loop)),
            clips.add(Clip::new(puff, Playback::Once)),
        )
    };
    let ways = [Facing::Down, Facing::Up, Facing::Left, Facing::Right];
    let world = app.world_mut();
    for index in 0..ANIMATED {
        let animator = if index % 4 == 0 {
            Animator::new(puff)
        } else {
            Animator::starting_at(walk, Duration::from_millis(u64::from(index % 400)))
        };
        let at = Vec2::new((index % 100) as f32 * 5.0, (index / 100) as f32 * 3.0);
        world
            .spawn()
            .with(Transform::at(at))
            .with(Sprite::new(sheet, Vec2::new(12.0, 20.0)))
            .with(animator)
            .with(ways[index as usize % 4]);
    }
    app
}

/// `facade_frame_10k`: a 240 Hz frame, through `App::update`, of 10,000
/// animated sprites. `nadir::animate` advances each animator, writes its
/// frame into its sprite, sends the events of the clips that end (2,500
/// every tenth frame: clips of 40 ms, which a frame system restarts), and
/// mirrors the sprites that face left; `nadir::extract_sprites` puts them
/// into the `RenderFrame`. About one frame in four also runs a tick, whose
/// `nadir::snapshot` copies 10,000 `PrevTransform`s. Nothing is drawn.
pub(crate) fn frame(bencher: &mut Bencher) {
    let mut app = animated();
    for _ in 0..SETTLE_FRAMES {
        app.update(QUARTER_TICK, &[]);
    }
    bencher.iter(|| {
        app.update(black_box(QUARTER_TICK), &[]);
        app.world().resource::<RenderFrame>().sprites.len()
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

    #[test]
    fn frames_of_the_animation_benchmark_change_the_sprites_and_end_clips() {
        let mut app = animated();
        let uvs = |app: &App| -> Vec<Rect> {
            let sprites = app.world().read::<Sprite>();
            sprites.iter().map(|(_, sprite)| sprite.uv).collect()
        };
        app.update(QUARTER_TICK, &[]);
        let first = uvs(&app);
        let mut ticks = 0;
        for _ in 0..24 {
            app.update(QUARTER_TICK, &[]);
            ticks += app.world().resource::<TickStats>().ticks;
        }
        let world = app.world();
        let changed = first
            .iter()
            .zip(&uvs(&app))
            .filter(|(before, after)| before != after)
            .count();
        assert!(changed > ANIMATED as usize / 4, "{changed} changed");
        // 104 ms: each clip of 40 ms that plays once ended, was restarted
        // and ended again.
        assert_eq!(world.resource::<Replayed>().0, ANIMATED as usize / 2);
        assert_eq!(ticks, 6, "a tick in about four frames");
        let out = world.resource::<RenderFrame>();
        assert_eq!(out.sprites.len(), ANIMATED as usize);
        let mirrored = out.sprites.iter().filter(|sprite| sprite.flip.x).count();
        assert_eq!(mirrored, ANIMATED as usize / 4);
    }

    #[test]
    fn the_animation_benchmark_times_its_work() {
        let mut bencher = Bencher::new(Config::fast());
        frame(&mut bencher);
        let measurement = bencher.finish().unwrap();
        assert!(measurement.median_ns() > 0.0);
    }
}
