//! The stress scene, `--scene stress-sprites`: the field with 10,000 more
//! people walking about in view, for the budget of 10,000 sprites on
//! screen at 60 frames a second, in at most 30 draw calls, with no
//! allocation in a frame (`docs/00-overview.md`, roadmap M3).
//!
//! Everything that stands on the grass, the player included, is drawn from
//! one atlas texture, so that the 10,203 sprites are 2 batches: the grass,
//! and the rest. The camera stays on the view's centre, and the crowd
//! inside the view.

use nadir::prelude::*;

use crate::{PROPS, Player, Prop, Sprites, push_grass};

/// How many people walk about.
pub(crate) const CROWD: usize = 10_000;

/// The view, which the crowd stays inside.
const VIEW: Vec2 = Vec2::new(480.0, 270.0);

/// The atlas: `assets/sprites/atlas.qoi`, 64 pixels square.
const ATLAS: &str = "sprites/atlas.qoi";
const ATLAS_SIZE: f32 = 64.0;

/// A region of the atlas, from pixels to its normalized `uv`.
const fn region(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::new(
        Vec2::new(x / ATLAS_SIZE, y / ATLAS_SIZE),
        Vec2::new((x + width) / ATLAS_SIZE, (y + height) / ATLAS_SIZE),
    )
}

/// Where the person, the tree and the rock are in the atlas. Each has a
/// transparent pixel around it.
const PERSON: Rect = region(1.0, 1.0, 12.0, 20.0);
const TREE: Rect = region(18.0, 1.0, 24.0, 48.0);
const ROCK: Rect = region(1.0, 22.0, 16.0, 12.0);

/// A person's size, in pixels.
const PERSON_SIZE: Vec2 = Vec2::new(12.0, 20.0);

/// How often a walker turns: once in this many ticks, each at its own time.
const TURN_TICKS: u64 = 120;

/// One of the crowd.
struct Walker {
    /// Where their feet are.
    position: Interpolated<Vec2>,
    /// Pixels a second.
    velocity: Vec2,
    tint: Color,
}

/// The crowd, and the atlas they are drawn from.
struct Crowd {
    atlas: Handle<Texture>,
    walkers: Vec<Walker>,
}

/// Adds the stress scene's crowd and systems to the field's `app`.
pub(crate) fn add(mut app: App) -> App {
    let atlas = app
        .resources_mut()
        .resource_mut::<AssetServer>()
        .load(ATLAS);
    let walkers = (0..CROWD as u32).map(walker).collect();
    app.insert_resource(Crowd { atlas, walkers })
        .add_fixed_system("walk_crowd", walk_crowd)
        .add_frame_system("fix_camera", fix_camera)
        .add_frame_system("draw_crowd", draw_crowd)
}

/// Walker `index`, where it starts: somewhere in view, walking at 20 to 60
/// pixels a second. A quarter of the crowd is tinted.
fn walker(index: u32) -> Walker {
    let tints = [
        Color::rgb(255, 190, 190),
        Color::rgb(190, 210, 255),
        Color::rgb(210, 255, 190),
    ];
    let position = Vec2::new(
        PERSON_SIZE.x * 0.5 + unit(index, 0) * (VIEW.x - PERSON_SIZE.x),
        PERSON_SIZE.y + unit(index, 1) * (VIEW.y - PERSON_SIZE.y),
    );
    Walker {
        position: Interpolated::new(position),
        velocity: heading(index, 0),
        tint: if index.is_multiple_of(4) {
            tints[(index / 4) as usize % tints.len()]
        } else {
            Color::WHITE
        },
    }
}

/// A velocity of 20 to 60 pixels a second in a direction chosen by
/// `index` and `turn`.
fn heading(index: u32, turn: u32) -> Vec2 {
    let angle = unit(index, 2 + 2 * turn) * std::f32::consts::TAU;
    let speed = 20.0 + 40.0 * unit(index, 3 + 2 * turn);
    Vec2::new(angle.cos(), angle.sin()) * speed
}

