//! [`Schedule`]: the systems, in stages, and the order they run in.

use std::fmt;

use nadir_core::time::{Frame, Tick};

use crate::World;
use crate::system::SystemScope;

/// The stages of a tick, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FixedStage {
    /// Reading the latched input into the world.
    Input,
    /// Game logic: AI, abilities, damage.
    Gameplay,
    /// Moving things, with collision.
    Movement,
    /// What follows from the moves: children follow parents, triggers.
    PostMovement,
    /// The end of the tick: timers, despawns.
    Cleanup,
}

impl FixedStage {
    /// Every stage, in order.
    pub const ALL: [Self; 5] = [
        Self::Input,
        Self::Gameplay,
        Self::Movement,
        Self::PostMovement,
        Self::Cleanup,
    ];
}

/// The stages of a frame, after its ticks, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameStage {
    /// Animation playback.
    Animate,
    /// The camera, between the ticks' positions.
    Camera,
    /// The interface.
    Ui,
    /// Filling the frame to draw from the world.
    Extract,
}

impl FrameStage {
    /// Every stage, in order.
    pub const ALL: [Self; 4] = [Self::Animate, Self::Camera, Self::Ui, Self::Extract];
}

/// A system that runs once a tick.
pub type FixedSystem = fn(&World, &Tick);
/// A system that runs once a frame.
pub type FrameSystem = fn(&World, &Frame);
/// Whether a system runs: asked right before each time it would.
pub type RunCondition = fn(&World) -> bool;

/// A system and what it needs to run.
struct Entry<C> {
    name: &'static str,
    run: fn(&World, &C),
    condition: Option<RunCondition>,
}

/// A stage, of a tick or of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// A stage of each tick.
    Fixed(FixedStage),
    /// A stage of each frame.
    Frame(FrameStage),
}

/// One system, as [`Schedule::describe`] lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemInfo {
    /// Its stage.
    pub stage: Stage,
    /// Its name.
    pub name: &'static str,
    /// Whether a run condition decides each time if it runs.
    pub conditional: bool,
}

impl fmt::Display for SystemInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.stage {
            Stage::Fixed(stage) => write!(f, "tick  {stage:?}: {}", self.name)?,
            Stage::Frame(stage) => write!(f, "frame {stage:?}: {}", self.name)?,
        }
        if self.conditional {
            write!(f, " (conditional)")?;
        }
        Ok(())
    }
}

/// The systems of a game, by stage. They run strictly in stage order, then
/// in the order they were added. Nothing runs in parallel, and nothing is
/// reordered.
///
/// ```
/// use nadir_core::time::Tick;
/// use nadir_ecs::{FixedStage, Schedule, World};
///
/// struct Ticks(u64);
///
/// let mut world = World::new();
/// world.insert_resource(Ticks(0));
/// let mut schedule = Schedule::new();
/// schedule.add_fixed(FixedStage::Gameplay, "count", |world, _| {
///     world.resource_mut::<Ticks>().0 += 1;
/// });
/// schedule.run_fixed(&mut world, &Tick { index: 0, dt: 1.0 / 60.0 });
/// assert_eq!(world.resource::<Ticks>().0, 1);
/// ```
pub struct Schedule {
    fixed: [Vec<Entry<Tick>>; FixedStage::ALL.len()],
    frame: [Vec<Entry<Frame>>; FrameStage::ALL.len()],
}

impl Schedule {
    /// A schedule with no systems.
    pub fn new() -> Self {
        Self {
            fixed: std::array::from_fn(|_| Vec::new()),
            frame: std::array::from_fn(|_| Vec::new()),
        }
    }

    /// Adds a system to a stage of each tick, after the stage's others.
    pub fn add_fixed(
        &mut self,
        stage: FixedStage,
        name: &'static str,
        run: FixedSystem,
    ) -> &mut Self {
        self.fixed[stage as usize].push(Entry {
            name,
            run,
            condition: None,
        });
        self
    }

    /// Adds a system to a stage of each tick, to run when `condition` holds.
    pub fn add_fixed_if(
        &mut self,
        stage: FixedStage,
        name: &'static str,
        run: FixedSystem,
        condition: RunCondition,
    ) -> &mut Self {
        self.fixed[stage as usize].push(Entry {
            name,
            run,
            condition: Some(condition),
        });
        self
    }

    /// Adds a system to a stage of each frame, after the stage's others.
    pub fn add_frame(
        &mut self,
        stage: FrameStage,
        name: &'static str,
        run: FrameSystem,
    ) -> &mut Self {
        self.frame[stage as usize].push(Entry {
            name,
            run,
            condition: None,
        });
        self
    }

    /// Adds a system to a stage of each frame, to run when `condition`
    /// holds.
    pub fn add_frame_if(
        &mut self,
        stage: FrameStage,
        name: &'static str,
        run: FrameSystem,
        condition: RunCondition,
    ) -> &mut Self {
        self.frame[stage as usize].push(Entry {
            name,
            run,
            condition: Some(condition),
        });
        self
    }

    /// Runs one tick: clears the events' tick buffers, runs the fixed
    /// stages' systems, then applies their commands.
    pub fn run_fixed(&self, world: &mut World, tick: &Tick) {
        world.clear_tick_events();
        for stage in &self.fixed {
            for entry in stage {
                run(world, entry, tick);
            }
        }
        world.apply_commands();
    }

    /// Runs the frame, after its ticks: runs the frame stages' systems,
    /// applies their commands, then clears the events' frame buffers.
    pub fn run_frame(&self, world: &mut World, frame: &Frame) {
        for stage in &self.frame {
            for entry in stage {
                run(world, entry, frame);
            }
        }
        world.apply_commands();
        world.clear_frame_events();
    }

    /// Every system, in the order they run: the tick's, then the frame's.
    pub fn describe(&self) -> impl Iterator<Item = SystemInfo> + '_ {
        let fixed = FixedStage::ALL
            .iter()
            .zip(&self.fixed)
            .flat_map(|(&stage, entries)| {
                entries.iter().map(move |entry| SystemInfo {
                    stage: Stage::Fixed(stage),
                    name: entry.name,
                    conditional: entry.condition.is_some(),
                })
            });
        let frame = FrameStage::ALL
            .iter()
            .zip(&self.frame)
            .flat_map(|(&stage, entries)| {
                entries.iter().map(move |entry| SystemInfo {
                    stage: Stage::Frame(stage),
                    name: entry.name,
                    conditional: entry.condition.is_some(),
                })
            });
        fixed.chain(frame)
    }
}

impl Default for Schedule {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs one system, if its condition holds, named for panic messages and
/// timed for the profiler.
fn run<C>(world: &World, entry: &Entry<C>, context: &C) {
    let _named = SystemScope::enter(entry.name);
    nadir_core::profile_scope!(entry.name);
    if entry.condition.is_none_or(|condition| condition(world)) {
        (entry.run)(world, context);
    }
}
