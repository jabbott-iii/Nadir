//! The stress scene, `--scene stress-sprites`: 10,000 people walking about
//! in view, on a lawn of grass sprites with ten trees and rocks (the field
//! as it was before its map of tiles, kept so that the scene measures
//! sprites alone), for the budget of 10,000 sprites on screen at 60 frames
//! a second, in at most 30 draw calls, with no allocation in a frame
//! (`docs/00-overview.md`, roadmap M3).
//!
//! Everything that stands on the grass, the player included, is drawn from
//! one atlas texture, so that the 10,203 sprites are 2 batches: the grass,
//! and the rest. The camera stays on the view's centre, and the crowd
//! inside the view. Each walker is an entity, which the engine moves by
//! its velocity and animates: a walk of four frames from the atlas, each
//! walker at its own point in it, facing the way it goes. The player is
//! tinted yellow and drawn over the crowd, one
//! layer up, to be found in it: the crowd covers each spot about 20 deep.
//! They walk with WASD inside the view, which stays put, and stop at its
//! edges.

use std::time::Duration;

use nadir::prelude::*;

use crate::{Gait, Prop, STEP, Sprites, spawn_player};

/// How many people walk about.
pub(crate) const CROWD: usize = 10_000;

/// Where the trees and rocks stand: where their feet are.
const PROPS: [(Prop, i32, i32); 10] = [
    (Prop::Tree, 120, 120),
    (Prop::Tree, 200, 90),
    (Prop::Tree, 330, 140),
    (Prop::Tree, 420, 230),
    (Prop::Tree, 90, 260),
    (Prop::Tree, 300, 300),
    (Prop::Rock, 160, 200),
    (Prop::Rock, 280, 220),
    (Prop::Rock, 380, 110),
    (Prop::Rock, 60, 170),
];

/// The view, which the crowd stays inside.
const VIEW: Vec2 = Vec2::new(480.0, 270.0);

/// The atlas: `assets/sprites/atlas.qoi`, 128 × 64 pixels.
pub(crate) const ATLAS: &str = "sprites/atlas.qoi";
const ATLAS_SIZE: Vec2 = Vec2::new(128.0, 64.0);

/// A region of the atlas, from pixels to its normalized `uv`.
const fn region(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::new(
        Vec2::new(x / ATLAS_SIZE.x, y / ATLAS_SIZE.y),
        Vec2::new((x + width) / ATLAS_SIZE.x, (y + height) / ATLAS_SIZE.y),
    )
}

/// Where the person standing, the tree and the rock are in the atlas. Each
/// has a transparent pixel around it.
pub(crate) const PERSON: Rect = region(1.0, 1.0, 12.0, 20.0);
const TREE: Rect = region(18.0, 1.0, 24.0, 48.0);
const ROCK: Rect = region(1.0, 22.0, 16.0, 12.0);

/// The four frames of the person's walk in the atlas, in a row, apart.
pub(crate) const WALK: [Rect; 4] = [
    region(65.0, 1.0, 12.0, 20.0),
    region(78.0, 1.0, 12.0, 20.0),
    region(91.0, 1.0, 12.0, 20.0),
    region(104.0, 1.0, 12.0, 20.0),
];

/// A person's size, in pixels.
pub(crate) const PERSON_SIZE: Vec2 = Vec2::new(12.0, 20.0);

/// The player's tint in a crowd, so that they can be told from it.
pub(crate) const PLAYER_TINT: Color = Color::rgb(255, 225, 60);

/// How often a walker turns: once in this many ticks, each at its own time.
const TURN_TICKS: u64 = 120;

nadir::scene::reflect_newtype! {
    /// One of a crowd: its number, which picks its headings.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Walker(pub(crate) u32);
}

impl Component for Walker {}

/// How fast a crowd walks: its walkers' headings are multiplied by it.
pub(crate) struct Pace(pub(crate) f32);

