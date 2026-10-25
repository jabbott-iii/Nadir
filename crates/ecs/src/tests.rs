//! Tests of the world, commands, events and schedule together.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};

use nadir_core::Pcg32;
use nadir_core::testing::check_sized;
use nadir_core::time::{Frame, Tick};

use crate::{Component, Entity, Events, FixedStage, FrameStage, Schedule, Stage, World};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct A(u32);
impl Component for A {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct B(u32);
impl Component for B {}

fn world() -> World {
    let mut world = World::new();
    world.register::<A>();
    world.register::<B>();
    world
}

fn tick(index: u64) -> Tick {
    Tick {
        index,
        dt: 1.0 / 60.0,
    }
}

const FRAME: Frame = Frame {
    dt: 1.0 / 60.0,
    alpha: 0.0,
};

/// The message of the panic `f` makes.
fn panic_message(f: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(f)).expect_err("a panic");
    payload
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default()
}

// ---- The model-based property test -------------------------------------

/// Which component a step is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Which {
    A,
    B,
}

/// One step of a random history. `who` picks one of the entities handed
/// out so far, dead ones included.
#[derive(Clone, Copy, Debug)]
enum Step {
    Spawn,
    SpawnByCommand {
        a: Option<u32>,
        b: Option<u32>,
    },
    Insert {
        who: u32,
        which: Which,
        value: u32,
        command: bool,
    },
    Remove {
        who: u32,
        which: Which,
        command: bool,
    },
    Despawn {
        who: u32,
        command: bool,
    },
    Apply,
}

fn step(rng: &mut Pcg32) -> Step {
    let who = rng.range(0..1000u32);
    let which = if rng.range(0..2u32) == 0 {
        Which::A
    } else {
        Which::B
    };
    let command = rng.range(0..2u32) == 0;
    let value = rng.range(0..100u32);
    let maybe = |rng: &mut Pcg32| (rng.range(0..2u32) == 0).then(|| rng.range(0..100u32));
    match rng.range(0..12u32) {
        0 | 1 => Step::Spawn,
        2 | 3 => Step::SpawnByCommand {
            a: maybe(rng),
            b: maybe(rng),
        },
        4..=6 => Step::Insert {
            who,
            which,
            value,
            command,
        },
        7 => Step::Remove {
            who,
            which,
            command,
        },
        8 | 9 => Step::Despawn { who, command },
        _ => Step::Apply,
    }
}

/// What a command does when it is applied, in the model.
#[derive(Clone, Copy, Debug)]
enum Queued {
    Insert(usize, Which, u32),
    Remove(usize, Which),
    Despawn(usize),
}

/// The reference: which entities are alive, their components, and the
/// commands not yet applied.
#[derive(Default)]
struct Model {
    entities: Vec<Entity>,
    alive: BTreeSet<usize>,
    components: BTreeMap<(usize, Which), u32>,
    queued: Vec<Queued>,
}

impl Model {
    fn despawn(&mut self, id: usize) {
        if self.alive.remove(&id) {
            self.components.retain(|&(owner, _), _| owner != id);
        }
    }

    fn apply(&mut self) {
        for queued in std::mem::take(&mut self.queued) {
            match queued {
                Queued::Insert(id, which, value) => {
                    if self.alive.contains(&id) {
                        self.components.insert((id, which), value);
                    }
                }
                Queued::Remove(id, which) => {
                    self.components.remove(&(id, which));
                }
                Queued::Despawn(id) => self.despawn(id),
            }
        }
    }
}

