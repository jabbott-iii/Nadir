//! Replays: the game's actions played from a recording in place of the live
//! input ([`App::play_replay`](crate::App::play_replay)), and [`Replay`], how
//! far one got.

use nadir_ecs::World;
use nadir_input::{ActionSet, ActionState, Input, InputError, InputPlayer};
use nadir_scene::{ReflectContext, TypeRegistry};

use crate::app::LatchFn;

/// A hash of every reflected component of `world`, as
/// [`TypeRegistry::hash_world`] takes it, with the context the engine gives
/// it. Determinism tests compare it run against run, tick after tick.
///
/// It is the same after the same ticks of the same replay, from the same
/// start, whatever the frames were, as long as the game's frame systems
/// change no reflected component and spawn or despawn nothing, and no tick
/// system reads what frames set (an [`Animator`](crate::Animator)'s time,
/// the `Sprite` fields that `nadir::animate` writes, [`TickStats`](crate::TickStats)),
/// whether an asset has loaded, or the pointer or the wheel (ADR-0025).
///
/// # Panics
///
/// If the world has no [`TypeRegistry`] (every [`App`](crate::App)'s has
/// one), or a reflected type's components are borrowed mutably elsewhere.
pub fn world_hash(world: &World) -> u64 {
    let registry = world.resource::<TypeRegistry>();
    registry.hash_world(world, &mut ReflectContext::new("nadir::world_hash"))
}

/// How the replay that [`App::play_replay`](crate::App::play_replay) plays
/// is going: a resource that it adds.
///
/// `ticks` counts the replay's ticks that fed the actions so far. While a
/// tick's systems run, it counts that tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Replay {
    /// Its ticks feed the actions.
    Playing {
        /// The ticks played, this one included.
        ticks: u64,
    },
    /// It ended with its last tick, the `ticks`-th. The ticks after it are
    /// latched from the live input.
    Ended {
        /// The ticks it held.
        ticks: u64,
    },
    /// After `ticks` ticks, the next could not be read. The ticks after
    /// the last played are latched from the live input.
    Failed {
        /// The ticks played before the one that could not be read.
        ticks: u64,
        /// Why it could not be read.
        error: InputError,
    },
}

/// A replay that is playing, whatever its action set.
pub(crate) trait PlayReplay {
    /// Starts a tick: latches the live input into a set of its own, so that
    /// [`Input`] keeps its ticks and nothing of it reaches the actions, and
    /// puts the replay's next tick into the game's [`ActionState`]. Returns
    /// the ticks played, this one included.
    fn latch(&mut self, input: &mut Input, world: &World) -> u64;

    /// The ticks played so far.
    fn ticks(&self) -> u64;

    /// What reading ahead found after the tick just latched: `Ok(true)` if
    /// another tick follows, `Ok(false)` at the end, or why the next tick
    /// cannot be read.
    fn ahead(&self) -> &Result<bool, InputError>;
}

/// A replay of the action set `A`, one tick read ahead of the actions, so
/// that its end is known as its last tick ends.
pub(crate) struct ReplayInput<A: ActionSet> {
    player: InputPlayer<'static, A>,
    /// The next tick's state.
    next: ActionState<A>,
    ahead: Result<bool, InputError>,
    ticks: u64,
    /// The live input's latch, into a set of its own.
    live: LatchFn,
}

impl<A: ActionSet> ReplayInput<A> {
    /// Reads `player`'s first tick ahead. `live` latches the live input
    /// into a set of its own.
    pub(crate) fn new(mut player: InputPlayer<'static, A>, live: LatchFn) -> Self {
        let mut next = ActionState::new();
        let ahead = player.next_tick(&mut next);
        Self {
            player,
            next,
            ahead,
            ticks: 0,
            live,
        }
    }
}

impl<A: ActionSet> PlayReplay for ReplayInput<A> {
    fn latch(&mut self, input: &mut Input, world: &World) -> u64 {
        (self.live)(input, world);
        // `next_tick` fills every action, so the state swapped out is
        // overwritten whole.
        std::mem::swap(&mut *world.resource_mut::<ActionState<A>>(), &mut self.next);
        self.ticks += 1;
        self.ahead = self.player.next_tick(&mut self.next);
        self.ticks
    }

    fn ticks(&self) -> u64 {
        self.ticks
    }

    fn ahead(&self) -> &Result<bool, InputError> {
        &self.ahead
    }
}
