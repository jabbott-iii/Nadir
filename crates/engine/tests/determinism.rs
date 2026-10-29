//! The determinism test (roadmap M4.7, `docs/04-quality.md`): a small game,
//! built from `nadir`'s public API, replays `determinism/replay.nrpl` and must
//! reach the world hashes in `determinism/hashes.txt`, tick after tick,
//! whatever the frames.
//!
//! The game: a player steered by a `Move` action and dashing on `Dash`, 200
//! walkers on a walled grid of tiles that turn now and then, a tenth of
//! them triggers, and pickups that drop near the player and score when
//! entered. Every random number comes from one seeded [`Pcg32`], at setup
//! and then in fixed systems, in a fixed order.
//!
//! The game's own systems use `+`, `-`, `*`, `/` and casts to integers; the
//! engine's tick systems add `sqrt`, `floor`, `ceil`, `abs`, `min`, `max`
//! and `clamp`. All of them round exactly alike on every system, but a
//! zero's sign or a NaN's bits from `min`, `max` or `clamp` may differ. The
//! hash ignores them, so one `hashes.txt` serves Linux, Windows and macOS
//! unless one changes a later value, which CI's runs on all three would
//! show (ADR-0025).
//!
//! The fixtures come from a live run of a scripted input, in frames of
//! mixed lengths, which records its replay. To write them anew:
//! `NADIR_BLESS=1 cargo test -p nadir --test determinism`. That checks the
//! run twice, and its replay at three other pacings, before it writes.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nadir::ecs::{Component, Events, FixedStage, World};
use nadir::input::{
    ActionKind, ActionMap, ActionSet, ActionState, Binding, InputPlayer, InputRecorder,
};
use nadir::log::{error, info};
use nadir::prelude::*;
use nadir::scene::{FromValue, ReflectContext, TypeRegistry};
use nadir::{Event, Replay, SaveId};

/// The seed of the game's random numbers.
const SEED: u64 = 24_301;

/// How many ticks the live run runs, at least: ten seconds.
const TICKS: u64 = 600;

/// The grid: 40 × 30 tiles of 16 pixels.
const COLUMNS: i32 = 40;
const ROWS: i32 = 30;
const TILE: f32 = 16.0;

/// How many walkers there are.
const WALKERS: u32 = 200;

/// How fast the player walks, in pixels a second, and how many ticks a
/// dash lasts, at three times that.
const SPEED: f32 = 90.0;
const DASH_TICKS: u32 = 12;

/// One tick's length, as a frame.
const TICK: Duration = Duration::from_nanos(16_666_667);

/// The live run's frames, round and round: of no tick, of one, of several.
const LIVE: [Duration; 8] = [
    TICK,
    Duration::from_nanos(8_333_333),
    Duration::from_nanos(41_666_667),
    Duration::ZERO,
    Duration::from_nanos(50_000_000),
    TICK,
    Duration::from_nanos(5_555_555),
    Duration::from_nanos(33_333_334),
];

/// Frames of seven ticks: the last of a replay runs live ticks after it.
const SEVEN: [Duration; 1] = [Duration::from_nanos(116_666_669)];

/// Another pacing for a replay, unlike the live run's.
const ODD: [Duration; 6] = [
    Duration::from_nanos(25_000_000),
    Duration::ZERO,
    Duration::from_nanos(4_166_667),
    Duration::from_nanos(66_666_667),
    Duration::from_nanos(8_333_333),
    TICK,
];

// -- the game ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Move,
    Dash,
}

impl ActionSet for Act {
    const ALL: &'static [Self] = &[Act::Move, Act::Dash];

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        match self {
            Act::Move => "move",
            Act::Dash => "dash",
        }
    }

    fn kind(self) -> ActionKind {
        match self {
            Act::Move => ActionKind::Axis2,
            Act::Dash => ActionKind::Button,
        }
    }
}

nadir::scene::reflect_newtype! {
    /// The pickups the player entered.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Score(u32);
}