/// Runs `steps` on a world and the model, and compares them after each.
fn run_history(steps: &[Step]) -> Result<(), String> {
    let mut world = world();
    let mut model = Model::default();
    for (number, &step) in steps.iter().enumerate() {
        let pick = |who: u32, model: &Model| {
            (!model.entities.is_empty()).then(|| who as usize % model.entities.len())
        };
        match step {
            Step::Spawn => {
                model.entities.push(world.spawn().id());
                model.alive.insert(model.entities.len() - 1);
            }
            Step::SpawnByCommand { a, b } => {
                let mut commands = world.commands();
                let mut spawned = commands.spawn();
                let id = model.entities.len();
                if let Some(a) = a {
                    spawned = spawned.with(A(a));
                    model.queued.push(Queued::Insert(id, Which::A, a));
                }
                if let Some(b) = b {
                    spawned = spawned.with(B(b));
                    model.queued.push(Queued::Insert(id, Which::B, b));
                }
                model.entities.push(spawned.id());
                model.alive.insert(id);
            }
            Step::Insert {
                who,
                which,
                value,
                command,
            } => {
                let Some(id) = pick(who, &model) else {
                    continue;
                };
                let entity = model.entities[id];
                if command {
                    let mut commands = world.commands();
                    match which {
                        Which::A => commands.insert(entity, A(value)),
                        Which::B => commands.insert(entity, B(value)),
                    }
                    model.queued.push(Queued::Insert(id, which, value));
                } else if model.alive.contains(&id) {
                    // Directly only on the living, as setup code does: the
                    // sets do not know who is alive.
                    match which {
                        Which::A => drop(world.write::<A>().insert(entity, A(value))),
                        Which::B => drop(world.write::<B>().insert(entity, B(value))),
                    }
                    model.components.insert((id, which), value);
                }
            }
            Step::Remove {
                who,
                which,
                command,
            } => {
                let Some(id) = pick(who, &model) else {
                    continue;
                };
                let entity = model.entities[id];
                if command {
                    let mut commands = world.commands();
                    match which {
                        Which::A => commands.remove::<A>(entity),
                        Which::B => commands.remove::<B>(entity),
                    }
                    model.queued.push(Queued::Remove(id, which));
                } else {
                    match which {
                        Which::A => drop(world.write::<A>().remove(entity)),
                        Which::B => drop(world.write::<B>().remove(entity)),
                    }
                    model.components.remove(&(id, which));
                }
            }
            Step::Despawn { who, command } => {
                let Some(id) = pick(who, &model) else {
                    continue;
                };
                if command {
                    world.commands().despawn(model.entities[id]);
                    model.queued.push(Queued::Despawn(id));
                } else {
                    let was_alive = world.despawn(model.entities[id]);
                    if was_alive != model.alive.contains(&id) {
                        return Err(format!("step {number}: despawn said {was_alive}"));
                    }
                    model.despawn(id);
                }
            }
            Step::Apply => {
                world.apply_commands();
                model.apply();
            }
        }
        compare(&world, &model)
            .map_err(|error| format!("after step {number}, {step:?}: {error}"))?;
    }
    Ok(())
}

fn compare(world: &World, model: &Model) -> Result<(), String> {
    if world.entity_count() != model.alive.len() {
        return Err(format!(
            "{} entities, not {}",
            world.entity_count(),
            model.alive.len()
        ));
    }
    let (a, b) = (world.read::<A>(), world.read::<B>());
    a.check_invariants()?;
    b.check_invariants()?;
    for (id, &entity) in model.entities.iter().enumerate() {
        let alive = model.alive.contains(&id);
        if world.is_alive(entity) != alive {
            return Err(format!("{entity:?} alive: {}", !alive));
        }
        let expected_a = model.components.get(&(id, Which::A)).copied();
        let expected_b = model.components.get(&(id, Which::B)).copied();
        if a.get(entity).map(|a| a.0) != expected_a || b.get(entity).map(|b| b.0) != expected_b {
            return Err(format!(
                "{entity:?} has {:?} and {:?}, not {expected_a:?} and {expected_b:?}",
                a.get(entity),
                b.get(entity)
            ));
        }
    }
    let in_world = |set: Vec<(Entity, u32)>| set.into_iter().collect::<BTreeSet<_>>();
    let in_model = |which| {
        model
            .components
            .iter()
            .filter(|((_, w), _)| *w == which)
            .map(|(&(id, _), &value)| (model.entities[id], value))
            .collect::<BTreeSet<_>>()
    };
    if in_world(a.iter().map(|(e, a)| (e, a.0)).collect()) != in_model(Which::A)
        || in_world(b.iter().map(|(e, b)| (e, b.0)).collect()) != in_model(Which::B)
    {
        return Err("iteration differs from the model".into());
    }
    Ok(())
}

impl PartialOrd for Entity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Entity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.index(), self.generation()).cmp(&(other.index(), other.generation()))
    }
}