/// A number in 0..1 for `index` and `salt`: the same every run.
fn unit(index: u32, salt: u32) -> f32 {
    // A 32-bit integer hash (lowbias32, by Chris Wellons).
    let mut x = index.wrapping_mul(0x9e37_79b9) ^ salt.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Moves the crowd one tick: each walker steps, bounces off the edges of
/// the view, and now and then turns.
fn walk_crowd(resources: &Resources, tick: &Tick) {
    let mut crowd = resources.resource_mut::<Crowd>();
    let half = PERSON_SIZE.x * 0.5;
    for (index, walker) in crowd.walkers.iter_mut().enumerate() {
        let index = index as u32;
        walker.position.snapshot();
        if (tick.index + u64::from(index)).is_multiple_of(TURN_TICKS) {
            let turn = (tick.index / TURN_TICKS) as u32;
            walker.velocity = heading(index, turn + 1);
        }
        let mut next = walker.position.current + walker.velocity * tick.dt;
        if next.x < half || next.x > VIEW.x - half {
            walker.velocity.x = -walker.velocity.x;
            next.x = next.x.clamp(half, VIEW.x - half);
        }
        if next.y < PERSON_SIZE.y || next.y > VIEW.y {
            walker.velocity.y = -walker.velocity.y;
            next.y = next.y.clamp(PERSON_SIZE.y, VIEW.y);
        }
        walker.position.current = next;
    }
}

/// Keeps the camera on the view's centre, where the crowd is.
fn fix_camera(resources: &Resources, _frame: &Frame) {
    resources.resource_mut::<RenderFrame>().camera.position = VIEW * 0.5;
}

/// The grass, then everything on it from the atlas: the trees and rocks,
/// the crowd and the player.
fn draw_crowd(resources: &Resources, frame: &Frame) {
    let crowd = resources.resource::<Crowd>();
    let player = resources.resource::<Player>();
    let mut out = resources.resource_mut::<RenderFrame>();
    push_grass(&mut out, resources.resource::<Sprites>().grass);
    let atlas = |position: Vec2, size: Vec2, uv: Rect| {
        let mut sprite = SpriteDraw::new(crowd.atlas, position, size);
        sprite.uv = uv;
        sprite.layer = 1;
        sprite
    };
    for &(kind, x, y) in &PROPS {
        let position = Vec2::new(x as f32, y as f32);
        out.sprites.push(match kind {
            Prop::Tree => atlas(position, Vec2::new(24.0, 48.0), TREE),
            Prop::Rock => atlas(position, Vec2::new(16.0, 12.0), ROCK),
        });
    }
    for walker in &crowd.walkers {
        let mut sprite = atlas(walker.position.lerp(frame.alpha), PERSON_SIZE, PERSON);
        sprite.flip.x = walker.velocity.x < 0.0;
        sprite.color = walker.tint;
        out.sprites.push(sprite);
    }
    let mut person = atlas(player.position.lerp(frame.alpha), PERSON_SIZE, PERSON);
    person.flip.x = player.facing_left;
    out.sprites.push(person);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scene;
    use crate::tests::{scene, tick};

    /// One frame of the stress scene, as the loop runs it.
    fn frame(resources: &Resources, index: u64) {
        crate::move_player(resources, &tick());
        walk_crowd(resources, &Tick { index, ..tick() });
        resources.resource_mut::<RenderFrame>().clear();
        let frame = Frame {
            dt: 1.0 / 60.0,
            alpha: 0.5,
        };
        fix_camera(resources, &frame);
        draw_crowd(resources, &frame);
    }

    #[test]
    fn a_frame_draws_everything_on_the_grass_from_the_atlas() {
        let app = scene(Scene::StressSprites);
        let resources = app.resources();
        frame(resources, 0);
        let out = resources.resource::<RenderFrame>();
        assert_eq!(out.sprites.len(), 16 * 12 + PROPS.len() + CROWD + 1);
        let atlas = resources.resource::<Crowd>().atlas;
        let (grass, rest) = out.sprites.split_at(16 * 12);
        assert!(grass.iter().all(|sprite| sprite.layer == 0));
        assert!(
            rest.iter()
                .all(|sprite| sprite.texture == atlas && sprite.layer == 1)
        );
        assert_eq!(out.camera.position, VIEW * 0.5);
    }

    #[test]
    fn the_crowd_stays_in_view_and_turns() {
        let app = scene(Scene::StressSprites);
        let resources = app.resources();
        let start: Vec<Vec2> = resources
            .resource::<Crowd>()
            .walkers
            .iter()
            .map(|walker| walker.velocity)
            .collect();
        // Long enough for every walker to reach an edge and to turn.
        for index in 0..600 {
            walk_crowd(resources, &Tick { index, ..tick() });
        }
        let crowd = resources.resource::<Crowd>();
        let half = PERSON_SIZE.x * 0.5;
        for walker in &crowd.walkers {
            let feet = walker.position.current;
            assert!((half..=VIEW.x - half).contains(&feet.x), "{feet:?}");
            assert!((PERSON_SIZE.y..=VIEW.y).contains(&feet.y), "{feet:?}");
        }
        let turned = crowd
            .walkers
            .iter()
            .zip(&start)
            .filter(|(walker, start)| walker.velocity != **start)
            .count();
        assert!(turned > CROWD * 9 / 10, "{turned}");
    }

    #[test]
    fn the_atlas_regions_are_inside_the_atlas_and_apart() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/sprites/atlas.qoi"
        ))
        .expect("the atlas");
        let size = |at: usize| {
            u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        assert_eq!((size(4), size(8)), (64, 64));
        let regions = [PERSON, TREE, ROCK];
        for (index, region) in regions.iter().enumerate() {
            assert!(region.min.x > 0.0 && region.min.y > 0.0, "{region:?}");
            assert!(region.max.x < 1.0 && region.max.y < 1.0, "{region:?}");
            for other in &regions[index + 1..] {
                let apart = region.max.x < other.min.x
                    || other.max.x < region.min.x
                    || region.max.y < other.min.y
                    || other.max.y < region.min.y;
                assert!(apart, "{region:?} and {other:?} touch");
            }
        }
    }

    /// A steady frame of the scene's own systems allocates nothing.
    #[cfg(feature = "dev")]
    #[test]
    fn a_steady_frame_of_the_scene_allocates_nothing() {
        let app = scene(Scene::StressSprites);
        let resources = app.resources();
        for index in 0..3 {
            frame(resources, index);
        }
        let start = nadir::thread_alloc_counts();
        for index in 3..103 {
            frame(resources, index);
        }
        let allocated = nadir::thread_alloc_counts().since(start);
        assert_eq!(allocated, nadir::AllocCounts::default());
    }
}
