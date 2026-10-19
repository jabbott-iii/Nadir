//! The error type for `nadir-input`.

use std::fmt;

use crate::{ActionKind, Binding};

/// Errors produced by `nadir-input`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputError {
    /// The game's [`ActionSet`](crate::ActionSet) breaks a rule of the trait.
    ActionSet {
        /// What is wrong.
        problem: SetProblem,
    },
    /// A binding cannot go where it was asked to.
    Bind {
        /// The name of the action.
        action: &'static str,
        /// The slot it was asked to go in.
        slot: usize,
        /// Why not.
        kind: BindErrorKind,
    },
    /// A replay is malformed, or was recorded with another action set.
    Replay {
        /// The byte offset of the problem in the replay.
        offset: usize,
        /// What is wrong.
        kind: ReplayErrorKind,
    },
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActionSet { problem } => write!(f, "the action set is not valid: {problem}"),
            Self::Bind { action, slot, kind } => {
                write!(f, "cannot bind slot {slot} of `{action}`: {kind}")
            }
            Self::Replay { offset, kind } => write!(f, "replay error at byte {offset}: {kind}"),
        }
    }
}

impl std::error::Error for InputError {}

/// How an [`ActionSet`](crate::ActionSet) breaks the rules of the trait.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetProblem {
    /// `ALL` is empty.
    Empty,
    /// `ALL` has more actions than a replay can name.
    TooMany {
        /// How many it has.
        count: usize,
        /// The most it may have.
        limit: usize,
    },
    /// An action's `index` is not its position in `ALL`.
    WrongIndex {
        /// The action's name.
        action: &'static str,
        /// Its position in `ALL`.
        position: usize,
        /// What `index` returned.
        index: usize,
    },
    /// An action's name is empty.
    EmptyName {
        /// Its position in `ALL`.
        position: usize,
    },
    /// Two actions have the same name.
    DuplicateName {
        /// The name.
        name: &'static str,
    },
}

impl fmt::Display for SetProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("it has no actions"),
            Self::TooMany { count, limit } => {
                write!(f, "it has {count} actions, more than the {limit} allowed")
            }
            Self::WrongIndex {
                action,
                position,
                index,
            } => write!(
                f,
                "`{action}` is at position {position} in `ALL`, but its index is {index}"
            ),
            Self::EmptyName { position } => {
                write!(f, "the action at position {position} has an empty name")
            }
            Self::DuplicateName { name } => write!(f, "two actions are named `{name}`"),
        }
    }
}

/// Why a binding cannot go where it was asked to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindErrorKind {
    /// The binding cannot drive an action of this kind: a key composite
    /// gives a direction, so it fits only an [`ActionKind::Axis2`] action,
    /// and a single key or button fits only the other two.
    WrongKind {
        /// The binding.
        binding: Binding,
        /// The kind of the action.
        kind: ActionKind,
    },
    /// The slot does not exist.
    NoSuchSlot {
        /// How many slots an action has.
        slots: usize,
    },
}

impl fmt::Display for BindErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongKind { binding, kind } => {
                write!(f, "{binding:?} cannot drive an action of kind {kind:?}")
            }
            Self::NoSuchSlot { slots } => {
                write!(f, "an action has {slots} slots, numbered from 0")
            }
        }
    }
}

/// What is wrong with a replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayErrorKind {
    /// The replay does not start with the magic bytes `NRPL`.
    NotAReplay,
    /// The replay has a format version this build cannot read.
    UnknownVersion {
        /// The version in the replay.
        found: u16,
    },
    /// The replay was recorded with another number of actions.
    ActionCount {
        /// The number in this build's action set.
        expected: usize,
        /// The number in the replay.
        found: u16,
    },
    /// The replay was recorded with other action names or kinds.
    OtherActionSet,
    /// An action's kind in the replay differs from this build's.
    ActionKind {
        /// The action's name.
        action: &'static str,
        /// Its kind in this build.
        expected: ActionKind,
        /// The byte in the replay.
        found: u8,
    },
    /// The replay ends where more was due.
    UnexpectedEnd {
        /// What should be there.
        expected: &'static str,
    },
    /// Bits that no action uses are set.
    UnusedBitsSet,
    /// An action is just pressed but not held, which latching never gives.
    PressedButNotHeld {
        /// The action's name.
        action: &'static str,
    },
    /// An action is held and just released but was not just pressed, which
    /// latching never gives: once released, it is held again only by a
    /// new press.
    ReleasedButHeld {
        /// The action's name.
        action: &'static str,
    },
    /// An axis value is outside its range: 0 to 32767 for
    /// [`ActionKind::Axis1`], -32767 to 32767 for [`ActionKind::Axis2`].
    AxisOutOfRange {
        /// The action's name.
        action: &'static str,
        /// The value.
        found: i16,
    },
    /// A direction is longer than 1.
    DirectionTooLong {
        /// The action's name.
        action: &'static str,
    },
}

impl fmt::Display for ReplayErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAReplay => f.write_str("not a replay: it does not start with `NRPL`"),
            Self::UnknownVersion { found } => write!(f, "format version {found} is not known"),
            Self::ActionCount { expected, found } => write!(
                f,
                "recorded with {found} actions, but this build has {expected}"
            ),
            Self::OtherActionSet => {
                f.write_str("recorded with other action names or kinds than this build's")
            }
            Self::ActionKind {
                action,
                expected,
                found,
            } => write!(
                f,
                "`{action}` has kind byte {found} in the replay, but is {expected:?} in this build"
            ),
            Self::UnexpectedEnd { expected } => {
                write!(f, "the replay ends where {expected} should be")
            }
            Self::UnusedBitsSet => f.write_str("bits that no action uses are set"),
            Self::PressedButNotHeld { action } => {
                write!(f, "`{action}` is just pressed but not held")
            }
            Self::ReleasedButHeld { action } => write!(
                f,
                "`{action}` is held and just released, but was not just pressed"
            ),
            Self::AxisOutOfRange { action, found } => {
                write!(
                    f,
                    "`{action}` has the axis value {found}, outside its range"
                )
            }
            Self::DirectionTooLong { action } => {
                write!(f, "`{action}` has a direction longer than 1")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use nadir_core::Key;

    use super::*;

    #[test]
    fn errors_say_where_and_what() {
        let cases = [
            (
                InputError::ActionSet {
                    problem: SetProblem::DuplicateName { name: "jump" },
                },
                "the action set is not valid: two actions are named `jump`",
            ),
            (
                InputError::Bind {
                    action: "move",
                    slot: 0,
                    kind: BindErrorKind::WrongKind {
                        binding: Binding::Key(Key::W),
                        kind: ActionKind::Axis2,
                    },
                },
                "cannot bind slot 0 of `move`: Key(W) cannot drive an action of kind Axis2",
            ),
            (
                InputError::Replay {
                    offset: 16,
                    kind: ReplayErrorKind::UnexpectedEnd {
                        expected: "the action kinds",
                    },
                },
                "replay error at byte 16: the replay ends where the action kinds should be",
            ),
            (
                InputError::Replay {
                    offset: 6,
                    kind: ReplayErrorKind::ActionCount {
                        expected: 3,
                        found: 4,
                    },
                },
                "replay error at byte 6: recorded with 4 actions, but this build has 3",
            ),
        ];
        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
        }
    }
}
