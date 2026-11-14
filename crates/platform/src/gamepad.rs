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

//! Gamepads, on every system: which pad an event is of, and what a pad's
//! buttons and axes are now.
//!
//! Each system's backend keeps a [`GamepadState`] for each pad, and changes
//! it only through the methods here, which report each change as an
//! [`Event`]: so the events always add up to the state.

use nadir_core::{GamepadAxis, GamepadButton};

use crate::Event;

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
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
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
        let mut state = GamepadState::new("pad".to_string());
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
        assert!(!state.at_rest());
    }

    #[test]
    fn axes_are_clamped_to_their_ranges() {
        let mut state = GamepadState::new(String::new());
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
        let mut state = GamepadState::new(String::new());
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
