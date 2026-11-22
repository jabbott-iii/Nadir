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

//! Gamepads, on every system: which pad an event is of, which family of pad
//! it is, and what its buttons and axes are now.
//!
//! Each system's backend keeps a [`GamepadState`] for each pad, and changes
//! it only through the methods here, which report each change as an
//! [`Event`]: so the events always add up to the state.

use nadir_core::{GamepadAxis, GamepadButton};

use crate::Event;

/// Microsoft's USB vendor id.
const MICROSOFT: u16 = 0x045e;

/// Sony's USB vendor id.
const SONY: u16 = 0x054c;

/// Nintendo's USB vendor id.
const NINTENDO: u16 = 0x057e;

/// Which maker's labels a gamepad's buttons carry, for a game's button
/// prompts.
///
/// A [`GamepadButton`] names a place on the pad, not what is printed there:
/// [`GamepadButton::A`] is the bottom face button on every pad, which an
/// Xbox pad labels A, a PlayStation pad ✕ and a Nintendo pad B. A prompt
/// that tells the player which button to press needs the pad's family to
/// pick the label. [`GamepadState::family`] gives it.
///
/// More families may come, so a `match` on one needs a `_` arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum GamepadFamily {
    /// Microsoft's pads, and the pads other makers build for the Xbox:
    /// A, B, X and Y on the face buttons, LB and RB, LT and RT.
    Xbox,
    /// Sony's pads: ✕, ○, □ and △, L1 and R1, L2 and R2.
    PlayStation,
    /// Nintendo's pads, and the pads other makers build for the Switch: B,
    /// A, Y and X in the places of an Xbox pad's A, B, X and Y (so its A is
    /// the right button, not the bottom one), L and R, ZL and ZR.
    Nintendo,
    /// Any other pad, or one whose family the system cannot tell.
    Other,
}

impl GamepadFamily {
    /// Every family, in order.
    pub const ALL: &'static [GamepadFamily] = &[
        GamepadFamily::Xbox,
        GamepadFamily::PlayStation,
        GamepadFamily::Nintendo,
        GamepadFamily::Other,
    ];

    /// The family's place in [`GamepadFamily::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The family's name, as its variant is spelled: `"Xbox"`,
    /// `"PlayStation"`, `"Nintendo"` or `"Other"`.
    pub const fn name(self) -> &'static str {
        match self {
            GamepadFamily::Xbox => "Xbox",
            GamepadFamily::PlayStation => "PlayStation",
            GamepadFamily::Nintendo => "Nintendo",
            GamepadFamily::Other => "Other",
        }
    }

    /// The family that a pad's USB vendor id says: Microsoft's, Sony's or
    /// Nintendo's, and [`GamepadFamily::Other`] for every other maker,
    /// whose pads may be of any family.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn from_vendor(vendor: u16) -> Self {
        match vendor {
            MICROSOFT => GamepadFamily::Xbox,
            SONY => GamepadFamily::PlayStation,
            NINTENDO => GamepadFamily::Nintendo,
            _ => GamepadFamily::Other,
        }
    }
}

/// Which gamepad an event is of.
///
/// [`Platform::poll_gamepads`](crate::Platform::poll_gamepads) numbers the
/// pads from 1, in the order they connect, and never gives a number twice:
/// a pad unplugged and plugged in again is a new pad, with a new id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GamepadId(u32);

