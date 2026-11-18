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

//! Game states (`docs/subsystems/scene.md`, "Game states"; ADR-0067): the
//! [`GameState`] a game is in, the [`StateMachine`] resource that holds it
//! and the transition queued, run conditions by [`StateKind`], and the
//! enter and exit systems of [`StateSystems`], which make the queued
//! transition between ticks.
//!
//! The machine never changes state by itself: whoever runs the schedule
//! calls [`StateSystems::apply`] between ticks, as the facade's `App` does
//! after each tick and after the frame stages. So a transition queued in a
//! tick is made after that tick, and the rest of the tick sees the state
//! it started in.

use std::fmt;

use nadir_core::log::{debug, info};
use nadir_ecs::{RunCondition, World, run_system};

/// A level, by the name the facade's `load_level` takes: the level file
/// `levels/<name>.klvl`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LevelId(String);

impl LevelId {
    /// The level named `name`. Whether a level of that name exists is
    /// found when it loads.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Its name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for LevelId {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for LevelId {
    fn from(name: String) -> Self {
        Self(name)
    }
}

impl fmt::Display for LevelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The state a game is in.
///
/// The facade gives some of them a meaning of its own: in [`Loading`] it
/// loads the level behind a loading screen, then goes on to [`InGame`]; in
/// [`Paused`] the world stands still. The others are the game's to use.
///
/// [`Loading`]: GameState::Loading
/// [`InGame`]: GameState::InGame
/// [`Paused`]: GameState::Paused
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum GameState {
    /// Starting: the state a [`StateMachine`] starts in by default.
    #[default]
    Boot,
    /// Loading a level.
    Loading(LevelId),
    /// The main menu.
    MainMenu,
    /// Playing.
    InGame,
    /// Paused, from play.
    Paused,
}

impl GameState {
    /// The kind of state this is: its variant, without the level.
    pub fn kind(&self) -> StateKind {
        match self {
            Self::Boot => StateKind::Boot,
            Self::Loading(_) => StateKind::Loading,
            Self::MainMenu => StateKind::MainMenu,
            Self::InGame => StateKind::InGame,
            Self::Paused => StateKind::Paused,
        }
    }
}

impl fmt::Display for GameState {
    /// The variant's name, with the level of [`GameState::Loading`]:
    /// `Loading(field)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Loading(level) => write!(f, "Loading({level})"),
            other => write!(f, "{:?}", other.kind()),
        }
    }
}

/// A kind of [`GameState`]: its variant, without the level of
/// [`GameState::Loading`]. Run conditions, enter systems and exit systems
/// are by kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StateKind {
    /// [`GameState::Boot`].
    Boot,
    /// [`GameState::Loading`], of any level.
    Loading,
    /// [`GameState::MainMenu`].
    MainMenu,
    /// [`GameState::InGame`].
    InGame,
    /// [`GameState::Paused`].
    Paused,
}

impl StateKind {
    /// Every kind, in the order of [`GameState`]'s variants.
    pub const ALL: [Self; 5] = [
        Self::Boot,
        Self::Loading,
        Self::MainMenu,
        Self::InGame,
        Self::Paused,
    ];

    /// A run condition that holds while the world's [`StateMachine`] is in
    /// a state of this kind, for
    /// [`Schedule::add_fixed_if`](nadir_ecs::Schedule::add_fixed_if) and
    /// [`Schedule::add_frame_if`](nadir_ecs::Schedule::add_frame_if). It
    /// asks the state the machine is in, not the one queued.
    ///
    /// Asked in a world with no `StateMachine`, it panics, naming the
    /// resource and the system.
    pub fn condition(self) -> RunCondition {
        match self {
            Self::Boot => |world| in_kind(world, StateKind::Boot),
            Self::Loading => |world| in_kind(world, StateKind::Loading),
            Self::MainMenu => |world| in_kind(world, StateKind::MainMenu),
            Self::InGame => |world| in_kind(world, StateKind::InGame),
            Self::Paused => |world| in_kind(world, StateKind::Paused),
        }
    }
}

