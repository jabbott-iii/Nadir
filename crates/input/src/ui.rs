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

//! What a UI reads of the window's events: the keys with their repeats,
//! typed text, and the mouse, in order.

use nadir_core::{Key, MouseButton, Vec2};
use nadir_platform::Event;

/// A window event that a UI reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiEvent {
    /// The pointer is here now: physical pixels from the top-left corner of
    /// the inside of the window.
    PointerMoved(Vec2),
    /// A mouse button went down or up.
    Button {
        /// Which button.
        button: MouseButton,
        /// Down (`true`) or up (`false`).
        pressed: bool,
    },
    /// The wheel turned, in lines: +y away from the user, +x to the right.
    Wheel(Vec2),
    /// A key went down or up. A press may be a repeat of a key held down,
    /// which a text field takes as another press.
    Key {
        /// Which physical key.
        key: Key,
        /// Down (`true`) or up (`false`).
        pressed: bool,
        /// Whether the keyboard is repeating a key held down.
        repeat: bool,
    },
    /// A character typed, after its key press: see
    /// [`Event::Text`](nadir_platform::Event::Text).
    Text(char),
    /// The window lost the keyboard focus. The releases that the platform
    /// reports for what was held then come right after it, so that a UI
    /// lets go of a widget held without taking the button's release as a
    /// click.
    FocusLost,
}

/// The most events kept between two clears. Past it, the oldest half is
/// dropped: a UI that never clears them still uses bounded memory.
const MAX_EVENTS: usize = 4096;

/// The window's events that a UI reads, in the order they happened, until
/// the UI clears them, once a frame.
///
/// [`Input`](crate::Input) is for gameplay actions, latched once a tick: it
/// ignores the keys' repeats and typed text, and holds keys back while a UI
/// has them. A UI needs what it leaves out, at its own pace: a text field
/// takes each repeat of Backspace and each character typed. The pointer, the
/// buttons and the wheel are among them, in order, so that a click on
/// another field and typing in the same frame go where they happened.
///
/// It is fed the same events as `Input`, and holds nothing back: what a UI
/// takes, it tells `Input` with [`Input::set_capture`](crate::Input::set_capture).
///
/// Clear it every frame, also while the UI is hidden. At 4096 events, the
/// oldest half is dropped, so that a reader that never clears it uses
/// bounded memory; a release dropped so would leave the UI thinking a key
/// or a button is still down.
///
/// ```
/// use nadir_core::Key;
/// use nadir_input::{UiEvent, UiEvents};
/// use nadir_platform::Event;
///
/// let mut ui = UiEvents::new();
/// ui.handle(&Event::Key { key: Key::Backspace, pressed: true, repeat: false });
/// ui.handle(&Event::Key { key: Key::Backspace, pressed: true, repeat: true });
/// ui.handle(&Event::Text('é'));
/// ui.handle(&Event::Resized(nadir_core::UVec2::new(800, 600)));
/// assert_eq!(
///     ui.events(),
///     [
///         UiEvent::Key { key: Key::Backspace, pressed: true, repeat: false },
///         UiEvent::Key { key: Key::Backspace, pressed: true, repeat: true },
///         UiEvent::Text('é'),
///     ]
/// );
/// // Each frame, once the UI has read them.
/// ui.clear();
/// assert!(ui.events().is_empty());
/// ```
#[derive(Clone, Debug)]
pub struct UiEvents {
    events: Vec<UiEvent>,
}

impl UiEvents {
    /// No events yet.
    pub fn new() -> Self {
        Self {
            events: Vec::with_capacity(64),
        }
    }

    /// Takes in a window event.
    ///
    /// - The keys, repeats included, typed text, the pointer, the mouse
    ///   buttons, the wheel and the loss of the focus are kept.
    /// - A move of the pointer right after another replaces it: only where
    ///   the pointer is matters, between two other events.
    /// - Every other event is ignored.
    pub fn handle(&mut self, event: &Event) {
        let event = match *event {
            Event::MouseMoved(position) => {
                if let Some(UiEvent::PointerMoved(last)) = self.events.last_mut() {
                    *last = position;
                    return;
                }
                UiEvent::PointerMoved(position)
            }
            Event::MouseButton { button, pressed } => UiEvent::Button { button, pressed },
            Event::MouseWheel(lines) => UiEvent::Wheel(lines),
            Event::Key {
                key,
                pressed,
                repeat,
            } => UiEvent::Key {
                key,
                pressed,
                repeat,
            },
            Event::Text(character) => UiEvent::Text(character),
            Event::Focused(false) => UiEvent::FocusLost,
            _ => return,
        };
        if self.events.len() == MAX_EVENTS {
            self.events.drain(..MAX_EVENTS / 2);
        }
        self.events.push(event);
    }

    /// The events kept since the last [`UiEvents::clear`], oldest first.
    pub fn events(&self) -> &[UiEvent] {
        &self.events
    }

    /// Forgets the events kept, keeping their memory for the next ones.
    pub fn clear(&mut self) {
        self.events.clear();
    }
}

impl Default for UiEvents {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use nadir_core::UVec2;

    use super::*;

    fn key(key: Key, pressed: bool, repeat: bool) -> Event {
        Event::Key {
            key,
            pressed,
            repeat,
        }
    }

    fn ui_key(key: Key, pressed: bool, repeat: bool) -> UiEvent {
        UiEvent::Key {
            key,
            pressed,
            repeat,
        }
    }

    fn fed(events: &[Event]) -> UiEvents {
        let mut ui = UiEvents::new();
        for event in events {
            ui.handle(event);
        }
        ui
    }