impl GamepadId {
    /// The id whose number is `raw`: for tests and replays, which make
    /// events of their own.
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// The id's number.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// What a gamepad's buttons and axes are now, as the events reported so far
/// leave them: see
/// [`Platform::gamepad_state`](crate::Platform::gamepad_state).
#[derive(Clone, Debug, PartialEq)]
pub struct GamepadState {
    name: String,
    family: GamepadFamily,
    /// Bit `n` is set while `GamepadButton::ALL[n]` is down.
    buttons: u32,
    /// In the order of `GamepadAxis::ALL`.
    axes: [f32; GamepadAxis::ALL.len()],
}

// Every button has a bit.
const _: () = assert!(GamepadButton::ALL.len() <= u32::BITS as usize);

impl GamepadState {
    /// The name the pad gives itself, such as "Microsoft X-Box 360 pad".
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The pad's family, which says how its buttons are labeled. It does
    /// not change while the pad is plugged in.
    ///
    /// - On Linux, it comes from the pad's driver if the driver is one
    ///   family's: `xpad` and `hid-microsoft` are Xbox's, `hid-playstation`
    ///   and `hid-sony` PlayStation's, `hid-nintendo` Nintendo's. Those
    ///   drivers also take pads by other makers, such as PDP's and PowerA's
    ///   Xbox pads (`xpad`) and Hori's Switch pads (`hid-nintendo`).
    /// - Otherwise it comes from the pad's USB vendor id: Microsoft's,
    ///   Sony's or Nintendo's. Any other pad is
    ///   [`GamepadFamily::Other`].
    pub fn family(&self) -> GamepadFamily {
        self.family
    }

    /// Whether the button is down.
    pub fn pressed(&self, button: GamepadButton) -> bool {
        self.buttons & (1 << button.index()) != 0
    }

    /// The axis's value: -1 to 1 for a stick's axis, with +Y down, and 0 to
    /// 1 for a trigger. Raw: no dead zone is taken off.
    pub fn axis(&self, axis: GamepadAxis) -> f32 {
        self.axes[axis.index()]
    }
}

// The backends' side. Only Linux has pads yet; Windows gets them with M8.2
// and macOS with M8.3, and calls these then.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl GamepadState {
    /// A pad at rest: no button down, every axis at 0.
    pub(crate) fn new(name: String, family: GamepadFamily) -> Self {
        Self {
            name,
            family,
            buttons: 0,
            axes: [0.0; GamepadAxis::ALL.len()],
        }
    }

    /// Whether every button is up and every axis at 0.
    #[cfg(test)]
    pub(crate) fn at_rest(&self) -> bool {
        self.buttons == 0 && self.axes.iter().all(|&value| value == 0.0)
    }

    /// Sets the button, and reports it if it changed.
    pub(crate) fn set_button(
        &mut self,
        id: GamepadId,
        button: GamepadButton,
        pressed: bool,
        out: &mut Vec<Event>,
    ) {
        if self.pressed(button) == pressed {
            return;
        }
        self.buttons ^= 1 << button.index();
        out.push(Event::GamepadButton {
            id,
            button,
            pressed,
        });
    }

    /// Sets the axis, clamped to its range, and reports it if it changed.
    /// A value that is not a number is taken as 0.
    pub(crate) fn set_axis(
        &mut self,
        id: GamepadId,
        axis: GamepadAxis,
        value: f32,
        out: &mut Vec<Event>,
    ) {
        let low = if axis.is_trigger() { 0.0 } else { -1.0 };
        let value = if value.is_nan() {
            0.0
        } else {
            value.clamp(low, 1.0)
        };
        let slot = &mut self.axes[axis.index()];
        if *slot == value {
            return;
        }
        *slot = value;
        out.push(Event::GamepadAxis { id, axis, value });
    }

