//! The sandbox game. It drives the Nadir engine's requirements.
//!
//! A person walks a field of grass among trees and rocks, moved with WASD
//! or the arrow keys, and the camera follows them. Once a second it logs
//! how fast the simulation ticks and the frames come, and what was drawn.
//! Close the window to quit.
//!
//! - `--fps N`: at most `N` frames a second; 0 for no limit (default 60)
//! - `--no-vsync`: do not wait for the display's refresh

use std::time::Duration;

use nadir::log::info;
use nadir::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = nadir::cli::Args::from_env();
    let fps: Option<u32> = args.parsed("--fps")?;
    let vsync = !args.flag("--no-vsync");
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
        "sandbox: at most {} frames a second, vsync {}",
        config
            .frame_rate_limit
            .filter(|&fps| fps > 0)
            .map_or("unlimited".to_string(), |fps| fps.to_string()),
        if vsync { "on" } else { "off" }
    );
    game(App::new(config))?.run()?;
    Ok(())
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

/// The person.
struct Player {
    /// Where their feet are.
    position: Interpolated<Vec2>,
    /// Whether they face left.
    facing_left: bool,
}

/// The textures.
struct Sprites {
    person: Handle<Texture>,
    tree: Handle<Texture>,
    rock: Handle<Texture>,
    grass: Handle<Texture>,
}

/// The things that do not move.
struct Props(Vec<SpriteDraw>);

/// What the once-a-second report counts.
struct Report {
    stopwatch: Stopwatch,
    rate: TickRate,
    ticks: u64,
    frames: u64,
    frames_reported: u64,
}

/// Sets the game up on `app`: its textures, actions, state and systems.
fn game(mut app: App) -> Result<App, nadir::input::InputError> {
    let sprites = {
        let mut assets = app.resources_mut().resource_mut::<AssetServer>();
        Sprites {
            person: assets.load("sprites/person.qoi"),
            tree: assets.load("sprites/tree.qoi"),
            rock: assets.load("sprites/rock.qoi"),
            grass: assets.load("sprites/grass.qoi"),
        }
    };
    let props = Props(props(&sprites));

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

    Ok(app
        .add_actions(map)
        .insert_resource(sprites)
        .insert_resource(props)
        .insert_resource(Player {
            position: Interpolated::new(Vec2::new(240.0, 160.0)),
            facing_left: false,
        })
        .insert_resource(Report {
            stopwatch: Stopwatch::start(),
            rate: TickRate::default(),
            ticks: 0,
            frames: 0,
            frames_reported: 0,
        })
        .add_fixed_system("move_player", move_player)
        .add_fixed_system("count_ticks", |resources, _| {
            resources.resource_mut::<Report>().ticks += 1;
        })
        .add_frame_system("follow_camera", follow_camera)
        .add_frame_system("draw", draw)
        .add_frame_system("report", report))
}

/// Trees and rocks about the field.
fn props(sprites: &Sprites) -> Vec<SpriteDraw> {
    let mut props = Vec::new();
    for (x, y) in [
        (120, 120),
        (200, 90),
        (330, 140),
        (420, 230),
        (90, 260),
        (300, 300),
    ] {
        props.push(prop(sprites.tree, x, y, Vec2::new(24.0, 48.0)));
    }
    for (x, y) in [(160, 200), (280, 220), (380, 110), (60, 170)] {
        props.push(prop(sprites.rock, x, y, Vec2::new(16.0, 12.0)));
    }
    props
}

fn prop(texture: Handle<Texture>, x: i32, y: i32, size: Vec2) -> SpriteDraw {
    let mut sprite = SpriteDraw::new(texture, Vec2::new(x as f32, y as f32), size);
    sprite.layer = 1;
    sprite
}

/// Walks the person by the `Move` action.
fn move_player(resources: &Resources, tick: &Tick) {
    let direction = resources.resource::<ActionState<Act>>().axis2(Act::Move);
    let mut player = resources.resource_mut::<Player>();
    player.position.snapshot();
    player.position.current += direction * SPEED * tick.dt;
    if direction.x != 0.0 {
        player.facing_left = direction.x < 0.0;
    }
}

/// Centres the view on the person, where this frame draws them.
fn follow_camera(resources: &Resources, frame: &Frame) {
    let position = resources.resource::<Player>().position.lerp(frame.alpha);
    resources.resource_mut::<RenderFrame>().camera.position = position;
}