/// Adds the stress scene's props, crowd, player and systems to `app`.
pub(crate) fn add(app: App) -> App {
    let atlas = app.world().resource_mut::<AssetServer>().load(ATLAS);
    let mut app = app.register_component::<Walker>();
    let gait = Gait::new(&app, atlas, PERSON, WALK);
    let world = app.world_mut();
    let sprite = |size: Vec2, uv: Rect| {
        let mut sprite = Sprite::new(atlas, size);
        sprite.uv = uv;
        sprite.layer = 1;
        sprite
    };
    for &(kind, x, y) in &PROPS {
        let prop = match kind {
            Prop::Tree => sprite(Vec2::new(24.0, 48.0), TREE),
            Prop::Rock => sprite(Vec2::new(16.0, 12.0), ROCK),
        };
        world
            .spawn()
            .with(Transform::at(Vec2::new(x as f32, y as f32)))
            .with(prop);
    }
    for index in 0..CROWD as u32 {
        let velocity = heading(index, 0);
        let mut walker = sprite(PERSON_SIZE, PERSON);
        walker.color = tint(index);
        world
            .spawn()
            .with(Walker(index))
            .with(Transform::at(start(index)))
            .with(Velocity(velocity))
            .with(walker)
            .with(Animator::starting_at(gait.walk, stagger(index)))
            .with(facing(velocity));
    }
    // Last, and a layer up: the same atlas right after the crowd, so still
    // one batch with it.
    let mut person = sprite(PERSON_SIZE, PERSON);
    person.color = PLAYER_TINT;
    person.layer = 2;
    spawn_player(world, person, &gait);
    app.insert_resource(Pace(1.0))
        .insert_resource(gait)
        .add_fixed_system(FixedStage::Gameplay, "turn_crowd", turn_crowd)
        .add_fixed_system(FixedStage::Movement, "bounce_crowd", bounce_crowd)
        .add_fixed_system(
            FixedStage::Movement,
            "keep_player_in_view",
            keep_player_in_view,
        )
        .add_frame_system(FrameStage::Camera, "fix_camera", fix_camera)
        .add_frame_system(FrameStage::Extract, "draw_grass", draw_grass)
}

/// The clear color, and grass in 32-pixel tiles over the view, under
/// everything.
fn draw_grass(world: &World, _frame: &Frame) {
    let grass = world.resource::<Sprites>().grass;
    let mut out = world.resource_mut::<RenderFrame>();
    out.clear = Color::rgb(60, 90, 50);
    for row in 0..12 {
        for column in 0..16 {
            let corner = Vec2::new(column as f32 * 32.0, row as f32 * 32.0);
            let mut tile = SpriteDraw::new(grass, corner, Vec2::splat(32.0));
            tile.pivot = Vec2::ZERO;
            tile.sort = SortMode::Fixed(0);
            out.sprites.push(tile);
        }
    }
}

/// Where walker `index` starts: somewhere in view.
fn start(index: u32) -> Vec2 {
    Vec2::new(
        PERSON_SIZE.x * 0.5 + unit(index, 0) * (VIEW.x - PERSON_SIZE.x),
        PERSON_SIZE.y + unit(index, 1) * (VIEW.y - PERSON_SIZE.y),
    )
}

/// A quarter of the crowd is tinted.
fn tint(index: u32) -> Color {
    let tints = [
        Color::rgb(255, 190, 190),
        Color::rgb(190, 210, 255),
        Color::rgb(210, 255, 190),
    ];
    if index.is_multiple_of(4) {
        tints[(index / 4) as usize % tints.len()]
    } else {
        Color::WHITE
    }
}

/// How far into the walk walker `index` starts, so that the crowd is not
/// in step.
pub(crate) fn stagger(index: u32) -> Duration {
    (STEP * 4).mul_f32(unit(index, 70))
}

/// The way a walker going at `velocity` faces: the art is drawn from the
/// side.
pub(crate) fn facing(velocity: Vec2) -> Facing {
    if velocity.x < 0.0 {
        Facing::Left
    } else {
        Facing::Right
    }
}

/// A velocity of 20 to 60 pixels a second in a direction chosen by
/// `index` and `turn`.
pub(crate) fn heading(index: u32, turn: u32) -> Vec2 {
    let angle = unit(index, 2 + 2 * turn) * std::f32::consts::TAU;
    let speed = 20.0 + 40.0 * unit(index, 3 + 2 * turn);
    Vec2::new(angle.cos(), angle.sin()) * speed
}

