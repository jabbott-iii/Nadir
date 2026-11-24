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

//! Physical input identifiers: keyboard keys, mouse buttons, and a
//! gamepad's buttons and axes.
//!
//! A [`Key`] names a place on the keyboard, not the character printed there.
//! The key right of Tab is [`Key::Q`] on every layout, though an AZERTY
//! keyboard prints `A` on it. So bindings such as WASD stay where the
//! player's fingers are. Text that the player types is a separate thing, and
//! comes from the platform as characters.
//!
//! A [`GamepadButton`] is a place on the pad too, named after the Xbox pad:
//! [`GamepadButton::A`] is the bottom face button, which a PlayStation pad
//! labels ✕ and a Nintendo one B.

/// Declares [`Key`] and [`Key::ALL`] from one list, so that the two cannot
/// disagree.
macro_rules! keys {
    ($( $(#[$attr:meta])* $name:ident = $usage:literal, )+) => {
        /// A physical key.
        ///
        /// The variants are in the order of the USB HID usage tables
        /// (keyboard page, `0x07`), and the value of each is its usage id.
        /// Names follow the US layout.
        ///
        /// Not every keyboard has every key, and not every operating system
        /// reports every key. Each platform backend lists the ones it cannot
        /// report. Keys that only Japanese and Korean keyboards have are not
        /// here yet.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u8)]
        pub enum Key {
            $( $(#[$attr])* $name = $usage, )+
        }

        impl Key {
            /// Every key, in order.
            pub const ALL: &'static [Key] = &[$( Key::$name, )+];

            /// The key's name: the variant's, as in `"W"`, `"Digit1"` or
            /// `"LeftShift"`. Settings files name keys by it, so renaming a
            /// variant makes the files that name it miss it.
            pub const fn name(self) -> &'static str {
                match self {
                    $( Key::$name => stringify!($name), )+
                }
            }
        }
    };
}

keys! {
    /// `A` on a US keyboard.
    A = 0x04,
    /// `B` on a US keyboard.
    B = 0x05,
    /// `C` on a US keyboard.
    C = 0x06,
    /// `D` on a US keyboard.
    D = 0x07,
    /// `E` on a US keyboard.
    E = 0x08,
    /// `F` on a US keyboard.
    F = 0x09,
    /// `G` on a US keyboard.
    G = 0x0A,
    /// `H` on a US keyboard.
    H = 0x0B,
    /// `I` on a US keyboard.
    I = 0x0C,
    /// `J` on a US keyboard.
    J = 0x0D,
    /// `K` on a US keyboard.
    K = 0x0E,
    /// `L` on a US keyboard.
    L = 0x0F,
    /// `M` on a US keyboard.
    M = 0x10,
    /// `N` on a US keyboard.
    N = 0x11,
    /// `O` on a US keyboard.
    O = 0x12,
    /// `P` on a US keyboard.
    P = 0x13,
    /// `Q` on a US keyboard.
    Q = 0x14,
    /// `R` on a US keyboard.
    R = 0x15,
    /// `S` on a US keyboard.
    S = 0x16,
    /// `T` on a US keyboard.
    T = 0x17,
    /// `U` on a US keyboard.
    U = 0x18,
    /// `V` on a US keyboard.
    V = 0x19,
    /// `W` on a US keyboard.
    W = 0x1A,
    /// `X` on a US keyboard.
    X = 0x1B,
    /// `Y` on a US keyboard.
    Y = 0x1C,
    /// `Z` on a US keyboard.
    Z = 0x1D,
    /// `1` in the top row.
    Digit1 = 0x1E,
    /// `2` in the top row.
    Digit2 = 0x1F,
    /// `3` in the top row.
    Digit3 = 0x20,
    /// `4` in the top row.
    Digit4 = 0x21,
    /// `5` in the top row.
    Digit5 = 0x22,
    /// `6` in the top row.
    Digit6 = 0x23,
    /// `7` in the top row.
    Digit7 = 0x24,
    /// `8` in the top row.
    Digit8 = 0x25,
    /// `9` in the top row.
    Digit9 = 0x26,
    /// `0` in the top row.
    Digit0 = 0x27,
    /// Enter, or Return, in the main block.
    Enter = 0x28,
    /// Escape.
    Escape = 0x29,
    /// Backspace.
    Backspace = 0x2A,
    /// Tab.
    Tab = 0x2B,
    /// The space bar.
    Space = 0x2C,
    /// `-` on a US keyboard.
    Minus = 0x2D,
    /// `=` on a US keyboard.
    Equal = 0x2E,
    /// `[` on a US keyboard.
    LeftBracket = 0x2F,
    /// `]` on a US keyboard.
    RightBracket = 0x30,
    /// `\` on a US keyboard. On an ISO keyboard, the key left of Enter.
    Backslash = 0x31,
    /// `;` on a US keyboard.
    Semicolon = 0x33,
    /// `'` on a US keyboard.
    Quote = 0x34,
    /// `` ` `` on a US keyboard, left of `1`.
    Grave = 0x35,
    /// `,` on a US keyboard.
    Comma = 0x36,
    /// `.` on a US keyboard.
    Period = 0x37,
    /// `/` on a US keyboard.
    Slash = 0x38,
    /// Caps Lock.
    CapsLock = 0x39,
    /// F1.
    F1 = 0x3A,
    /// F2.
    F2 = 0x3B,
    /// F3.
    F3 = 0x3C,
    /// F4.
    F4 = 0x3D,
    /// F5.
    F5 = 0x3E,
    /// F6.
    F6 = 0x3F,
    /// F7.
    F7 = 0x40,
    /// F8.
    F8 = 0x41,
    /// F9.
    F9 = 0x42,
    /// F10.
    F10 = 0x43,
    /// F11.
    F11 = 0x44,
    /// F12.
    F12 = 0x45,
    /// Print Screen.
    PrintScreen = 0x46,
    /// Scroll Lock.
    ScrollLock = 0x47,
    /// Pause.
    Pause = 0x48,
    /// Insert.
    Insert = 0x49,
    /// Home.
    Home = 0x4A,
    /// Page Up.
    PageUp = 0x4B,
    /// Delete, the forward one.
    Delete = 0x4C,
    /// End.
    End = 0x4D,
    /// Page Down.
    PageDown = 0x4E,
    /// The right arrow.
    Right = 0x4F,
    /// The left arrow.
    Left = 0x50,
    /// The down arrow.
    Down = 0x51,
    /// The up arrow.
    Up = 0x52,
    /// Num Lock. On an Apple keyboard, Clear.
    NumLock = 0x53,
    /// `/` on the numeric keypad.
    NumpadDivide = 0x54,
    /// `*` on the numeric keypad.
    NumpadMultiply = 0x55,
    /// `-` on the numeric keypad.
    NumpadSubtract = 0x56,
    /// `+` on the numeric keypad.
    NumpadAdd = 0x57,
    /// Enter on the numeric keypad.
    NumpadEnter = 0x58,
    /// `1` on the numeric keypad.
    Numpad1 = 0x59,
    /// `2` on the numeric keypad.
    Numpad2 = 0x5A,
    /// `3` on the numeric keypad.
    Numpad3 = 0x5B,
    /// `4` on the numeric keypad.
    Numpad4 = 0x5C,
    /// `5` on the numeric keypad.
    Numpad5 = 0x5D,
    /// `6` on the numeric keypad.
    Numpad6 = 0x5E,
    /// `7` on the numeric keypad.
    Numpad7 = 0x5F,
    /// `8` on the numeric keypad.
    Numpad8 = 0x60,
    /// `9` on the numeric keypad.
    Numpad9 = 0x61,
    /// `0` on the numeric keypad.
    Numpad0 = 0x62,
    /// `.` on the numeric keypad.
    NumpadDecimal = 0x63,
    /// The extra key of an ISO keyboard, between the left Shift and `Z`.
    IntlBackslash = 0x64,
    /// The menu key, also called Application.
    Menu = 0x65,
    /// `=` on the numeric keypad, as on an Apple keyboard.
    NumpadEqual = 0x67,
    /// F13.
    F13 = 0x68,
    /// F14.
    F14 = 0x69,
    /// F15.
    F15 = 0x6A,
    /// F16.
    F16 = 0x6B,
    /// F17.
    F17 = 0x6C,
    /// F18.
    F18 = 0x6D,
    /// F19.
    F19 = 0x6E,
    /// F20.
    F20 = 0x6F,
    /// F21.
    F21 = 0x70,
    /// F22.
    F22 = 0x71,
    /// F23.
    F23 = 0x72,
    /// F24.
    F24 = 0x73,
    /// The left Control.
    LeftControl = 0xE0,
    /// The left Shift.
    LeftShift = 0xE1,
    /// The left Alt, or Option.
    LeftAlt = 0xE2,
    /// The left Super: the Windows or Command key.
    LeftSuper = 0xE3,
    /// The right Control.
    RightControl = 0xE4,
    /// The right Shift.
    RightShift = 0xE5,
    /// The right Alt, Option or AltGr.
    RightAlt = 0xE6,
    /// The right Super: the Windows or Command key.
    RightSuper = 0xE7,
}

impl Key {
    /// The key's id in the USB HID usage tables, keyboard page.
    pub const fn usage(self) -> u8 {
        self as u8
    }

    /// The key whose [`name`](Self::name) is `name`, if there is one. Names
    /// are case-sensitive: `"W"`, not `"w"`.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|key| key.name() == name)
    }
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MouseButton {
    /// The main button, under the index finger of a right-handed user.
    Left,
    /// The secondary button.
    Right,
    /// The middle button, which is often the wheel.
    Middle,
    /// The first side button, which browsers use for "back".
    Back,
    /// The second side button, which browsers use for "forward".
    Forward,
}

impl MouseButton {
    /// Every button, in order.
    pub const ALL: &'static [MouseButton] = &[
        MouseButton::Left,
        MouseButton::Right,
        MouseButton::Middle,
        MouseButton::Back,
        MouseButton::Forward,
    ];

    /// The button's name: the variant's, as in `"Left"`. Settings files
    /// name buttons by it.
    pub const fn name(self) -> &'static str {
        match self {
            MouseButton::Left => "Left",
            MouseButton::Right => "Right",
            MouseButton::Middle => "Middle",
            MouseButton::Back => "Back",
            MouseButton::Forward => "Forward",
        }
    }

    /// The button whose [`name`](Self::name) is `name`, if there is one.
    /// Names are case-sensitive.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|button| button.name() == name)
    }
}

