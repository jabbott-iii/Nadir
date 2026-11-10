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

//! Editing one line of text: the caret, the selection, and the edits
//! typed, in order.

use nadir_core::Key;

use crate::input::Edit;

/// The caret and the other end of the selection, as byte offsets of the
/// text: no selection when they are equal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Caret {
    pub(crate) caret: usize,
    pub(crate) anchor: usize,
}

impl Caret {
    /// The caret and anchor at `byte`.
    pub(crate) fn at(byte: usize) -> Self {
        Self {
            caret: byte,
            anchor: byte,
        }
    }

    /// The selection, from its start to its end.
    pub(crate) fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    /// Moves both ends inside `text` and onto character boundaries, down,
    /// so that a text changed under them cannot be cut mid-character.
    pub(crate) fn fit(&mut self, text: &str) {
        self.caret = floor_boundary(text, self.caret);
        self.anchor = floor_boundary(text, self.anchor);
    }
}

/// `byte` held to `text`'s length and moved down to a character boundary.
fn floor_boundary(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

/// The boundary before `byte`, a boundary, or 0.
fn previous(text: &str, byte: usize) -> usize {
    text[..byte]
        .char_indices()
        .next_back()
        .map_or(0, |(start, _)| start)
}

/// The boundary after `byte`, a boundary, or the length.
fn next(text: &str, byte: usize) -> usize {
    text[byte..]
        .chars()
        .next()
        .map_or(byte, |c| byte + c.len_utf8())
}

/// What an edit did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Edited {
    /// The text changed.
    pub(crate) changed: bool,
    /// Enter was pressed.
    pub(crate) submitted: bool,
    /// Escape was pressed.
    pub(crate) escaped: bool,
}

