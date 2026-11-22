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

//! Bindings: which keys and buttons drive which actions.

use std::fmt;
use std::marker::PhantomData;

use nadir_core::{GamepadAxis, GamepadButton, Key, MouseButton};

use crate::code::{Code, Inputs};
use crate::{ActionKind, ActionSet, BindErrorKind, InputError, action};

/// One of a gamepad's two sticks, for [`Binding::PadStick`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stick {
    /// The left stick: [`GamepadAxis::LeftX`] and [`GamepadAxis::LeftY`].
    Left,
    /// The right stick: [`GamepadAxis::RightX`] and [`GamepadAxis::RightY`].
    Right,
}

impl Stick {
    /// Both sticks, in order.
    pub const ALL: &'static [Stick] = &[Stick::Left, Stick::Right];

    /// The stick's axes: its x, then its y.
    pub const fn axes(self) -> [GamepadAxis; 2] {
        match self {
            Stick::Left => [GamepadAxis::LeftX, GamepadAxis::LeftY],
            Stick::Right => [GamepadAxis::RightX, GamepadAxis::RightY],
        }
    }
}

/// Which half of a gamepad's axis a [`Binding::PadAxis`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AxisSign {
    /// Toward -1: a stick pushed left or up.
    Negative,
    /// Toward 1: a stick pushed right or down, or a trigger pulled.
    Positive,
}

/// A key or button, or a group of them, that drives an action.
///
/// The gamepad bindings read every pad that is connected, as one: a button
/// is down while it is down on any pad.
/// [`Input::set_deadzones`](crate::Input::set_deadzones) sets where a pad's
/// sticks and triggers start to count.
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
    /// A gamepad's button, as for [`Binding::Key`].
    PadButton(GamepadButton),
    /// One half of a gamepad's axis: a stick pushed one way, or a trigger
    /// pulled. A trigger has only its positive half.
    ///
    /// - For an [`ActionKind::Button`], it presses once its value reaches
    ///   the deadzones' press point, and lets go once it falls below their
    ///   release point, a little lower, so that a value that hovers at the
    ///   press point does not press and release over and over.
    /// - For an [`ActionKind::Axis1`], its value counts past the deadzone,
    ///   rescaled to go from 0 to 1.
    ///
    /// A stick's half is its direction's x or y, after the stick's radial
    /// deadzone, so pushing a stick up does not also move it left.
    PadAxis {
        /// The axis.
        axis: GamepadAxis,
        /// Which half of it.
        sign: AxisSign,
    },
    /// A gamepad's stick, for an [`ActionKind::Axis2`]: its direction past
    /// the radial deadzone, rescaled so that it goes from length 0 to 1.
    /// +Y is down, as it is for the pad's axes.
    PadStick(Stick),
}

