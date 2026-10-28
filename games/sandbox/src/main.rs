//! The sandbox game. It drives the Nadir engine's requirements.
//!
//! A person walks a field, a map of tiles built by hand (`map.rs`): grass,
//! paths, flowers and rippling water, walled in stone, with tall walls,
//! trees and rocks to walk behind and in front of. They are moved with WASD
//! or the arrow keys, collide with the walls, the water, the trees and the
//! rocks, and the camera follows them. Once a second it logs how fast the
//! simulation ticks and the frames come, how long the ticks take, and what
//! was drawn; with the `dev` feature, also how often the frames allocated.
//! Close the window to quit.
//!
//! - `--fps N`: at most `N` frames a second; 0 for no limit (default 60)
//! - `--no-vsync`: do not wait for the display's refresh
//! - `--scene NAME`: `field` (the default); `stress-sprites`, where 10,000
//!   people walk about in view on a lawn of grass sprites (`stress.rs`); or
//!   `stress-entities`, where 5,000 people jostle in a maze of walls
//!   (`stress_entities.rs`)

mod map;
mod stress;
mod stress_entities;

use std::fmt;
use std::time::Duration;

use nadir::log::info;
use nadir::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = nadir::cli::Args::from_env();
    let fps: Option<u32> = args.parsed("--fps")?;
    let vsync = !args.flag("--no-vsync");
    let scene = match args.value("--scene")? {
        None => Scene::Field,
        Some(name) => Scene::from_name(&name).ok_or_else(|| {
            format!(
                "there is no scene `{name}`: try `field`, `stress-sprites` or `stress-entities`"
            )
        })?,
    };
    // `cargo dev` ends in `--` already, so `cargo dev -- --fps 144` would
    // pass the options as positionals: refuse them rather than ignore them.
    if let Some(extra) = args.finish()?.first() {
        return Err(format!("unexpected argument `{extra}`").into());
    }

    let config = Config {
        title: "Sandbox".into(),
        game: "Sandbox".into(),
        frame_rate_limit: Some(fps.unwrap_or(60)),
        vsync,
        assets: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
        ..Config::default()
    };
    info!(
        "sandbox: scene {}, at most {} frames a second, vsync {}",
        scene.name(),
        config
            .frame_rate_limit
            .filter(|&fps| fps > 0)
            .map_or("unlimited".to_string(), |fps| fps.to_string()),
        if vsync { "on" } else { "off" }
    );
    game(App::new(config), scene)?.run()?;
    Ok(())
}

/// What the sandbox shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scene {
    /// The person among trees and rocks.
    Field,
    /// 10,000 people walking about in view, on a lawn of grass sprites with
    /// ten trees and rocks: the field as it was before its map of tiles.
    StressSprites,
    /// 5,000 people jostling in a maze of walls.
    StressEntities,
}

impl Scene {
    /// The scene of a `--scene` name.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "field" => Some(Scene::Field),
            "stress-sprites" => Some(Scene::StressSprites),
            "stress-entities" => Some(Scene::StressEntities),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Scene::Field => "field",
            Scene::StressSprites => "stress-sprites",
            Scene::StressEntities => "stress-entities",
        }
    }
}

/// What the player does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    /// Walk: WASD or the arrow keys.
    Move,
}

impl ActionSet for Act {
    const ALL: &'static [Self] = &[Act::Move];

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        "move"
    }

    fn kind(self) -> ActionKind {
        ActionKind::Axis2
    }
}

/// How fast the person walks, in pixels a second: two a tick.
const SPEED: f32 = 120.0;

/// Where the person starts: where their feet are.
const PLAYER_START: Vec2 = Vec2::new(240.0, 160.0);

/// The textures.
struct Sprites {
    person: Handle<Texture>,
    tree: Handle<Texture>,
    rock: Handle<Texture>,
    grass: Handle<Texture>,
    /// The tile set of the field and of the maze ([`map::TILE_SET`]).
    tiles: Handle<Texture>,
}

