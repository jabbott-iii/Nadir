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

//! The events a window reports.

use nadir_core::{Key, MouseButton, UVec2, Vec2};

/// Something that happened to the window or to an input device.
///
/// Events arrive in the order the operating system reported them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The user asked to close the window. The window stays open: the game
    /// decides what happens next.
    CloseRequested,
    /// The inside of the window has a new size, in physical pixels.
    Resized(UVec2),
    /// The scale factor is this from now on. One of these comes before any
    /// other event.
    ScaleFactorChanged(f32),
    /// The window gained (`true`) or lost (`false`) the keyboard focus.
    ///
    /// Right after `Focused(false)`, every key and button that was down gets
    /// a release event, so nothing stays held while the window cannot see it
    /// go up. They come after it, so that a reader can tell them from the
    /// releases the user made: a UI then lets go of a held widget without
    /// taking the release for a click.
    Focused(bool),
    /// A key went down or up.
    Key {
        /// Which physical key.
        key: Key,
        /// Down (`true`) or up (`false`).
        pressed: bool,
        /// Whether this is the keyboard repeating a key that is held down.
        /// Only a press can be a repeat.
        repeat: bool,
    },
    /// A character the user typed, after the key press that typed it.
    ///
    /// - One event a character. A character composed of several key presses
    ///   (a dead key and a letter, a Compose sequence, an input method's
    ///   choice on Linux and Windows) comes as one event, after the last of
    ///   them. Those presses are still [`Event::Key`]s.
    /// - A key held down types its character again with each repeat.
    /// - Never a control character: Enter, Tab, Backspace, Escape and Delete
    ///   come only as [`Event::Key`]s.
    /// - A key pressed with a shortcut's modifier types nothing. On Linux
    ///   that is Control, Alt or Super, and Alt Gr types. On Windows it is
    ///   Control without Alt, which with Control is Alt Gr there, or the
    ///   Windows key; Alt alone is the window menu's. On macOS it is ⌘, or
    ///   Control without Option, and Option types.
    /// - macOS has no input method yet: text that only an input method types
    ///   (in Chinese or Japanese, say) does not come there.
    Text(char),
    /// The pointer is here now: physical pixels from the top-left corner of
    /// the inside of the window, with y growing downward.
    MouseMoved(Vec2),
    /// A mouse button went down or up.
    MouseButton {
        /// Which button.
        button: MouseButton,
        /// Down (`true`) or up (`false`).
        pressed: bool,
    },
    /// The wheel turned, in lines. Positive y is away from the user, and
    /// positive x is to the right.
    MouseWheel(Vec2),
}