/// Whether the world's machine is in a state of `kind`.
fn in_kind(world: &World, kind: StateKind) -> bool {
    world.resource::<StateMachine>().is(kind)
}

/// The game's state and the transition queued: a world resource.
///
/// Systems ask it ([`StateMachine::state`], [`StateMachine::is`]) and queue
/// a transition with [`StateMachine::set`]; [`StateSystems::apply`] makes
/// it, between ticks, running the exit and enter systems.
///
/// ```
/// use nadir_scene::{GameState, StateKind, StateMachine};
///
/// let mut machine = StateMachine::new(GameState::MainMenu);
/// machine.set(GameState::Loading("field".into()));
/// // Queued, not made: the machine is where it was until it is applied.
/// assert!(machine.is(StateKind::MainMenu));
/// assert_eq!(machine.queued(), Some(&GameState::Loading("field".into())));
/// ```
#[derive(Debug, Default)]
pub struct StateMachine {
    state: GameState,
    queued: Option<GameState>,
    /// Whether the enter systems of the state it started in have run.
    started: bool,
}

impl StateMachine {
    /// A machine that starts in `first`: the first [`StateSystems::apply`]
    /// runs `first`'s enter systems.
    pub fn new(first: GameState) -> Self {
        Self {
            state: first,
            queued: None,
            started: false,
        }
    }

    /// The state it is in.
    pub fn state(&self) -> &GameState {
        &self.state
    }

    /// Whether it is in a state of `kind`.
    pub fn is(&self, kind: StateKind) -> bool {
        self.state.kind() == kind
    }

    /// The transition queued, if any: the state it goes to.
    pub fn queued(&self) -> Option<&GameState> {
        self.queued.as_ref()
    }

    /// Queues a transition to `next`, which [`StateSystems::apply`] makes
    /// between ticks. It replaces a transition queued before. Once applied,
    /// a transition to the state it is in does nothing.
    pub fn set(&mut self, next: GameState) {
        if let Some(earlier) = self.queued.replace(next) {
            debug!("the transition to {earlier} gives way to a later one");
        }
    }
}

/// A transition, as its exit and enter systems are given it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    /// The state left; `None` for the enter systems of the state a machine
    /// starts in.
    pub from: Option<GameState>,
    /// The state entered.
    pub to: GameState,
}

/// An enter or exit system: runs once when its kind of state is entered
/// or left, between ticks, with the transition. It borrows from the world
/// as any system does, and its commands apply once the others of its
/// transition and its kind (exit or enter) have run.
pub type TransitionSystem = fn(&World, &Transition);

/// A named system of a kind of state.
#[derive(Clone, Copy)]
struct Hook {
    name: &'static str,
    run: TransitionSystem,
}

/// The enter and exit systems of each kind of state, and what makes a
/// transition.
///
/// ```
/// use nadir_ecs::World;
/// use nadir_scene::{GameState, StateKind, StateMachine, StateSystems};
///
/// struct Log(Vec<String>);
///
/// let mut world = World::new();
/// world.insert_resource(StateMachine::new(GameState::MainMenu));
/// world.insert_resource(Log(Vec::new()));
/// let mut systems = StateSystems::new();
/// systems
///     .on_exit(StateKind::MainMenu, "hide_menu", |world, to| {
///         world.resource_mut::<Log>().0.push(format!("exit for {}", to.to));
///     })
///     .on_enter(StateKind::InGame, "spawn_player", |world, from| {
///         world.resource_mut::<Log>().0.push(format!("enter from {:?}", from.from));
///     });
///
/// systems.apply(&mut world); // the main menu's own enter: none here
/// world.resource_mut::<StateMachine>().set(GameState::InGame);
/// systems.apply(&mut world);
/// assert!(world.resource::<StateMachine>().is(StateKind::InGame));
/// assert_eq!(
///     world.resource::<Log>().0,
///     ["exit for InGame", "enter from Some(MainMenu)"],
/// );
/// ```
#[derive(Clone, Default)]
pub struct StateSystems {
    enter: [Vec<Hook>; StateKind::ALL.len()],
    exit: [Vec<Hook>; StateKind::ALL.len()],
}