    /// Puts the pad at rest, reporting each button that goes up and each
    /// axis that goes back to 0: before the pad is reported disconnected.
    pub(crate) fn rest(&mut self, id: GamepadId, out: &mut Vec<Event>) {
        for &button in GamepadButton::ALL {
            self.set_button(id, button, false, out);
        }
        for &axis in GamepadAxis::ALL {
            self.set_axis(id, axis, 0.0, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAD: GamepadId = GamepadId::from_raw(3);

    #[test]
    fn an_id_keeps_its_number() {
        assert_eq!(PAD.raw(), 3);
        assert!(GamepadId::from_raw(1) < GamepadId::from_raw(2));
    }

    #[test]
    fn only_changes_are_reported() {
        let mut state = GamepadState::new("pad".to_string(), GamepadFamily::Xbox);
        assert!(state.at_rest());
        let mut out = Vec::new();
        state.set_button(PAD, GamepadButton::A, true, &mut out);
        state.set_button(PAD, GamepadButton::A, true, &mut out);
        state.set_axis(PAD, GamepadAxis::LeftX, 0.5, &mut out);
        state.set_axis(PAD, GamepadAxis::LeftX, 0.5, &mut out);
        state.set_button(PAD, GamepadButton::Y, false, &mut out);
        assert_eq!(
            out,
            [
                Event::GamepadButton {
                    id: PAD,
                    button: GamepadButton::A,
                    pressed: true
                },
                Event::GamepadAxis {
                    id: PAD,
                    axis: GamepadAxis::LeftX,
                    value: 0.5
                },
            ]
        );
        assert!(state.pressed(GamepadButton::A));
        assert!(!state.pressed(GamepadButton::B));
        assert_eq!(state.axis(GamepadAxis::LeftX), 0.5);
        assert_eq!(state.name(), "pad");
        assert_eq!(state.family(), GamepadFamily::Xbox);
        assert!(!state.at_rest());
    }

    #[test]
    fn families_are_in_order_and_named_as_spelled() {
        for (index, &family) in GamepadFamily::ALL.iter().enumerate() {
            assert_eq!(family.index(), index);
            assert_eq!(family.name(), format!("{family:?}"));
        }
    }

    #[test]
    fn three_vendor_ids_name_a_family() {
        assert_eq!(GamepadFamily::from_vendor(0x045e), GamepadFamily::Xbox);
        assert_eq!(
            GamepadFamily::from_vendor(0x054c),
            GamepadFamily::PlayStation
        );
        assert_eq!(GamepadFamily::from_vendor(0x057e), GamepadFamily::Nintendo);
        // PDP, Hori and Valve make pads of every family, or of none.
        for vendor in [0x0e6f, 0x0f0d, 0x28de, 0x0000, 0xffff] {
            assert_eq!(
                GamepadFamily::from_vendor(vendor),
                GamepadFamily::Other,
                "{vendor:04x}"
            );
        }
    }

    #[test]
    fn axes_are_clamped_to_their_ranges() {
        let mut state = GamepadState::new(String::new(), GamepadFamily::Other);
        let mut out = Vec::new();
        state.set_axis(PAD, GamepadAxis::RightY, -3.0, &mut out);
        state.set_axis(PAD, GamepadAxis::LeftTrigger, -0.5, &mut out);
        state.set_axis(PAD, GamepadAxis::RightTrigger, 2.0, &mut out);
        state.set_axis(PAD, GamepadAxis::LeftY, f32::NAN, &mut out);
        assert_eq!(state.axis(GamepadAxis::RightY), -1.0);
        // A trigger does not go below 0, so this one did not move.
        assert_eq!(state.axis(GamepadAxis::LeftTrigger), 0.0);
        assert_eq!(state.axis(GamepadAxis::RightTrigger), 1.0);
        assert_eq!(state.axis(GamepadAxis::LeftY), 0.0);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn rest_reports_what_goes_back() {
        let mut state = GamepadState::new(String::new(), GamepadFamily::Other);
        let mut out = Vec::new();
        state.set_button(PAD, GamepadButton::DPadLeft, true, &mut out);
        state.set_axis(PAD, GamepadAxis::RightTrigger, 0.25, &mut out);
        out.clear();
        state.rest(PAD, &mut out);
        assert_eq!(
            out,
            [
                Event::GamepadButton {
                    id: PAD,
                    button: GamepadButton::DPadLeft,
                    pressed: false
                },
                Event::GamepadAxis {
                    id: PAD,
                    axis: GamepadAxis::RightTrigger,
                    value: 0.0
                },
            ]
        );
        assert!(state.at_rest());
    }
}
