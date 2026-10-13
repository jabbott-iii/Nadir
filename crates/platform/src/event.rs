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
    /// Before `Focused(false)`, every key and button that was down gets a
    /// release event, so nothing stays held while the window cannot see it
    /// go up.
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