impl StateSystems {
    /// No enter or exit systems.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a system to run each time a state of `kind` is entered, after
    /// the kind's others.
    pub fn on_enter(
        &mut self,
        kind: StateKind,
        name: &'static str,
        system: TransitionSystem,
    ) -> &mut Self {
        self.enter[kind as usize].push(Hook { name, run: system });
        self
    }

    /// Adds a system to run each time a state of `kind` is left, after the
    /// kind's others.
    pub fn on_exit(
        &mut self,
        kind: StateKind,
        name: &'static str,
        system: TransitionSystem,
    ) -> &mut Self {
        self.exit[kind as usize].push(Hook { name, run: system });
        self
    }

    /// Makes one transition, if one is due, and returns it. Call it between
    /// ticks, never inside one.
    ///
    /// The first call makes the machine's start: it runs the enter systems
    /// of the state it started in, from `None`, and leaves a transition
    /// queued before it for the next call. After that, it takes the queued
    /// transition: one to the state the machine is in is dropped, at debug
    /// level, and returns `None`. Otherwise:
    ///
    /// 1. the exit systems of the state left run, in the order they were
    ///    added, while the machine is still in it; then their commands
    ///    apply;
    /// 2. the machine goes to the new state;
    /// 3. the enter systems of the new state run, in the order they were
    ///    added; then their commands apply.
    ///
    /// The transition is logged. One that these systems queue waits for the
    /// next call. Each runs as a schedule's system does, named for borrow
    /// panics and timed for the profiler ([`run_system`]).
    ///
    /// # Panics
    ///
    /// If the world has no [`StateMachine`], or it is borrowed elsewhere.
    pub fn apply(&self, world: &mut World) -> Option<Transition> {
        let transition = {
            let mut machine = world.resource_mut::<StateMachine>();
            if !machine.started {
                machine.started = true;
                Transition {
                    from: None,
                    to: machine.state.clone(),
                }
            } else {
                let next = machine.queued.take()?;
                if next == machine.state {
                    debug!("the game is in {next} already: no transition");
                    return None;
                }
                Transition {
                    from: Some(machine.state.clone()),
                    to: next,
                }
            }
        };
        if let Some(from) = &transition.from {
            self.run(world, &self.exit[from.kind() as usize], &transition);
            world.resource_mut::<StateMachine>().state = transition.to.clone();
            info!("the game goes from {from} to {}", transition.to);
        } else {
            info!("the game starts in {}", transition.to);
        }
        self.run(
            world,
            &self.enter[transition.to.kind() as usize],
            &transition,
        );
        Some(transition)
    }

    /// Runs `hooks` for `transition`, then applies their commands.
    fn run(&self, world: &mut World, hooks: &[Hook], transition: &Transition) {
        for hook in hooks {
            run_system(world, hook.name, hook.run, transition);
        }
        world.apply_commands();
    }

    /// Every enter and exit system, by kind of state in the order of
    /// [`StateKind::ALL`]: each kind's exit systems, then its enter
    /// systems, each in the order they were added, which is the order they
    /// run in.
    pub fn describe(&self) -> impl Iterator<Item = StateSystemInfo> + '_ {
        StateKind::ALL.iter().flat_map(move |&kind| {
            let exits = self.exit[kind as usize]
                .iter()
                .map(move |hook| StateSystemInfo {
                    kind,
                    enter: false,
                    name: hook.name,
                });
            let enters = self.enter[kind as usize]
                .iter()
                .map(move |hook| StateSystemInfo {
                    kind,
                    enter: true,
                    name: hook.name,
                });
            exits.chain(enters)
        })
    }
}

/// One enter or exit system, as [`StateSystems::describe`] lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateSystemInfo {
    /// The kind of state it is for.
    pub kind: StateKind,
    /// Whether it runs on entering the state, rather than on leaving it.
    pub enter: bool,
    /// Its name.
    pub name: &'static str,
}

impl fmt::Display for StateSystemInfo {
    /// As `exit  MainMenu: hide_menu` or `enter InGame: spawn_player`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let when = if self.enter { "enter" } else { "exit " };
        write!(f, "{when} {:?}: {}", self.kind, self.name)
    }
}