nadir::scene::reflect_newtype! {
    /// The ticks left of the player's dash.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Dashing(u32);
}

nadir::scene::reflect_newtype! {
    /// A walker's number, which says when it turns.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Walker(u32);
}

nadir::scene::reflect_struct! {
    /// Something to pick up: a tag.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Pickup;
}

impl Component for Score {}
impl Component for Dashing {}
impl Component for Walker {}
impl Component for Pickup {}

/// The game's random numbers.
struct Dice(Pcg32);

/// What happened, for the checks that the run went where it should.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tally {
    dashes: u32,
    picked: u32,
    /// Pickups spawned into the slot of an entity despawned before.
    reused: u32,
    /// Entries into a walker's trigger.
    walker_entries: u32,
}

/// The replay of the run, tick after tick.
struct Recorder(InputRecorder<Act>);

/// The world's hashes at the start of each tick: after the ticks before.
struct Hashes {
    lines: Vec<Line>,
    /// A registry of each reflected type alone, for the columns.
    each: Vec<TypeRegistry>,
}

/// The world's hash, then each reflected type's.
type Line = Vec<u64>;

/// The steps of the hash: the whole, then each type, in the order the
/// world's registry has them.
fn hash_line(world: &World, each: &[TypeRegistry]) -> Line {
    let mut cx = ReflectContext::new("the determinism test's columns");
    let mut line = vec![nadir::world_hash(world)];
    line.extend(each.iter().map(|one| one.hash_world(world, &mut cx)));
    line
}

/// A registry of `T` alone.
fn alone<T: Component + FromValue>() -> TypeRegistry {
    let mut registry = TypeRegistry::new();
    registry.register_component::<T>();
    registry
}

/// The reflected types, each in a registry of its own, in the order the
/// game's registry has them.
fn each_type() -> Vec<TypeRegistry> {
    vec![
        alone::<Transform>(),
        alone::<Velocity>(),
        alone::<Player>(),
        alone::<SaveId>(),
        alone::<Facing>(),
        alone::<Score>(),
        alone::<Dashing>(),
        alone::<Walker>(),
        alone::<Pickup>(),
    ]
}

/// First in the tick: the hash of the world the last tick left.
fn hash_tick(world: &World, _: &Tick) {
    let mut hashes = world.resource_mut::<Hashes>();
    let line = hash_line(world, &hashes.each);
    hashes.lines.push(line);
}

fn record(world: &World, _: &Tick) {
    let state = world.resource::<ActionState<Act>>();
    world.resource_mut::<Recorder>().0.record(&state);
}

/// Steers the player by `Move`, and starts a dash on `Dash`.
fn steer(world: &World, _: &Tick) {
    let actions = world.resource::<ActionState<Act>>();
    let direction = actions.axis2(Act::Move);
    let players = world.read::<Player>();
    let mut velocities = world.write::<Velocity>();
    let mut dashes = world.write::<Dashing>();
    let mut facings = world.write::<Facing>();
    for (player, _) in players.iter() {
        let Some(dash) = dashes.get_mut(player) else {
            continue;
        };
        if actions.just_pressed(Act::Dash) && dash.0 == 0 {
            dash.0 = DASH_TICKS;
            world.resource_mut::<Tally>().dashes += 1;
        }
        let speed = if dash.0 > 0 {
            dash.0 -= 1;
            SPEED * 3.0
        } else {
            SPEED
        };
        if let Some(velocity) = velocities.get_mut(player) {
            velocity.0 = direction * speed;
        }
        if direction.x != 0.0
            && let Some(facing) = facings.get_mut(player)
        {
            *facing = facing_of(direction);
        }
    }
}

fn facing_of(direction: Vec2) -> Facing {
    if direction.x < 0.0 {
        Facing::Left
    } else {
        Facing::Right
    }
}

/// A velocity of up to 60 pixels a second each way.
fn heading(dice: &mut Pcg32) -> Vec2 {
    Vec2::new(dice.range(-60.0..60.0), dice.range(-60.0..60.0))
}