/// A thing that does not move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prop {
    Tree,
    Rock,
}

/// The tile layer a scene draws, and its clear color, which shows outside
/// it.
struct Ground {
    tiles: Handle<TileLayer>,
    outside: Color,
}

/// What the once-a-second report counts.
struct Report {
    stopwatch: Stopwatch,
    rate: TickRate,
    ticks: u64,
    frames: u64,
    frames_reported: u64,
    /// The ticks timed since the last report, how long they took, and the
    /// longest of them ([`TickStats`]).
    timed_ticks: u32,
    tick_time: Duration,
    longest_tick: Duration,
}

/// Sets the game up on `app`: its textures, actions, entities and systems.
fn game(app: App, scene: Scene) -> Result<App, nadir::input::InputError> {
    let sprites = {
        let mut assets = app.world().resource_mut::<AssetServer>();
        Sprites {
            person: assets.load("sprites/person.qoi"),
            tree: assets.load("sprites/tree.qoi"),
            rock: assets.load("sprites/rock.qoi"),
            grass: assets.load("sprites/grass.qoi"),
            tiles: assets.load(map::TILE_SET),
        }
    };

    let mut map = ActionMap::<Act>::new()?;
    let keys = |up, down, left, right| Binding::KeyComposite2 {
        up,
        down,
        left,
        right,
    };
    map.bind(Act::Move, 0, keys(Key::W, Key::S, Key::A, Key::D))?;
    map.bind(
        Act::Move,
        1,
        keys(Key::Up, Key::Down, Key::Left, Key::Right),
    )?;

    let app = app
        .add_actions(map)
        .insert_resource(Report {
            stopwatch: Stopwatch::start(),
            rate: TickRate::default(),
            ticks: 0,
            frames: 0,
            frames_reported: 0,
            timed_ticks: 0,
            tick_time: Duration::ZERO,
            longest_tick: Duration::ZERO,
        })
        .add_fixed_system(FixedStage::Gameplay, "move_player", move_player)
        .add_fixed_system(FixedStage::Cleanup, "count_ticks", count_ticks)
        .add_frame_system(FrameStage::Ui, "report", report);
    #[cfg(feature = "dev")]
    let app = app.insert_resource(AllocTally::default());
    let app = match scene {
        Scene::Field => field(app, &sprites),
        Scene::StressSprites => stress::add(app),
        Scene::StressEntities => stress_entities::add(app, sprites.tiles),
    };
    Ok(app.insert_resource(sprites))
}

/// A collider at the feet: a box of `half` half-extents, whose bottom edge
/// is the entity's position, on `layer`, colliding with `mask`.
fn at_feet(half: Vec2, layer: LayerMask, mask: LayerMask) -> Collider {
    Collider(nadir::collision::Collider {
        offset: Vec2::new(0.0, -half.y),
        layer,
        mask,
        ..nadir::collision::Collider::solid(Shape::Aabb { half })
    })
}

/// The field: the map's ground and grid, its tall walls, trees and rocks,
/// then the person, and the camera on them.
fn field(mut app: App, sprites: &Sprites) -> App {
    let set = map::tile_set(sprites.tiles);
    let map = map::build(set);
    let world = app.world_mut();
    for &feet in &map.walls {
        world
            .spawn()
            .with(Transform::at(feet))
            .with(map::tall_wall(set));
    }
    // A trunk or a rock stops the person at its foot, so that they are
    // drawn behind it when they walk into it from above, and in front of it
    // from below.
    for &(kind, feet) in &map.props {
        let (mut sprite, half) = match kind {
            Prop::Tree => (
                Sprite::new(sprites.tree, Vec2::new(24.0, 48.0)),
                Vec2::new(4.0, 3.0),
            ),
            Prop::Rock => (
                Sprite::new(sprites.rock, Vec2::new(16.0, 12.0)),
                Vec2::new(7.0, 4.0),
            ),
        };
        sprite.layer = 1;
        world
            .spawn()
            .with(Transform::at(feet))
            .with(sprite)
            .with(at_feet(half, map::WORLD, map::WORLD));
    }
    let mut person = Sprite::new(sprites.person, Vec2::new(12.0, 20.0));
    person.layer = 1;
    let player = spawn_player(world, person);
    world.write::<Collider>().insert(
        player,
        at_feet(Vec2::new(5.0, 3.0), map::PEOPLE, map::WORLD),
    );
    world.resource_mut::<Collisions>().set_grid(map.grid);
    let tiles = world.resource_mut::<TileLayers>().add(map.ground);
    app.insert_resource(Ground {
        tiles,
        outside: Color::rgb(84, 84, 96),
    })
    .add_frame_system(FrameStage::Camera, "follow_camera", follow_camera)
    .add_frame_system(FrameStage::Extract, "draw_tiles", draw_tiles)
}

