//! Bindings: which keys and buttons drive which actions.

use std::fmt;
use std::marker::PhantomData;

use nadir_core::{Key, MouseButton};

use crate::code::{Code, Inputs};
use crate::{ActionKind, ActionSet, BindErrorKind, InputError, action};

/// A key or button, or a group of them, that drives an action.
///
/// Gamepad bindings come with gamepad support (roadmap M8.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Binding {
    /// A key, for an [`ActionKind::Button`], or for an
    /// [`ActionKind::Axis1`] that it holds at 1.
    Key(Key),
    /// A mouse button, as for [`Binding::Key`].
    MouseButton(MouseButton),
    /// Four keys that give a direction, for an [`ActionKind::Axis2`]: WASD
    /// or the arrows. A diagonal has length 1, and opposite keys cancel.
    KeyComposite2 {
        /// The key for up, which is -Y.
        up: Key,
        /// The key for down, which is +Y.
        down: Key,
        /// The key for left, which is -X.
        left: Key,
        /// The key for right, which is +X.
        right: Key,
    },
}

impl Binding {
    /// The inputs the binding reads. For a composite: up, down, left, right.
    pub(crate) fn inputs(&self) -> Inputs {
        match *self {
            Self::Key(key) => Inputs::one(Code::key(key)),
            Self::MouseButton(button) => Inputs::one(Code::mouse(button)),
            Self::KeyComposite2 {
                up,
                down,
                left,
                right,
            } => Inputs::four([up, down, left, right].map(Code::key)),
        }
    }

    /// Whether the binding can drive an action of this kind.
    fn fits(&self, kind: ActionKind) -> bool {
        match self {
            Self::Key(_) | Self::MouseButton(_) => kind != ActionKind::Axis2,
            Self::KeyComposite2 { .. } => kind == ActionKind::Axis2,
        }
    }

    /// Whether the two bindings read a key or button in common.
    fn overlaps(&self, other: &Binding) -> bool {
        let theirs = other.inputs();
        self.inputs()
            .as_slice()
            .iter()
            .any(|code| theirs.as_slice().contains(code))
    }
}

/// How many bindings an action can have. A controls menu shows them as
/// columns, so each keeps its slot when another is removed.
pub const SLOTS: usize = 4;

/// The bindings of every action of the set `A`.
///
/// An action is held while any of its bound keys and buttons is down. A
/// [`ActionKind::Button`] or [`ActionKind::Axis1`] is worth 1 while held,
/// and an [`ActionKind::Axis2`] is the sum of its composites' directions,
/// no longer than 1. [`Input::latch`](crate::Input::latch) has the details.
pub struct ActionMap<A> {
    bindings: Vec<[Option<Binding>; SLOTS]>,
    set: PhantomData<A>,
}

impl<A: ActionSet> ActionMap<A> {
    /// A map with no bindings.
    ///
    /// # Errors
    ///
    /// Fails with [`InputError::ActionSet`] if the set breaks a rule of
    /// [`ActionSet`].
    pub fn new() -> Result<Self, InputError> {
        action::validate::<A>()?;
        Ok(Self {
            bindings: vec![[None; SLOTS]; A::ALL.len()],
            set: PhantomData,
        })
    }

    /// Puts `binding` in a slot of `action`, and returns the binding that
    /// was there.
    ///
    /// It does not check whether another action reads the same keys: a
    /// default table may share one on purpose, such as Escape for both a
    /// pause action and a menu's back action. Ask [`ActionMap::conflict`]
    /// first when the player rebinds.
    ///
    /// # Errors
    ///
    /// Fails with [`InputError::Bind`], and changes nothing, if the slot
    /// does not exist or the binding cannot drive an action of that kind.
    pub fn bind(
        &mut self,
        action: A,
        slot: usize,
        binding: Binding,
    ) -> Result<Option<Binding>, InputError> {
        let fail = |kind| {
            Err(InputError::Bind {
                action: action.name(),
                slot,
                kind,
            })
        };
        if slot >= SLOTS {
            return fail(BindErrorKind::NoSuchSlot { slots: SLOTS });
        }
        if !binding.fits(action.kind()) {
            return fail(BindErrorKind::WrongKind {
                binding,
                kind: action.kind(),
            });
        }
        Ok(self.bindings[action.index()][slot].replace(binding))
    }

    /// Empties a slot of `action`, and returns the binding that was there.
    /// A slot that does not exist is empty.
    pub fn unbind(&mut self, action: A, slot: usize) -> Option<Binding> {
        self.bindings[action.index()]
            .get_mut(slot)
            .and_then(Option::take)
    }

    /// The slots of `action`.
    pub fn bindings(&self, action: A) -> &[Option<Binding>; SLOTS] {
        &self.bindings[action.index()]
    }

    /// The first other slot, in index and slot order, whose binding reads a
    /// key or button that `binding` reads, if `binding` went into `slot` of
    /// `action`. Another slot of the same action counts; the slot being
    /// replaced does not.
    ///
    /// This is how a rebinding menu finds a conflict, and then swaps the
    /// bindings or refuses.
    pub fn conflict(&self, action: A, slot: usize, binding: &Binding) -> Option<(A, usize)> {
        A::ALL.iter().find_map(|&other| {
            self.bindings[other.index()]
                .iter()
                .enumerate()
                .find(|&(other_slot, existing)| {
                    (other, other_slot) != (action, slot)
                        && existing.is_some_and(|existing| existing.overlaps(binding))
                })
                .map(|(other_slot, _)| (other, other_slot))
        })
    }