/// A gamepad's button, by its place on a standard pad, named after the Xbox
/// pad's.
///
/// The four face buttons are named by where they are, not by what is printed
/// on them: [`A`](Self::A) is the bottom one, [`B`](Self::B) the right,
/// [`X`](Self::X) the left and [`Y`](Self::Y) the top. A PlayStation pad's
/// ✕ is `A`, and a Nintendo pad's B is `A` while its A is `B`. The triggers
/// are not buttons but axes: [`GamepadAxis::LeftTrigger`] and
/// [`GamepadAxis::RightTrigger`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GamepadButton {
    /// The bottom face button: Xbox A, PlayStation ✕, Nintendo B.
    A,
    /// The right face button: Xbox B, PlayStation ○, Nintendo A.
    B,
    /// The left face button: Xbox X, PlayStation □, Nintendo Y.
    X,
    /// The top face button: Xbox Y, PlayStation △, Nintendo X.
    Y,
    /// The left shoulder button, above the left trigger: LB, L1, L.
    LeftBumper,
    /// The right shoulder button, above the right trigger: RB, R1, R.
    RightBumper,
    /// The left one of the two small buttons in the middle: Xbox View (Back
    /// on the 360), PlayStation Create or Share, Nintendo −.
    Back,
    /// The right one of the two small buttons in the middle: Xbox Menu
    /// (Start on the 360), PlayStation Options, Nintendo +.
    Start,
    /// The button with the maker's logo: Xbox, PS, Home. A system can keep
    /// it for itself, and a game should not need it.
    Guide,
    /// Pressing the left stick in: LS, L3.
    LeftStick,
    /// Pressing the right stick in: RS, R3.
    RightStick,
    /// The d-pad's up.
    DPadUp,
    /// The d-pad's down.
    DPadDown,
    /// The d-pad's left.
    DPadLeft,
    /// The d-pad's right.
    DPadRight,
}