/// The person, at [`PLAYER_START`], drawn as `sprite`.
fn spawn_player(world: &mut World, sprite: Sprite) -> Entity {
    world
        .spawn()
        .with(Player)
        .with(Transform::at(PLAYER_START))
        .with(Velocity::default())
        .with(sprite)
        .id()
}

/// Walks the person by the `Move` action, and faces them the way they go.
fn move_player(world: &World, _tick: &Tick) {
    let direction = world.resource::<ActionState<Act>>().axis2(Act::Move);
    let players = world.read::<Player>();
    let mut velocities = world.write::<Velocity>();
    let mut sprites = world.write::<Sprite>();
    for (player, _) in players.iter() {
        if let Some(velocity) = velocities.get_mut(player) {
            velocity.0 = direction * SPEED;
        }
        if direction.x != 0.0
            && let Some(sprite) = sprites.get_mut(player)
        {
            sprite.flip.x = direction.x < 0.0;
        }
    }
}

fn count_ticks(world: &World, _tick: &Tick) {
    world.resource_mut::<Report>().ticks += 1;
}

/// Where this frame draws `entity`: `alpha` of the way from where it was
/// at the start of the last tick to where that tick left it.
fn drawn_position(world: &World, entity: Entity, alpha: f32) -> Option<Vec2> {
    let current = world.read::<Transform>().get(entity)?.position;
    let previous = world.read::<PrevTransform>().get(entity).copied();
    Some(previous.map_or(current, |previous| previous.0.position.lerp(current, alpha)))
}

/// Centres the view on the person, where this frame draws them.
fn follow_camera(world: &World, frame: &Frame) {
    let player = world.read::<Player>().entities().first().copied();
    if let Some(position) = player.and_then(|player| drawn_position(world, player, frame.alpha)) {
        world.resource_mut::<RenderFrame>().camera.position = position;
    }
}

/// The scene's tile layer, under everything, and its clear color around it.
fn draw_tiles(world: &World, _frame: &Frame) {
    let ground = world.resource::<Ground>();
    let mut out = world.resource_mut::<RenderFrame>();
    out.clear = ground.outside;
    out.tile_layers.push(TileLayerDraw::new(ground.tiles));
}

/// How often the frames allocated since the last report (feature `dev`).
#[cfg(feature = "dev")]
#[derive(Default)]
struct AllocTally {
    /// Allocations in the frames since the last report.
    since_report: u64,
    /// What the last report's own log line allocated: the log allocates
    /// until its ring of records is full. It is not the frames' doing, so
    /// it is taken back out.
    report_line: u64,
}

/// The allocation part of a report: nothing without the `dev` feature.
struct Allocations(Option<u64>);

impl fmt::Display for Allocations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(count) => write!(f, "; allocated {count} times since the last report"),
            None => Ok(()),
        }
    }
}

/// How long the ticks took, for a report: the mean and the longest, in
/// milliseconds.
struct TickTimes {
    ticks: u32,
    time: Duration,
    longest: Duration,
}

impl fmt::Display for TickTimes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ticks == 0 {
            return write!(f, "no tick ran");
        }
        let mean = self.time.as_secs_f64() * 1000.0 / f64::from(self.ticks);
        let longest = self.longest.as_secs_f64() * 1000.0;
        write!(f, "a tick took {mean:.2} ms, {longest:.2} ms at most")
    }
}

