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

//! Keys, mouse buttons and the gamepads' inputs as small numbers, and sets
//! of them.

use nadir_core::{GamepadAxis, GamepadButton, Key, MouseButton};

use crate::{AxisSign, Stick};

/// A key, a mouse button or a gamepad's input as a number, which latching
/// sees go down and up:
///
/// - a key's HID usage, below `0x100`;
/// - `0x100` plus a mouse button's position in [`MouseButton::ALL`];
/// - from `0x140`, the gamepads': a button, a half of an axis past the
///   press point, a half of an axis past its deadzone, and a stick out of
///   its deadzone. Each is down while it is down on any pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Code(u16);

/// Where the mouse buttons' codes start.
const MOUSE: u16 = 0x100;
/// Where the gamepads' buttons' codes start: one 64-bit word of their own.
const PAD_BUTTON: u16 = 0x140;
/// Where the codes of the halves of the axes, past the press point, start.
const PAD_PRESS: u16 = 0x150;
/// Where the codes of the halves of the axes, past their deadzone, start.
const PAD_ACTIVE: u16 = 0x160;
/// Where the codes of the sticks, out of their deadzone, start.
const STICK: u16 = 0x170;

impl Code {
    /// The code of a key.
    pub(crate) const fn key(key: Key) -> Self {
        Self(key.usage() as u16)
    }

    /// The code of a mouse button.
    pub(crate) const fn mouse(button: MouseButton) -> Self {
        Self(MOUSE + button as u16)
    }

    /// The code of a gamepad's button, down while it is down on any pad.
    pub(crate) const fn pad_button(button: GamepadButton) -> Self {
        Self(PAD_BUTTON + button.index() as u16)
    }

    /// The code of a half of an axis as a button: down from the press point
    /// until it falls below the release point, on any pad.
    pub(crate) const fn pad_press(axis: GamepadAxis, sign: AxisSign) -> Self {
        Self(PAD_PRESS + half(axis, sign))
    }

    /// The code of a half of an axis as an
    /// [`ActionKind::Axis1`](crate::ActionKind::Axis1): down while it is
    /// past its deadzone, on any pad.
    pub(crate) const fn pad_active(axis: GamepadAxis, sign: AxisSign) -> Self {
        Self(PAD_ACTIVE + half(axis, sign))
    }

    /// The code of a stick: down while it is out of its deadzone, on any
    /// pad.
    pub(crate) const fn stick(stick: Stick) -> Self {
        Self(STICK + stick as u16)
    }

    const fn word_and_bit(self) -> (usize, u64) {
        ((self.0 / 64) as usize, 1 << (self.0 % 64))
    }
}

/// A half of an axis as a number from 0 to 11: two per axis.
const fn half(axis: GamepadAxis, sign: AxisSign) -> u16 {
    axis.index() as u16 * 2 + sign as u16
}

/// Enough 64-bit words for every code: the last stick's is `0x171`.
const WORDS: usize = 6;

/// A set of codes, with no allocation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CodeSet([u64; WORDS]);

impl CodeSet {
    /// Every key's code: those below [`MOUSE`].
    pub(crate) const KEYS: Self = Self([u64::MAX, u64::MAX, u64::MAX, u64::MAX, 0, 0]);

    /// Every mouse button's code: those from [`MOUSE`] to the pads'.
    pub(crate) const MOUSE: Self = Self([0, 0, 0, 0, u64::MAX, 0]);

    /// Every gamepad's code: those from [`PAD_BUTTON`] on.
    pub(crate) const PADS: Self = Self([0, 0, 0, 0, 0, u64::MAX]);

    /// The codes in either set.
    pub(crate) fn union(self, other: Self) -> Self {
        Self(core::array::from_fn(|word| self.0[word] | other.0[word]))
    }

    /// The codes in both sets.
    pub(crate) fn intersection(self, other: Self) -> Self {
        Self(core::array::from_fn(|word| self.0[word] & other.0[word]))
    }

    pub(crate) fn contains(&self, code: Code) -> bool {
        let (word, bit) = code.word_and_bit();
        self.0[word] & bit != 0
    }

    pub(crate) fn insert(&mut self, code: Code) {
        let (word, bit) = code.word_and_bit();
        self.0[word] |= bit;
    }

    pub(crate) fn remove(&mut self, code: Code) {
        let (word, bit) = code.word_and_bit();
        self.0[word] &= !bit;
    }