impl Binding {
    /// The inputs the binding reads, for finding conflicts: what a player
    /// would call the same key or button. For a composite: up, down, left,
    /// right; a stick reads each half of its axes, as buttons.
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
            Self::PadButton(button) => Inputs::one(Code::pad_button(button)),
            Self::PadAxis { axis, sign } => Inputs::one(Code::pad_press(axis, sign)),
            Self::PadStick(stick) => {
                let [x, y] = stick.axes();
                Inputs::four([
                    Code::pad_press(y, AxisSign::Negative),
                    Code::pad_press(y, AxisSign::Positive),
                    Code::pad_press(x, AxisSign::Negative),
                    Code::pad_press(x, AxisSign::Positive),
                ])
            }
        }
    }

    /// The inputs whose going down and up make an action of this kind go
    /// down and up, when the binding drives it.
    pub(crate) fn latch_inputs(&self, kind: ActionKind) -> Inputs {
        match *self {
            Self::PadAxis { axis, sign } if kind == ActionKind::Axis1 => {
                Inputs::one(Code::pad_active(axis, sign))
            }
            Self::PadStick(stick) => Inputs::one(Code::stick(stick)),
            _ => self.inputs(),
        }
    }

    /// Whether the binding can drive an action of this kind.
    fn fits(&self, kind: ActionKind) -> bool {
        match self {
            Self::Key(_) | Self::MouseButton(_) | Self::PadButton(_) | Self::PadAxis { .. } => {
                kind != ActionKind::Axis2
            }
            Self::KeyComposite2 { .. } | Self::PadStick(_) => kind == ActionKind::Axis2,
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
/// An action is held while any of its bound keys and buttons is down, or a
/// bound stick or trigger is past its deadzone (past its press point, for
/// an [`ActionKind::Button`]). A [`ActionKind::Button`] is worth 1 while
/// held. An [`ActionKind::Axis1`] is worth 1 while a key or button holds
/// it, and otherwise what its halves of axes give. An [`ActionKind::Axis2`]
/// is the sum of its composites' and sticks' directions, no longer than 1.
/// [`Input::latch`](crate::Input::latch) has the details.
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
    /// does not exist, the binding cannot drive an action of that kind, or
    /// it is the negative half of a trigger, which never moves.
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
        if let Binding::PadAxis {
            axis,
            sign: AxisSign::Negative,
        } = binding
            && axis.is_trigger()
        {
            return fail(BindErrorKind::NoNegativeHalf { axis });
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
    fn pad_bindings_fit_their_kinds() {
        let mut map = map();
        let stick = Binding::PadStick(Stick::Left);
        let up = Binding::PadAxis {
            axis: GamepadAxis::LeftY,
            sign: AxisSign::Negative,
        };
        let trigger = Binding::PadAxis {
            axis: GamepadAxis::RightTrigger,
            sign: AxisSign::Positive,
        };
        assert_eq!(map.bind(Act::Move, 1, stick), Ok(None));
        assert_eq!(
            map.bind(Act::Jump, 2, Binding::PadButton(GamepadButton::A)),
            Ok(None)
        );
        assert_eq!(map.bind(Act::Jump, 3, up), Ok(None));
        assert_eq!(map.bind(Act::Throttle, 0, trigger), Ok(None));
        let refused = |binding, action, slot, kind| {
            Err(InputError::Bind {
                action,
                slot,
                kind: BindErrorKind::WrongKind { binding, kind },
            })
        };
        assert_eq!(
            map.bind(Act::Pause, 2, stick),
            refused(stick, "pause", 2, ActionKind::Button)
        );
        assert_eq!(
            map.bind(Act::Move, 2, up),
            refused(up, "move", 2, ActionKind::Axis2)
        );
        assert_eq!(
            map.bind(
                Act::Throttle,
                1,
                Binding::PadAxis {
                    axis: GamepadAxis::LeftTrigger,
                    sign: AxisSign::Negative,
                }
            ),
            Err(InputError::Bind {
                action: "throttle",
                slot: 1,
                kind: BindErrorKind::NoNegativeHalf {
                    axis: GamepadAxis::LeftTrigger
                },
            })
        );
        assert_eq!(map.bindings(Act::Throttle)[1], None, "nothing changed");
    }

    #[test]
    fn a_stick_conflicts_with_the_halves_of_its_axes() {
        let mut map = map();
        map.bind(Act::Move, 1, Binding::PadStick(Stick::Left))
            .expect("fits");
        map.bind(Act::Jump, 2, Binding::PadButton(GamepadButton::A))
            .expect("fits");
        let half = |axis, sign| Binding::PadAxis { axis, sign };
        assert_eq!(
            map.conflict(Act::Pause, 1, &half(GamepadAxis::LeftX, AxisSign::Positive)),
            Some((Act::Move, 1))
        );
        assert_eq!(
            map.conflict(
                Act::Pause,
                1,
                &half(GamepadAxis::RightX, AxisSign::Positive)
            ),
            None
        );
        assert_eq!(
            map.conflict(Act::Pause, 1, &Binding::PadButton(GamepadButton::A)),
            Some((Act::Jump, 2))
        );
        assert_eq!(
            map.conflict(Act::Pause, 1, &Binding::PadButton(GamepadButton::B)),
            None
        );
        // A pad's button shares nothing with a key, nor a stick with WASD.
        assert_eq!(map.conflict(Act::Pause, 1, &Binding::Key(Key::B)), None);
        assert_eq!(
            map.conflict(Act::Move, 2, &Binding::PadStick(Stick::Right)),
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