/// Logs the tick and frame rates, how long the ticks took, and what was
/// drawn, once a second.
fn report(world: &World, _frame: &Frame) {
    // The last frame's allocations, read in this one.
    #[cfg(feature = "dev")]
    let mut tally = {
        let mut tally = world.resource_mut::<AllocTally>();
        let last = world.resource::<nadir::FrameAllocs>().0.allocations;
        let report_line = std::mem::take(&mut tally.report_line);
        tally.since_report += last.saturating_sub(report_line);
        tally
    };
    let mut report = world.resource_mut::<Report>();
    report.frames += 1;
    let ticked = *world.resource::<TickStats>();
    report.timed_ticks += ticked.ticks;
    report.tick_time += ticked.time;
    report.longest_tick = report.longest_tick.max(ticked.longest);
    let elapsed = report.stopwatch.elapsed();
    let ticks = report.ticks;
    let since = report.rate.reported_at;
    if let Some(rate) = report.rate.sample(elapsed, ticks) {
        let frames = report.frames - report.frames_reported;
        report.frames_reported = report.frames;
        let fps = frames as f64 / (elapsed - since).as_secs_f64();
        let times = TickTimes {
            ticks: std::mem::take(&mut report.timed_ticks),
            time: std::mem::take(&mut report.tick_time),
            longest: std::mem::take(&mut report.longest_tick),
        };
        let stats = *world.resource::<FrameStats>();
        #[cfg(feature = "dev")]
        let (allocations, before) = (
            Allocations(Some(std::mem::take(&mut tally.since_report))),
            nadir::thread_alloc_counts(),
        );
        #[cfg(not(feature = "dev"))]
        let allocations = Allocations(None);
        info!(
            "tick rate: {:.2} Hz over the last second, {:.2} Hz since the start \
             ({} ticks in {:.1} s); {times}; {fps:.1} frames a second; drawn: {} sprites \
             in {} batches, {} not ready, {} tiles in {} chunks{allocations}",
            rate.recent_hz,
            rate.average_hz,
            rate.ticks,
            rate.elapsed.as_secs_f64(),
            stats.sprites,
            stats.batches,
            stats.skipped,
            stats.tiles,
            stats.chunks
        );
        #[cfg(feature = "dev")]
        {
            tally.report_line = nadir::thread_alloc_counts().since(before).allocations;
        }
    }
}

/// How often the tick rate is reported.
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// Measures the tick rate against real time.
#[derive(Debug, Default)]
struct TickRate {
    /// When the last report was made, since the start.
    reported_at: Duration,
    /// How many ticks had run by then.
    reported_ticks: u64,
}

/// One measurement of the tick rate.
#[derive(Debug, PartialEq)]
struct RateReport {
    /// The rate since the report before this one.
    recent_hz: f64,
    /// The rate since the start.
    average_hz: f64,
    /// All the ticks so far.
    ticks: u64,
    /// The time since the start.
    elapsed: Duration,
}

