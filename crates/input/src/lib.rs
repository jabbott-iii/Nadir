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

//! Input for the Nadir engine: turns window events into the state of the
//! game's actions, once a fixed tick. Gameplay code reads actions, never
//! keys or buttons. `docs/subsystems/input.md` has the design.
//!
//! - The game names its actions with an [`ActionSet`], and an
//!   [`ActionMap`] binds keys and mouse buttons to them.
//! - [`Input`] takes in the window's events and keeps every change between
//!   two ticks, so that a tap between them is not lost.
//! - [`Input::latch`] makes the [`ActionState`] of a tick.
//! - [`InputRecorder`] and [`InputPlayer`] write and read replays: the
//!   state of every tick, which plays back exactly.
//!
//! ```
//! use nadir_core::{Key, Vec2};
//! use nadir_input::{ActionKind, ActionMap, ActionSet, ActionState, Binding, Input};
//! use nadir_platform::Event;
//!
//! #[derive(Clone, Copy, Debug, PartialEq, Eq)]
//! enum Act {
//!     Move,
//!     Jump,
//! }
//!
//! impl ActionSet for Act {
//!     const ALL: &'static [Self] = &[Act::Move, Act::Jump];
//!
//!     fn index(self) -> usize {
//!         self as usize
//!     }
//!
//!     fn name(self) -> &'static str {
//!         ["move", "jump"][self as usize]
//!     }
//!
//!     fn kind(self) -> ActionKind {
//!         [ActionKind::Axis2, ActionKind::Button][self as usize]
//!     }
//! }
//!
//! let mut map = ActionMap::<Act>::new()?;
//! map.bind(Act::Jump, 0, Binding::Key(Key::Space))?;
//! map.bind(
//!     Act::Move,
//!     0,
//!     Binding::KeyComposite2 { up: Key::W, down: Key::S, left: Key::A, right: Key::D },
//! )?;
//!
//! let mut input = Input::new();
//! let mut state = ActionState::new();
//! // Space goes down and up between two ticks, while D is held.
//! for (key, pressed) in [(Key::D, true), (Key::Space, true), (Key::Space, false)] {
//!     input.handle(&Event::Key { key, pressed, repeat: false });
//! }
//! input.latch(&map, &mut state);
//! assert!(state.just_pressed(Act::Jump) && state.just_released(Act::Jump));
//! assert_eq!(state.axis2(Act::Move), Vec2::new(1.0, 0.0));
//! # Ok::<(), nadir_input::InputError>(())
//! ```
//!
//! The facade crate, `nadir_engine`, re-exports this crate as
//! `nadir::input`, and its prelude has [`ActionKind`], [`ActionMap`],
//! [`ActionSet`], [`ActionState`], [`Binding`] and [`Input`]. Every `App`
//! keeps an [`Input`] resource and feeds it every window event.
//! `App::add_actions` adds the game's [`ActionMap`] and [`ActionState`] as
//! resources, and latches the state once a tick, before the tick stages;
//! `App::play_replay` plays an [`InputPlayer`] in their place. This crate
//! depends on `nadir-core`, for [`Key`](nadir_core::Key),
//! [`MouseButton`](nadir_core::MouseButton) and [`Vec2`](nadir_core::Vec2),
//! and on `nadir-platform`, for [`Event`](nadir_platform::Event). It has
//! no OS code of its own.
//!
//! Built so far (roadmap M3.1): keyboard and mouse bindings, latching, text
//! mode, rebinding conflicts and replays. Gamepads, deadzones and the
//! active device come with M8.4, the rebinding capture flow with M9.2,
//! bindings in the settings file with M9.3, and typed text with M7.9.
//!
//! # The parts
//!
//! - **Actions.** The game's enum implements [`ActionSet`]: each action's
//!   index, name, [`ActionKind`] and whether it stays live in text mode
//!   ([`ActionSet::ui_allowed`]). [`ActionSet::from_name`] finds one by
//!   name.
//! - **Bindings.** A [`Binding`] is a key, a mouse button or a four-key
//!   composite. An [`ActionMap`] holds [`SLOTS`] of them per action:
//!   [`ActionMap::bind`], [`ActionMap::unbind`], [`ActionMap::bindings`],
//!   and [`ActionMap::conflict`] for a rebinding menu.
//! - **Devices.** [`Input`] takes the window's events with
//!   [`Input::handle`] and makes a tick's state with [`Input::latch`].
//!   [`Input::set_text_mode`] and [`Input::text_mode`] turn text mode on
//!   and off. [`Input::pointer`] and [`Input::wheel`] are for the UI and
//!   aiming: they are not actions.
//! - **State.** [`ActionState`] is what gameplay code reads:
//!   [`ActionState::pressed`], [`ActionState::just_pressed`],
//!   [`ActionState::just_released`], [`ActionState::value`] and
//!   [`ActionState::axis2`].
//! - **Replays.** [`InputRecorder::record`] adds a tick's state, and
//!   [`InputRecorder::bytes`] or [`InputRecorder::into_bytes`] give the
//!   replay. [`InputPlayer::new`] checks its header against the action set,
//!   and [`InputPlayer::next_tick`] fills a state from each tick in turn.
//! - **Errors.** [`InputError`], with a [`SetProblem`], a
//!   [`BindErrorKind`] or a [`ReplayErrorKind`].
//!
//! # Rebinding
//!
//! A binding must fit its action's kind, and a rebinding menu asks
//! [`ActionMap::conflict`] before it binds:
//!
//! ```
//! use nadir_core::Key;
//! use nadir_input::{ActionKind, ActionMap, ActionSet, BindErrorKind, Binding, InputError};
//!
//! #[derive(Clone, Copy, Debug, PartialEq, Eq)]
//! enum Act {
//!     Move,
//!     Jump,
//!     Dash,
//! }
//!
//! impl ActionSet for Act {
//!     const ALL: &'static [Self] = &[Act::Move, Act::Jump, Act::Dash];
//!
//!     fn index(self) -> usize {
//!         self as usize
//!     }
//!
//!     fn name(self) -> &'static str {
//!         ["move", "jump", "dash"][self as usize]
//!     }
//!
//!     fn kind(self) -> ActionKind {
//!         [ActionKind::Axis2, ActionKind::Button, ActionKind::Button][self as usize]
//!     }
//! }
//!
//! let mut map = ActionMap::<Act>::new()?;
//! let wasd = Binding::KeyComposite2 { up: Key::W, down: Key::S, left: Key::A, right: Key::D };
//! map.bind(Act::Move, 0, wasd)?;
//! map.bind(Act::Jump, 0, Binding::Key(Key::Space))?;
//!
//! // The player picks D for dash: the composite reads it already.
//! let d = Binding::Key(Key::D);
//! assert_eq!(map.conflict(Act::Dash, 0, &d), Some((Act::Move, 0)));
//! map.bind(Act::Dash, 0, Binding::Key(Key::LeftShift))?;
//!
//! // A single key cannot give a direction, and nothing changes.
//! let refused = map.bind(Act::Move, 1, Binding::Key(Key::Up));
//! assert!(matches!(
//!     refused,
//!     Err(InputError::Bind { action: "move", slot: 1, kind: BindErrorKind::WrongKind { .. } })
//! ));
//! assert_eq!(map.bindings(Act::Move), &[Some(wasd), None, None, None]);
//! # Ok::<(), nadir_input::InputError>(())
//! ```
//!
//! # Using it well
//!
//! - **Latch once a tick, with one map.** [`Input::latch`] starts the next
//!   tick, so a second call sees nothing of the one before. A second
//!   reader, such as one that runs once a frame, needs an [`Input`] of its
//!   own, fed the same events. Bindings changed between two latches apply
//!   to the whole tick.
//! - **Edges.** An action goes down when the first of its bound keys and
//!   buttons goes down, and up when the last goes up. A tap between two
//!   ticks is held for one tick, and is both just pressed and just released
//!   in it. Key repeats are ignored, and losing the window's focus releases
//!   everything that is down.
//! - **Text mode.** While a text field has the keyboard, gameplay actions
//!   read as released and only `ui_allowed` actions stay live. Keys and
//!   buttons held when it begins, or pressed during it, stay hidden from
//!   gameplay actions until they are released, so leaving text mode presses
//!   nothing.
//! - **Errors.** [`ActionMap::new`], [`InputRecorder::new`] and
//!   [`InputPlayer::new`] check the [`ActionSet`]'s rules: some actions,
//!   no more than 65,535, each at its index in `ALL`, with names that are
//!   not empty and differ. A failed [`ActionMap::bind`] changes nothing. A
//!   replay error carries the byte offset of the problem; after one,
//!   [`InputPlayer::next_tick`] leaves the state as it was. [`InputError`]
//!   implements `Display` and `std::error::Error`.
//! - **Replays play back exactly.** Values are stored as `i16` steps of
//!   1/32767, live and replayed alike, so a replayed [`ActionState`]
//!   equals the live one. A replay holds a hash of every action's name and
//!   kind: renaming an action, changing its kind, or adding one makes old
//!   replays unreadable. Replays do not hold the pointer or the wheel, so a
//!   fixed system that reads those does not replay. The format, version 1,
//!   is described in `docs/subsystems/input.md`.
//! - **Limits.** Each action has [`SLOTS`] bindings. Between two latches,
//!   [`Input`] keeps up to 1,024 changes; past that, the oldest half is
//!   folded into the state they started from, and their edges are lost.
//! - **Allocation.** [`Input`] reuses its buffer of changes, so handling
//!   events and latching allocate nothing once it has grown. An
//!   [`ActionState`] allocates when it is made, and an [`InputRecorder`]'s
//!   bytes grow with each tick.

mod action;
mod binding;
mod code;
mod error;
mod input;
mod replay;
mod state;

pub use action::{ActionKind, ActionSet};
pub use binding::{ActionMap, Binding, SLOTS};
pub use error::{BindErrorKind, InputError, ReplayErrorKind, SetProblem};
pub use input::Input;
pub use replay::{InputPlayer, InputRecorder};
pub use state::ActionState;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

#[cfg(test)]
pub(crate) mod tests {
    //! An action set for the unit tests.

    use crate::{ActionKind, ActionSet};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Act {
        Move,
        Jump,
        Pause,
        Throttle,
    }

    impl ActionSet for Act {
        const ALL: &'static [Self] = &[Act::Move, Act::Jump, Act::Pause, Act::Throttle];

        fn index(self) -> usize {
            self as usize
        }

        fn name(self) -> &'static str {
            ["move", "jump", "pause", "throttle"][self as usize]
        }

        fn kind(self) -> ActionKind {
            [
                ActionKind::Axis2,
                ActionKind::Button,
                ActionKind::Button,
                ActionKind::Axis1,
            ][self as usize]
        }

        fn ui_allowed(self) -> bool {
            self == Act::Pause
        }
    }
}
