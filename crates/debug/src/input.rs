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

//! The debug UI's input: window events in their order, taken a frame at a
//! time.

use std::collections::VecDeque;

use nadir_core::{Key, MouseButton, Vec2};

/// The most events kept. Past it the oldest go, so that a debug UI that is
/// not drawn still uses bounded memory.
const MAX_EVENTS: usize = 4096;

/// One window event, as the debug UI keeps it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Event {
    Moved(Vec2),
    Button(bool),
    Wheel(Vec2),
    Key { key: Key, pressed: bool },
    Text(char),
    FocusLost,
}

/// The modifier keys that are down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Modifiers {
    /// Left and right Shift, Control, Alt and Super, in that order.
    down: [bool; 8],
}

impl Modifiers {
    /// Records `key` going down or up, if it is a modifier.
    fn set(&mut self, key: Key, pressed: bool) {
        let slot = match key {
            Key::LeftShift => 0,
            Key::RightShift => 1,
            Key::LeftControl => 2,
            Key::RightControl => 3,
            Key::LeftAlt => 4,
            Key::RightAlt => 5,
            Key::LeftSuper => 6,
            Key::RightSuper => 7,
            _ => return,
        };
        self.down[slot] = pressed;
    }

    fn shift(&self) -> bool {
        self.down[0] || self.down[1]
    }

    /// Control or Super, with no Alt: AltGr arrives on Windows as Control
    /// and Alt, and types text.
    fn command(&self) -> bool {
        let alt = self.down[4] || self.down[5];
        (self.down[2] || self.down[3] || self.down[6] || self.down[7]) && !alt
    }
}

/// An edit of the focused text field, in the order typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edit {
    /// A character typed.
    Char(char),
    /// A key pressed, or repeated, with Shift, and with Control or Super
    /// and no Alt (`command`).
    Key {
        key: Key,
        shift: bool,
        command: bool,
    },
}

/// What a frame takes from the input: where the pointer was at each edge
/// of the left button, in output pixels, and the edits in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Taken {
    /// Where the pointer is after the frame's events.
    pub(crate) pointer: Vec2,
    /// Where the button went down, if it did.
    pub(crate) press: Option<Vec2>,
    /// Where it went up, if it did, and whether that was the window losing
    /// the focus, which clicks nothing.
    pub(crate) release: Option<(Vec2, bool)>,
    /// Whether it is down after the frame's events.
    pub(crate) down: bool,
    /// The wheel's turn, in lines; +y is away from the user.
    pub(crate) wheel: Vec2,
    pub(crate) edits: Vec<Edit>,
}

/// The debug UI's input: the window's events, fed in their order, by the
/// engine from its window (roadmap M7.10) or by a test.
///
/// [`DebugUi::frame`](crate::DebugUi::frame) takes the events of one frame:
/// at most one press and one release of the left button, so that a click is
/// judged where it was pressed and where it was released, however late the
/// frame. The frame ends at the release, so that a drag ends where the
/// button went up; a press after a release, or after text or keys, waits for
/// the next frame, so that what was typed goes to the field that had the
/// keyboard. Pointer moves in a row are kept as the last one, and past 4096
/// events the oldest go: a debug UI not drawn for a while should
/// [`clear`](DebugInput::clear) its input when it is drawn again.
#[derive(Clone, Debug, Default)]
pub struct DebugInput {
    events: VecDeque<Event>,
    /// As the events taken so far left them.
    pointer: Vec2,
    down: bool,
    modifiers: Modifiers,
}

impl DebugInput {
    /// An input with no events, the pointer at (0, 0) and nothing down.
    pub fn new() -> Self {
        Self::default()
    }

    /// The pointer moved to `position`: the output's pixels from its
    /// top-left corner, as `nadir_input::Input::pointer` gives it.
    pub fn pointer_moved(&mut self, position: Vec2) {
        if let Some(Event::Moved(last)) = self.events.back_mut() {
            *last = position;
        } else {
            self.push(Event::Moved(position));
        }
    }

    /// A mouse button went down or up. The debug UI uses the left one only.
    pub fn button(&mut self, button: MouseButton, pressed: bool) {
        if button == MouseButton::Left {
            self.push(Event::Button(pressed));
        }
    }

    /// The wheel turned by `lines`: +y is away from the user, which
    /// scrolls toward the top.
    pub fn wheel(&mut self, lines: Vec2) {
        self.push(Event::Wheel(lines));
    }

