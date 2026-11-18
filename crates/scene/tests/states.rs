// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The game states' tests of `docs/subsystems/scene.md`: enter and exit
//! systems run in order, and no transition happens mid-tick.

use std::panic::{AssertUnwindSafe, catch_unwind};

use nadir_core::time::Tick;
use nadir_ecs::{Component, FixedStage, Schedule, World};
use nadir_scene::{
    GameState, LevelId, StateKind, StateMachine, StateSystems, Transition, TransitionSystem,
};

/// What ran, in order, as `name: the state the machine was in`.
#[derive(Default)]
struct Log(Vec<String>);

fn note(world: &World, name: &str) {
    let state = world.resource::<StateMachine>().state().to_string();
    world
        .resource_mut::<Log>()
        .0
        .push(format!("{name}: {state}"));
}

/// A marker the systems spawn, to see when commands apply.
struct Mark;
impl Component for Mark {}

fn world(first: GameState) -> World {
    let mut world = World::new();
    world.insert_resource(StateMachine::new(first));
    world.insert_resource(Log::default());
    world.register::<Mark>();
    world
}

fn tick(index: u64) -> Tick {
    Tick {
        index,
        dt: 1.0 / 60.0,
    }
}

fn log(world: &World) -> Vec<String> {
    world.resource::<Log>().0.clone()
}

fn set(world: &World, next: GameState) {
    world.resource_mut::<StateMachine>().set(next);
}

/// A transition system that notes its name and the marks it sees.
macro_rules! hook {
    ($name:literal) => {
        (|world: &World, _: &Transition| {
            let marks = world.read::<Mark>().len();
            note(world, &format!(concat!($name, " ({} marks)"), marks));
        }) as TransitionSystem
    };
}

#[test]
fn exits_run_before_enters_each_in_the_order_added_and_their_commands_between() {
    let mut world = world(GameState::MainMenu);
    let mut systems = StateSystems::new();
    systems
        .on_enter(StateKind::InGame, "spawn_player", hook!("spawn_player"))
        .on_exit(StateKind::MainMenu, "hide_menu", hook!("hide_menu"))
        .on_exit(StateKind::MainMenu, "drop_menu", |world, _| {
            note(world, "drop_menu");
            world.commands().spawn().with(Mark);
        })
        .on_enter(StateKind::InGame, "show_hud", |world, _| {
            note(world, "show_hud");
            world.commands().spawn().with(Mark);
        })
        .on_enter(StateKind::MainMenu, "show_menu", hook!("show_menu"))
        .on_exit(StateKind::InGame, "never", hook!("never"));

    // The start: the first state's enter systems, from no state.
    let start = systems.apply(&mut world).expect("the start");
    assert_eq!(start.from, None);
    assert_eq!(start.to, GameState::MainMenu);
    assert_eq!(log(&world), ["show_menu (0 marks): MainMenu"]);
    // Nothing queued, nothing more.
    assert_eq!(systems.apply(&mut world), None);

    world.resource_mut::<Log>().0.clear();
    set(&world, GameState::InGame);
    let made = systems.apply(&mut world).expect("a transition");
    assert_eq!(
        made,
        Transition {
            from: Some(GameState::MainMenu),
            to: GameState::InGame,
        }
    );
    // The exits in the state they leave, in the order added; their mark
    // applied before the enters, in the new state; the enters' mark after.
    assert_eq!(
        log(&world),
        [
            "hide_menu (0 marks): MainMenu",
            "drop_menu: MainMenu",
            "spawn_player (1 marks): InGame",
            "show_hud: InGame",
        ]
    );
    assert_eq!(world.read::<Mark>().len(), 2);
    assert!(world.resource::<StateMachine>().is(StateKind::InGame));
    assert_eq!(world.resource::<StateMachine>().queued(), None);

    let listed: Vec<String> = systems.describe().map(|s| s.to_string()).collect();
    assert_eq!(
        listed,
        [
            "exit  MainMenu: hide_menu",
            "exit  MainMenu: drop_menu",
            "enter MainMenu: show_menu",
            "exit  InGame: never",
            "enter InGame: spawn_player",
            "enter InGame: show_hud",
        ]
    );
}

#[test]
fn a_transition_queued_before_the_start_waits_for_the_call_after_it() {
    let mut world = world(GameState::Boot);
    let mut systems = StateSystems::new();
    systems
        .on_enter(StateKind::Boot, "boot", hook!("boot"))
        .on_exit(StateKind::Boot, "leave_boot", hook!("leave_boot"))
        .on_enter(StateKind::MainMenu, "menu", hook!("menu"));
    set(&world, GameState::MainMenu);
    assert_eq!(
        systems.apply(&mut world).map(|it| it.from),
        Some(None),
        "the start first"
    );
    assert_eq!(log(&world), ["boot (0 marks): Boot"]);
    systems.apply(&mut world).expect("then the queued one");
    assert_eq!(
        log(&world),
        [
            "boot (0 marks): Boot",
            "leave_boot (0 marks): Boot",
            "menu (0 marks): MainMenu"
        ]
    );
}

