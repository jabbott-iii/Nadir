//! The engine's systems, which connect the components to the subsystems:
//! interpolation, movement and collision, triggers, parents, animation and
//! drawing.
//! [`App::new`](crate::App::new) adds each first in its stage (ADR-0022).

use std::sync::atomic::{AtomicBool, Ordering};

use nadir_core::Vec2;
use nadir_core::log::warn;
use nadir_core::time::{Frame, Tick};
use nadir_ecs::{Entity, Events, SparseSet, World};
use nadir_render::RenderFrame;

use crate::animation::AnimationClock;
use crate::{
    AnimationFinished, Animator, Clips, Collider, Collisions, Facing, Parent, PrevTransform,
    Sprite, Transform, TriggerEnter, TriggerExit, Velocity,
};

/// The most steps from a child up to the entity it goes with.
const MOST_PARENTS: usize = 8;

/// Whether a chain of parents too long, or a loop, has been reported.
static WARNED_OF_CHAIN: AtomicBool = AtomicBool::new(false);

/// Whether an animator of a clip no longer in [`Clips`] has been reported.
static WARNED_OF_CLIP: AtomicBool = AtomicBool::new(false);

/// `nadir::snapshot`, first in the tick's `Input` stage: remembers where
/// each entity is before the tick moves it, for frames to draw from. It
/// gives a [`PrevTransform`] to every drawn entity that has none, and to
/// the root of every drawn child, which the child is drawn from.
pub(crate) fn snapshot(world: &World, _tick: &Tick) {
    let transforms = world.read::<Transform>();
    let sprites = world.read::<Sprite>();
    let parents = world.read::<Parent>();
    let mut previous = world.write::<PrevTransform>();
    for (entity, previous) in previous.iter_mut() {
        if let Some(&transform) = transforms.get(entity) {
            previous.0 = transform;
        }
    }
    // Added at once, not through a command, so that a system later in this
    // tick can write it: a teleport in an entity's first tick.
    for (drawn, _) in sprites.iter() {
        let root = if parents.contains(drawn) {
            root(&parents, drawn).map(|(root, _)| root)
        } else {
            None
        };
        for entity in [Some(drawn), root].into_iter().flatten() {
            if !previous.contains(entity)
                && world.is_alive(entity)
                && let Some(&transform) = transforms.get(entity)
            {
                previous.insert(entity, PrevTransform(transform));
            }
        }
    }
}

/// `nadir::movement`, first in the tick's `Movement` stage: rebuilds the
/// [`Collisions`] world, then moves every entity with a [`Velocity`]. One
/// with a [`Collider`] slides along what it meets, and the ones after it
/// see where it went; one without moves freely.
pub(crate) fn movement(world: &World, tick: &Tick) {
    let mut collisions = world.resource_mut::<Collisions>();
    let colliders = world.read::<Collider>();
    let velocities = world.read::<Velocity>();
    let mut transforms = world.write::<Transform>();
    collisions.rebuild(&colliders, &transforms);
    collisions.move_colliders(&velocities, &mut transforms, tick.dt);
    for (entity, velocity) in velocities.iter() {
        if collisions.index(entity).is_none()
            && let Some(transform) = transforms.get_mut(entity)
        {
            transform.position += velocity.0 * tick.dt;
        }
    }
}

/// `nadir::parent_offsets`, first in the tick's `PostMovement` stage: puts
/// each entity with a [`Parent`] where its chain of parents puts it.
pub(crate) fn parent_offsets(world: &World, _tick: &Tick) {
    let parents = world.read::<Parent>();
    let mut transforms = world.write::<Transform>();
    for (child, _) in parents.iter() {
        let Some((root, offset)) = root(&parents, child) else {
            continue;
        };
        // The root has no parent, so this loop never writes it.
        let Some(&Transform { position }) = transforms.get(root) else {
            continue;
        };
        if let Some(transform) = transforms.get_mut(child) {
            transform.position = position + offset;
        }
    }
}

/// `nadir::triggers`, after `nadir::parent_offsets`: moves every collider
/// to its entity's position, then sends the trigger pairs that began and
/// ended as [`TriggerEnter`] and [`TriggerExit`] events.
pub(crate) fn triggers(world: &World, _tick: &Tick) {
    let mut collisions = world.resource_mut::<Collisions>();
    collisions.sync(&world.read::<Transform>());
    let mut enter = world.resource_mut::<Events<TriggerEnter>>();
    let mut exit = world.resource_mut::<Events<TriggerExit>>();
    collisions.update_triggers(&mut enter, &mut exit);
}

/// `nadir::animate`, first in the frame's `Animate` stage: advances every
/// [`Animator`] by how long the simulation ran since the last frame (the
/// change in `RenderFrame::time`), writes the texture and uv of the frame
/// it shows into its entity's [`Sprite`], and sends [`AnimationFinished`]
/// for each clip that plays once and ended. Then it mirrors the Sprite of
/// each entity that faces [`Facing::Left`], and unmirrors the rest that
/// have a `Facing`.
pub(crate) fn animate(world: &World, _frame: &Frame) {
    let by = {
        let now = world.resource::<RenderFrame>().time;
        let mut clock = world.resource_mut::<AnimationClock>();
        let by = now.saturating_sub(clock.0);
        clock.0 = now;
        by
    };
    let clips = world.resource::<Clips>();
    let mut animators = world.write::<Animator>();
    let mut sprites = world.write::<Sprite>();
    let mut finished = world.resource_mut::<Events<AnimationFinished>>();
    for (entity, animator) in animators.iter_mut() {
        let Some(clip) = clips.get(animator.clip()) else {
            if !WARNED_OF_CLIP.swap(true, Ordering::Relaxed) {
                warn!(
                    "{entity:?} plays {:?}, which is not in `Clips`: it stays where it is",
                    animator.clip()
                );
            }
            continue;
        };
        let (index, ended) = animator.advance(clip, by);
        if ended {
            finished.send(AnimationFinished {
                entity,
                clip: animator.clip(),
            });
        }
        if let Some(sprite) = sprites.get_mut(entity) {
            let frame = clip.frames()[index];
            sprite.texture = frame.texture;
            sprite.uv = frame.uv;
        }
    }
    for (entity, &facing) in world.read::<Facing>().iter() {
        if let Some(sprite) = sprites.get_mut(entity) {
            sprite.flip.x = facing == Facing::Left;
        }
    }
}