    #[test]
    fn repeats_and_text_reach_the_ui_in_order() {
        let ui = fed(&[
            key(Key::A, true, false),
            Event::Text('a'),
            key(Key::A, true, true),
            Event::Text('a'),
            key(Key::A, false, false),
            key(Key::Backspace, true, false),
            key(Key::Backspace, true, true),
            key(Key::Backspace, true, true),
            key(Key::Backspace, false, false),
        ]);
        assert_eq!(
            ui.events(),
            [
                ui_key(Key::A, true, false),
                UiEvent::Text('a'),
                ui_key(Key::A, true, true),
                UiEvent::Text('a'),
                ui_key(Key::A, false, false),
                ui_key(Key::Backspace, true, false),
                ui_key(Key::Backspace, true, true),
                ui_key(Key::Backspace, true, true),
                ui_key(Key::Backspace, false, false),
            ]
        );
    }

    #[test]
    fn the_mouse_keeps_its_place_among_the_keys() {
        let left = MouseButton::Left;
        let ui = fed(&[
            Event::Text('x'),
            Event::MouseMoved(Vec2::new(10.0, 20.0)),
            Event::MouseButton {
                button: left,
                pressed: true,
            },
            Event::MouseButton {
                button: left,
                pressed: false,
            },
            Event::MouseWheel(Vec2::new(0.0, -1.0)),
            Event::Text('y'),
        ]);
        assert_eq!(
            ui.events(),
            [
                UiEvent::Text('x'),
                UiEvent::PointerMoved(Vec2::new(10.0, 20.0)),
                UiEvent::Button {
                    button: left,
                    pressed: true
                },
                UiEvent::Button {
                    button: left,
                    pressed: false
                },
                UiEvent::Wheel(Vec2::new(0.0, -1.0)),
                UiEvent::Text('y'),
            ]
        );
    }

    #[test]
    fn a_move_after_a_move_replaces_it() {
        let ui = fed(&[
            Event::MouseMoved(Vec2::new(1.0, 1.0)),
            Event::MouseMoved(Vec2::new(2.0, 2.0)),
            Event::MouseMoved(Vec2::new(3.0, 3.0)),
            Event::Text('a'),
            Event::MouseMoved(Vec2::new(4.0, 4.0)),
            Event::Resized(UVec2::new(10, 10)),
            Event::MouseMoved(Vec2::new(5.0, 5.0)),
        ]);
        // An ignored event between two moves does not part them.
        assert_eq!(
            ui.events(),
            [
                UiEvent::PointerMoved(Vec2::new(3.0, 3.0)),
                UiEvent::Text('a'),
                UiEvent::PointerMoved(Vec2::new(5.0, 5.0)),
            ]
        );
    }

    #[test]
    fn the_loss_of_the_focus_keeps_its_place_among_the_releases() {
        let left = MouseButton::Left;
        let button = |pressed| Event::MouseButton {
            button: left,
            pressed,
        };
        let ui_button = |pressed| UiEvent::Button {
            button: left,
            pressed,
        };
        // As the platform reports it: the news, then the releases of what
        // was held.
        let ui = fed(&[
            key(Key::LeftShift, true, false),
            button(true),
            Event::Text('a'),
            Event::Focused(false),
            key(Key::LeftShift, false, false),
            button(false),
            Event::Focused(true),
        ]);
        assert_eq!(
            ui.events(),
            [
                ui_key(Key::LeftShift, true, false),
                ui_button(true),
                UiEvent::Text('a'),
                UiEvent::FocusLost,
                ui_key(Key::LeftShift, false, false),
                ui_button(false),
            ]
        );
        // A click just before the loss of the focus stays a click.
        let ui = fed(&[button(true), button(false), Event::Focused(false)]);
        assert_eq!(
            ui.events(),
            [ui_button(true), ui_button(false), UiEvent::FocusLost]
        );
    }

    #[test]
    fn the_window_events_are_ignored() {
        let ui = fed(&[
            Event::CloseRequested,
            Event::Resized(UVec2::new(10, 10)),
            Event::ScaleFactorChanged(2.0),
            Event::Focused(true),
        ]);
        assert!(ui.events().is_empty());
    }

    #[test]
    fn clearing_keeps_the_memory() {
        let mut ui = fed(&[Event::Text('a'); 100]);
        let capacity = ui.events.capacity();
        ui.clear();
        assert!(ui.events().is_empty());
        ui.handle(&Event::Text('b'));
        assert_eq!(ui.events(), [UiEvent::Text('b')]);
        assert_eq!(ui.events.capacity(), capacity);
        assert!(UiEvents::default().events().is_empty());
    }

    #[test]
    fn events_never_cleared_use_bounded_memory_and_keep_the_newest() {
        let mut ui = UiEvents::new();
        for index in 0..MAX_EVENTS {
            ui.handle(&Event::Text(
                char::from_u32(0x4E00 + index as u32).expect("a CJK character"),
            ));
        }
        assert_eq!(ui.events().len(), MAX_EVENTS);
        ui.handle(&Event::Text('!'));
        assert_eq!(ui.events().len(), MAX_EVENTS / 2 + 1);
        let first = char::from_u32(0x4E00 + (MAX_EVENTS / 2) as u32).expect("a CJK character");
        assert_eq!(ui.events().first(), Some(&UiEvent::Text(first)));
        assert_eq!(ui.events().last(), Some(&UiEvent::Text('!')));
        let capacity = ui.events.capacity();
        for _ in 0..10 * MAX_EVENTS {
            ui.handle(&Event::Text('?'));
        }
        assert!(ui.events().len() <= MAX_EVENTS);
        assert_eq!(ui.events.capacity(), capacity, "no growth past the bound");
    }
}