/// Turns each walker once every 90 ticks, each at its own time.
fn turn(world: &World, tick: &Tick) {
    let walkers = world.read::<Walker>();
    let mut velocities = world.write::<Velocity>();
    let mut facings = world.write::<Facing>();
    let mut dice = world.resource_mut::<Dice>();
    for (walker, &Walker(index)) in walkers.iter() {
        if (tick.index + u64::from(index)).is_multiple_of(90)
            && let Some(velocity) = velocities.get_mut(walker)
        {
            velocity.0 = heading(&mut dice.0);
            if let Some(facing) = facings.get_mut(walker) {
                *facing = facing_of(velocity.0);
            }
        }
    }
}

/// Every 30 ticks, drops a pickup a tile or none from the player each
/// way, but not on it, on a tile that is not solid.
fn drop_pickups(world: &World, tick: &Tick) {
    if !tick.index.is_multiple_of(30) {
        return;
    }
    let mut dice = world.resource_mut::<Dice>();
    let steps = [-1.0, 0.0, 1.0];
    let x = steps[dice.0.range(0..3u32) as usize];
    let y = if x == 0.0 {
        [-1.0, 1.0][dice.0.range(0..2u32) as usize]
    } else {
        steps[dice.0.range(0..3u32) as usize]
    };
    let players = world.read::<Player>();
    let transforms = world.read::<Transform>();
    let Some(&player) = players.entities().first() else {
        return;
    };
    let Some(feet) = transforms.get(player).map(|it| it.position) else {
        return;
    };
    let at = feet + Vec2::new(x, y) * TILE;
    let collisions = world.resource::<Collisions>();
    let grid = collisions.grid();
    let cell = IVec2::new((at.x / TILE) as i32, (at.y / TILE) as i32);
    if grid.is_solid(cell) {
        return;
    }
    let pickup = world
        .commands()
        .spawn()
        .with(Transform::at(at))
        .with(Collider::trigger(Shape::Circle { radius: 6.0 }))
        .with(Pickup)
        .id();
    if pickup.generation().get() > 1 {
        world.resource_mut::<Tally>().reused += 1;
    }
}

/// After the triggers: the player scores a pickup it enters, which goes.
fn collect(world: &World, _: &Tick) {
    let enters = world.resource::<Events<TriggerEnter>>();
    let pickups = world.read::<Pickup>();
    let walkers = world.read::<Walker>();
    let mut scores = world.write::<Score>();
    let mut tally = world.resource_mut::<Tally>();
    for enter in enters.tick_events() {
        if pickups.contains(enter.trigger)
            && let Some(score) = scores.get_mut(enter.other)
        {
            score.0 += 1;
            tally.picked += 1;
            world.commands().despawn(enter.trigger);
        } else if walkers.contains(enter.trigger) {
            tally.walker_entries += 1;
        }
    }
}

/// A collider at the feet: a box of `half` half-extents whose bottom edge
/// is the entity's position.
fn at_feet(half: Vec2, trigger: bool) -> Collider {
    let shape = Shape::Aabb { half };
    let base = if trigger {
        nadir::collision::Collider::trigger(shape)
    } else {
        nadir::collision::Collider::solid(shape)
    };
    Collider(nadir::collision::Collider {
        offset: Vec2::new(0.0, -half.y),
        ..base
    })
}

/// The middle of a tile.
fn middle(cell: IVec2) -> Vec2 {
    (Vec2::new(cell.x as f32, cell.y as f32) + Vec2::splat(0.5)) * TILE
}

/// Where the player starts: the middle of the grid.
const START: IVec2 = IVec2::new(COLUMNS / 2, ROWS / 2);