impl GamepadButton {
    /// Every button, in order.
    pub const ALL: &'static [GamepadButton] = &[
        GamepadButton::A,
        GamepadButton::B,
        GamepadButton::X,
        GamepadButton::Y,
        GamepadButton::LeftBumper,
        GamepadButton::RightBumper,
        GamepadButton::Back,
        GamepadButton::Start,
        GamepadButton::Guide,
        GamepadButton::LeftStick,
        GamepadButton::RightStick,
        GamepadButton::DPadUp,
        GamepadButton::DPadDown,
        GamepadButton::DPadLeft,
        GamepadButton::DPadRight,
    ];

    /// The button's place in [`GamepadButton::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The button's name: the variant's, as in `"A"` or `"LeftBumper"`.
    /// Settings files name buttons by it.
    pub const fn name(self) -> &'static str {
        match self {
            GamepadButton::A => "A",
            GamepadButton::B => "B",
            GamepadButton::X => "X",
            GamepadButton::Y => "Y",
            GamepadButton::LeftBumper => "LeftBumper",
            GamepadButton::RightBumper => "RightBumper",
            GamepadButton::Back => "Back",
            GamepadButton::Start => "Start",
            GamepadButton::Guide => "Guide",
            GamepadButton::LeftStick => "LeftStick",
            GamepadButton::RightStick => "RightStick",
            GamepadButton::DPadUp => "DPadUp",
            GamepadButton::DPadDown => "DPadDown",
            GamepadButton::DPadLeft => "DPadLeft",
            GamepadButton::DPadRight => "DPadRight",
        }
    }

    /// The button whose [`name`](Self::name) is `name`, if there is one.
    /// Names are case-sensitive.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|button| button.name() == name)
    }
}