/// Applies `edit` to `text` at `caret`, which fits `text`.
///
/// A character replaces the selection. Backspace and Delete remove it, or
/// the character before or after the caret. Left and Right move the caret
/// a character, or with no Shift end a selection at its edge; Home and End
/// go to an end; Shift keeps the anchor, selecting. A with Control or
/// Super selects all. Enter submits, Escape escapes, and other keys do
/// nothing here.
pub(crate) fn apply(text: &mut String, caret: &mut Caret, edit: Edit) -> Edited {
    let mut edited = Edited::default();
    let (start, end) = caret.selection();
    let cut = |text: &mut String, caret: &mut Caret| {
        if start != end {
            text.replace_range(start..end, "");
            *caret = Caret::at(start);
            true
        } else {
            false
        }
    };
    match edit {
        Edit::Char(character) => {
            cut(text, caret);
            text.insert(caret.caret, character);
            *caret = Caret::at(caret.caret + character.len_utf8());
            edited.changed = true;
        }
        Edit::Key {
            key,
            shift,
            command,
        } => match key {
            Key::Backspace | Key::Delete => {
                if cut(text, caret) {
                    edited.changed = true;
                } else {
                    let at = caret.caret;
                    let (from, to) = if key == Key::Backspace {
                        (previous(text, at), at)
                    } else {
                        (at, next(text, at))
                    };
                    if from != to {
                        text.replace_range(from..to, "");
                        *caret = Caret::at(from);
                        edited.changed = true;
                    }
                }
            }
            Key::Left | Key::Right | Key::Home | Key::End => {
                let to = match key {
                    Key::Home => 0,
                    Key::End => text.len(),
                    _ if !shift && start != end => {
                        if key == Key::Left {
                            start
                        } else {
                            end
                        }
                    }
                    Key::Left => previous(text, caret.caret),
                    _ => next(text, caret.caret),
                };
                caret.caret = to;
                if !shift {
                    caret.anchor = to;
                }
            }
            Key::A if command => {
                *caret = Caret {
                    caret: text.len(),
                    anchor: 0,
                };
            }
            Key::Enter | Key::NumpadEnter => edited.submitted = true,
            Key::Escape => edited.escaped = true,
            _ => {}
        },
    }
    edited
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key) -> Edit {
        Edit::Key {
            key,
            shift: false,
            command: false,
        }
    }

    fn shifted(key: Key) -> Edit {
        Edit::Key {
            key,
            shift: true,
            command: false,
        }
    }

    /// Applies `edits` to `text` with the caret at `at`; the text, caret
    /// and anchor after.
    fn run(text: &str, at: usize, edits: &[Edit]) -> (String, usize, usize) {
        let mut text = text.to_string();
        let mut caret = Caret::at(at);
        for &edit in edits {
            apply(&mut text, &mut caret, edit);
        }
        (text, caret.caret, caret.anchor)
    }

    #[test]
    fn typing_inserts_at_the_caret_and_backspace_and_delete_remove_one_character() {
        let typed: Vec<Edit> = "abc".chars().map(Edit::Char).collect();
        assert_eq!(run("", 0, &typed), ("abc".into(), 3, 3));
        assert_eq!(run("ac", 1, &[Edit::Char('b')]), ("abc".into(), 2, 2));
        assert_eq!(run("abc", 3, &[key(Key::Backspace)]), ("ab".into(), 2, 2));
        assert_eq!(run("abc", 0, &[key(Key::Delete)]), ("bc".into(), 0, 0));
        // At the ends, nothing to remove.
        assert_eq!(run("ab", 0, &[key(Key::Backspace)]), ("ab".into(), 0, 0));
        assert_eq!(run("ab", 2, &[key(Key::Delete)]), ("ab".into(), 2, 2));
        // One character, however many bytes.
        assert_eq!(run("aéb", 3, &[key(Key::Backspace)]), ("ab".into(), 1, 1));
        assert_eq!(run("aéb", 1, &[key(Key::Delete)]), ("ab".into(), 1, 1));
        // In their order: c, then Backspace, is nothing.
        assert_eq!(
            run("", 0, &[Edit::Char('c'), key(Key::Backspace)]),
            ("".into(), 0, 0)
        );
    }

    #[test]
    fn the_caret_moves_by_characters_and_shift_selects() {
        assert_eq!(run("aéb", 3, &[key(Key::Left)]), ("aéb".into(), 1, 1));
        assert_eq!(run("aéb", 1, &[key(Key::Right)]), ("aéb".into(), 3, 3));
        assert_eq!(run("ab", 0, &[key(Key::Left)]), ("ab".into(), 0, 0));
        assert_eq!(run("ab", 2, &[key(Key::Right)]), ("ab".into(), 2, 2));
        assert_eq!(run("abc", 1, &[key(Key::End)]), ("abc".into(), 3, 3));
        assert_eq!(run("abc", 1, &[key(Key::Home)]), ("abc".into(), 0, 0));
        // Shift keeps the anchor.
        let selected = run("abcd", 3, &[shifted(Key::Left), shifted(Key::Left)]);
        assert_eq!(selected, ("abcd".into(), 1, 3));
        assert_eq!(run("abcd", 1, &[shifted(Key::End)]), ("abcd".into(), 4, 1));
    }

    #[test]
    fn typing_replaces_a_selection_and_left_or_right_ends_it_at_its_edge() {
        let mut text = "abcd".to_string();
        let mut caret = Caret {
            caret: 1,
            anchor: 3,
        };
        apply(&mut text, &mut caret, Edit::Char('x'));
        assert_eq!((text.as_str(), caret), ("axd", Caret::at(2)));

        for (key, at) in [(Key::Left, 1), (Key::Right, 3)] {
            let mut caret = Caret {
                caret: 3,
                anchor: 1,
            };
            apply(&mut "abcd".to_string(), &mut caret, self::key(key));
            assert_eq!(caret, Caret::at(at), "{key:?}");
        }
        let mut caret = Caret {
            caret: 3,
            anchor: 1,
        };
        let mut text = "abcd".to_string();
        let edited = apply(&mut text, &mut caret, key(Key::Delete));
        assert!(edited.changed);
        assert_eq!((text.as_str(), caret), ("ad", Caret::at(1)));
    }

    #[test]
    fn control_or_super_a_selects_all_and_enter_and_escape_are_told() {
        let command = Edit::Key {
            key: Key::A,
            shift: false,
            command: true,
        };
        assert_eq!(run("abc", 1, &[command]), ("abc".into(), 3, 0));
        // A with no Control is not a shortcut, and comes as text.
        assert_eq!(run("abc", 1, &[key(Key::A)]), ("abc".into(), 1, 1));
        let mut text = "x".to_string();
        let mut caret = Caret::at(1);
        let enter = apply(&mut text, &mut caret, key(Key::Enter));
        let numpad = apply(&mut text, &mut caret, key(Key::NumpadEnter));
        let escape = apply(&mut text, &mut caret, key(Key::Escape));
        assert!(enter.submitted && numpad.submitted && escape.escaped);
        assert!(!enter.changed && !escape.changed);
        // Keys a caller may use, as history or completion, do nothing here.
        for key in [Key::Up, Key::Down, Key::Tab, Key::PageUp, Key::PageDown] {
            assert_eq!(run("ab", 1, &[self::key(key)]), ("ab".into(), 1, 1));
        }
    }

    #[test]
    fn a_caret_fits_a_text_changed_under_it() {
        let mut caret = Caret {
            caret: 9,
            anchor: 2,
        };
        caret.fit("aé");
        // 9 is past the end; 2 is inside é, which starts at 1.
        assert_eq!(
            caret,
            Caret {
                caret: 3,
                anchor: 1
            }
        );
        let mut caret = Caret::at(2);
        caret.fit("a");
        let mut text = "a".to_string();
        apply(&mut text, &mut caret, Edit::Char('b'));
        assert_eq!(text, "ab");
    }
}