/// The game, set up: its grid, walkers and player, its systems, and the
/// recorder and the hashes. With `reversed`, the walkers' colliders go in
/// the other way round, which changes the order they move in.
fn game(reversed: bool) -> App {
    let mut map = ActionMap::<Act>::new().expect("a valid set");
    let wasd = Binding::KeyComposite2 {
        up: Key::W,
        down: Key::S,
        left: Key::A,
        right: Key::D,
    };
    map.bind(Act::Move, 0, wasd).expect("fits");
    map.bind(Act::Dash, 0, Binding::Key(Key::Space))
        .expect("fits");
    let mut app = App::new(Config::default())
        .add_actions(map)
        .register_component::<Score>()
        .register_component::<Dashing>()
        .register_component::<Walker>()
        .register_component::<Pickup>()
        .insert_resource(Tally::default())
        .insert_resource(Recorder(InputRecorder::new().expect("a valid set")))
        .insert_resource(Hashes {
            lines: Vec::new(),
            each: each_type(),
        })
        .add_fixed_system(FixedStage::Input, "hash_tick", hash_tick)
        .add_fixed_system(FixedStage::Input, "record", record)
        .add_fixed_system(FixedStage::Gameplay, "steer", steer)
        .add_fixed_system(FixedStage::Gameplay, "turn", turn)
        .add_fixed_system(FixedStage::Gameplay, "drop_pickups", drop_pickups)
        .add_fixed_system(FixedStage::PostMovement, "collect", collect);

    let mut dice = Pcg32::new(SEED, 1);
    let mut grid = TileGrid::new(COLUMNS as u32, ROWS as u32, TILE, Vec2::ZERO);
    let mut open = Vec::new();
    for y in 0..ROWS {
        for x in 0..COLUMNS {
            let cell = IVec2::new(x, y);
            let edge = x == 0 || y == 0 || x == COLUMNS - 1 || y == ROWS - 1;
            let near_start = (x - START.x).abs() <= 2 && (y - START.y).abs() <= 2;
            if edge || (!near_start && dice.range(0..8u32) == 0) {
                grid.set_solid(cell, true);
            } else if !near_start {
                open.push(cell);
            }
        }
    }
    dice.shuffle(&mut open);
    let walkers: Vec<_> = (0..WALKERS)
        .zip(&open)
        .map(|(index, &cell)| {
            let velocity = heading(&mut dice);
            (index, middle(cell) + Vec2::new(0.0, 3.0), velocity)
        })
        .collect();

    let world = app.world_mut();
    world.resource_mut::<Collisions>().set_grid(grid);
    world
        .spawn()
        .with(Player)
        .with(Transform::at(middle(START) + Vec2::new(0.0, 3.0)))
        .with(Velocity::default())
        .with(at_feet(Vec2::new(5.0, 3.0), false))
        .with(Facing::Right)
        .with(Score(0))
        .with(Dashing(0));
    let spawned: Vec<_> = walkers
        .iter()
        .map(|&(index, feet, velocity)| {
            world
                .spawn()
                .with(Walker(index))
                .with(Transform::at(feet))
                .with(Velocity(velocity))
                .with(facing_of(velocity))
                .id()
        })
        .collect();
    // The colliders last, in the order asked: the order the walkers move
    // in. Every collider is on every layer, so walkers stop one another.
    let mut order: Vec<_> = spawned.iter().zip(&walkers).collect();
    if reversed {
        order.reverse();
    }
    for (&entity, &(index, _, _)) in order {
        world
            .write::<Collider>()
            .insert(entity, at_feet(Vec2::new(5.0, 3.0), index % 10 == 0));
    }
    let registered: Vec<&str> = app
        .world()
        .resource::<TypeRegistry>()
        .iter()
        .map(|registration| registration.name())
        .collect();
    assert_eq!(
        registered,
        type_names(),
        "each_type lists the registry's types"
    );
    app.insert_resource(Dice(dice))
}

// -- runs ----------------------------------------------------------------------

