//! The engine's systems, which connect the components to the subsystems:
//! interpolation, movement and collision, triggers, parents, and drawing.
//! [`App::new`](crate::App::new) adds each first in its stage (ADR-0022).

use std::sync::atomic::{AtomicBool, Ordering};

use nadir_core::Vec2;
use nadir_core::log::warn;
use nadir_core::time::{Frame, Tick};
use nadir_ecs::{Entity, Events, SparseSet, World};
use nadir_render::RenderFrame;

use crate::{
    Collider, Collisions, Parent, PrevTransform, Sprite, Transform, TriggerEnter, TriggerExit,
    Velocity,
};

/// The most steps from a child up to the entity it goes with.
const MOST_PARENTS: usize = 8;

/// Whether a chain of parents too long, or a loop, has been reported.
static WARNED_OF_CHAIN: AtomicBool = AtomicBool::new(false);

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
    use std::time::Duration;

    use nadir_assets::Handle;
    use nadir_collision::{LayerMask, Shape, TileGrid};
    use nadir_core::{GenIndex, IVec2};
    use nadir_ecs::{FixedStage, FrameStage};
    use nadir_render::Texture;

    use super::*;
    use crate::{App, Config};

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
}