#[test]
fn random_histories_match_the_model() {
    check_sized(
        "the world matches a BTreeMap model",
        300,
        |rng, size| (0..size * 3).map(|_| step(rng)).collect::<Vec<_>>(),
        |steps| run_history(steps),
    );
}

// ---- Generations ---------------------------------------------------------

#[test]
fn a_stale_entity_reads_nothing_and_its_commands_do_nothing() {
    let mut world = world();
    let old = world.spawn().with(A(1)).id();
    world.despawn(old);
    let new = world.spawn().with(A(2)).id();
    assert_eq!(new.index(), old.index(), "the slot is reused");
    assert!(!world.is_alive(old) && world.is_alive(new));
    assert_eq!(world.read::<A>().get(old), None);

    let mut commands = world.commands();
    commands.insert(old, A(99));
    commands.insert(old, B(99));
    commands.remove::<A>(old);
    commands.despawn(old);
    world.apply_commands();
    assert!(world.is_alive(new));
    assert_eq!(world.read::<A>().get(new), Some(&A(2)));
    assert!(world.read::<B>().is_empty());
}

// ---- Borrows -------------------------------------------------------------

#[derive(Debug)]
struct Health;
impl Component for Health {}

fn write_twice(world: &World, _: &Tick) {
    let _first = world.write::<Health>();
    let _second = world.write::<Health>();
}

fn read_while_writing(world: &World, _: &Tick) {
    let _writing = world.write::<Health>();
    let _reading = world.read::<Health>();
}

fn resource_twice(world: &World, _: &Tick) {
    let _first = world.resource_mut::<u32>();
    let _second = world.resource::<u32>();
}

fn unregistered(world: &World, _: &Tick) {
    let _ = world.read::<A>();
}

/// The panic of a schedule whose one system is `system`, named `name`.
fn system_panic(name: &'static str, system: crate::FixedSystem) -> String {
    let mut world = World::new();
    world.register::<Health>();
    world.insert_resource(0u32);
    let mut schedule = Schedule::new();
    schedule.add_fixed(FixedStage::Gameplay, name, system);
    panic_message(|| schedule.run_fixed(&mut world, &tick(0)))
}

#[test]
fn a_conflicting_borrow_panics_naming_the_system_and_the_type() {
    let message = system_panic("damage", write_twice);
    assert!(
        message.starts_with("the component `nadir_ecs::tests::Health` is borrowed elsewhere")
            && message.contains("(system `damage`)"),
        "{message}"
    );
    let message = system_panic("heal", read_while_writing);
    assert!(
        message.starts_with("the component `nadir_ecs::tests::Health` is being changed elsewhere")
            && message.contains("(system `heal`)"),
        "{message}"
    );
    let message = system_panic("score", resource_twice);
    assert!(
        message.starts_with("the resource `u32` is being changed elsewhere")
            && message.contains("(system `score`)"),
        "{message}"
    );
    let message = system_panic("lost", unregistered);
    assert!(
        message.starts_with("the component `nadir_ecs::tests::A` is not registered")
            && message.contains("(system `lost`)"),
        "{message}"
    );
    // Outside any system, the name is gone again.
    let world = World::new();
    let message = panic_message(|| drop(world.resource::<u64>()));
    assert!(message.contains("(system `(none)`)"), "{message}");
}

// ---- Events --------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Hit(u64);

/// What the readers saw: each tick's events, and each frame's.
#[derive(Default)]
struct Seen {
    ticks: Vec<Vec<u64>>,
    frames: Vec<Vec<u64>>,
}

fn send_hit(world: &World, tick: &Tick) {
    world.resource_mut::<Events<Hit>>().send(Hit(tick.index));
}

fn read_tick_hits(world: &World, _: &Tick) {
    let hits: Vec<u64> = world
        .resource::<Events<Hit>>()
        .tick_events()
        .iter()
        .map(|hit| hit.0)
        .collect();
    world.resource_mut::<Seen>().ticks.push(hits);
}

fn send_frame_hit(world: &World, _: &Frame) {
    world.resource_mut::<Events<Hit>>().send(Hit(100));
}

fn read_frame_hits(world: &World, _: &Frame) {
    let hits: Vec<u64> = world
        .resource::<Events<Hit>>()
        .frame_events()
        .iter()
        .map(|hit| hit.0)
        .collect();
    world.resource_mut::<Seen>().frames.push(hits);
}