impl TickRate {
    /// Takes the time since the start and the ticks run so far. Returns a
    /// report if [`REPORT_INTERVAL`] has passed since the last one.
    fn sample(&mut self, elapsed: Duration, ticks: u64) -> Option<RateReport> {
        let since_report = elapsed.checked_sub(self.reported_at)?;
        if since_report < REPORT_INTERVAL {
            return None;
        }
        let recent_ticks = ticks.saturating_sub(self.reported_ticks);
        let report = RateReport {
            recent_hz: recent_ticks as f64 / since_report.as_secs_f64(),
            average_hz: ticks as f64 / elapsed.as_secs_f64(),
            ticks,
            elapsed,
        };
        self.reported_at = elapsed;
        self.reported_ticks = ticks;
        Some(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// One tick's length, as a frame: it runs one tick.
    pub(crate) const TICK: Duration = Duration::from_nanos(16_666_667);

    #[test]
    fn nothing_is_reported_within_the_first_second() {
        let mut rate = TickRate::default();
        assert_eq!(rate.sample(Duration::ZERO, 0), None);
        assert_eq!(rate.sample(500 * MS, 30), None);
        assert_eq!(rate.sample(999 * MS, 59), None);
    }

    #[test]
    fn a_report_comes_once_a_second_with_both_rates() {
        let mut rate = TickRate::default();
        let first = rate.sample(1000 * MS, 60);
        let expected = RateReport {
            recent_hz: 60.0,
            average_hz: 60.0,
            ticks: 60,
            elapsed: 1000 * MS,
        };
        assert_eq!(first, Some(expected));
        // Not again until a second after that report.
        assert_eq!(rate.sample(1016 * MS, 61), None);
        assert_eq!(rate.sample(1999 * MS, 119), None);

        // A slow second: 30 ticks. The average covers both seconds.
        let second = rate.sample(2000 * MS, 90);
        let expected = RateReport {
            recent_hz: 30.0,
            average_hz: 45.0,
            ticks: 90,
            elapsed: 2000 * MS,
        };
        assert_eq!(second, Some(expected));
    }

    #[test]
    fn a_late_report_measures_the_time_that_really_passed() {
        let mut rate = TickRate::default();
        // The frame that reports came 250 ms late, and ticks kept running.
        let report = rate.sample(1250 * MS, 75);
        assert_eq!(report.map(|report| report.recent_hz), Some(60.0));
        // The next second is counted from that report.
        assert_eq!(rate.sample(2249 * MS, 134), None);
        let report = rate.sample(2250 * MS, 135);
        assert_eq!(report.map(|report| report.recent_hz), Some(60.0));
    }

    #[test]
    fn a_report_gives_the_mean_and_the_longest_tick() {
        let times = TickTimes {
            ticks: 4,
            time: 10 * MS,
            longest: 4 * MS,
        };
        assert_eq!(times.to_string(), "a tick took 2.50 ms, 4.00 ms at most");
        let none = TickTimes {
            ticks: 0,
            time: Duration::ZERO,
            longest: Duration::ZERO,
        };
        assert_eq!(none.to_string(), "no tick ran");
    }

    /// The field, set up as the game sets it, without a window.
    fn sandbox() -> App {
        scene(Scene::Field)
    }

    /// A scene, set up as the game sets it, without a window.
    pub(crate) fn scene(scene: Scene) -> App {
        game(
            App::new(Config {
                assets: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
                ..Config::default()
            }),
            scene,
        )
        .expect("the sandbox sets up")
    }

    /// Keeps the once-a-second report quiet: it goes by the wall clock, and
    /// its log line allocates, which a count of a scene's frames leaves out
    /// as the game does (`AllocTally::report_line`).
    #[cfg(feature = "dev")]
    pub(crate) fn without_reports(app: &App) {
        app.world().resource_mut::<Report>().rate.reported_at = Duration::MAX;
    }

    /// The player.
    pub(crate) fn player(app: &App) -> Entity {
        let players = app.world().read::<Player>();
        assert_eq!(players.len(), 1, "one player");
        players.entities()[0]
    }

    pub(crate) fn position(app: &App, entity: Entity) -> Vec2 {
        let transforms = app.world().read::<Transform>();
        transforms.get(entity).expect("it has a Transform").position
    }

    #[test]
    fn the_schedule_runs_the_games_systems_among_the_engines() {
        let printed: Vec<String> = sandbox()
            .schedule()
            .describe()
            .map(|system| system.to_string())
            .collect();
        assert_eq!(
            printed,
            [
                "tick  Input: nadir::snapshot",
                "tick  Gameplay: move_player",
                "tick  Movement: nadir::movement",
                "tick  PostMovement: nadir::parent_offsets",
                "tick  PostMovement: nadir::triggers",
                "tick  Cleanup: count_ticks",
                "frame Camera: follow_camera",
                "frame Ui: report",
                "frame Extract: nadir::extract_sprites",
                "frame Extract: draw_tiles",
            ]
        );
    }

    #[test]
    fn the_person_walks_two_pixels_a_tick_and_faces_the_way_they_go() {
        let mut app = sandbox();
        let player = player(&app);
        let press = nadir::Event::Key {
            key: Key::A,
            pressed: true,
            repeat: false,
        };
        app.update(TICK, &[press]);
        assert_eq!(position(&app, player) - PLAYER_START, Vec2::new(-2.0, 0.0));
        let world = app.world();
        let previous = world.read::<PrevTransform>().get(player).copied();
        assert_eq!(previous, Some(PrevTransform(Transform::at(PLAYER_START))));
        assert!(world.read::<Sprite>().get(player).expect("drawn").flip.x);
        // Held for another tick: two more pixels.
        app.update(TICK, &[]);
        assert_eq!(position(&app, player) - PLAYER_START, Vec2::new(-4.0, 0.0));
    }

    #[test]
    fn a_frame_draws_the_map_its_walls_and_props_and_the_person() {
        let mut app = sandbox();
        // Half a tick: drawn where everything is, with no tick yet.
        app.update(8 * MS, &[]);
        let world = app.world();
        let out = world.resource::<RenderFrame>();
        let map = map::build(map::tile_set(world.resource::<Sprites>().tiles));
        assert_eq!(out.sprites.len(), map.walls.len() + map.props.len() + 1);
        assert!(out.sprites.iter().all(|sprite| sprite.layer == 1));
        let person = world.resource::<Sprites>().person;
        let drawn: Vec<&SpriteDraw> = out
            .sprites
            .iter()
            .filter(|sprite| sprite.texture == person)
            .collect();
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].position, PLAYER_START);
        assert_eq!((drawn[0].layer, drawn[0].size), (1, Vec2::new(12.0, 20.0)));
        assert_eq!(out.camera.position, PLAYER_START);
        // The ground, under everything, and around it the stone's color.
        let ground = world.resource::<Ground>();
        assert_eq!(out.tile_layers, [TileLayerDraw::new(ground.tiles)]);
        assert_eq!(out.clear, ground.outside);
        let layers = world.resource::<TileLayers>();
        let tiles = layers.get(ground.tiles).expect("the ground");
        assert_eq!(tiles.size(), UVec2::new(map::WIDTH, map::HEIGHT));
    }