/// A gamepad's analog axis.
///
/// A stick's axes go from -1 to 1, 0 at rest, with +X to the right and +Y
/// toward the player, which is down, as in the engine's world. A trigger goes
/// from 0, released, to 1, pulled all the way; a pad whose triggers are only
/// buttons gives 0 or 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GamepadAxis {
    /// The left stick, left (-1) to right (1).
    LeftX,
    /// The left stick, up (-1) to down (1).
    LeftY,
    /// The right stick, left (-1) to right (1).
    RightX,
    /// The right stick, up (-1) to down (1).
    RightY,
    /// The left trigger, 0 to 1: LT, L2, ZL.
    LeftTrigger,
    /// The right trigger, 0 to 1: RT, R2, ZR.
    RightTrigger,
}

impl GamepadAxis {
    /// Every axis, in order.
    pub const ALL: &'static [GamepadAxis] = &[
        GamepadAxis::LeftX,
        GamepadAxis::LeftY,
        GamepadAxis::RightX,
        GamepadAxis::RightY,
        GamepadAxis::LeftTrigger,
        GamepadAxis::RightTrigger,
    ];

    /// The axis's place in [`GamepadAxis::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The axis's name: the variant's, as in `"LeftX"` or
    /// `"RightTrigger"`. Settings files name axes by it.
    pub const fn name(self) -> &'static str {
        match self {
            GamepadAxis::LeftX => "LeftX",
            GamepadAxis::LeftY => "LeftY",
            GamepadAxis::RightX => "RightX",
            GamepadAxis::RightY => "RightY",
            GamepadAxis::LeftTrigger => "LeftTrigger",
            GamepadAxis::RightTrigger => "RightTrigger",
        }
    }

    /// The axis whose [`name`](Self::name) is `name`, if there is one.
    /// Names are case-sensitive.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|axis| axis.name() == name)
    }

    /// Whether this is a trigger, which goes from 0 to 1, and not a stick's
    /// axis, which goes from -1 to 1.
    pub const fn is_trigger(self) -> bool {
        matches!(self, GamepadAxis::LeftTrigger | GamepadAxis::RightTrigger)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_in_usage_order_without_repeats() {
        assert_eq!(Key::ALL.len(), 118);
        for pair in Key::ALL.windows(2) {
            assert!(pair[0].usage() < pair[1].usage(), "{pair:?}");
            assert!(pair[0] < pair[1], "{pair:?}");
        }
    }

    #[test]
    fn usages_match_the_hid_usage_tables() {
        // Spot checks against "HID Usage Tables", section 10, at the start
        // and end of each run of keys.
        let expected = [
            (Key::A, 0x04),
            (Key::Q, 0x14),
            (Key::W, 0x1A),
            (Key::Z, 0x1D),
            (Key::Digit1, 0x1E),
            (Key::Digit9, 0x26),
            (Key::Digit0, 0x27),
            (Key::Enter, 0x28),
            (Key::Space, 0x2C),
            (Key::Backslash, 0x31),
            (Key::Semicolon, 0x33),
            (Key::Slash, 0x38),
            (Key::CapsLock, 0x39),
            (Key::F1, 0x3A),
            (Key::F12, 0x45),
            (Key::PrintScreen, 0x46),
            (Key::Delete, 0x4C),
            (Key::Right, 0x4F),
            (Key::Up, 0x52),
            (Key::NumLock, 0x53),
            (Key::NumpadEnter, 0x58),
            (Key::Numpad1, 0x59),
            (Key::Numpad9, 0x61),
            (Key::Numpad0, 0x62),
            (Key::NumpadDecimal, 0x63),
            (Key::IntlBackslash, 0x64),
            (Key::Menu, 0x65),
            (Key::NumpadEqual, 0x67),
            (Key::F13, 0x68),
            (Key::F24, 0x73),
            (Key::LeftControl, 0xE0),
            (Key::LeftSuper, 0xE3),
            (Key::RightControl, 0xE4),
            (Key::RightSuper, 0xE7),
        ];
        for (key, usage) in expected {
            assert_eq!(key.usage(), usage, "{key:?}");
        }
    }

    #[test]
    fn letters_and_function_keys_are_contiguous() {
        let run = |first: Key, last: Key| {
            Key::ALL
                .iter()
                .filter(|key| (first..=last).contains(key))
                .count()
        };
        assert_eq!(run(Key::A, Key::Z), 26);
        assert_eq!(Key::Z.usage() - Key::A.usage(), 25);
        assert_eq!(run(Key::Digit1, Key::Digit0), 10);
        assert_eq!(run(Key::F1, Key::F12), 12);
        assert_eq!(run(Key::F13, Key::F24), 12);
        assert_eq!(run(Key::Numpad1, Key::Numpad0), 10);
        assert_eq!(run(Key::LeftControl, Key::RightSuper), 8);
    }

    #[test]
    fn every_mouse_button_is_listed_once() {
        assert_eq!(MouseButton::ALL.len(), 5);
        for pair in MouseButton::ALL.windows(2) {
            assert!(pair[0] < pair[1], "{pair:?}");
        }
    }

    #[test]
    fn every_gamepad_button_and_axis_is_listed_once_at_its_index() {
        assert_eq!(GamepadButton::ALL.len(), 15);
        for (index, button) in GamepadButton::ALL.iter().enumerate() {
            assert_eq!(button.index(), index, "{button:?}");
        }
        assert_eq!(GamepadAxis::ALL.len(), 6);
        for (index, axis) in GamepadAxis::ALL.iter().enumerate() {
            assert_eq!(axis.index(), index, "{axis:?}");
        }
        let triggers: Vec<_> = GamepadAxis::ALL
            .iter()
            .filter(|axis| axis.is_trigger())
            .collect();
        assert_eq!(
            triggers,
            [&GamepadAxis::LeftTrigger, &GamepadAxis::RightTrigger]
        );
    }

    #[test]
    fn each_name_is_its_variant_and_reads_back() {
        // A settings file names inputs this way, so a name must stay the
        // variant's and lead back to it.
        fn check<T: Copy + PartialEq + std::fmt::Debug>(
            all: &[T],
            name: fn(T) -> &'static str,
            from_name: fn(&str) -> Option<T>,
        ) {
            for &item in all {
                assert_eq!(name(item), format!("{item:?}"));
                assert_eq!(from_name(name(item)), Some(item));
            }
        }
        check(Key::ALL, Key::name, Key::from_name);
        check(MouseButton::ALL, MouseButton::name, MouseButton::from_name);
        check(
            GamepadButton::ALL,
            GamepadButton::name,
            GamepadButton::from_name,
        );
        check(GamepadAxis::ALL, GamepadAxis::name, GamepadAxis::from_name);
    }

    #[test]
    fn names_are_case_sensitive_and_unknown_names_are_none() {
        assert_eq!(Key::from_name("LeftShift"), Some(Key::LeftShift));
        assert_eq!(Key::from_name("leftshift"), None);
        assert_eq!(Key::from_name("w"), None);
        assert_eq!(Key::from_name(""), None);
        assert_eq!(MouseButton::from_name("left"), None);
        assert_eq!(GamepadButton::from_name("a"), None);
        assert_eq!(
            GamepadAxis::from_name("RightTrigger"),
            Some(GamepadAxis::RightTrigger)
        );
        assert_eq!(GamepadAxis::from_name("Trigger"), None);
    }
}
