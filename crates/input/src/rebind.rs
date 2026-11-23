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

//! Rebinding: a controls menu asks [`Input`](crate::Input) for the next
//! press, which becomes a binding ([`Input::begin_rebind`]).
//!
//! [`Input::begin_rebind`]: crate::Input::begin_rebind

use nadir_core::{GamepadAxis, Key};

use crate::{ActionKind, Binding, Stick};

/// What a rebinding heard: see
/// [`Input::begin_rebind`](crate::Input::begin_rebind).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rebound {
    /// The binding the press makes, of the kind the rebinding asked for.
    Bound(Binding),
    /// Escape was pressed: the binding stays as it was.
    Canceled,
}

/// What a press heard while a rebinding for an action of `kind` listens
/// makes of it: `None` if it does not count.
///
/// The press is given as the binding of the one key or button it is: a
/// [`Binding::Key`], [`Binding::MouseButton`] or [`Binding::PadButton`], or
/// a [`Binding::PadAxis`] for a half of an axis that reached the press
/// point.
pub(crate) fn heard(kind: ActionKind, press: Binding) -> Option<Rebound> {
    match press {
        Binding::Key(Key::Escape) => Some(Rebound::Canceled),
        // Held with other keys; never a binding of its own here.
        Binding::Key(key) if is_modifier(key) => None,
        Binding::Key(_) | Binding::MouseButton(_) | Binding::PadButton(_) => {
            (kind != ActionKind::Axis2).then_some(Rebound::Bound(press))
        }
        Binding::PadAxis { axis, .. } => match kind {
            ActionKind::Axis2 => stick(axis).map(|stick| Rebound::Bound(Binding::PadStick(stick))),
            ActionKind::Button | ActionKind::Axis1 => Some(Rebound::Bound(press)),
        },
        Binding::KeyComposite2 { .. } | Binding::PadStick(_) => None,
    }
}

/// Shift, Control, Alt and Super, left and right: usages `0xE0` to `0xE7`.
fn is_modifier(key: Key) -> bool {
    (0xE0..=0xE7).contains(&key.usage())
}

/// The stick `axis` is one of, or `None` for a trigger.
fn stick(axis: GamepadAxis) -> Option<Stick> {
    Stick::ALL
        .iter()
        .copied()
        .find(|stick| stick.axes().contains(&axis))
}

#[cfg(test)]
mod tests {
    use nadir_core::{GamepadButton, MouseButton};

    use super::*;
    use crate::AxisSign;

    const ONE: [ActionKind; 2] = [ActionKind::Button, ActionKind::Axis1];

    fn half(axis: GamepadAxis, sign: AxisSign) -> Binding {
        Binding::PadAxis { axis, sign }
    }

    #[test]
    fn a_key_a_mouse_button_or_a_pad_button_binds_a_button_or_an_axis1() {
        for kind in ONE {
            for press in [
                Binding::Key(Key::Space),
                Binding::Key(Key::Enter),
                Binding::MouseButton(MouseButton::Right),
                Binding::PadButton(GamepadButton::Y),
            ] {
                assert_eq!(heard(kind, press), Some(Rebound::Bound(press)), "{kind:?}");
            }
        }
    }

    #[test]
    fn a_half_of_an_axis_binds_itself_or_its_stick() {
        let right = half(GamepadAxis::RightX, AxisSign::Positive);
        let trigger = half(GamepadAxis::LeftTrigger, AxisSign::Positive);
        for kind in ONE {
            assert_eq!(heard(kind, right), Some(Rebound::Bound(right)));
            assert_eq!(heard(kind, trigger), Some(Rebound::Bound(trigger)));
        }
        let up = half(GamepadAxis::LeftY, AxisSign::Negative);
        assert_eq!(
            heard(ActionKind::Axis2, up),
            Some(Rebound::Bound(Binding::PadStick(Stick::Left)))
        );
        assert_eq!(
            heard(ActionKind::Axis2, right),
            Some(Rebound::Bound(Binding::PadStick(Stick::Right)))
        );
        assert_eq!(heard(ActionKind::Axis2, trigger), None);
    }

    #[test]
    fn keys_and_buttons_cannot_bind_a_direction() {
        for press in [
            Binding::Key(Key::W),
            Binding::MouseButton(MouseButton::Left),
            Binding::PadButton(GamepadButton::DPadUp),
        ] {
            assert_eq!(heard(ActionKind::Axis2, press), None);
        }
    }

    #[test]
    fn escape_cancels_and_modifiers_are_skipped_for_every_kind() {
        let modifiers = Key::ALL.iter().filter(|key| is_modifier(**key));
        assert_eq!(modifiers.clone().count(), 8);
        for kind in [ActionKind::Button, ActionKind::Axis1, ActionKind::Axis2] {
            assert_eq!(
                heard(kind, Binding::Key(Key::Escape)),
                Some(Rebound::Canceled)
            );
            for &key in modifiers.clone() {
                assert_eq!(heard(kind, Binding::Key(key)), None, "{key:?}");
            }
        }
    }
}