/// The game's tick: it pauses at tick 1, and notes, at the end of each
/// tick, which of the two cleanups ran.
fn paused_at_tick_one() -> Schedule {
    let mut schedule = Schedule::new();
    schedule
        .add_fixed_if(
            FixedStage::Gameplay,
            "pause",
            |world, tick| {
                if tick.index == 1 {
                    set(world, GameState::Paused);
                }
            },
            StateKind::InGame.condition(),
        )
        .add_fixed_if(
            FixedStage::Cleanup,
            "playing",
            |world, tick| note(world, &format!("playing {}", tick.index)),
            StateKind::InGame.condition(),
        )
        .add_fixed_if(
            FixedStage::Cleanup,
            "paused",
            |world, tick| note(world, &format!("paused {}", tick.index)),
            StateKind::Paused.condition(),
        );
    schedule
}

#[test]
fn no_transition_happens_mid_tick() {
    let mut world = world(GameState::InGame);
    let mut systems = StateSystems::new();
    systems.on_enter(StateKind::Paused, "show_pause", hook!("show_pause"));
    let schedule = paused_at_tick_one();
    systems.apply(&mut world);

    for index in 0..3 {
        schedule.run_fixed(&mut world, &tick(index));
        if index == 1 {
            // Queued in the tick, and the tick ran on in the state it began
            // in: its later systems still ran as playing.
            let machine = world.resource::<StateMachine>();
            assert!(machine.is(StateKind::InGame));
            assert_eq!(machine.queued(), Some(&GameState::Paused));
        }
        // Between ticks, as the facade does.
        systems.apply(&mut world);
    }
    assert_eq!(
        log(&world),
        [
            "playing 0: InGame",
            "playing 1: InGame",
            "show_pause (0 marks): Paused",
            "paused 2: Paused",
        ]
    );
}

#[test]
fn the_last_transition_queued_wins_and_one_to_the_state_it_is_in_does_nothing() {
    let mut world = world(GameState::InGame);
    let mut systems = StateSystems::new();
    systems
        .on_exit(StateKind::InGame, "leave", hook!("leave"))
        .on_enter(StateKind::InGame, "enter", hook!("enter"));
    systems.apply(&mut world);
    world.resource_mut::<Log>().0.clear();

    set(&world, GameState::InGame);
    assert_eq!(systems.apply(&mut world), None);
    assert_eq!(world.resource::<StateMachine>().queued(), None);
    assert!(log(&world).is_empty());

    set(&world, GameState::Paused);
    set(&world, GameState::MainMenu);
    let made = systems.apply(&mut world).expect("a transition");
    assert_eq!(made.to, GameState::MainMenu);
    assert_eq!(log(&world), ["leave (0 marks): InGame"]);
}

#[test]
fn a_transition_an_enter_system_queues_waits_for_the_next_call() {
    let mut world = world(GameState::Boot);
    let mut systems = StateSystems::new();
    systems
        .on_enter(StateKind::Boot, "to_menu", |world, _| {
            set(world, GameState::MainMenu);
        })
        .on_enter(StateKind::MainMenu, "menu", hook!("menu"));
    systems.apply(&mut world);
    assert!(world.resource::<StateMachine>().is(StateKind::Boot));
    assert_eq!(
        world.resource::<StateMachine>().queued(),
        Some(&GameState::MainMenu)
    );
    systems.apply(&mut world);
    assert!(world.resource::<StateMachine>().is(StateKind::MainMenu));
    assert_eq!(log(&world), ["menu (0 marks): MainMenu"]);
}

#[test]
fn loading_is_one_kind_whatever_the_level_and_the_systems_see_which() {
    let mut world = world(GameState::Loading(LevelId::new("field")));
    let mut systems = StateSystems::new();
    systems
        .on_exit(StateKind::Loading, "loaded", |world, transition| {
            let Some(GameState::Loading(level)) = &transition.from else {
                panic!("from a level: {transition:?}");
            };
            note(world, &format!("loaded {level}"));
        })
        .on_enter(StateKind::Loading, "loading", |world, transition| {
            let GameState::Loading(level) = &transition.to else {
                panic!("to a level: {transition:?}");
            };
            note(world, &format!("loading {}", level.as_str()));
        });
    let loading = StateKind::Loading.condition();
    let playing = StateKind::InGame.condition();
    systems.apply(&mut world);
    assert!(loading(&world) && !playing(&world));
    set(&world, GameState::Loading("cave".into()));
    systems.apply(&mut world);
    assert!(loading(&world));
    assert_eq!(
        log(&world),
        [
            "loading field: Loading(field)",
            "loaded field: Loading(field)",
            "loading cave: Loading(cave)"
        ]
    );
    assert_eq!(GameState::Loading("cave".into()).kind(), StateKind::Loading);
    for (state, kind) in [
        (GameState::Boot, StateKind::Boot),
        (GameState::MainMenu, StateKind::MainMenu),
        (GameState::InGame, StateKind::InGame),
        (GameState::Paused, StateKind::Paused),
    ] {
        assert_eq!(state.kind(), kind);
        assert_eq!(state.to_string(), format!("{kind:?}"));
    }
    assert_eq!(StateMachine::default().state(), &GameState::Boot);
}

#[test]
fn a_transition_system_that_borrows_wrongly_is_named_in_the_panic() {
    let mut world = world(GameState::Boot);
    let mut systems = StateSystems::new();
    systems.on_enter(StateKind::Boot, "greedy", |world, _| {
        let _log = world.resource_mut::<Log>();
        let _again = world.resource::<Log>();
    });
    let payload =
        catch_unwind(AssertUnwindSafe(|| systems.apply(&mut world))).expect_err("a panic");
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(message.contains("(system `greedy`)"), "{message}");
}