    /// A key went down, up, or repeated while held (`repeat`): a repeat is
    /// a press again, as editing wants.
    pub fn key(&mut self, key: Key, pressed: bool, repeat: bool) {
        let _ = repeat;
        self.push(Event::Key { key, pressed });
    }

    /// Text was typed. Control characters, a tab and newlines among them,
    /// are left out: the keys that make them are seen as keys.
    pub fn text(&mut self, text: &str) {
        for character in text.chars().filter(|c| !c.is_control()) {
            self.push(Event::Text(character));
        }
    }

    /// The window lost the focus: the button and the modifiers are let go,
    /// and a widget held is released without a click.
    pub fn focus_lost(&mut self) {
        self.push(Event::FocusLost);
    }

    /// Whether events wait to be taken.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Drops the events not taken, keeping where they leave the pointer and
    /// what they leave held: for a debug UI drawn again after frames it was
    /// not, so that the clicks the game had then are not replayed on it.
    pub fn clear(&mut self) {
        for event in self.events.drain(..) {
            match event {
                Event::Moved(position) => self.pointer = position,
                Event::Button(pressed) => self.down = pressed,
                Event::Key { key, pressed } => self.modifiers.set(key, pressed),
                Event::FocusLost => {
                    self.down = false;
                    self.modifiers = Modifiers::default();
                }
                Event::Wheel(_) | Event::Text(_) => {}
            }
        }
    }