    /// Holds `key` down for `ticks` ticks.
    fn hold(app: &mut App, key: Key, ticks: u32) {
        let press = nadir::Event::Key {
            key,
            pressed: true,
            repeat: false,
        };
        app.update(TICK, &[press]);
        for _ in 1..ticks {
            app.update(TICK, &[]);
        }
        let release = nadir::Event::Key {
            key,
            pressed: false,
            repeat: false,
        };
        app.update(TICK, &[release]);
    }

    /// Moves the player to `feet`, between ticks.
    fn put(app: &mut App, feet: Vec2) {
        let player = player(app);
        app.world()
            .write::<Transform>()
            .insert(player, Transform::at(feet));
        app.world()
            .write::<PrevTransform>()
            .insert(player, PrevTransform(Transform::at(feet)));
    }

    #[test]
    fn the_person_slides_along_the_maps_border_wall_without_snagging() {
        // Under the top wall, whose bottom is at y = 16: with W and D held,
        // the person goes up to it, then slides along it to the right,
        // across its seams, never stopped. `Move` is a unit direction, so
        // they go 2 / √2 pixels a tick each way.
        let mut app = sandbox();
        let player = player(&app);
        put(&mut app, Vec2::new(100.5, 40.0));
        let press = |key| nadir::Event::Key {
            key,
            pressed: true,
            repeat: false,
        };
        app.update(TICK, &[press(Key::W), press(Key::D)]);
        let step = SPEED / 60.0 / 2f32.sqrt();
        let mut last = position(&app, player);
        for tick in 0..150 {
            app.update(TICK, &[]);
            let now = position(&app, player);
            assert!(
                (now.x - last.x - step).abs() < 1e-3,
                "tick {tick}: from {last:?} to {now:?}"
            );
            last = now;
        }
        // The collider's top, 6 above the feet, is at the wall's bottom.
        assert!((last.y - 22.0).abs() < 1e-2, "{last:?}");
        assert!(last.x > 100.5 + 150.0 * step - 1e-2, "{last:?}");
    }