#[test]
fn tick_readers_see_their_tick_and_frame_readers_every_tick_since_the_last_frame() {
    let mut world = World::new();
    world.add_event::<Hit>();
    world.add_event::<Hit>();
    world.insert_resource(Seen::default());
    let mut schedule = Schedule::new();
    // Readers are added before writers, in later stages.
    schedule
        .add_fixed(FixedStage::Cleanup, "read_tick_hits", read_tick_hits)
        .add_fixed(FixedStage::Gameplay, "send_hit", send_hit)
        .add_frame(FrameStage::Ui, "read_frame_hits", read_frame_hits);

    // A frame of three ticks, then one of none, then one of one.
    let mut index = 0;
    for ticks in [3, 0, 1] {
        for _ in 0..ticks {
            schedule.run_fixed(&mut world, &tick(index));
            index += 1;
        }
        schedule.run_frame(&mut world, &FRAME);
    }
    let seen = world.resource::<Seen>();
    assert_eq!(seen.ticks, [vec![0], vec![1], vec![2], vec![3]]);
    assert_eq!(seen.frames, [vec![0, 1, 2], vec![], vec![3]]);
}

#[test]
fn an_events_resource_inserted_by_hand_is_cleared_once_its_type_is_added() {
    let mut world = World::new();
    world.insert_resource(Events::<Hit>::new());
    world.add_event::<Hit>();
    world.insert_resource(Seen::default());
    let mut schedule = Schedule::new();
    schedule
        .add_fixed(FixedStage::Gameplay, "send_hit", send_hit)
        .add_fixed(FixedStage::Cleanup, "read_tick_hits", read_tick_hits)
        .add_frame(FrameStage::Ui, "read_frame_hits", read_frame_hits);
    for index in 0..2 {
        schedule.run_fixed(&mut world, &tick(index));
        schedule.run_frame(&mut world, &FRAME);
    }
    let seen = world.resource::<Seen>();
    assert_eq!(seen.ticks, [vec![0], vec![1]]);
    assert_eq!(seen.frames, [vec![0], vec![1]]);
}

#[test]
fn a_frame_writer_reaches_the_frames_readers_and_no_tick() {
    let mut world = World::new();
    world.add_event::<Hit>();
    world.insert_resource(Seen::default());
    let mut schedule = Schedule::new();
    schedule
        .add_frame(FrameStage::Animate, "send_frame_hit", send_frame_hit)
        .add_frame(FrameStage::Ui, "read_frame_hits", read_frame_hits)
        .add_fixed(FixedStage::Cleanup, "read_tick_hits", read_tick_hits);
    schedule.run_frame(&mut world, &FRAME);
    schedule.run_fixed(&mut world, &tick(0));
    schedule.run_frame(&mut world, &FRAME);
    let seen = world.resource::<Seen>();
    assert_eq!(seen.frames, [vec![100], vec![100]]);
    assert_eq!(seen.ticks, [Vec::<u64>::new()]);
}

// ---- The schedule --------------------------------------------------------