/// The scripted input of the live run: at frame `.0`, key `.1` goes down
/// (`true`) or up. Several keys at one frame go in that order.
const SCRIPT: &[(u32, Key, bool)] = &[
    (0, Key::D, true),
    (40, Key::S, true),
    (70, Key::D, false),
    // A dash, tapped within a frame.
    (100, Key::Space, true),
    (100, Key::Space, false),
    (120, Key::A, true),
    (150, Key::S, false),
    (160, Key::Space, true),
    (163, Key::Space, false),
    (200, Key::W, true),
    (230, Key::A, false),
    (260, Key::D, true),
    (300, Key::W, false),
    // Opposite keys cancel.
    (330, Key::A, true),
    (350, Key::D, false),
    // A tap of S within a frame.
    (380, Key::S, true),
    (380, Key::S, false),
    (400, Key::A, false),
    (400, Key::Space, true),
    (401, Key::Space, false),
    (420, Key::W, true),
    (420, Key::S, true),
    (440, Key::W, false),
    (440, Key::S, false),
    (450, Key::D, true),
];

/// What a run gave: its replay, the world's hash after each of its ticks
/// and before the first, and its tally.
struct Run {
    replay: Vec<u8>,
    lines: Vec<Line>,
    tally: Tally,
    /// Whether a frame ran live ticks after the replay's last.
    straddled: bool,
}

/// Ends a run of `ticks` ticks: the hash of the world as its last tick left
/// it, unless a later tick, live, took it already.
fn finish(app: &App, ticks: usize) -> Run {
    let hashes = app.world().resource::<Hashes>();
    let mut lines = hashes.lines.clone();
    lines.truncate(ticks);
    lines.push(hash_line(app.world(), &hashes.each));
    if hashes.lines.len() > ticks {
        lines[ticks] = hashes.lines[ticks].clone();
    }
    Run {
        replay: app.world().resource::<Recorder>().0.bytes().to_vec(),
        lines,
        tally: *app.world().resource::<Tally>(),
        straddled: hashes.lines.len() > ticks,
    }
}

/// The game, live, with the scripted input, in frames of [`LIVE`]'s
/// lengths, until [`TICKS`] ticks have run.
fn live(reversed: bool) -> Run {
    let mut app = game(reversed);
    let mut ticks = 0;
    let mut events = Vec::new();
    for frame in 0.. {
        if ticks >= TICKS {
            break;
        }
        events.clear();
        events.extend(SCRIPT.iter().filter(|&&(at, _, _)| at == frame).map(
            |&(_, key, pressed)| Event::Key {
                key,
                pressed,
                repeat: false,
            },
        ));
        app.update(LIVE[frame as usize % LIVE.len()], &events);
        ticks += u64::from(app.world().resource::<TickStats>().ticks);
    }
    let ticks = app.world().resource::<Hashes>().lines.len();
    finish(&app, ticks)
}

/// The game, playing `replay` in frames of `pacing`'s lengths until it
/// ends, the replay's ticks and no more.
fn replayed(replay: &[u8], pacing: &[Duration]) -> Run {
    let bytes: &'static [u8] = Box::leak(replay.to_vec().into_boxed_slice());
    let player = InputPlayer::<Act>::new(bytes).expect("a replay of the test's actions");
    let mut app = game(false).play_replay(player);
    for frame in 0..100_000 {
        if !matches!(*app.world().resource::<Replay>(), Replay::Playing { .. }) {
            break;
        }
        app.update(pacing[frame % pacing.len()], &[]);
    }
    let Replay::Ended { ticks } = *app.world().resource::<Replay>() else {
        panic!(
            "the replay did not end: {:?}",
            *app.world().resource::<Replay>()
        );
    };
    let ticks = usize::try_from(ticks).expect("a few hundred");
    let run = finish(&app, ticks);
    // What it played is what was recorded, tick for tick. The last frame
    // may have run live ticks after it, which were recorded too.
    assert!(run.replay.len() >= replay.len());
    assert_eq!(
        &run.replay[..replay.len()],
        replay,
        "the replay played as recorded"
    );
    run
}

// -- the fixtures --------------------------------------------------------------

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/determinism")
}