/// A number in 0..1 for `index` and `salt`: the same every run.
pub(crate) fn unit(index: u32, salt: u32) -> f32 {
    // A 32-bit integer hash (lowbias32, by Chris Wellons).
    let mut x = index.wrapping_mul(0x9e37_79b9) ^ salt.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Now and then turns each walker: once every [`TURN_TICKS`], each at its
/// own time, to a new heading times the [`Pace`].
pub(crate) fn turn_crowd(world: &World, tick: &Tick) {
    let pace = world.resource::<Pace>().0;
    let walkers = world.read::<Walker>();
    let mut velocities = world.write::<Velocity>();
    for (entity, &Walker(index)) in walkers.iter() {
        if (tick.index + u64::from(index)).is_multiple_of(TURN_TICKS)
            && let Some(velocity) = velocities.get_mut(entity)
        {
            let turn = (tick.index / TURN_TICKS) as u32;
            velocity.0 = heading(index, turn + 1) * pace;
        }
    }
}

/// After the engine has moved them: turns back each walker that left the
/// view, puts it back at the edge, and faces every walker the way it goes.
fn bounce_crowd(world: &World, _tick: &Tick) {
    let walkers = world.read::<Walker>();
    let mut transforms = world.write::<Transform>();
    let mut velocities = world.write::<Velocity>();
    let mut facings = world.write::<Facing>();
    let half = PERSON_SIZE.x * 0.5;
    for (entity, _) in walkers.iter() {
        let (Some(transform), Some(velocity)) =
            (transforms.get_mut(entity), velocities.get_mut(entity))
        else {
            continue;
        };
        let feet = &mut transform.position;
        if feet.x < half || feet.x > VIEW.x - half {
            velocity.0.x = -velocity.0.x;
            feet.x = feet.x.clamp(half, VIEW.x - half);
        }
        if feet.y < PERSON_SIZE.y || feet.y > VIEW.y {
            velocity.0.y = -velocity.0.y;
            feet.y = feet.y.clamp(PERSON_SIZE.y, VIEW.y);
        }
        if let Some(way) = facings.get_mut(entity) {
            *way = facing(velocity.0);
        }
    }
}

/// After the engine has moved them: stops the player at the edges of the
/// view, which the camera does not leave.
fn keep_player_in_view(world: &World, _tick: &Tick) {
    let players = world.read::<Player>();
    let mut transforms = world.write::<Transform>();
    let half = PERSON_SIZE.x * 0.5;
    for (player, _) in players.iter() {
        if let Some(transform) = transforms.get_mut(player) {
            let feet = &mut transform.position;
            feet.x = feet.x.clamp(half, VIEW.x - half);
            feet.y = feet.y.clamp(PERSON_SIZE.y, VIEW.y);
        }
    }
}

/// Keeps the camera on the view's centre, where the crowd is.
fn fix_camera(world: &World, _frame: &Frame) {
    world.resource_mut::<RenderFrame>().camera.position = VIEW * 0.5;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scene;
    use crate::tests::{TICK, player, position, scene};

    #[test]
    fn a_frame_draws_everything_on_the_grass_from_the_atlas() {
        let mut app = scene(Scene::StressSprites);
        app.update(TICK, &[]);
        let world = app.world();
        let out = world.resource::<RenderFrame>();
        assert_eq!(out.sprites.len(), 16 * 12 + PROPS.len() + CROWD + 1);
        let atlas = world
            .read::<Sprite>()
            .get(player(&app))
            .expect("drawn")
            .texture;
        let assets = world.resource::<AssetServer>();
        let loaded = assets.assets::<Texture>();
        assert_eq!(
            loaded.and_then(|textures| textures.handle(ATLAS)),
            Some(atlas)
        );
        let (grass, rest): (Vec<&SpriteDraw>, Vec<&SpriteDraw>) =
            out.sprites.iter().partition(|sprite| sprite.layer == 0);
        assert_eq!(grass.len(), 16 * 12);
        assert!(rest.iter().all(|sprite| sprite.texture == atlas));
        // The player, tinted, over everything else on the grass.
        let (over, crowd): (Vec<&SpriteDraw>, Vec<&SpriteDraw>) =
            rest.iter().partition(|sprite| sprite.layer == 2);
        assert!(crowd.iter().all(|sprite| sprite.layer == 1));
        assert_eq!(over.len(), 1);
        assert_eq!(over[0].position, position(&app, player(&app)));
        assert_eq!(over[0].color, PLAYER_TINT);
        assert_eq!(over[0].uv, PERSON, "the player stands");
        assert_eq!(out.camera.position, VIEW * 0.5);
        // The crowd walks, out of step: every frame of the walk is drawn,
        // each by about a quarter of the crowd. The rest are the props.
        let mut frames = [0; 4];
        for walker in crowd.iter().filter(|sprite| sprite.size == PERSON_SIZE) {
            let frame = WALK.iter().position(|&uv| uv == walker.uv);
            frames[frame.expect("a frame of the walk")] += 1;
        }
        assert_eq!(frames.iter().sum::<usize>(), CROWD);
        assert!(frames.iter().all(|&n| n > CROWD / 5), "{frames:?}");
    }

    #[test]
    fn the_person_walks_among_the_crowd_too() {
        let mut app = scene(Scene::StressSprites);
        let player = player(&app);
        let press = nadir::Event::Key {
            key: Key::D,
            pressed: true,
            repeat: false,
        };
        app.update(TICK, &[press]);
        for _ in 0..59 {
            app.update(TICK, &[]);
        }
        let moved = position(&app, player) - crate::PLAYER_START;
        assert!(
            (moved.x - 120.0).abs() < 1e-2 && moved.y == 0.0,
            "{moved:?}"
        );
        // Held on, they stop at the edge of the view, as the crowd does.
        for _ in 0..120 {
            app.update(TICK, &[]);
        }
        let feet = position(&app, player);
        assert_eq!(feet.x, VIEW.x - PERSON_SIZE.x * 0.5, "{feet:?}");
    }

    #[test]
    fn the_crowd_stays_in_view_turns_and_faces_the_way_it_goes() {
        let mut app = scene(Scene::StressSprites);
        let start: Vec<Vec2> = (0..CROWD as u32).map(|index| heading(index, 0)).collect();
        // Long enough for every walker to reach an edge and to turn.
        for _ in 0..600 {
            app.update(TICK, &[]);
        }
        let world = app.world();
        let walkers = world.read::<Walker>();
        let transforms = world.read::<Transform>();
        let velocities = world.read::<Velocity>();
        let sprites = world.read::<Sprite>();
        let half = PERSON_SIZE.x * 0.5;
        let mut turned = 0;
        for (entity, &Walker(index)) in walkers.iter() {
            let feet = transforms.get(entity).expect("a walker").position;
            assert!((half..=VIEW.x - half).contains(&feet.x), "{feet:?}");
            assert!((PERSON_SIZE.y..=VIEW.y).contains(&feet.y), "{feet:?}");
            let velocity = velocities.get(entity).expect("a walker").0;
            let flip = sprites.get(entity).expect("a walker").flip.x;
            assert_eq!(flip, velocity.x < 0.0, "walker {index}");
            if velocity != start[index as usize] {
                turned += 1;
            }
        }
        assert_eq!(walkers.len(), CROWD);
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
        assert_eq!((size(4), size(8)), (128, 64));
        let regions = [PERSON, TREE, ROCK, WALK[0], WALK[1], WALK[2], WALK[3]];
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

    /// A steady frame of the scene allocates nothing.
    #[cfg(feature = "dev")]
    #[test]
    fn a_steady_frame_of_the_scene_allocates_nothing() {
        let mut app = scene(Scene::StressSprites);
        crate::tests::without_reports(&app);
        for _ in 0..3 {
            app.update(TICK, &[]);
        }
        let start = nadir::thread_alloc_counts();
        for _ in 0..100 {
            app.update(TICK, &[]);
        }
        let allocated = nadir::thread_alloc_counts().since(start);
        assert_eq!(allocated, nadir::AllocCounts::default());
    }
}
