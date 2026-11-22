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

//! Gamepads: the deadzones, the device last used, and every connected pad's
//! buttons and axes, which the gamepad bindings read.
//!
//! A deadzone is the part of a stick's or trigger's travel that reads as
//! 0. A stick at rest is rarely at exactly 0, and a worn one drifts, so
//! without one a character would creep along on its own.

use nadir_core::{GamepadAxis, GamepadButton, Vec2};
use nadir_platform::GamepadId;

use crate::{AxisSign, InputError, Stick};

/// Where a gamepad's sticks and triggers start to count, and where a half
/// of an axis bound to a button presses.
/// [`Input::set_deadzones`](crate::Input::set_deadzones) sets them; every
/// pad uses the same ones.
///
/// - A stick's deadzone is radial: it is judged by how far the stick is
///   from the center, whichever way it points, so that a diagonal is not
///   cut more than a straight push. Inside `stick_inner` the stick reads
///   0; from `stick_outer` on it reads length 1, because many sticks stop
///   short of their ends; in between, the length is rescaled to go from 0
///   to 1, so that the stick can move a character slowly.
/// - A trigger reads 0 up to `trigger`, then goes from 0 to 1 over the
///   rest of its travel.
/// - A [`Binding::PadAxis`](crate::Binding::PadAxis) bound to an
///   [`ActionKind::Button`](crate::ActionKind::Button) presses once its
///   value, after the deadzone, reaches `press`, and lets go once it falls
///   below `release`. The gap keeps a value that hovers at the press point
///   from pressing and releasing over and over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Deadzones {
    /// How far from the center a stick must be to count: 0.2.
    pub stick_inner: f32,
    /// How far from the center a stick reads as length 1: 0.95.
    pub stick_outer: f32,
    /// How far a trigger must be pulled to count: 0.1.
    pub trigger: f32,
    /// Where a half of an axis bound to a button presses: 0.5.
    pub press: f32,
    /// Where it lets go again: 0.4.
    pub release: f32,
}

impl Deadzones {
    /// The deadzones every [`Input`](crate::Input) starts with.
    pub const DEFAULT: Self = Self {
        stick_inner: 0.2,
        stick_outer: 0.95,
        trigger: 0.1,
        press: 0.5,
        release: 0.4,
    };

    /// A stick's direction after its deadzone, from its raw position: its
    /// length goes from 0, at `stick_inner` from the center and inside it,
    /// to 1, at `stick_outer` and beyond, and it keeps its direction.
    ///
    /// The length is at most 1, give or take a few parts in ten million
    /// of `f32` rounding, which storing it in an
    /// [`ActionState`](crate::ActionState) takes off. A raw position that is
    /// not a number reads 0.
    pub fn stick(&self, raw: Vec2) -> Vec2 {
        let length = raw.length();
        if length.is_nan() || length <= self.stick_inner {
            return Vec2::ZERO;
        }
        let scaled = ((length - self.stick_inner) / (self.stick_outer - self.stick_inner)).min(1.0);
        // Divided first, so that a stick pushed straight to its edge reads
        // exactly 1: an axis divided by its own length is exact, where
        // multiplying by the length's reciprocal rounds twice.
        (raw / length) * scaled
    }

    /// A trigger's value after its deadzone: 0 up to `trigger`, then from 0
    /// to 1 at the end of its travel. A raw value that is not a number reads
    /// 0.
    pub fn trigger(&self, raw: f32) -> f32 {
        if raw.is_nan() || raw <= self.trigger {
            return 0.0;
        }
        ((raw - self.trigger) / (1.0 - self.trigger)).min(1.0)
    }

    /// Fails if a rule the math needs is broken. Not a number breaks every
    /// rule it is in.
    pub(crate) fn check(&self) -> Result<(), InputError> {
        let rules = [
            (
                0.0 <= self.stick_inner
                    && self.stick_inner < self.stick_outer
                    && self.stick_outer <= 1.0,
                "0 <= stick_inner < stick_outer <= 1",
            ),
            (
                0.0 <= self.trigger && self.trigger < 1.0,
                "0 <= trigger < 1",
            ),
            (
                0.0 < self.release && self.release <= self.press && self.press <= 1.0,
                "0 < release <= press <= 1",
            ),
        ];
        match rules.iter().find(|(kept, _)| !kept) {
            Some(&(_, rule)) => Err(InputError::Deadzones { rule }),
            None => Ok(()),
        }
    }
}