    fn push(&mut self, event: Event) {
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    /// Takes the events of one frame into `out`.
    pub(crate) fn take(&mut self, out: &mut Taken) {
        out.press = None;
        out.release = None;
        out.wheel = Vec2::ZERO;
        out.edits.clear();
        while let Some(&event) = self.events.front() {
            // A frame ends at its release, and a press waits for the next
            // frame after a release or after text and keys, which go to the
            // field that had the keyboard before it.
            match event {
                Event::Button(true)
                    if out.press.is_some() || out.release.is_some() || !out.edits.is_empty() =>
                {
                    break;
                }
                Event::Button(false) | Event::Moved(_) if out.release.is_some() => break,
                _ => {}
            }
            self.events.pop_front();
            match event {
                Event::Moved(position) => self.pointer = position,
                Event::Button(true) => {
                    if !self.down {
                        self.down = true;
                        out.press = Some(self.pointer);
                    }
                }
                Event::Button(false) => {
                    if self.down {
                        self.down = false;
                        out.release = Some((self.pointer, false));
                    }
                }
                Event::Wheel(lines) => out.wheel += lines,
                Event::Key { key, pressed } => {
                    self.modifiers.set(key, pressed);
                    if pressed {
                        out.edits.push(Edit::Key {
                            key,
                            shift: self.modifiers.shift(),
                            command: self.modifiers.command(),
                        });
                    }
                }
                Event::Text(character) => out.edits.push(Edit::Char(character)),
                Event::FocusLost => {
                    if self.down {
                        self.down = false;
                        out.release = Some((self.pointer, true));
                    }
                    self.modifiers = Modifiers::default();
                }
            }
        }
        out.pointer = self.pointer;
        out.down = self.down;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(input: &mut DebugInput) -> Taken {
        let mut taken = Taken::default();
        input.take(&mut taken);
        taken
    }

    #[test]
    fn a_press_and_its_release_are_judged_where_they_happened() {
        let mut input = DebugInput::new();
        input.pointer_moved(Vec2::new(1.0, 1.0));
        input.button(MouseButton::Left, true);
        input.pointer_moved(Vec2::new(5.0, 1.0));
        input.pointer_moved(Vec2::new(9.0, 1.0));
        input.button(MouseButton::Left, false);
        input.pointer_moved(Vec2::new(20.0, 1.0));
        // Not the left button: not seen.
        input.button(MouseButton::Right, true);
        let taken = take(&mut input);
        assert_eq!(taken.press, Some(Vec2::new(1.0, 1.0)));
        assert_eq!(taken.release, Some((Vec2::new(9.0, 1.0), false)));
        // The frame ends at the release: a drag ends where it went up.
        assert_eq!((taken.pointer, taken.down), (Vec2::new(9.0, 1.0), false));
        assert!(!input.is_empty());
        assert_eq!(take(&mut input).pointer, Vec2::new(20.0, 1.0));
        assert!(input.is_empty());
    }

    #[test]
    fn a_press_after_typing_waits_for_the_next_frame() {
        let mut input = DebugInput::new();
        input.text("ab");
        input.key(Key::Enter, true, false);
        input.button(MouseButton::Left, true);
        input.text("c");
        let first = take(&mut input);
        assert_eq!((first.press, first.edits.len()), (None, 3));
        let second = take(&mut input);
        assert!(second.press.is_some());
        assert_eq!(second.edits, [Edit::Char('c')]);
    }

    #[test]
    fn clearing_drops_the_events_and_keeps_what_they_hold() {
        let mut input = DebugInput::new();
        input.key(Key::LeftShift, true, false);
        input.pointer_moved(Vec2::new(4.0, 5.0));
        input.button(MouseButton::Left, true);
        input.button(MouseButton::Left, false);
        input.button(MouseButton::Left, true);
        input.text("lost");
        input.clear();
        assert!(input.is_empty());
        // Held, with no press to judge; Shift still down.
        input.key(Key::Left, true, false);
        input.button(MouseButton::Left, false);
        let taken = take(&mut input);
        assert_eq!(taken.press, None);
        assert_eq!(taken.release, Some((Vec2::new(4.0, 5.0), false)));
        assert_eq!(
            taken.edits,
            [Edit::Key {
                key: Key::Left,
                shift: true,
                command: false
            }]
        );
    }

    #[test]
    fn a_press_after_a_release_waits_for_the_next_frame() {
        let mut input = DebugInput::new();
        input.button(MouseButton::Left, true);
        let first = take(&mut input);
        assert!(first.press.is_some() && first.down);
        // The drag ends, and a new press starts, in one frame.
        input.button(MouseButton::Left, false);
        input.pointer_moved(Vec2::new(3.0, 0.0));
        input.button(MouseButton::Left, true);
        input.text("a");
        let second = take(&mut input);
        assert_eq!(second.release, Some((Vec2::ZERO, false)));
        assert_eq!((second.press, second.down), (None, false));
        assert!(second.edits.is_empty(), "after the cut");
        let third = take(&mut input);
        assert_eq!(third.press, Some(Vec2::new(3.0, 0.0)));
        assert_eq!(third.edits, [Edit::Char('a')]);
        // Two clicks in one frame are two frames.
        input.button(MouseButton::Left, false);
        input.button(MouseButton::Left, true);
        input.button(MouseButton::Left, false);
        assert!(take(&mut input).release.is_some());
        let last = take(&mut input);
        assert!(last.press.is_some() && last.release.is_some());
    }

    #[test]
    fn edits_keep_their_order_with_the_modifiers_of_their_time() {
        let mut input = DebugInput::new();
        input.text("c");
        input.key(Key::LeftShift, true, false);
        input.key(Key::Left, true, false);
        input.key(Key::Left, true, true);
        input.key(Key::LeftShift, false, false);
        input.key(Key::Backspace, true, false);
        input.key(Key::RightControl, true, false);
        input.key(Key::A, true, false);
        // AltGr: Control and Alt, which types text, selects nothing.
        input.key(Key::RightAlt, true, false);
        input.key(Key::A, true, false);
        input.text("\tą\n");
        let key = |key, shift, command| Edit::Key {
            key,
            shift,
            command,
        };
        assert_eq!(
            take(&mut input).edits,
            [
                Edit::Char('c'),
                key(Key::LeftShift, true, false),
                key(Key::Left, true, false),
                key(Key::Left, true, false),
                key(Key::Backspace, false, false),
                key(Key::RightControl, false, true),
                key(Key::A, false, true),
                key(Key::RightAlt, false, false),
                key(Key::A, false, false),
                Edit::Char('ą'),
            ]
        );
    }

    #[test]
    fn losing_the_focus_lets_go_without_a_click() {
        let mut input = DebugInput::new();
        input.key(Key::LeftShift, true, false);
        input.button(MouseButton::Left, true);
        take(&mut input);
        input.focus_lost();
        input.key(Key::Left, true, false);
        let taken = take(&mut input);
        assert_eq!(taken.release, Some((Vec2::ZERO, true)));
        assert!(!taken.down);
        assert_eq!(
            taken.edits,
            [Edit::Key {
                key: Key::Left,
                shift: false,
                command: false
            }]
        );
    }

    #[test]
    fn moves_in_a_row_are_one_and_the_events_are_bounded() {
        let mut input = DebugInput::new();
        for x in 0..10_000 {
            input.pointer_moved(Vec2::new(x as f32, 0.0));
        }
        assert_eq!(input.events.len(), 1);
        for _ in 0..10_000 {
            input.wheel(Vec2::new(0.0, 1.0));
        }
        assert_eq!(input.events.len(), MAX_EVENTS);
        assert_eq!(take(&mut input).wheel, Vec2::new(0.0, MAX_EVENTS as f32));
    }
}