/// Whether to write the fixtures: `NADIR_BLESS=1`.
fn blessing() -> bool {
    match std::env::var("NADIR_BLESS") {
        Ok(value) if value == "1" => true,
        Ok(value) if value.is_empty() || value == "0" => false,
        Ok(value) => panic!("NADIR_BLESS is `{value}`: set it to 1 to bless, or leave it unset"),
        Err(_) => false,
    }
}

/// The names of the hashes' columns after the whole.
fn type_names() -> Vec<&'static str> {
    each_type()
        .iter()
        .map(|registry| registry.iter().next().expect("one type").name())
        .collect()
}

/// `hashes.txt`'s text for `lines`.
fn hashes_text(lines: &[Line]) -> String {
    let mut text = String::from(
        "# The world's hash at each tick of replay.nrpl, whole and by type:\n\
         # crates/engine/tests/determinism.rs writes it, with NADIR_BLESS=1.\n",
    );
    text.push_str(&format!("seed {SEED}\n"));
    text.push_str(&format!("types {}\n", type_names().join(" ")));
    for (tick, line) in lines.iter().enumerate() {
        let hashes: Vec<String> = line.iter().map(|hash| format!("{hash:016x}")).collect();
        text.push_str(&format!("{tick} {}\n", hashes.join(" ")));
    }
    text
}

/// The lines of `hashes.txt`, checked strictly.
fn parse_hashes(text: &str) -> Vec<Line> {
    let mut seed = None;
    let mut types = None;
    let mut lines = Vec::new();
    for (number, row) in text.lines().enumerate() {
        let at = || format!("hashes.txt line {}", number + 1);
        if row.starts_with('#') {
            continue;
        }
        if let Some(value) = row.strip_prefix("seed ") {
            seed = Some(
                value
                    .parse::<u64>()
                    .unwrap_or_else(|_| panic!("{}: a seed", at())),
            );
        } else if let Some(names) = row.strip_prefix("types ") {
            types = Some(names.split(' ').map(str::to_string).collect::<Vec<_>>());
        } else {
            let mut fields = row.split(' ');
            let tick = fields.next().and_then(|tick| tick.parse::<usize>().ok());
            assert_eq!(tick, Some(lines.len()), "{}: the ticks in order", at());
            let line: Line = fields
                .map(|hash| {
                    u64::from_str_radix(hash, 16).unwrap_or_else(|_| panic!("{}: a hash", at()))
                })
                .collect();
            assert_eq!(line.len(), 1 + each_type().len(), "{}: every column", at());
            lines.push(line);
        }
    }
    assert_eq!(
        seed,
        Some(SEED),
        "hashes.txt was blessed with another seed: bless it again"
    );
    let names: Vec<String> = type_names().iter().map(|name| name.to_string()).collect();
    assert_eq!(
        types,
        Some(names),
        "hashes.txt has other types: bless it again"
    );
    lines
}

/// The stored replay and hashes.
fn stored() -> (Vec<u8>, Vec<Line>) {
    let read = |name: &str| {
        let path = fixtures().join(name);
        fs::read(&path).unwrap_or_else(|error| {
            panic!(
                "{}: {error}. Write it with NADIR_BLESS=1 cargo test -p nadir --test determinism",
                path.display()
            )
        })
    };
    let replay = read("replay.nrpl");
    let text = String::from_utf8(read("hashes.txt")).expect("hashes.txt is text");
    (replay, parse_hashes(&text))
}

/// Fails, naming the first tick at which `found` differs from `expected`
/// and the types that differ there, after writing `found` beside the
/// build's other test output, for a fixture of another system.
#[track_caller]
fn assert_same_hashes(found: &[Line], expected: &[Line], what: &str) {
    let first = found
        .iter()
        .zip(expected)
        .position(|(found, expected)| found != expected);
    if first.is_none() && found.len() == expected.len() {
        return;
    }
    let name = what.replace(|c: char| !c.is_ascii_alphanumeric(), "-");
    let actual = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("determinism-{name}.txt"));
    let text = hashes_text(found);
    let written = fs::write(&actual, &text).is_ok();
    error!("{what}: the hashes found:\n{text}");
    let Some(tick) = first else {
        panic!(
            "{what}: {} ticks, not {}",
            found.len().saturating_sub(1),
            expected.len().saturating_sub(1)
        );
    };
    let types: Vec<&str> = type_names()
        .into_iter()
        .zip(found[tick][1..].iter().zip(&expected[tick][1..]))
        .filter(|(_, (found, expected))| found != expected)
        .map(|(name, _)| name)
        .collect();
    panic!(
        "{what}: the world differs after tick {tick}, in {types:?}; the hashes found are in {}{}",
        actual.display(),
        if written { "" } else { " (not written)" }
    );
}

