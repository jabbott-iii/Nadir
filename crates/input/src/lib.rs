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