impl Default for Deadzones {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The device the player used last, which a game's prompts follow: "A" for
/// a gamepad, "Space" for the keyboard. See
/// [`Input::active_device`](crate::Input::active_device).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Device {
    /// The keyboard and the mouse, which are used together.
    #[default]
    KeyboardMouse,
    /// A gamepad.
    Gamepad(GamepadId),
}

/// The stick an axis belongs to, and whether it is the stick's x. `None`
/// for a trigger.
fn stick_of(axis: GamepadAxis) -> Option<(Stick, bool)> {
    match axis {
        GamepadAxis::LeftX => Some((Stick::Left, true)),
        GamepadAxis::LeftY => Some((Stick::Left, false)),
        GamepadAxis::RightX => Some((Stick::Right, true)),
        GamepadAxis::RightY => Some((Stick::Right, false)),
        GamepadAxis::LeftTrigger | GamepadAxis::RightTrigger => None,
    }
}

/// A connected pad's buttons and axes, as its events left them.
#[derive(Clone, Copy, Debug)]
struct Pad {
    id: GamepadId,
    /// Bit `n` is set while `GamepadButton::ALL[n]` is down.
    buttons: u32,
    /// Raw, in the order of `GamepadAxis::ALL`.
    axes: [f32; GamepadAxis::ALL.len()],
}

const _: () = assert!(GamepadButton::ALL.len() <= u32::BITS as usize);

impl Pad {
    fn pressed(&self, button: GamepadButton) -> bool {
        self.buttons & (1 << button.index()) != 0
    }

    fn stick(&self, stick: Stick, deadzones: &Deadzones) -> Vec2 {
        let [x, y] = stick.axes();
        deadzones.stick(Vec2::new(self.axes[x.index()], self.axes[y.index()]))
    }

    /// A half of an axis, after the deadzone, from 0 to 1.
    fn half(&self, axis: GamepadAxis, sign: AxisSign, deadzones: &Deadzones) -> f32 {
        let value = match stick_of(axis) {
            Some((stick, is_x)) => {
                let direction = self.stick(stick, deadzones);
                if is_x { direction.x } else { direction.y }
            }
            None => deadzones.trigger(self.axes[axis.index()]),
        };
        match sign {
            AxisSign::Negative => (-value).max(0.0),
            AxisSign::Positive => value.max(0.0),
        }
    }

    /// Whether the stick or trigger `axis` belongs to is past its deadzone.
    fn moved(&self, axis: GamepadAxis, deadzones: &Deadzones) -> bool {
        match stick_of(axis) {
            Some((stick, _)) => self.stick(stick, deadzones) != Vec2::ZERO,
            None => deadzones.trigger(self.axes[axis.index()]) > 0.0,
        }
    }
}

/// Every connected pad, and the deadzones they are read with.
#[derive(Clone, Debug)]
pub(crate) struct Pads {
    pads: Vec<Pad>,
    pub(crate) deadzones: Deadzones,
}

impl Pads {
    pub(crate) fn new() -> Self {
        Self {
            // Players rarely plug in more; past it, a connection allocates.
            pads: Vec::with_capacity(4),
            deadzones: Deadzones::DEFAULT,
        }
    }

    /// Adds a pad, at rest, unless it is already known.
    pub(crate) fn connect(&mut self, id: GamepadId) {
        self.pad(id);
    }

    pub(crate) fn disconnect(&mut self, id: GamepadId) {
        self.pads.retain(|pad| pad.id != id);
    }

    /// Records a button of a pad, which is added if its connection was not
    /// seen.
    pub(crate) fn set_button(&mut self, id: GamepadId, button: GamepadButton, down: bool) {
        let pad = self.pad(id);
        if down {
            pad.buttons |= 1 << button.index();
        } else {
            pad.buttons &= !(1 << button.index());
        }
    }

    /// Records an axis of a pad, which is added if its connection was not
    /// seen. Returns whether the stick or trigger it belongs to is now past
    /// its deadzone.
    pub(crate) fn set_axis(&mut self, id: GamepadId, axis: GamepadAxis, value: f32) -> bool {
        let deadzones = self.deadzones;
        let pad = self.pad(id);
        pad.axes[axis.index()] = value;
        pad.moved(axis, &deadzones)
    }

    /// Whether `button` is down on any pad.
    pub(crate) fn pressed(&self, button: GamepadButton) -> bool {
        self.pads.iter().any(|pad| pad.pressed(button))
    }

    /// A half of an axis, after the deadzone: the largest on any pad.
    pub(crate) fn half(&self, axis: GamepadAxis, sign: AxisSign) -> f32 {
        self.pads
            .iter()
            .map(|pad| pad.half(axis, sign, &self.deadzones))
            .fold(0.0, f32::max)
    }