/// The systems that ran, in order.
#[derive(Default)]
struct Ran(Vec<&'static str>);

fn ran(world: &World, name: &'static str) {
    world.resource_mut::<Ran>().0.push(name);
}

#[test]
fn systems_run_by_stage_then_as_added_and_only_when_their_condition_holds() {
    let mut world = World::new();
    world.insert_resource(Ran::default());
    let mut schedule = Schedule::new();
    schedule
        .add_fixed(FixedStage::Cleanup, "cleanup", |w, _| ran(w, "cleanup"))
        .add_fixed(FixedStage::Input, "input_1", |w, _| ran(w, "input_1"))
        .add_frame(FrameStage::Extract, "extract", |w, _| ran(w, "extract"))
        .add_fixed_if(
            FixedStage::Gameplay,
            "never",
            |w, _| ran(w, "never"),
            |_| false,
        )
        .add_fixed(FixedStage::Gameplay, "gameplay", |w, _| ran(w, "gameplay"))
        .add_frame_if(
            FrameStage::Animate,
            "animate",
            |w, _| ran(w, "animate"),
            |_| true,
        )
        .add_fixed(FixedStage::Input, "input_2", |w, _| ran(w, "input_2"));
    schedule.run_fixed(&mut world, &tick(0));
    schedule.run_frame(&mut world, &FRAME);
    assert_eq!(
        world.resource::<Ran>().0,
        [
            "input_1", "input_2", "gameplay", "cleanup", "animate", "extract"
        ]
    );

    let described: Vec<String> = schedule
        .describe()
        .map(|system| system.to_string())
        .collect();
    assert_eq!(
        described,
        [
            "tick  Input: input_1",
            "tick  Input: input_2",
            "tick  Gameplay: never (conditional)",
            "tick  Gameplay: gameplay",
            "tick  Cleanup: cleanup",
            "frame Animate: animate (conditional)",
            "frame Extract: extract",
        ]
    );
    let first = schedule.describe().next().expect("a system");
    assert_eq!(first.stage, Stage::Fixed(FixedStage::Input));
}

// ---- Commands ------------------------------------------------------------

#[test]
fn commands_apply_in_the_order_they_were_queued() {
    let mut world = world();
    let (one, two, three) = (world.spawn().id(), world.spawn().id(), world.spawn().id());
    let mut commands = world.commands();
    // Insert, remove, insert: the last insert stays.
    commands.insert(one, A(1));
    commands.remove::<A>(one);
    commands.insert(one, A(2));
    // Insert, then remove: nothing stays.
    commands.insert(two, A(3));
    commands.remove::<A>(two);
    // Remove, then insert: it stays.
    commands.remove::<B>(three);
    commands.insert(three, B(4));
    // An insert after its entity's despawn is dropped.
    commands.despawn(three);
    commands.insert(three, A(5));
    // A spawned entity is alive at once, its components come later.
    let spawned = commands.spawn().with(A(6)).with(B(7)).without::<B>().id();
    assert!(world.is_alive(spawned));
    assert_eq!(world.read::<A>().get(spawned), None);
    assert_eq!(world.queued(), 12);

    world.apply_commands();
    assert_eq!(world.queued(), 0);
    let a = world.read::<A>();
    assert_eq!(
        (a.get(one), a.get(two), a.get(spawned)),
        (Some(&A(2)), None, Some(&A(6)))
    );
    assert!(!world.is_alive(three));
    assert_eq!(world.read::<B>().get(spawned), None);
    assert_eq!(world.entity_count(), 3);
}

#[test]
fn a_steady_stream_of_commands_keeps_its_buffers() {
    // The ecs crate cannot count allocations (it may not depend on
    // nadir-platform), so this checks that no buffer grows: what a steady
    // stream allocates, if anything, would come from that.
    let mut world = world();
    world.add_event::<Hit>();
    let mut schedule = Schedule::new();
    schedule.add_fixed(FixedStage::Gameplay, "spawn_and_despawn", |world, tick| {
        let mut commands = world.commands();
        for value in 0..50 {
            commands.spawn().with(A(value)).with(B(value));
        }
        let a = world.read::<A>();
        for &entity in a.entities() {
            commands.despawn(entity);
        }
        world.resource_mut::<Events<Hit>>().send(Hit(tick.index));
    });
    let round = |world: &mut World, index| {
        schedule.run_fixed(world, &tick(index));
        schedule.run_frame(world, &FRAME);
    };
    for index in 0..3 {
        round(&mut world, index);
    }
    // Read between rounds, with the queues just emptied: a buffer that was
    // freed rather than cleared would show 0 here.
    let warm = world.capacities();
    assert!(warm.iter().all(|&capacity| capacity > 0), "{warm:?}");
    for index in 3..103 {
        round(&mut world, index);
    }
    assert_eq!(world.capacities(), warm);
    assert_eq!(
        world.entity_count(),
        50,
        "one round's people, alive till the next"
    );
}

#[test]
#[should_panic(expected = "the component `nadir_ecs::tests::Health` is not registered")]
fn a_command_for_an_unregistered_type_panics_at_once() {
    let world = World::new();
    let mut commands = world.commands();
    let entity = commands.spawn().id();
    commands.insert(entity, Health);
}

#[test]
fn despawning_twice_does_nothing_the_second_time() {
    let mut world = world();
    let entity = world.spawn().with(A(1)).with(B(2)).id();
    assert!(world.despawn(entity));
    assert!(!world.despawn(entity));
    assert!(world.read::<A>().is_empty() && world.read::<B>().is_empty());
}
