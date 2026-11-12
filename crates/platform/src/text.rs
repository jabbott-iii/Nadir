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

//! What every backend's typed text follows: which characters are typed,
//! when a key press types nothing, and joining UTF-16 surrogates. It needs
//! no window, so the rules of all three systems are tested on any of them.
//! (Linux reads its shortcuts from X's modifier state, in its own module.)

use crate::Event;

/// Whether the modifiers held with a key press make it a shortcut, which
/// types nothing, on Windows and macOS: Control without Alt, or ⌘ or the
/// Windows key. Alt alone types on macOS, where it is Option; on Windows
/// it is the window menu's (`WM_SYSCHAR`), and with Control it is Alt Gr,
/// which types the layout's third character.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(crate) fn shortcut(control: bool, alt: bool, command: bool) -> bool {
    (control && !alt) || command
}

/// Whether `character` is one that Apple gives a key that types nothing,
/// such as an arrow or a function key, from `NSUpArrowFunctionKey` on.
/// `AppKit/NSEvent.h` reserves U+F700 to U+F8FF for them, and names them up
/// to `NSModeSwitchFunctionKey = 0xF747,`; U+F8FF, the last, is the Apple
/// logo, which Option+Shift+K types.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn is_apple_function_key(character: char) -> bool {
    ('\u{F700}'..='\u{F7FF}').contains(&character)
}

/// Appends an [`Event::Text`] for each character of `typed`, but control
/// characters, unless `shortcut` says that the keys held make it a shortcut.
pub(crate) fn push_text(
    typed: impl IntoIterator<Item = char>,
    shortcut: bool,
    out: &mut Vec<Event>,
) {
    if shortcut {
        return;
    }
    out.extend(
        typed
            .into_iter()
            .filter(|c| !c.is_control())
            .map(Event::Text),
    );
}

/// Joins UTF-16 code units into characters, as they come one at a time.
/// Windows types in them.
#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Utf16 {
    /// A high surrogate, waiting for the low one.
    high: Option<u16>,
}

#[cfg(any(target_os = "windows", test))]
impl Utf16 {
    /// Takes the next unit. Returns the character it ends, if any: itself,
    /// or the pair a low surrogate ends. A high surrogate waits for the low
    /// one. A surrogate with no partner gives nothing.
    pub(crate) fn push(&mut self, unit: u16) -> Option<char> {
        let high = self.high.take();
        match unit {
            0xD800..=0xDBFF => {
                self.high = Some(unit);
                None
            }
            0xDC00..=0xDFFF => {
                let high = high?;
                let code =
                    0x1_0000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(unit) - 0xDC00);
                char::from_u32(code)
            }
            _ => char::from_u32(u32::from(unit)),
        }
    }

    /// Forgets a high surrogate that is waiting: its low one will not come.
    pub(crate) fn reset(&mut self) {
        self.high = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_or_command_is_a_shortcut_and_alt_gr_is_not() {
        // Control, Alt, ⌘ or the Windows key.
        assert!(!shortcut(false, false, false));
        assert!(shortcut(true, false, false));
        assert!(shortcut(false, false, true));
        // Option on macOS types the layout's other characters.
        assert!(!shortcut(false, true, false));
        // Alt Gr on Windows: a Left Control that Windows makes up, and Right
        // Alt. So is Control with Alt typed by hand, and on macOS Control
        // with Option.
        assert!(!shortcut(true, true, false));
        // ⌘ stays a shortcut with the others.
        assert!(shortcut(false, true, true));
        assert!(shortcut(true, true, true));
    }

    #[test]
    fn apple_function_key_characters_are_told_from_typed_ones() {
        // `NSUpArrowFunctionKey`, `NSF1FunctionKey`, `NSModeSwitchFunctionKey`.
        for key in ['\u{F700}', '\u{F704}', '\u{F747}', '\u{F7FF}'] {
            assert!(is_apple_function_key(key), "{key:?}");
        }
        // The Apple logo, and the characters around the range.
        for typed in ['\u{F8FF}', '\u{F6FF}', '\u{F800}', 'a', '\u{E000}'] {
            assert!(!is_apple_function_key(typed), "{typed:?}");
        }
    }

    fn typed(text: &str, shortcut: bool) -> Vec<Event> {
        let mut out = vec![Event::Focused(true)];
        push_text(text.chars(), shortcut, &mut out);
        out.remove(0);
        out
    }

    #[test]
    fn each_character_is_an_event_in_order_after_what_was_there() {
        let mut out = vec![Event::CloseRequested];
        push_text("aé€😀".chars(), false, &mut out);
        assert_eq!(
            out,
            [
                Event::CloseRequested,
                Event::Text('a'),
                Event::Text('é'),
                Event::Text('€'),
                Event::Text('😀'),
            ]
        );
        assert_eq!(typed("", false), []);
    }

    #[test]
    fn control_characters_are_left_out() {
        // Enter, Tab, Backspace, Escape, Delete, Control+A, and a C1 control.
        assert_eq!(typed("\r\n\t\u{8}\u{1b}\u{7f}\u{1}\u{85}", false), []);
        assert_eq!(typed("a\tb", false), [Event::Text('a'), Event::Text('b')]);
        // A space and a joiner are typed, not controls.
        assert_eq!(
            typed(" \u{200d}", false),
            [Event::Text(' '), Event::Text('\u{200d}')]
        );
    }

    #[test]
    fn a_shortcut_types_nothing() {
        assert_eq!(typed("a1 ", true), []);
    }

    #[test]
    fn units_of_the_basic_plane_are_characters() {
        let mut utf16 = Utf16::default();
        assert_eq!(utf16.push(u16::from(b'a')), Some('a'));
        assert_eq!(utf16.push(0x00E9), Some('é'));
        assert_eq!(utf16.push(0x20AC), Some('€'));
        assert_eq!(utf16.push(0xFFFD), Some('\u{FFFD}'));
        assert_eq!(utf16.push(0x0000), Some('\0'));
    }

    #[test]
    fn a_surrogate_pair_is_one_character() {
        let mut utf16 = Utf16::default();
        // U+1F600 is D83D DE00, and U+10FFFF is DBFF DFFF.
        assert_eq!(utf16.push(0xD83D), None);
        assert_eq!(utf16.push(0xDE00), Some('😀'));
        assert_eq!(utf16.push(0xDBFF), None);
        assert_eq!(utf16.push(0xDFFF), Some('\u{10FFFF}'));
        assert_eq!(utf16.push(0xD800), None);
        assert_eq!(utf16.push(0xDC00), Some('\u{10000}'));
    }

    #[test]
    fn a_surrogate_with_no_partner_gives_nothing() {
        let mut utf16 = Utf16::default();
        // A low one alone.
        assert_eq!(utf16.push(0xDE00), None);
        // A high one followed by a character: the character stays.
        assert_eq!(utf16.push(0xD83D), None);
        assert_eq!(utf16.push(u16::from(b'x')), Some('x'));
        assert_eq!(utf16.push(0xDE00), None);
        // A high one followed by another high one: the second waits.
        assert_eq!(utf16.push(0xD83D), None);
        assert_eq!(utf16.push(0xD83D), None);
        assert_eq!(utf16.push(0xDE00), Some('😀'));
    }

    #[test]
    fn a_reset_forgets_the_waiting_surrogate() {
        let mut utf16 = Utf16::default();
        assert_eq!(utf16.push(0xD83D), None);
        utf16.reset();
        assert_eq!(utf16.push(0xDE00), None);
        assert_eq!(utf16, Utf16::default());
    }
}