    /// A stick's direction, after the deadzone: the longest on any pad, the
    /// first pad's of equal ones.
    pub(crate) fn stick(&self, stick: Stick) -> Vec2 {
        let mut longest = Vec2::ZERO;
        for pad in &self.pads {
            let direction = pad.stick(stick, &self.deadzones);
            if direction.length_squared() > longest.length_squared() {
                longest = direction;
            }
        }
        longest
    }

    fn pad(&mut self, id: GamepadId) -> &mut Pad {
        let at = match self.pads.iter().position(|pad| pad.id == id) {
            Some(at) => at,
            None => {
                self.pads.push(Pad {
                    id,
                    buttons: 0,
                    axes: [0.0; GamepadAxis::ALL.len()],
                });
                self.pads.len() - 1
            }
        };
        &mut self.pads[at]
    }
}

/// The stick or trigger an axis belongs to, as the codes it drives: for a
/// stick, its two axes; for a trigger, the trigger alone.
pub(crate) fn group(axis: GamepadAxis) -> (Option<Stick>, &'static [GamepadAxis]) {
    match stick_of(axis) {
        Some((Stick::Left, _)) => (Some(Stick::Left), &[GamepadAxis::LeftX, GamepadAxis::LeftY]),
        Some((Stick::Right, _)) => (
            Some(Stick::Right),
            &[GamepadAxis::RightX, GamepadAxis::RightY],
        ),
        None if axis == GamepadAxis::LeftTrigger => (None, &[GamepadAxis::LeftTrigger]),
        None => (None, &[GamepadAxis::RightTrigger]),
    }
}

#[cfg(test)]
mod tests {
    use nadir_core::testing::check;

    use super::*;

    const DZ: Deadzones = Deadzones::DEFAULT;

    #[test]
    fn a_stick_reads_0_inside_its_deadzone_and_1_from_its_outer_edge() {
        assert_eq!(DZ.stick(Vec2::ZERO), Vec2::ZERO);
        assert_eq!(DZ.stick(Vec2::new(0.2, 0.0)), Vec2::ZERO);
        assert_eq!(DZ.stick(Vec2::new(0.1, -0.1)), Vec2::ZERO);
        assert_eq!(DZ.stick(Vec2::new(0.0, -0.95)), Vec2::new(0.0, -1.0));
        assert_eq!(DZ.stick(Vec2::new(-1.0, 0.0)), Vec2::new(-1.0, 0.0));
        // Halfway between the edges reads half.
        let half = DZ.stick(Vec2::new(0.575, 0.0));
        assert!((half.x - 0.5).abs() < 1e-6, "{half:?}");
        // The pad of roadmap task M8.1's check: its left stick reached
        // -0.97 up, which now reads as all the way.
        assert_eq!(DZ.stick(Vec2::new(0.0, -0.9688)), Vec2::new(0.0, -1.0));
        // A diagonal keeps its direction.
        let diagonal = DZ.stick(Vec2::new(0.5, 0.5));
        assert!((diagonal.x - diagonal.y).abs() < 1e-7, "{diagonal:?}");
        assert_eq!(DZ.stick(Vec2::new(f32::NAN, 0.5)), Vec2::ZERO);
    }

    #[test]
    fn a_trigger_reads_0_up_to_its_deadzone_then_0_to_1() {
        assert_eq!(DZ.trigger(0.0), 0.0);
        assert_eq!(DZ.trigger(0.1), 0.0);
        assert_eq!(DZ.trigger(1.0), 1.0);
        assert!((DZ.trigger(0.55) - 0.5).abs() < 1e-6);
        assert_eq!(DZ.trigger(f32::NAN), 0.0);
        assert_eq!(DZ.trigger(-1.0), 0.0);
    }