    /// Every code in the set, in order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = Code> + '_ {
        self.0.iter().enumerate().flat_map(|(word, &bits)| {
            (0..64u16)
                .filter(move |bit| bits & (1 << bit) != 0)
                .map(move |bit| Code(word as u16 * 64 + bit))
        })
    }
}

/// The codes a binding reads: one, or four for a composite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Inputs {
    codes: [Code; 4],
    len: usize,
}

impl Inputs {
    pub(crate) const fn one(code: Code) -> Self {
        Self {
            codes: [code; 4],
            len: 1,
        }
    }

    pub(crate) const fn four(codes: [Code; 4]) -> Self {
        Self { codes, len: 4 }
    }

    pub(crate) fn as_slice(&self) -> &[Code] {
        &self.codes[..self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIGNS: [AxisSign; 2] = [AxisSign::Negative, AxisSign::Positive];

    /// Every gamepad code, in order.
    fn pad_codes() -> Vec<Code> {
        let halves = || {
            GamepadAxis::ALL
                .iter()
                .flat_map(|&axis| SIGNS.map(|sign| (axis, sign)))
        };
        GamepadButton::ALL
            .iter()
            .map(|&button| Code::pad_button(button))
            .chain(halves().map(|(axis, sign)| Code::pad_press(axis, sign)))
            .chain(halves().map(|(axis, sign)| Code::pad_active(axis, sign)))
            .chain(Stick::ALL.iter().map(|&stick| Code::stick(stick)))
            .collect()
    }

    #[test]
    fn every_key_and_button_has_its_own_code_within_the_set() {
        let codes: Vec<Code> = Key::ALL
            .iter()
            .map(|&key| Code::key(key))
            .chain(MouseButton::ALL.iter().map(|&button| Code::mouse(button)))
            .chain(pad_codes())
            .collect();
        let mut set = CodeSet::default();
        for &code in &codes {
            assert!(!set.contains(code), "{code:?} twice");
            set.insert(code);
        }
        assert_eq!(set.iter().collect::<Vec<_>>(), codes);
        assert!(Code::stick(Stick::Right).0 < (WORDS * 64) as u16);
    }

    #[test]
    fn the_keys_the_mouse_and_the_pads_split_every_code() {
        for &key in Key::ALL {
            assert!(CodeSet::KEYS.contains(Code::key(key)), "{key:?}");
            assert!(!CodeSet::MOUSE.contains(Code::key(key)), "{key:?}");
            assert!(!CodeSet::PADS.contains(Code::key(key)), "{key:?}");
        }
        for &button in MouseButton::ALL {
            assert!(CodeSet::MOUSE.contains(Code::mouse(button)), "{button:?}");
            assert!(!CodeSet::KEYS.contains(Code::mouse(button)), "{button:?}");
            assert!(!CodeSet::PADS.contains(Code::mouse(button)), "{button:?}");
        }
        for code in pad_codes() {
            assert!(CodeSet::PADS.contains(code), "{code:?}");
            assert!(!CodeSet::KEYS.contains(code), "{code:?}");
            assert!(!CodeSet::MOUSE.contains(code), "{code:?}");
        }
        assert_eq!(
            CodeSet::KEYS.intersection(CodeSet::MOUSE),
            CodeSet::default()
        );
        assert_eq!(
            CodeSet::PADS.intersection(CodeSet::KEYS.union(CodeSet::MOUSE)),
            CodeSet::default()
        );
    }

    #[test]
    fn union_and_intersection_combine_sets() {
        let mut both = CodeSet::default();
        both.insert(Code::key(Key::W));
        both.insert(Code::mouse(MouseButton::Left));
        let mut keys = CodeSet::default();
        keys.insert(Code::key(Key::W));
        assert_eq!(both.intersection(CodeSet::KEYS), keys);
        let mut mouse = CodeSet::default();
        mouse.insert(Code::mouse(MouseButton::Left));
        assert_eq!(both.intersection(CodeSet::MOUSE), mouse);
        assert_eq!(keys.union(mouse), both);
        assert_eq!(both.union(both), both);
    }

    #[test]
    fn removing_leaves_the_others() {
        let mut set = CodeSet::default();
        set.insert(Code::key(Key::W));
        set.insert(Code::mouse(MouseButton::Left));
        set.remove(Code::key(Key::W));
        assert!(!set.contains(Code::key(Key::W)));
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            [Code::mouse(MouseButton::Left)]
        );
    }
}