/// The grass, the props and the person.
fn draw(resources: &Resources, frame: &Frame) {
    let sprites = resources.resource::<Sprites>();
    let player = resources.resource::<Player>();
    let mut out = resources.resource_mut::<RenderFrame>();
    out.clear = Color::rgb(60, 90, 50);
    // Grass in 32-pixel tiles over the field, under everything.
    for row in 0..12 {
        for column in 0..16 {
            let corner = Vec2::new(column as f32 * 32.0, row as f32 * 32.0);
            let mut tile = SpriteDraw::new(sprites.grass, corner, Vec2::splat(32.0));
            tile.pivot = Vec2::ZERO;
            tile.sort = SortMode::Fixed(0);
            out.sprites.push(tile);
        }
    }
    out.sprites
        .extend_from_slice(&resources.resource::<Props>().0);
    let mut person = SpriteDraw::new(
        sprites.person,
        player.position.lerp(frame.alpha),
        Vec2::new(12.0, 20.0),
    );
    person.layer = 1;
    person.flip.x = player.facing_left;
    out.sprites.push(person);
}

/// Logs the tick and frame rates, and what was drawn, once a second.
fn report(resources: &Resources, _frame: &Frame) {
    let mut report = resources.resource_mut::<Report>();
    report.frames += 1;
    let elapsed = report.stopwatch.elapsed();
    let ticks = report.ticks;
    let since = report.rate.reported_at;
    if let Some(rate) = report.rate.sample(elapsed, ticks) {
        let frames = report.frames - report.frames_reported;
        report.frames_reported = report.frames;
        let fps = frames as f64 / (elapsed - since).as_secs_f64();
        let stats = *resources.resource::<FrameStats>();
        info!(
            "tick rate: {:.2} Hz over the last second, {:.2} Hz since the start \
             ({} ticks in {:.1} s); {fps:.1} frames a second; drawn: {} sprites in {} \
             batches, {} not ready",
            rate.recent_hz,
            rate.average_hz,
            rate.ticks,
            rate.elapsed.as_secs_f64(),
            stats.sprites,
            stats.batches,
            stats.skipped
        );
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

    /// The sandbox's resources, set up as the game sets them, without a
    /// window.
    fn sandbox() -> App {
        game(App::new(Config {
            assets: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
            ..Config::default()
        }))
        .expect("the sandbox sets up")
    }

    fn tick() -> Tick {
        Tick {
            index: 0,
            dt: 1.0 / 60.0,
        }
    }

    #[test]
    fn the_person_walks_two_pixels_a_tick_and_faces_the_way_they_go() {
        let app = sandbox();
        let resources = app.resources();
        let start = resources.resource::<Player>().position.current;
        resources
            .resource_mut::<Input>()
            .handle(&nadir::Event::Key {
                key: Key::A,
                pressed: true,
                repeat: false,
            });
        {
            let map = resources.resource::<ActionMap<Act>>();
            let mut state = resources.resource_mut::<ActionState<Act>>();
            resources.resource_mut::<Input>().latch(&map, &mut state);
        }
        move_player(resources, &tick());
        let player = resources.resource::<Player>();
        assert_eq!(player.position.current - start, Vec2::new(-2.0, 0.0));
        assert_eq!(player.position.previous, start);
        assert!(player.facing_left);
    }

    #[test]
    fn a_frame_draws_the_grass_the_props_and_the_person() {
        let app = sandbox();
        let resources = app.resources();
        let frame = Frame {
            dt: 0.016,
            alpha: 0.5,
        };
        follow_camera(resources, &frame);
        draw(resources, &frame);
        let out = resources.resource::<RenderFrame>();
        let props = resources.resource::<Props>().0.len();
        assert_eq!(out.sprites.len(), 16 * 12 + props + 1);
        let person = out.sprites.last().expect("the person");
        assert_eq!(
            person.position,
            resources.resource::<Player>().position.current
        );
        assert_eq!(out.camera.position, person.position);
    }

    #[test]
    fn every_sprite_file_is_there() {
        let app = sandbox();
        for name in ["person", "tree", "rock", "grass"] {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sprites/");
            let bytes = std::fs::read(format!("{path}{name}.qoi")).expect("the sprite file");
            assert!(bytes.starts_with(b"qoif"), "{name}");
        }
        assert!(app.resources().resource::<AssetServer>().has_pending());
    }
}