    #[test]
    fn a_tree_is_in_front_of_the_person_above_it_and_behind_the_one_below() {
        // The tree of the map at cell (6, 9), whose feet are at (104, 160),
        // a trunk 8 wide and 6 high above them.
        let mut app = sandbox();
        let tree = Vec2::new(104.0, 160.0);
        let player = player(&app);
        // From above, walking down into the trunk: stopped above its foot.
        put(&mut app, tree - Vec2::new(0.0, 30.0));
        hold(&mut app, Key::S, 40);
        let above = position(&app, player);
        assert!((above.y - (tree.y - 6.0)).abs() < 1e-2, "{above:?}");
        // From below, walking up: stopped 6 below its foot.
        put(&mut app, tree + Vec2::new(0.0, 30.0));
        hold(&mut app, Key::W, 40);
        let below = position(&app, player);
        assert!((below.y - (tree.y + 6.0)).abs() < 1e-2, "{below:?}");
        // So the frame draws the person behind the tree, then in front.
        let mut order = |feet: Vec2| {
            put(&mut app, feet);
            app.update(TICK, &[]);
            let out = app.world().resource::<RenderFrame>();
            let sprites = app.world().resource::<Sprites>();
            let nearest = |texture| {
                out.sprites
                    .iter()
                    .filter(|sprite| sprite.texture == texture)
                    .min_by(|a, b| {
                        (a.position - tree)
                            .length()
                            .total_cmp(&(b.position - tree).length())
                    })
                    .copied()
                    .expect("drawn")
            };
            let (person, tree) = (nearest(sprites.person), nearest(sprites.tree));
            // Sorted against each other by their feet: one layer, y-sorted.
            assert_eq!((person.layer, person.sort), (tree.layer, SortMode::YSort));
            assert_eq!(tree.sort, SortMode::YSort);
            (person.position.y, tree.position.y)
        };
        let (person, tree_y) = order(above);
        assert!(person < tree_y, "{person} {tree_y}");
        let (person, tree_y) = order(below);
        assert!(person > tree_y, "{person} {tree_y}");
    }

    #[test]
    fn the_camera_follows_the_person_between_ticks() {
        let mut app = sandbox();
        let press = nadir::Event::Key {
            key: Key::D,
            pressed: true,
            repeat: false,
        };
        app.update(TICK, &[press]);
        // A quarter of the way to the next tick: drawn half a pixel on.
        app.update(TICK / 4, &[]);
        let camera = app.world().resource::<RenderFrame>().camera.position;
        assert!(
            (camera.x - (PLAYER_START.x + 0.5)).abs() < 1e-3,
            "{camera:?}"
        );
    }

    #[test]
    fn a_scene_is_chosen_by_name() {
        for scene in [Scene::Field, Scene::StressSprites, Scene::StressEntities] {
            assert_eq!(Scene::from_name(scene.name()), Some(scene));
        }
        assert_eq!(Scene::from_name("Field"), None);
        assert_eq!(Allocations(None).to_string(), "");
        assert_eq!(
            Allocations(Some(0)).to_string(),
            "; allocated 0 times since the last report"
        );
    }

    #[test]
    fn every_sprite_file_is_there() {
        let app = sandbox();
        for name in ["person", "tree", "rock", "grass", "atlas", "tiles"] {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sprites/");
            let bytes = std::fs::read(format!("{path}{name}.qoi")).expect("the sprite file");
            assert!(bytes.starts_with(b"qoif"), "{name}");
        }
        assert!(app.world().resource::<AssetServer>().has_pending());
    }
}