/// `nadir::extract_sprites`, first in the frame's `Extract` stage: draws
/// every entity with a [`Sprite`] and a [`Transform`], the frame's `alpha`
/// of the way from its [`PrevTransform`]. A child is drawn where its
/// parent is drawn, plus its offset.
pub(crate) fn extract_sprites(world: &World, frame: &Frame) {
    let sprites = world.read::<Sprite>();
    let transforms = world.read::<Transform>();
    let previous = world.read::<PrevTransform>();
    let parents = world.read::<Parent>();
    let mut out = world.resource_mut::<RenderFrame>();
    let drawn_at = |entity| {
        let current = transforms.get(entity)?.position;
        Some(previous.get(entity).map_or(current, |previous| {
            previous.0.position.lerp(current, frame.alpha)
        }))
    };
    for (entity, sprite) in sprites.iter() {
        let parented = if parents.contains(entity) {
            root(&parents, entity).and_then(|(root, offset)| Some(drawn_at(root)? + offset))
        } else {
            None
        };
        if let Some(position) = parented.or_else(|| drawn_at(entity)) {
            out.sprites.push(sprite.draw(position));
        }
    }
}

/// The entity that `child` goes with, the first up its chain of parents
/// that has none, and the sum of the offsets on the way. `None`, after a
/// warning the first time, if the chain is longer than [`MOST_PARENTS`]
/// or loops.
fn root(parents: &SparseSet<Parent>, child: Entity) -> Option<(Entity, Vec2)> {
    let mut at = child;
    let mut offset = Vec2::ZERO;
    for _ in 0..=MOST_PARENTS {
        let Some(parent) = parents.get(at) else {
            return Some((at, offset));
        };
        at = parent.entity;
        offset += parent.offset;
    }
    if !WARNED_OF_CHAIN.swap(true, Ordering::Relaxed) {
        warn!(
            "{child:?} has more than {MOST_PARENTS} parents above it, or its parents loop: it \
             stays where it is"
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::time::Duration;

    use nadir_assets::Handle;
    use nadir_collision::{LayerMask, Shape, TileGrid};
    use nadir_core::{GenIndex, IVec2, Rect};
    use nadir_ecs::{FixedStage, FrameStage};
    use nadir_render::{Flip, Texture};

    use super::*;
    use crate::{App, Clip, ClipFrame, Config, Playback};

    /// One tick's length, as a frame: it runs one tick.
    const TICK: Duration = Duration::from_nanos(16_666_667);
    const MS: Duration = Duration::from_millis(1);

    fn app() -> App {
        App::new(Config::default())
    }

    /// A texture handle, which nothing loads.
    fn texture() -> Handle<Texture> {
        Handle::new(GenIndex::new(0, std::num::NonZeroU32::MIN))
    }

    fn square(half: f32) -> Shape {
        Shape::Aabb {
            half: Vec2::splat(half),
        }
    }

    /// A velocity of `x` pixels a tick, to the right.
    fn per_tick(x: f32) -> Velocity {
        Velocity(Vec2::new(x * 60.0, 0.0))
    }

    fn at(x: f32, y: f32) -> Transform {
        Transform::at(Vec2::new(x, y))
    }

    fn position(app: &App, entity: Entity) -> Vec2 {
        app.world()
            .read::<Transform>()
            .get(entity)
            .expect("it has a Transform")
            .position
    }

    #[track_caller]
    fn assert_near(found: Vec2, expected: Vec2) {
        let off = (found - expected).abs();
        assert!(off.x < 1e-3 && off.y < 1e-3, "{found:?}, not {expected:?}");
    }

    // -- movement -----------------------------------------------------------

    #[test]
    fn colliders_slide_against_walls_and_the_rest_move_freely() {
        let mut app = app();
        let mut grid = TileGrid::new(10, 10, 16.0, Vec2::ZERO);
        // A wall from x = 80 to 96.
        grid.set_solid(IVec2::new(5, 0), true);
        app.world().resource_mut::<Collisions>().set_grid(grid);
        let world = app.world_mut();
        let slider = world
            .spawn()
            .with(at(60.0, 8.0))
            .with(per_tick(10.0))
            .with(Collider::solid(square(4.0)))
            .id();
        let ghost = world.spawn().with(at(60.0, 8.0)).with(per_tick(10.0)).id();
        let resting = world.spawn().with(at(20.0, 30.0)).with(per_tick(0.0)).id();
        for _ in 0..3 {
            app.update(TICK, &[]);
        }
        assert_near(position(&app, slider), Vec2::new(76.0, 8.0));
        assert_near(position(&app, ghost), Vec2::new(90.0, 8.0));
        assert_eq!(position(&app, resting), Vec2::new(20.0, 30.0));
    }

    #[test]
    fn a_player_slides_along_a_wall_of_tiles_without_stopping_at_a_seam() {
        // A floor of 40 tiles of 16 px, its top at y = 160.3, on layer 1,
        // and a player's collider at the feet: 10 × 5.4, on layer 0,
        // colliding with layer 1. Pushed 1.7 px a tick into the floor and
        // 1.3 along it, the player reaches it in six ticks, and then slides
        // 1.3 px every tick across 25 seams. The floor and the collider are
        // off the binary fractions, so that the contact rounds, and only the
        // collision crate's tolerance keeps the seams from stopping it.
        let mut app = app();
        let mut grid = TileGrid::new(40, 20, 16.0, Vec2::new(0.0, 0.3));
        for x in 0..40 {
            grid.set_solid(IVec2::new(x, 10), true);
        }
        grid.layer = LayerMask::bit(1);
        app.world().resource_mut::<Collisions>().set_grid(grid);
        let feet = |mask| {
            Collider(nadir_collision::Collider {
                offset: Vec2::new(0.0, -2.7),
                layer: LayerMask::bit(0),
                mask,
                ..nadir_collision::Collider::solid(Shape::Aabb {
                    half: Vec2::new(5.0, 2.7),
                })
            })
        };
        let push = Velocity(Vec2::new(1.3, 1.7) * 60.0);
        let world = app.world_mut();
        let player = world
            .spawn()
            .with(at(20.5, 150.3))
            .with(push)
            .with(feet(LayerMask::bit(1)))
            .id();
        // A mover whose mask lacks the tiles' layer, and the player's, goes
        // through them.
        let ghost = world
            .spawn()
            .with(at(20.5, 150.3))
            .with(push)
            .with(feet(LayerMask::bit(2)))
            .id();
        let mut last = position(&app, player);
        for tick in 1..=300 {
            app.update(TICK, &[]);
            let now = position(&app, player);
            assert!(
                (now.x - last.x - 1.3).abs() < 1e-3,
                "tick {tick}: from {last:?} to {now:?}"
            );
            assert!(
                now.y <= 160.3 + 1e-3,
                "tick {tick}: into the floor at {now:?}"
            );
            if tick > 6 {
                assert!(
                    now.y > 160.3 - 0.02,
                    "tick {tick}: off the floor at {now:?}"
                );
            }
            last = now;
        }
        assert!(last.x > 20.5 + 25.0 * 16.0 - 16.0);
        assert!(position(&app, ghost).y > 600.0, "the ghost fell through");
    }

    /// Two boxes 10 px apart: `front` moves 10 px, `back` 20 px after it.
    fn convoy(front_first: bool) -> (App, Entity, Entity) {
        let mut app = app();
        let spawn = |world: &mut World, x: f32, speed: f32| {
            world
                .spawn()
                .with(at(x, 0.0))
                .with(per_tick(speed))
                .with(Collider::solid(square(5.0)))
                .id()
        };
        let world = app.world_mut();
        let (front, back) = if front_first {
            let front = spawn(world, 20.0, 10.0);
            (front, spawn(world, 0.0, 20.0))
        } else {
            let back = spawn(world, 0.0, 20.0);
            (spawn(world, 20.0, 10.0), back)
        };
        app.update(TICK, &[]);
        (app, front, back)
    }

    #[test]
    fn a_later_mover_sees_where_an_earlier_one_went() {
        // The colliders move in the order they were added.
        let (app, front, back) = convoy(true);
        assert_near(position(&app, front), Vec2::new(30.0, 0.0));
        assert_near(position(&app, back), Vec2::new(20.0, 0.0));
        // Moved first, the back one meets the front one where it was.
        let (app, front, back) = convoy(false);
        assert_near(position(&app, front), Vec2::new(30.0, 0.0));
        assert_near(position(&app, back), Vec2::new(10.0, 0.0));
    }

    #[test]
    fn movers_go_in_the_colliders_order_whatever_the_transforms_order() {
        let mut app = app();
        let world = app.world_mut();
        let collider = || Collider::solid(square(5.0));
        let front = world.spawn().with(per_tick(10.0)).with(collider()).id();
        let back = world.spawn().with(per_tick(20.0)).with(collider()).id();
        // A collider with no Transform, so that the Transforms are fewer,
        // and the Transforms in the other order.
        world.spawn().with(collider());
        world.write::<Transform>().insert(back, at(0.0, 0.0));
        world.write::<Transform>().insert(front, at(20.0, 0.0));
        app.update(TICK, &[]);
        assert_near(position(&app, front), Vec2::new(30.0, 0.0));
        assert_near(position(&app, back), Vec2::new(20.0, 0.0));
    }

    #[test]
    fn a_despawned_entity_has_no_collider_index_though_its_slot_is_stale() {
        let mut app = app();
        let world = app.world_mut();
        let gone = world
            .spawn()
            .with(at(0.0, 0.0))
            .with(Collider::solid(square(1.0)))
            .id();
        let kept = world
            .spawn()
            .with(at(50.0, 0.0))
            .with(Collider::solid(square(1.0)))
            .id();
        app.update(TICK, &[]);
        assert_eq!(app.world().resource::<Collisions>().index(gone), Some(0));
        app.world_mut().despawn(gone);
        // Two rebuilds on, the maps are the first's again, where `gone`'s
        // slot still says 0: now `kept`'s index.
        app.update(TICK, &[]);
        app.update(TICK, &[]);
        let collisions = app.world().resource::<Collisions>();
        assert_eq!(collisions.index(gone), None);
        assert_eq!(collisions.index(kept), Some(0));
        assert_eq!(collisions.entity(0), kept);
    }

    #[test]
    fn a_solid_child_is_in_its_parents_way_unless_its_layer_is_not_in_the_mask() {
        let run = |child: Collider| {
            let mut app = app();
            let world = app.world_mut();
            let parent = world
                .spawn()
                .with(at(0.0, 0.0))
                .with(per_tick(30.0))
                .with(Collider(nadir_collision::Collider {
                    mask: LayerMask::bit(0),
                    ..nadir_collision::Collider::solid(square(4.0))
                }))
                .id();
            world.spawn().with(at(20.0, 0.0)).with(child).with(Parent {
                entity: parent,
                offset: Vec2::new(20.0, 0.0),
            });
            app.update(TICK, &[]);
            position(&app, parent).x
        };
        // In the way: the move stops 12 px on, where the boxes touch.
        let solid = Collider::solid(square(4.0));
        assert!((run(solid) - 12.0).abs() < 1e-3);
        // Not in the way: a trigger, or a layer the parent does not hit.
        assert!((run(Collider::trigger(square(4.0))) - 30.0).abs() < 1e-3);
        let elsewhere = Collider(nadir_collision::Collider {
            layer: LayerMask::bit(1),
            ..nadir_collision::Collider::solid(square(4.0))
        });
        assert!((run(elsewhere) - 30.0).abs() < 1e-3);
    }

    // -- triggers -------------------------------------------------------------

    /// The trigger events of each tick, by tick.
    #[derive(Default)]
    struct Seen(Vec<(u64, &'static str, Entity, Entity)>);

    fn record(world: &World, tick: &Tick) {
        let mut seen = world.resource_mut::<Seen>();
        for event in world.resource::<Events<TriggerEnter>>().tick_events() {
            seen.0
                .push((tick.index, "enter", event.trigger, event.other));
        }
        for event in world.resource::<Events<TriggerExit>>().tick_events() {
            seen.0
                .push((tick.index, "exit", event.trigger, event.other));
        }
    }

    fn recording_app() -> App {
        app().insert_resource(Seen::default()).add_fixed_system(
            FixedStage::Cleanup,
            "record",
            record,
        )
    }

    /// A trigger box from x = 90 to 110 and y = -10 to 10.
    fn zone(world: &mut World) -> Entity {
        world
            .spawn()
            .with(at(100.0, 0.0))
            .with(Collider::trigger(square(10.0)))
            .id()
    }

    #[test]
    fn triggers_send_events_by_entity_and_an_exit_for_a_despawned_one() {
        let mut app = recording_app();
        let world = app.world_mut();
        let zone = zone(world);
        let mover = world
            .spawn()
            .with(at(80.0, 0.0))
            .with(per_tick(10.0))
            .with(Collider::solid(Shape::Circle { radius: 4.0 }))
            .id();
        // Tick 0 moves it into the zone, and tick 1 further in.
        app.update(TICK, &[]);
        app.update(TICK, &[]);
        assert_eq!(
            app.world().resource::<Seen>().0,
            [(0, "enter", zone, mover)]
        );
        app.world_mut().despawn(mover);
        app.update(TICK, &[]);
        assert_eq!(
            app.world().resource::<Seen>().0[1..],
            [(2, "exit", zone, mover)],
            "the exit names the entity that left, though it is dead"
        );
        assert!(!app.world().is_alive(mover));
    }

    #[test]
    fn a_trigger_that_leaves_exits_and_one_that_a_game_moves_in_enters_at_once() {
        fn teleport(world: &World, tick: &Tick) {
            if tick.index == 1 {
                let jumper = world.resource::<Jumper>().0;
                if let Some(transform) = world.write::<Transform>().get_mut(jumper) {
                    transform.position = Vec2::new(100.0, 0.0);
                }
            }
        }
        struct Jumper(Entity);
        let mut app = recording_app().add_fixed_system(FixedStage::Movement, "teleport", teleport);
        let world = app.world_mut();
        let zone = zone(world);
        // Tick 0 takes it to x = 90, just inside the zone, and tick 1 out.
        let leaver = world
            .spawn()
            .with(at(100.0, 0.0))
            .with(per_tick(-10.0))
            .with(Collider::trigger(square(2.0)))
            .id();
        let jumper = world
            .spawn()
            .with(at(300.0, 0.0))
            .with(Collider::solid(square(2.0)))
            .id();
        app = app.insert_resource(Jumper(jumper));
        app.update(TICK, &[]);
        app.update(TICK, &[]);
        let mut seen = app.world().resource::<Seen>().0.clone();
        seen.sort_by_key(|&(tick, kind, trigger, other)| {
            (tick, kind, trigger.index(), other.index())
        });
        // Two triggers detect each other. The game moved the jumper after
        // the engine's movement, and the zone still saw it this tick.
        let mut expected = vec![
            (0, "enter", zone, leaver),
            (0, "enter", leaver, zone),
            (1, "enter", zone, jumper),
            (1, "exit", zone, leaver),
            (1, "exit", leaver, zone),
        ];
        expected.sort_by_key(|&(tick, kind, trigger, other)| {
            (tick, kind, trigger.index(), other.index())
        });
        assert_eq!(seen, expected);
    }

    #[test]
    fn a_childs_collider_follows_its_parent_before_the_triggers_look() {
        let mut app = recording_app();
        let world = app.world_mut();
        // A zone from x = 27 to 33.
        let zone = world
            .spawn()
            .with(at(30.0, 0.0))
            .with(Collider::trigger(square(3.0)))
            .id();
        let parent = world.spawn().with(at(0.0, 0.0)).with(per_tick(10.0)).id();
        let child = world
            .spawn()
            .with(at(20.0, 0.0))
            .with(Collider::solid(Shape::Circle { radius: 2.0 }))
            .with(Parent {
                entity: parent,
                offset: Vec2::new(20.0, 0.0),
            })
            .id();
        app.update(TICK, &[]);
        assert_near(position(&app, child), Vec2::new(30.0, 0.0));
        assert_eq!(
            app.world().resource::<Seen>().0,
            [(0, "enter", zone, child)]
        );
        let index = app.world().resource::<Collisions>().index(child);
        let collisions = app.world().resource::<Collisions>();
        let index = index.expect("the child has a collider");
        assert_eq!(collisions.entity(index), child);
        assert_near(collisions.world().position(index), Vec2::new(30.0, 0.0));
    }

    // -- parents --------------------------------------------------------------

    #[test]
    fn a_chain_of_parents_puts_the_last_child_in_the_same_place_in_any_order() {
        for hand_first in [true, false] {
            let mut app = app();
            let world = app.world_mut();
            let root = world.spawn().with(at(10.0, 10.0)).with(per_tick(1.0)).id();
            let hand = world.spawn().with(at(0.0, 0.0)).id();
            let sword = world.spawn().with(at(0.0, 0.0)).id();
            let to_hand = Parent {
                entity: root,
                offset: Vec2::new(5.0, 0.0),
            };
            let to_sword = Parent {
                entity: hand,
                offset: Vec2::new(0.0, 3.0),
            };
            {
                let mut parents = world.write::<Parent>();
                if hand_first {
                    parents.insert(hand, to_hand);
                    parents.insert(sword, to_sword);
                } else {
                    parents.insert(sword, to_sword);
                    parents.insert(hand, to_hand);
                }
            }
            app.update(TICK, &[]);
            assert_near(position(&app, hand), Vec2::new(16.0, 10.0));
            assert_near(position(&app, sword), Vec2::new(16.0, 13.0));
        }
    }

    #[test]
    fn a_loop_of_parents_or_a_dead_parent_leaves_the_children_where_they_are() {
        let mut app = app();
        let world = app.world_mut();
        let a = world.spawn().with(at(1.0, 0.0)).id();
        let b = world.spawn().with(at(2.0, 0.0)).id();
        let dead = world.spawn().with(at(50.0, 0.0)).id();
        let orphan = world.spawn().with(at(3.0, 0.0)).id();
        world.despawn(dead);
        {
            let mut parents = world.write::<Parent>();
            let to = |entity| Parent {
                entity,
                offset: Vec2::ONE,
            };
            parents.insert(a, to(b));
            parents.insert(b, to(a));
            parents.insert(orphan, to(dead));
        }
        app.update(TICK, &[]);
        assert_eq!(position(&app, a), Vec2::new(1.0, 0.0));
        assert_eq!(position(&app, b), Vec2::new(2.0, 0.0));
        assert_eq!(position(&app, orphan), Vec2::new(3.0, 0.0));
    }

    #[test]
    fn eight_parents_up_is_as_far_as_a_chain_goes() {
        let chain = |length: usize| {
            let mut app = app();
            let world = app.world_mut();
            let mut above = world.spawn().with(at(100.0, 0.0)).id();
            for _ in 0..length {
                let child = world.spawn().with(at(0.0, 0.0)).id();
                world.write::<Parent>().insert(
                    child,
                    Parent {
                        entity: above,
                        offset: Vec2::new(1.0, 0.0),
                    },
                );
                above = child;
            }
            app.update(TICK, &[]);
            position(&app, above).x
        };
        assert_eq!(chain(8), 108.0);
        assert_eq!(chain(9), 0.0, "too long: left where it was");
    }

    // -- interpolation and drawing ----------------------------------------------

    /// Where each frame drew its sprites, in order.
    #[derive(Default)]
    struct Drawn(Vec<Vec<Vec2>>);

    fn record_drawn(world: &World, _: &Frame) {
        let positions = world
            .resource::<RenderFrame>()
            .sprites
            .iter()
            .map(|sprite| sprite.position)
            .collect();
        world.resource_mut::<Drawn>().0.push(positions);
    }

    fn drawing_app() -> App {
        app().insert_resource(Drawn::default()).add_frame_system(
            FrameStage::Extract,
            "record_drawn",
            record_drawn,
        )
    }

    fn last_drawn(app: &App) -> Vec<Vec2> {
        app.world()
            .resource::<Drawn>()
            .0
            .last()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn a_drawn_entity_gets_a_prev_transform_and_is_drawn_between_ticks() {
        let mut app = drawing_app();
        let world = app.world_mut();
        let walker = world
            .spawn()
            .with(at(0.0, 0.0))
            .with(per_tick(6.0))
            .with(Sprite::new(texture(), Vec2::ONE))
            .id();
        let undrawn = world.spawn().with(at(0.0, 0.0)).with(per_tick(6.0)).id();
        // No tick yet: drawn where it is.
        app.update(5 * MS, &[]);
        assert_eq!(last_drawn(&app), [Vec2::ZERO]);
        assert!(!app.world().read::<PrevTransform>().contains(walker));
        // One tick, and 0.3 of the next: drawn 0.3 of the way.
        app.update(TICK, &[]);
        assert_near(last_drawn(&app)[0], Vec2::new(1.8, 0.0));
        let previous = app.world().read::<PrevTransform>();
        assert_eq!(previous.get(walker), Some(&PrevTransform(at(0.0, 0.0))));
        assert!(!previous.contains(undrawn), "only drawn entities get one");
    }

    /// The tick in which the teleport test's system teleports.
    struct TeleportAt(u64);

    #[test]
    fn a_teleport_that_writes_both_transforms_draws_nothing_on_the_way() {
        fn teleport(world: &World, tick: &Tick) {
            if tick.index == world.resource::<TeleportAt>().0 {
                let jump = Transform::at(Vec2::new(500.0, 0.0));
                let parents = world.read::<Parent>();
                for (entity, transform) in world.write::<Transform>().iter_mut() {
                    if !parents.contains(entity) {
                        *transform = jump;
                    }
                }
                for (entity, previous) in world.write::<PrevTransform>().iter_mut() {
                    if !parents.contains(entity) {
                        previous.0 = jump;
                    }
                }
            }
        }
        // In the entity's first tick too, which gives it its PrevTransform;
        // and a root that is not drawn, with a drawn child, which gets one
        // as well.
        for (tick, drawn_root) in [(0, true), (1, true), (0, false), (1, false)] {
            let mut app = drawing_app()
                .insert_resource(TeleportAt(tick))
                .add_fixed_system(FixedStage::Gameplay, "teleport", teleport);
            let world = app.world_mut();
            let root = world.spawn().with(at(0.0, 0.0)).with(per_tick(6.0)).id();
            let sprite = Sprite::new(texture(), Vec2::ONE);
            if drawn_root {
                world.write::<Sprite>().insert(root, sprite);
            } else {
                world.spawn().with(at(0.0, 0.0)).with(sprite).with(Parent {
                    entity: root,
                    offset: Vec2::ZERO,
                });
            }
            // Frames up to and past tick 1.
            for _ in 0..10 {
                app.update(4 * MS, &[]);
                let x = last_drawn(&app)[0].x;
                assert!(
                    x <= 6.0 + 1e-3 || x >= 500.0 - 1e-3,
                    "tick {tick}, drawn root {drawn_root}: drawn at {x}"
                );
            }
            assert!(last_drawn(&app)[0].x >= 500.0, "the teleport ran");
        }
    }

    #[test]
    fn a_child_is_drawn_where_its_parent_is_drawn_from_its_first_frame() {
        let mut app = drawing_app();
        let world = app.world_mut();
        let parent = world
            .spawn()
            .with(at(0.0, 0.0))
            .with(per_tick(6.0))
            .with(Sprite::new(texture(), Vec2::ONE))
            .id();
        world
            .spawn()
            .with(at(0.0, 0.0))
            .with(Sprite::new(texture(), Vec2::ONE))
            .with(Parent {
                entity: parent,
                offset: Vec2::new(0.0, -10.0),
            });
        // Before any tick has placed the child.
        app.update(5 * MS, &[]);
        assert_eq!(last_drawn(&app), [Vec2::ZERO, Vec2::new(0.0, -10.0)]);
        app.update(TICK, &[]);
        let drawn = last_drawn(&app);
        assert_near(drawn[0], Vec2::new(1.8, 0.0));
        assert_near(drawn[1], Vec2::new(1.8, -10.0));
    }

    #[test]
    fn a_child_of_an_undrawn_root_is_drawn_between_ticks_too() {
        let mut app = drawing_app();
        let world = app.world_mut();
        let root = world.spawn().with(at(0.0, 0.0)).with(per_tick(6.0)).id();
        world
            .spawn()
            .with(at(0.0, 0.0))
            .with(Sprite::new(texture(), Vec2::ONE))
            .with(Parent {
                entity: root,
                offset: Vec2::new(0.0, -10.0),
            });
        app.update(TICK, &[]);
        app.update(TICK, &[]);
        // 0.3 of the way from tick 1's 6 px to tick 2's 12 px.
        app.update(5 * MS, &[]);
        assert_near(last_drawn(&app)[0], Vec2::new(7.8, -10.0));
        assert!(app.world().read::<PrevTransform>().contains(root));
    }

    // -- animation ----------------------------------------------------------------

    /// A texture handle other than [`texture`]'s: the clips' own.
    fn sheet() -> Handle<Texture> {
        Handle::new(GenIndex::new(1, NonZeroU32::MIN))
    }

    /// The region of frame `index` of a strip of 8 across [`sheet`].
    fn strip(index: u32) -> Rect {
        let left = index as f32 / 8.0;
        Rect::new(Vec2::new(left, 0.0), Vec2::new(left + 0.125, 1.0))
    }

    /// A clip of frames of `ms` milliseconds each, the i-th showing
    /// `strip(i)` of [`sheet`], added to the app's [`Clips`].
    fn add_clip(app: &App, ms: &[u64], playback: Playback) -> Handle<Clip> {
        let frames = (0..)
            .zip(ms)
            .map(|(index, &ms)| ClipFrame::new(sheet(), strip(index), Duration::from_millis(ms)))
            .collect();
        app.world()
            .resource_mut::<Clips>()
            .add(Clip::new(frames, playback))
    }

    fn sprite_of(app: &App, entity: Entity) -> Sprite {
        *app.world().read::<Sprite>().get(entity).expect("a sprite")
    }

    fn animator_of(app: &App, entity: Entity) -> Animator {
        *app.world()
            .read::<Animator>()
            .get(entity)
            .expect("an animator")
    }

    // `RenderFrame::time` can be a few nanoseconds short of the frames' sum,
    // except at multiples of 50 ms (three ticks), so the tests' frames end
    // a millisecond or more from the clips' frame ends, or on 50 ms.

    #[test]
    fn an_animator_draws_its_clips_frames_texture_and_uv() {
        let mut app = app();
        let clip = add_clip(&app, &[30, 30, 30], Playback::Loop);
        app.world_mut()
            .spawn()
            .with(at(0.0, 0.0))
            .with(Sprite::new(texture(), Vec2::ONE))
            .with(Animator::new(clip));
        // At 10, 35, 65 and 95 ms: the last is 5 ms into the second time
        // round.
        for (elapsed, frame) in [(10 * MS, 0), (25 * MS, 1), (30 * MS, 2), (30 * MS, 0)] {
            app.update(elapsed, &[]);
            let drawn = app.world().resource::<RenderFrame>().sprites[0];
            assert_eq!(
                (drawn.texture, drawn.uv),
                (sheet(), strip(frame)),
                "frame {frame}"
            );
        }
    }

    #[test]
    fn animators_follow_the_simulation_clock_not_the_frames_length() {
        let mut app = app();
        let clip = add_clip(&app, &[100, 100, 100], Playback::Loop);
        let walker = app
            .world_mut()
            .spawn()
            .with(Sprite::new(texture(), Vec2::ONE))
            .with(Animator::new(clip))
            .id();
        // A long frame while the clock stands still, as in a pause.
        animate(
            app.world(),
            &Frame {
                dt: 0.1,
                alpha: 0.0,
            },
        );
        assert_eq!(animator_of(&app, walker).elapsed(), Duration::ZERO);
        // The clock goes on 150 ms in a frame of 1 ms.
        app.world().resource_mut::<RenderFrame>().time = 150 * MS;
        animate(
            app.world(),
            &Frame {
                dt: 0.001,
                alpha: 0.0,
            },
        );
        assert_eq!(animator_of(&app, walker).elapsed(), 150 * MS);
        assert_eq!(sprite_of(&app, walker).uv, strip(1));
        // And back, which it never goes: nothing moves.
        app.world().resource_mut::<RenderFrame>().time = 100 * MS;
        animate(
            app.world(),
            &Frame {
                dt: 0.001,
                alpha: 0.0,
            },
        );
        assert_eq!(animator_of(&app, walker).elapsed(), 150 * MS);
    }

    /// What the frames' readers of [`AnimationFinished`] saw, frame by
    /// frame, and how many the ticks' readers saw.
    #[derive(Default)]
    struct Finishes {
        frames: Vec<Vec<AnimationFinished>>,
        ticks: usize,
    }

    fn record_finishes(world: &World, _: &Frame) {
        let events = world.resource::<Events<AnimationFinished>>();
        let seen = events.frame_events().to_vec();
        world.resource_mut::<Finishes>().frames.push(seen);
    }

    fn count_tick_finishes(world: &World, _: &Tick) {
        let seen = world
            .resource::<Events<AnimationFinished>>()
            .tick_events()
            .len();
        world.resource_mut::<Finishes>().ticks += seen;
    }

    #[test]
    fn a_clip_that_plays_once_is_announced_once_in_the_frame_it_ends() {
        let mut app = app()
            .insert_resource(Finishes::default())
            .add_fixed_system(FixedStage::Gameplay, "count", count_tick_finishes)
            .add_frame_system(FrameStage::Ui, "record", record_finishes);
        let once = add_clip(&app, &[20, 30], Playback::Once);
        let looping = add_clip(&app, &[10, 10], Playback::Loop);
        let world = app.world_mut();
        let burst = world
            .spawn()
            .with(Sprite::new(texture(), Vec2::ONE))
            .with(Animator::new(once))
            .id();
        world
            .spawn()
            .with(Sprite::new(texture(), Vec2::ONE))
            .with(Animator::new(looping));
        // 50 ms, three ticks: exactly to the clip's end.
        app.update(50 * MS, &[]);
        assert_eq!(sprite_of(&app, burst).uv, strip(1));
        assert!(animator_of(&app, burst).is_finished());
        for _ in 0..4 {
            app.update(TICK, &[]);
        }
        assert_eq!(sprite_of(&app, burst).uv, strip(1), "held at its end");
        app.world()
            .write::<Animator>()
            .get_mut(burst)
            .expect("an animator")
            .restart();
        app.update(60 * MS, &[]);
        app.update(TICK, &[]);

        let ended = AnimationFinished {
            entity: burst,
            clip: once,
        };
        let finishes = app.world().resource::<Finishes>();
        // Once at its end, and once more after the restart; the looping
        // clip, round 9 times, never.
        let none = Vec::new();
        assert_eq!(
            finishes.frames,
            [
                vec![ended],
                none.clone(),
                none.clone(),
                none.clone(),
                none.clone(),
                vec![ended],
                none,
            ]
        );
        // Sent in a frame, the event is cleared before the next tick's
        // systems run: a guard against `nadir::animate` in a tick stage.
        assert_eq!(finishes.ticks, 0);
    }

    #[test]
    fn facing_left_mirrors_a_sprite_and_every_other_way_unmirrors_it() {
        let mut app = app();
        let mut mirrored = Sprite::new(texture(), Vec2::ONE);
        mirrored.flip = Flip { x: true, y: true };
        let world = app.world_mut();
        let facing = world.spawn().with(mirrored).with(Facing::Up).id();
        let animated_clip = add_clip(&app, &[30], Playback::Loop);
        let world = app.world_mut();
        let animated = world
            .spawn()
            .with(Sprite::new(texture(), Vec2::ONE))
            .with(Animator::new(animated_clip))
            .with(Facing::Left)
            .id();
        let unfacing = world.spawn().with(mirrored).id();
        let mut flips = Vec::new();
        for way in [
            Facing::Up,
            Facing::Left,
            Facing::Right,
            Facing::Left,
            Facing::Down,
        ] {
            *app.world()
                .write::<Facing>()
                .get_mut(facing)
                .expect("a facing") = way;
            app.update(TICK, &[]);
            flips.push(sprite_of(&app, facing).flip);
        }
        let flip = |x| Flip { x, y: true };
        assert_eq!(
            flips,
            [
                flip(false),
                flip(true),
                flip(false),
                flip(true),
                flip(false)
            ]
        );
        assert!(sprite_of(&app, animated).flip.x, "an animated one too");
        // Without a Facing, the flip is the game's.
        assert_eq!(sprite_of(&app, unfacing).flip, flip(true));
    }

    #[test]
    fn an_animator_added_late_moves_by_its_first_frames_step_only() {
        let mut app = app();
        let once = add_clip(&app, &[20, 30], Playback::Once);
        // 200 ms with no animator.
        app.update(200 * MS, &[]);
        let late = app
            .world_mut()
            .spawn()
            .with(Sprite::new(texture(), Vec2::ONE))
            .with(Animator::new(once))
            .id();
        app.update(10 * MS, &[]);
        assert_eq!(sprite_of(&app, late).uv, strip(0));
        assert!(!animator_of(&app, late).is_finished());
    }

    #[test]
    fn an_animator_without_a_sprite_plays_and_announces_its_end() {
        let mut app = app().insert_resource(Finishes::default()).add_frame_system(
            FrameStage::Ui,
            "record",
            record_finishes,
        );
        let once = add_clip(&app, &[20, 30], Playback::Once);
        let bare = app.world_mut().spawn().with(Animator::new(once)).id();
        app.update(50 * MS, &[]);
        assert!(animator_of(&app, bare).is_finished());
        let ended = AnimationFinished {
            entity: bare,
            clip: once,
        };
        assert_eq!(app.world().resource::<Finishes>().frames, [vec![ended]]);
    }

    /// The entity to start an animator on in tick 20, and its clip.
    #[derive(Clone, Copy)]
    struct Late(Entity, Handle<Clip>);

    /// Starts an animator in tick 20, at the time of the frame before.
    fn start_in_step(world: &World, tick: &Tick) {
        if tick.index == 20 {
            let Late(entity, clip) = *world.resource::<Late>();
            let time = world.resource::<RenderFrame>().time;
            world
                .write::<Animator>()
                .insert(entity, Animator::starting_at(clip, time));
        }
    }

    #[test]
    fn an_animator_started_at_the_frames_time_keeps_in_step_with_the_tiles() {
        let mut app = app().add_fixed_system(FixedStage::Gameplay, "start", start_in_step);
        let clip = add_clip(&app, &[100, 100, 100], Playback::Loop);
        let walker = app
            .world_mut()
            .spawn()
            .with(Sprite::new(texture(), Vec2::ONE))
            .id();
        let mut app = app.insert_resource(Late(walker, clip));
        let mut checked = 0;
        for _ in 0..200 {
            app.update(7 * MS, &[]);
            let time = app.world().resource::<RenderFrame>().time;
            let started = app.world().read::<Animator>().contains(walker);
            // A millisecond or more from a frame's end: the tiles' time is
            // whole milliseconds.
            let into = time.as_nanos() % 100_000_000;
            if started && (1_000_000..99_000_000).contains(&into) {
                // The frame tiles of 3 frames of 100 ms show (the
                // `tile_chunk` shader).
                let tile = (time.as_millis() / 100 % 3) as u32;
                assert_eq!(sprite_of(&app, walker).uv, strip(tile), "at {time:?}");
                checked += 1;
            }
        }
        assert!(checked > 100, "{checked}");
    }

    #[test]
    fn an_animator_whose_clip_was_removed_leaves_its_sprite_as_it_was() {
        let mut app = app();
        let clip = add_clip(&app, &[30, 30], Playback::Loop);
        let walker = app
            .world_mut()
            .spawn()
            .with(Sprite::new(texture(), Vec2::ONE))
            .with(Animator::new(clip))
            .id();
        app.update(10 * MS, &[]);
        assert_eq!(sprite_of(&app, walker).uv, strip(0));
        let before = animator_of(&app, walker);
        app.world().resource_mut::<Clips>().remove(clip);
        app.update(30 * MS, &[]);
        assert_eq!(sprite_of(&app, walker).uv, strip(0));
        assert_eq!(animator_of(&app, walker), before);
    }

    /// 5,000 colliding entities tick after tick, with entities spawned
    /// and despawned through commands and trigger events each tick:
    /// nothing allocates once the buffers have grown (the M4 exit, and the
    /// M4.1 note on commands and events).
    #[cfg(feature = "dev")]
    mod steady_ticks {
        use nadir_core::Pcg32;
        use nadir_platform::{AllocCounts, thread_alloc_counts};

        use super::*;

        const BODIES: usize = 5_000;
        /// Trigger zones that stay put.
        const ZONES: usize = 40;
        /// Triggers spawned each tick, each despawned [`CHURN_TICKS`]
        /// later.
        const CHURN: usize = 10;
        const CHURN_TICKS: usize = 4;
        /// Frames in a pass, every sixth of two ticks: 28 ticks, a whole
        /// number of churns.
        const FRAMES: usize = 24;

        /// Where each body starts, to start each pass from.
        struct Start(Vec<(Entity, Transform, Velocity)>);

        /// The churning triggers: where each slot of the ring spawns its
        /// trigger, and which entity holds it now.
        struct Churn {
            spots: Vec<Vec2>,
            live: Vec<Entity>,
            next: usize,
        }

        /// What the counted ticks did.
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        struct Tally {
            enters: u32,
            exits: u32,
            /// Exits that name an entity despawned since.
            dead_exits: u32,
        }

        fn churn(world: &World, _: &Tick) {
            let mut churn = world.resource_mut::<Churn>();
            let mut commands = world.commands();
            for _ in 0..CHURN {
                let slot = churn.next;
                if let Some(&old) = churn.live.get(slot) {
                    commands.despawn(old);
                }
                let trigger = commands
                    .spawn()
                    .with(Transform::at(churn.spots[slot]))
                    .with(Collider::trigger(Shape::Circle { radius: 10.0 }))
                    .with(Sprite::new(texture(), Vec2::ONE))
                    .id();
                if slot < churn.live.len() {
                    churn.live[slot] = trigger;
                } else {
                    churn.live.push(trigger);
                }
                churn.next = (slot + 1) % (CHURN * CHURN_TICKS);
            }
        }

        fn tally(world: &World, _: &Tick) {
            let mut tally = world.resource_mut::<Tally>();
            let enters = world.resource::<Events<TriggerEnter>>().tick_events().len();
            tally.enters += u32::try_from(enters).expect("a few");
            for exit in world.resource::<Events<TriggerExit>>().tick_events() {
                tally.exits += 1;
                if !world.is_alive(exit.trigger) || !world.is_alive(exit.other) {
                    tally.dead_exits += 1;
                }
            }
        }

        /// The crowd of `collision_slide_5k` (nadir-bench): 16 px tiles,
        /// one in five solid; boxes and circles 10 to 14 px across, a
        /// tenth of them triggers, moving 2 to 4 px a tick; and the zones
        /// and the churning triggers' spots among them.
        fn scene() -> App {
            let mut app = app()
                .add_fixed_system(FixedStage::Gameplay, "churn", churn)
                .add_fixed_system(FixedStage::Cleanup, "tally", tally)
                .insert_resource(Tally::default());
            let mut rng = Pcg32::new(0x5_7ead, 0);
            let mut grid = TileGrid::new(128, 128, 16.0, Vec2::ZERO);
            let mut open = Vec::new();
            for y in 0..128 {
                for x in 0..128 {
                    if rng.range(0..5u32) == 0 {
                        grid.set_solid(IVec2::new(x, y), true);
                    } else {
                        open.push(grid.cell_rect(IVec2::new(x, y)).center());
                    }
                }
            }
            rng.shuffle(&mut open);
            let world = app.world_mut();
            let mut start = Vec::with_capacity(BODIES);
            for (index, &middle) in open[..BODIES].iter().enumerate() {
                let half = Vec2::new(rng.range(5.0..7.0), rng.range(5.0..7.0));
                let shape = if index % 2 == 0 {
                    Shape::Circle { radius: half.x }
                } else {
                    Shape::Aabb { half }
                };
                let collider = if index % 10 == 0 {
                    Collider::trigger(shape)
                } else {
                    Collider::solid(shape)
                };
                let jitter = Vec2::new(rng.range(-1.0..1.0), rng.range(-1.0..1.0));
                let transform = Transform::at(middle + jitter);
                let angle = rng.range(0.0..std::f32::consts::TAU);
                let velocity = Velocity(Vec2::from_angle(angle) * rng.range(2.0..4.0) * 60.0);
                let body = world
                    .spawn()
                    .with(transform)
                    .with(velocity)
                    .with(collider)
                    .with(Sprite::new(texture(), half * 2.0))
                    .id();
                start.push((body, transform, velocity));
            }
            for &middle in &open[BODIES..BODIES + ZONES] {
                world
                    .spawn()
                    .with(Transform::at(middle))
                    .with(Collider::trigger(Shape::Circle { radius: 24.0 }));
            }
            let spots = open[BODIES + ZONES..][..CHURN * CHURN_TICKS].to_vec();
            world.resource_mut::<Collisions>().set_grid(grid);
            app.insert_resource(Start(start)).insert_resource(Churn {
                spots,
                live: Vec::with_capacity(CHURN * CHURN_TICKS),
                next: 0,
            })
        }

        /// Puts every body back where it started, in place.
        fn restart(app: &App) {
            let world = app.world();
            let start = world.resource::<Start>();
            let mut transforms = world.write::<Transform>();
            let mut velocities = world.write::<Velocity>();
            let mut previous = world.write::<PrevTransform>();
            for &(body, transform, velocity) in &start.0 {
                *transforms.get_mut(body).expect("a body") = transform;
                *velocities.get_mut(body).expect("a body") = velocity;
                if let Some(previous) = previous.get_mut(body) {
                    previous.0 = transform;
                }
            }
        }

        /// Runs a pass from the start and returns what it allocated.
        fn pass(app: &mut App) -> AllocCounts {
            restart(app);
            *app.world().resource_mut::<Tally>() = Tally::default();
            let before = thread_alloc_counts();
            for frame in 0..FRAMES {
                let ticks = if frame % 6 == 5 { 2 } else { 1 };
                app.update(TICK * ticks, &[]);
            }
            thread_alloc_counts().since(before)
        }

        #[test]
        fn a_steady_tick_of_5k_colliding_entities_allocates_nothing() {
            let mut app = scene();
            // The first pass grows the buffers, and the second starts as
            // the third will, after a pass: nothing it needs is new.
            assert_ne!(pass(&mut app), AllocCounts::default(), "the first grows");
            pass(&mut app);
            let second = *app.world().resource::<Tally>();
            let third = pass(&mut app);
            assert_eq!(third, AllocCounts::default(), "the steady pass allocated");

            // The pass did what it should have to count.
            let tally = *app.world().resource::<Tally>();
            assert_eq!(tally, second, "the same pass, the same events");
            assert!(tally.enters > 100 && tally.exits > 100, "{tally:?}");
            assert!(tally.dead_exits >= 10, "{tally:?}");
            let start = app.world().resource::<Start>();
            let transforms = app.world().read::<Transform>();
            let stopped = start
                .0
                .iter()
                .filter(|&&(body, from, velocity)| {
                    let went = transforms.get(body).expect("a body").position - from.position;
                    // 28 ticks of free movement.
                    went.length() < velocity.0.length() * 28.0 / 60.0 * 0.9
                })
                .count();
            assert!(stopped > 500, "{stopped} bodies were stopped");
            // The last frame ran two ticks.
            assert_eq!(app.world().resource::<crate::TickStats>().ticks, 2);
        }
    }

    /// Animators by the thousand, a quarter of them on a clip that plays
    /// once and that a frame system restarts when it ends, in frames of
    /// fixed lengths, some with no tick and some with two.
    #[cfg(feature = "dev")]
    mod steady_animation {
        use nadir_platform::{AllocCounts, thread_alloc_counts};

        use super::*;

        /// How many animate.
        const ANIMATORS: u32 = 10_000;
        /// The frames of a pass: three of a third of a tick, then one of
        /// two ticks, eight times, 400 ms in all.
        const FRAMES: usize = 32;

        /// Each animator as it starts a pass.
        struct Start(Vec<(Entity, Animator)>);

        /// The clips that ended in the counted frames.
        #[derive(Default)]
        struct Finished(usize);

        /// Restarts each clip that ended, as a game replays an effect.
        fn replay(world: &World, _: &Frame) {
            let events = world.resource::<Events<AnimationFinished>>();
            let mut animators = world.write::<Animator>();
            for ended in events.frame_events() {
                if let Some(animator) = animators.get_mut(ended.entity) {
                    animator.restart();
                }
            }
            world.resource_mut::<Finished>().0 += events.frame_events().len();
        }

        fn scene() -> App {
            let mut app = app().insert_resource(Finished::default()).add_frame_system(
                FrameStage::Animate,
                "replay",
                replay,
            );
            // Longer than a tick: between two ends of these clips a tick
            // clears the events' tick buffer, which frame sends fill too.
            let once = add_clip(&app, &[20, 20], Playback::Once);
            let walk = add_clip(&app, &[100, 100, 100, 100], Playback::Loop);
            let ways = [Facing::Down, Facing::Up, Facing::Left, Facing::Right];
            let world = app.world_mut();
            let mut start = Vec::with_capacity(ANIMATORS as usize);
            for index in 0..ANIMATORS {
                let animator = if index % 4 == 0 {
                    Animator::new(once)
                } else {
                    Animator::starting_at(walk, MS * index)
                };
                let entity = world
                    .spawn()
                    .with(at(index as f32, 0.0))
                    .with(Sprite::new(texture(), Vec2::ONE))
                    .with(animator)
                    .with(ways[index as usize % 4])
                    .id();
                start.push((entity, animator));
            }
            app.insert_resource(Start(start))
        }

        /// Runs a pass from the start and returns what it allocated.
        fn pass(app: &mut App) -> AllocCounts {
            {
                let world = app.world();
                let start = world.resource::<Start>();
                let mut animators = world.write::<Animator>();
                for &(entity, animator) in &start.0 {
                    *animators.get_mut(entity).expect("an animator") = animator;
                }
                world.resource_mut::<Finished>().0 = 0;
            }
            let before = thread_alloc_counts();
            for frame in 0..FRAMES {
                let length = if frame % 4 == 3 { TICK * 2 } else { TICK / 3 };
                app.update(length, &[]);
            }
            thread_alloc_counts().since(before)
        }

        #[test]
        fn a_steady_frame_of_10k_animators_allocates_nothing() {
            let mut app = scene();
            assert_ne!(pass(&mut app), AllocCounts::default(), "the first grows");
            pass(&mut app);
            let second = app.world().resource::<Finished>().0;
            let third = pass(&mut app);
            assert_eq!(third, AllocCounts::default(), "the steady pass allocated");
            // Each of the 2,500 clips of 40 ms ended in each 50 ms of
            // frames, in the frame of two ticks, and was restarted.
            let finished = app.world().resource::<Finished>().0;
            assert_eq!((finished, second), (2_500 * 8, 2_500 * 8));
        }
    }
}
