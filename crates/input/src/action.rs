//! Actions: what the game asks about, instead of keys and buttons.

use std::fmt;

use crate::{InputError, SetProblem};

/// What an action reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActionKind {
    /// Pressed or not, with the moments it goes down and up: jump, fire,
    /// confirm.
    Button,
    /// A value from 0 to 1. A key or button bound to it gives 1 while it is
    /// held.
    Axis1,
    /// A direction no longer than 1, such as movement. +Y is down.
    Axis2,
}

impl ActionKind {
    /// The byte that stands for the kind in a replay.
    pub(crate) const fn byte(self) -> u8 {
        match self {
            Self::Button => 0,
            Self::Axis1 => 1,
            Self::Axis2 => 2,
        }
    }
}

/// The game's actions, implemented by an enum of the game.
///
/// ```
/// use nadir_input::{ActionKind, ActionSet};
///
/// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// enum Act {
///     Move,
///     Jump,
///     Pause,
/// }
///
/// impl ActionSet for Act {
///     const ALL: &'static [Self] = &[Act::Move, Act::Jump, Act::Pause];
///
///     fn index(self) -> usize {
///         self as usize
///     }
///
///     fn name(self) -> &'static str {
///         match self {
///             Act::Move => "move",
///             Act::Jump => "jump",
///             Act::Pause => "pause",
///         }
///     }
///
///     fn kind(self) -> ActionKind {
///         match self {
///             Act::Move => ActionKind::Axis2,
///             Act::Jump | Act::Pause => ActionKind::Button,
///         }
///     }
///
///     fn ui_allowed(self) -> bool {
///         self == Act::Pause
///     }
/// }
///
/// assert_eq!(Act::from_name("jump"), Some(Act::Jump));
/// ```
///
/// The name, kind and `ui_allowed` of an action belong to the game's code.
/// Which keys drive it is data, in an [`ActionMap`](crate::ActionMap).
pub trait ActionSet: Copy + Eq + fmt::Debug + 'static {
    /// Every action, in index order: `ALL[i].index() == i`. Every value of
    /// the type must be in it.
    const ALL: &'static [Self];

    /// The action's position in [`ActionSet::ALL`]. For a plain enum listed
    /// in declaration order, `self as usize`.
    fn index(self) -> usize;

    /// The action's name: not empty, and different from every other's.
    /// Settings files refer to actions by name, so renaming one makes old
    /// bindings miss it. A replay holds a hash of every name, so renaming
    /// any action makes old replays unreadable.
    fn name(self) -> &'static str;

    /// What the action reports.
    fn kind(self) -> ActionKind;

    /// Whether the action stays live in text mode, as a key that closes the
    /// text field must. Gameplay actions should not.
    fn ui_allowed(self) -> bool {
        false
    }

    /// The action with this name, if there is one.
    fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|action| action.name() == name)
    }
}

/// The most actions a set may have: a replay stores the count as a `u16`.
pub(crate) const MAX_ACTIONS: usize = u16::MAX as usize;

/// Checks the rules of [`ActionSet`] that can be checked.
pub(crate) fn validate<A: ActionSet>() -> Result<(), InputError> {
    let fail = |problem| Err(InputError::ActionSet { problem });
    if A::ALL.is_empty() {
        return fail(SetProblem::Empty);
    }
    if A::ALL.len() > MAX_ACTIONS {
        return fail(SetProblem::TooMany {
            count: A::ALL.len(),
            limit: MAX_ACTIONS,
        });
    }
    for (position, action) in A::ALL.iter().enumerate() {
        if action.index() != position {
            return fail(SetProblem::WrongIndex {
                action: action.name(),
                position,
                index: action.index(),
            });
        }
        if action.name().is_empty() {
            return fail(SetProblem::EmptyName { position });
        }
        if A::ALL[..position]
            .iter()
            .any(|earlier| earlier.name() == action.name())
        {
            return fail(SetProblem::DuplicateName {
                name: action.name(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Declares a test action set: the variants, then for each its name.
    macro_rules! set {
        ($set:ident, [$($variant:ident),*], $index:expr, [$($name:literal),*]) => {
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            enum $set { $($variant),* }

            impl ActionSet for $set {
                const ALL: &'static [Self] = &[$($set::$variant),*];

                fn index(self) -> usize {
                    let index: fn(Self) -> usize = $index;
                    index(self)
                }

                fn name(self) -> &'static str {
                    [$($name),*][self as usize]
                }

                fn kind(self) -> ActionKind {
                    ActionKind::Button
                }
            }
        };
    }

    #[test]
    fn a_set_that_keeps_the_rules_is_valid() {
        set!(Good, [A, B, C], |action| action as usize, ["a", "b", "c"]);
        assert_eq!(validate::<Good>(), Ok(()));
        assert_eq!(Good::from_name("b"), Some(Good::B));
        assert_eq!(Good::from_name("d"), None);
        assert!(!Good::A.ui_allowed());
    }

    #[test]
    fn a_wrong_index_is_found() {
        set!(Swapped, [A, B], |action| 1 - action as usize, ["a", "b"]);
        assert_eq!(
            validate::<Swapped>(),
            Err(InputError::ActionSet {
                problem: SetProblem::WrongIndex {
                    action: "a",
                    position: 0,
                    index: 1,
                }
            })
        );
    }

    #[test]
    fn empty_and_repeated_names_are_found() {
        set!(Blank, [A, B], |action| action as usize, ["a", ""]);
        assert_eq!(
            validate::<Blank>(),
            Err(InputError::ActionSet {
                problem: SetProblem::EmptyName { position: 1 }
            })
        );
        set!(Twice, [A, B, C], |action| action as usize, ["a", "b", "a"]);
        assert_eq!(
            validate::<Twice>(),
            Err(InputError::ActionSet {
                problem: SetProblem::DuplicateName { name: "a" }
            })
        );
    }

    #[test]
    fn a_set_with_no_actions_is_refused() {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Nothing {}

        impl ActionSet for Nothing {
            const ALL: &'static [Self] = &[];

            fn index(self) -> usize {
                match self {}
            }

            fn name(self) -> &'static str {
                match self {}
            }

            fn kind(self) -> ActionKind {
                match self {}
            }
        }

        assert_eq!(
            validate::<Nothing>(),
            Err(InputError::ActionSet {
                problem: SetProblem::Empty
            })
        );
    }

    #[test]
    fn kinds_have_distinct_bytes() {
        let bytes =
            [ActionKind::Button, ActionKind::Axis1, ActionKind::Axis2].map(ActionKind::byte);
        assert_eq!(bytes, [0, 1, 2]);
    }
}