/// The checks that a run went where the test needs it to: the paths that
/// nondeterminism would show on.
#[track_caller]
fn assert_covered(run: &Run) {
    let tally = run.tally;
    info!("the run covered: {tally:?}");
    assert!(tally.dashes >= 2, "{tally:?}");
    assert!(tally.picked >= 2, "{tally:?}");
    assert!(tally.reused >= 1, "{tally:?}");
    assert!(tally.walker_entries >= 10, "{tally:?}");
    let changes = run
        .lines
        .windows(2)
        .filter(|pair| pair[0][0] != pair[1][0])
        .count();
    assert_eq!(changes, run.lines.len() - 1, "the world changes every tick");
}

// -- the tests -----------------------------------------------------------------

#[test]
fn a_live_run_records_the_stored_replay_and_hashes() {
    let run = live(false);
    assert_covered(&run);
    // The same run again, in the same process: the same.
    let again = live(false);
    assert_same_hashes(&again.lines, &run.lines, "a second live run");
    assert_eq!(again.replay, run.replay, "a second live run's replay");
    if blessing() {
        // Its replay, at three other pacings, before it is stored.
        assert_replays(&run.replay, &run.lines, "the live run's replay");
        fs::create_dir_all(fixtures()).expect("make the fixtures' folder");
        for (name, bytes) in [
            ("replay.nrpl", run.replay.clone()),
            ("hashes.txt", hashes_text(&run.lines).into_bytes()),
        ] {
            let path = fixtures().join(name);
            fs::write(&path, bytes).expect("write a fixture");
            info!("blessed {}", path.display());
        }
        return;
    }
    let (replay, lines) = stored();
    assert_eq!(
        run.replay, replay,
        "the live run's replay is the stored one"
    );
    assert_same_hashes(&run.lines, &lines, "the live run");
}

#[test]
fn a_replay_reaches_the_stored_hash_after_every_tick_whatever_the_frames() {
    if blessing() {
        return;
    }
    let (replay, lines) = stored();
    assert!(lines.len() > TICKS as usize, "{} ticks", lines.len() - 1);
    assert_replays(&replay, &lines, "the replay");
}

/// Plays `replay` in frames of one tick, of seven, and of mixed lengths,
/// and checks each against `lines`.
#[track_caller]
fn assert_replays(replay: &[u8], lines: &[Line], what: &str) {
    let mut straddled = false;
    for (pacing, name) in [
        (&[TICK][..], "one tick"),
        (&SEVEN, "seven"),
        (&ODD, "mixed"),
    ] {
        let played = replayed(replay, pacing);
        assert_same_hashes(&played.lines, lines, &format!("{what} in frames of {name}"));
        straddled |= played.straddled;
    }
    assert!(straddled, "a frame ran live ticks after the replay's end");
}

#[test]
fn the_hashes_follow_the_order_the_walkers_move_in() {
    // The same game, but the walkers' colliders inserted the other way
    // round: the same world at first, and another once they meet.
    // By `Transform`'s column, which leaves out the colliders' own order.
    let forward = live(false);
    let reversed = live(true);
    let transforms = |run: &Run| -> Vec<u64> { run.lines.iter().map(|line| line[1]).collect() };
    let (forward, reversed) = (transforms(&forward), transforms(&reversed));
    assert_eq!(forward[0], reversed[0]);
    assert_ne!(forward, reversed);
}