    /// The spec's deadzone property test (`docs/subsystems/input.md`,
    /// "Tests"): the output is in 0..1 and grows with the input.
    #[test]
    fn deadzones_give_0_to_1_and_grow_with_the_input() {
        check(
            "stick and trigger deadzones",
            2000,
            |rng| {
                let inner = rng.range(0..90u32) as f32 / 100.0;
                let outer = inner + (rng.range(1..101u32) as f32 / 100.0) * (1.0 - inner);
                let deadzones = Deadzones {
                    stick_inner: inner,
                    stick_outer: outer.min(1.0),
                    trigger: rng.range(0..100u32) as f32 / 100.0,
                    ..Deadzones::DEFAULT
                };
                let angle_x = rng.range(0..2001u32) as f32 / 1000.0 - 1.0;
                let angle_y = rng.range(0..2001u32) as f32 / 1000.0 - 1.0;
                let direction = Vec2::new(angle_x, angle_y).normalize_or_zero();
                let a = rng.range(0..1501u32) as f32 / 1000.0;
                let b = rng.range(0..1501u32) as f32 / 1000.0;
                (deadzones, direction, a.min(b), a.max(b))
            },
            |&(deadzones, direction, small, large)| {
                if deadzones.check().is_err() {
                    return Ok(());
                }
                let (low, high) = (
                    deadzones.stick(direction * small).length(),
                    deadzones.stick(direction * large).length(),
                );
                if !(0.0..=1.0 + 1e-6).contains(&low) || !(0.0..=1.0 + 1e-6).contains(&high) {
                    return Err(format!("a stick's length {low} or {high} is out of 0..1"));
                }
                if low > high + 1e-6 {
                    return Err(format!("{small} gave {low}, but {large} gave {high}"));
                }
                let (low, high) = (deadzones.trigger(small), deadzones.trigger(large));
                if !(0.0..=1.0).contains(&low) || !(0.0..=1.0).contains(&high) || low > high {
                    return Err(format!(
                        "a trigger gave {low} for {small}, {high} for {large}"
                    ));
                }
                Ok(())
            },
        );
    }

    #[test]
    fn deadzones_out_of_order_or_range_are_refused() {
        assert_eq!(DZ.check(), Ok(()));
        let broken = [
            (
                Deadzones {
                    stick_inner: 0.5,
                    stick_outer: 0.5,
                    ..DZ
                },
                "0 <= stick_inner < stick_outer <= 1",
            ),
            (
                Deadzones {
                    stick_outer: 1.5,
                    ..DZ
                },
                "0 <= stick_inner < stick_outer <= 1",
            ),
            (Deadzones { trigger: 1.0, ..DZ }, "0 <= trigger < 1"),
            (
                Deadzones {
                    trigger: f32::NAN,
                    ..DZ
                },
                "0 <= trigger < 1",
            ),
            (
                Deadzones { release: 0.6, ..DZ },
                "0 < release <= press <= 1",
            ),
            (
                Deadzones { release: 0.0, ..DZ },
                "0 < release <= press <= 1",
            ),
        ];
        for (deadzones, rule) in broken {
            assert_eq!(
                deadzones.check(),
                Err(InputError::Deadzones { rule }),
                "{deadzones:?}"
            );
        }
    }

    #[test]
    fn pads_merge_by_the_most_pushed() {
        let (one, two) = (GamepadId::from_raw(1), GamepadId::from_raw(2));
        let mut pads = Pads::new();
        pads.connect(one);
        assert!(
            !pads.set_axis(one, GamepadAxis::LeftX, 0.1),
            "in the deadzone"
        );
        assert!(
            pads.set_axis(two, GamepadAxis::LeftX, -1.0),
            "added, and moved"
        );
        assert!(pads.set_axis(one, GamepadAxis::RightTrigger, 1.0));
        assert_eq!(pads.stick(Stick::Left), Vec2::new(-1.0, 0.0));
        assert_eq!(pads.half(GamepadAxis::LeftX, AxisSign::Negative), 1.0);
        assert_eq!(pads.half(GamepadAxis::LeftX, AxisSign::Positive), 0.0);
        assert_eq!(
            pads.half(GamepadAxis::RightTrigger, AxisSign::Positive),
            1.0
        );
        assert_eq!(
            pads.half(GamepadAxis::RightTrigger, AxisSign::Negative),
            0.0
        );
        pads.set_button(one, GamepadButton::A, true);
        pads.set_button(two, GamepadButton::A, true);
        pads.set_button(one, GamepadButton::A, false);
        assert!(pads.pressed(GamepadButton::A), "still down on the other");
        pads.disconnect(two);
        assert!(!pads.pressed(GamepadButton::A));
        assert_eq!(pads.stick(Stick::Left), Vec2::ZERO);
        // A stick pushed up, a little to the side, stays below the press
        // point to the side.
        pads.set_axis(one, GamepadAxis::LeftY, -1.0);
        pads.set_axis(one, GamepadAxis::LeftX, 0.15);
        assert!(pads.half(GamepadAxis::LeftY, AxisSign::Negative) > 0.9);
        assert!(pads.half(GamepadAxis::LeftX, AxisSign::Positive) < 0.2);
    }
}