    /// The bindings of the action with this index, filled slots only.
    pub(crate) fn slots(&self, index: usize) -> impl Iterator<Item = &Binding> {
        self.bindings[index].iter().flatten()
    }
}

impl<A> Clone for ActionMap<A> {
    fn clone(&self) -> Self {
        Self {
            bindings: self.bindings.clone(),
            set: PhantomData,
        }
    }
}

impl<A: ActionSet> fmt::Debug for ActionMap<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(A::ALL.iter().map(|&action| {
                let slots = &self.bindings[action.index()];
                (action.name(), slots)
            }))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Act;

    fn wasd() -> Binding {
        Binding::KeyComposite2 {
            up: Key::W,
            down: Key::S,
            left: Key::A,
            right: Key::D,
        }
    }

    fn map() -> ActionMap<Act> {
        let mut map = ActionMap::new().expect("a valid set");
        map.bind(Act::Move, 0, wasd()).expect("fits");
        map.bind(Act::Jump, 0, Binding::Key(Key::Space))
            .expect("fits");
        map.bind(Act::Jump, 1, Binding::MouseButton(MouseButton::Left))
            .expect("fits");
        map.bind(Act::Pause, 0, Binding::Key(Key::Escape))
            .expect("fits");
        map
    }

    #[test]
    fn binding_fills_a_slot_and_returns_what_was_there() {
        let mut map = map();
        assert_eq!(
            map.bind(Act::Jump, 0, Binding::Key(Key::J)),
            Ok(Some(Binding::Key(Key::Space)))
        );
        assert_eq!(
            map.bindings(Act::Jump),
            &[
                Some(Binding::Key(Key::J)),
                Some(Binding::MouseButton(MouseButton::Left)),
                None,
                None
            ]
        );
        assert_eq!(map.unbind(Act::Jump, 0), Some(Binding::Key(Key::J)));
        assert_eq!(map.unbind(Act::Jump, 0), None);
        assert_eq!(map.unbind(Act::Jump, SLOTS), None);
        // The other slot keeps its place.
        assert_eq!(
            map.bindings(Act::Jump)[1],
            Some(Binding::MouseButton(MouseButton::Left))
        );
    }

    #[test]
    fn a_binding_must_fit_the_action_and_the_slot_must_exist() {
        let mut map = map();
        assert_eq!(
            map.bind(Act::Move, 1, Binding::Key(Key::Up)),
            Err(InputError::Bind {
                action: "move",
                slot: 1,
                kind: BindErrorKind::WrongKind {
                    binding: Binding::Key(Key::Up),
                    kind: ActionKind::Axis2,
                },
            })
        );
        assert_eq!(
            map.bind(Act::Jump, 1, wasd()),
            Err(InputError::Bind {
                action: "jump",
                slot: 1,
                kind: BindErrorKind::WrongKind {
                    binding: wasd(),
                    kind: ActionKind::Button,
                },
            })
        );
        assert_eq!(
            map.bind(Act::Jump, SLOTS, Binding::Key(Key::K)),
            Err(InputError::Bind {
                action: "jump",
                slot: SLOTS,
                kind: BindErrorKind::NoSuchSlot { slots: SLOTS },
            })
        );
        // Nothing changed.
        assert_eq!(
            map.bindings(Act::Jump)[1],
            Some(Binding::MouseButton(MouseButton::Left))
        );
        // Keys and buttons fit an Axis1 action.
        assert_eq!(
            map.bind(Act::Throttle, 0, Binding::Key(Key::LeftShift)),
            Ok(None)
        );
    }

    #[test]
    fn rebind_conflicts_are_detected() {
        let map = map();
        // A key another action reads.
        assert_eq!(
            map.conflict(Act::Pause, 0, &Binding::Key(Key::Space)),
            Some((Act::Jump, 0))
        );
        // A key inside another action's composite.
        assert_eq!(
            map.conflict(Act::Jump, 2, &Binding::Key(Key::D)),
            Some((Act::Move, 0))
        );
        // A composite that shares one key with a single binding.
        let arrows_and_escape = Binding::KeyComposite2 {
            up: Key::Up,
            down: Key::Down,
            left: Key::Left,
            right: Key::Escape,
        };
        assert_eq!(
            map.conflict(Act::Move, 1, &arrows_and_escape),
            Some((Act::Pause, 0))
        );
        // Another slot of the same action.
        assert_eq!(
            map.conflict(Act::Jump, 2, &Binding::Key(Key::Space)),
            Some((Act::Jump, 0))
        );
        // The slot being replaced is not a conflict, and a free key is none.
        assert_eq!(map.conflict(Act::Jump, 0, &Binding::Key(Key::Space)), None);
        assert_eq!(map.conflict(Act::Jump, 0, &Binding::Key(Key::K)), None);
        assert_eq!(
            map.conflict(Act::Jump, 2, &Binding::MouseButton(MouseButton::Right)),
            None
        );
    }

    #[test]
    fn a_map_prints_its_slots_by_action_name() {
        let text = format!("{:?}", map());
        assert!(text.starts_with("{\"move\": [Some(KeyComposite2"), "{text}");
        assert!(
            text.contains("\"pause\": [Some(Key(Escape)), None, None, None]"),
            "{text}"
        );
    }
}
