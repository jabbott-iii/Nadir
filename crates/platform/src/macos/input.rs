//! From AppKit's events to [`Event`]s.
//!
//! The window module reads each `NSEvent` and hands over what it found as an
//! [`Incoming`]. The helpers here, and [`Translator`], need no AppKit, so
//! their rules are tested here.

use nadir_core::{Key, MouseButton, UVec2, Vec2};
// The event types are used as `appkit::NSEventTypeKeyDown`, not imported by
// name: in a pattern, a bare name that is not upper case is linted as one
// that reads like a new variable.
use nadir_sys::macos::appkit::{
    self, NSEventModifierFlagCommand, NSEventModifierFlagControl, NSEventModifierFlagOption,
    NSEventModifierFlagShift, NX_DEVICELALTKEYMASK, NX_DEVICELCMDKEYMASK, NX_DEVICELCTLKEYMASK,
    NX_DEVICELSHIFTKEYMASK, NX_DEVICERALTKEYMASK, NX_DEVICERCMDKEYMASK, NX_DEVICERCTLKEYMASK,
    NX_DEVICERSHIFTKEYMASK,
};
use nadir_sys::macos::foundation::{CGFloat, NSInteger, NSPoint, NSUInteger};

use super::keymap;
use crate::Event;

/// How many points a trackpad scrolls for one line of a mouse wheel. A
/// trackpad reports its scrolling in points, and a wheel in lines.
const POINTS_PER_LINE: CGFloat = 10.0;

/// Every device bit of the modifier flags.
const SIDE_BITS: NSUInteger = NX_DEVICELCTLKEYMASK
    | NX_DEVICELSHIFTKEYMASK
    | NX_DEVICERSHIFTKEYMASK
    | NX_DEVICELCMDKEYMASK
    | NX_DEVICERCMDKEYMASK
    | NX_DEVICELALTKEYMASK
    | NX_DEVICERALTKEYMASK
    | NX_DEVICERCTLKEYMASK;

/// An event, reduced to the kinds and fields we use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Incoming {
    /// A key other than a modifier went down or up.
    Key { pressed: bool, code: u16 },
    /// A modifier key went down or up. `code` says which, and `flags` what
    /// is down now.
    FlagsChanged { code: u16, flags: NSUInteger },
    /// A button went down or up, with the pointer at `position`, in
    /// physical pixels.
    Button {
        pressed: bool,
        button: MouseButton,
        position: Vec2,
    },
    /// The pointer moved to `position`, in physical pixels.
    Motion { position: Vec2 },
    /// The wheel turned, or the trackpad scrolled, by this many lines.
    Wheel(Vec2),
}

/// Whether an event of this type is a button's going down or up.
pub(super) fn is_button_event(event_type: NSUInteger) -> bool {
    matches!(
        event_type,
        appkit::NSEventTypeLeftMouseDown
            | appkit::NSEventTypeLeftMouseUp
            | appkit::NSEventTypeRightMouseDown
            | appkit::NSEventTypeRightMouseUp
            | appkit::NSEventTypeOtherMouseDown
            | appkit::NSEventTypeOtherMouseUp
    )
}

/// Which button a mouse event is about, and whether it went down. `None`
/// for an event that is not a button's, or a button we have no name for.
/// `number` is the event's `buttonNumber`, which tells the other buttons
/// apart.
pub(super) fn mouse_button(
    event_type: NSUInteger,
    number: NSInteger,
) -> Option<(bool, MouseButton)> {
    let pressed = matches!(
        event_type,
        appkit::NSEventTypeLeftMouseDown
            | appkit::NSEventTypeRightMouseDown
            | appkit::NSEventTypeOtherMouseDown
    );
    let button = match event_type {
        appkit::NSEventTypeLeftMouseDown | appkit::NSEventTypeLeftMouseUp => MouseButton::Left,
        appkit::NSEventTypeRightMouseDown | appkit::NSEventTypeRightMouseUp => MouseButton::Right,
        appkit::NSEventTypeOtherMouseDown | appkit::NSEventTypeOtherMouseUp => match number {
            2 => MouseButton::Middle,
            3 => MouseButton::Back,
            4 => MouseButton::Forward,
            _ => return None,
        },
        _ => return None,
    };
    Some((pressed, button))
}

/// How far a scroll event scrolled, in lines: +y away from the user, and +x
/// to the right, whatever the user's choice of "natural" scrolling, which
/// `inverted` reports. `precise` marks a trackpad's deltas, which are in
/// points.
pub(super) fn wheel_lines(dx: CGFloat, dy: CGFloat, precise: bool, inverted: bool) -> Vec2 {
    let scale = if precise { 1.0 / POINTS_PER_LINE } else { 1.0 };
    // With natural scrolling, the content follows the fingers, and the
    // deltas are the other way round from the device.
    let sign = if inverted { -1.0 } else { 1.0 };
    // A positive horizontal delta scrolls toward the left.
    Vec2::new((-dx * sign * scale) as f32, (dy * sign * scale) as f32)
}

/// A position in the view, in points from its top-left (the view is
/// flipped), as physical pixels.
pub(super) fn to_pixels(point: NSPoint, scale: f32) -> Vec2 {
    Vec2::new(point.x as f32 * scale, point.y as f32 * scale)
}

/// Whether the modifier `key` is down, by the flags of a flags-changed
/// event, or `None` if `key` is not one of the eight modifiers.
fn modifier_down(key: Key, flags: NSUInteger) -> Option<bool> {
    let (side, either) = match key {
        Key::LeftShift => (NX_DEVICELSHIFTKEYMASK, NSEventModifierFlagShift),
        Key::RightShift => (NX_DEVICERSHIFTKEYMASK, NSEventModifierFlagShift),
        Key::LeftControl => (NX_DEVICELCTLKEYMASK, NSEventModifierFlagControl),
        Key::RightControl => (NX_DEVICERCTLKEYMASK, NSEventModifierFlagControl),
        Key::LeftAlt => (NX_DEVICELALTKEYMASK, NSEventModifierFlagOption),
        Key::RightAlt => (NX_DEVICERALTKEYMASK, NSEventModifierFlagOption),
        Key::LeftSuper => (NX_DEVICELCMDKEYMASK, NSEventModifierFlagCommand),
        Key::RightSuper => (NX_DEVICERCMDKEYMASK, NSEventModifierFlagCommand),
        _ => return None,
    };
    // Some keyboards, and some remote sessions, set no device bits. Then
    // the flag for either side is all there is.
    if flags & SIDE_BITS != 0 {
        Some(flags & side != 0)
    } else {
        Some(flags & either != 0)
    }
}

/// Turns events into [`Event`]s, and remembers what that takes: which keys
/// and buttons are down, where the pointer is, and the window's size and
/// scale factor.
pub(super) struct Translator {
    scale: f32,
    /// Whether the scale factor has been reported.
    scale_announced: bool,
    size: UVec2,
    focused: bool,
    pointer: Option<Vec2>,
    /// Which keys are down, by [`Key::usage`].
    keys_down: [bool; 256],
    /// Which buttons are down, in the order of [`MouseButton::ALL`].
    buttons_down: [bool; MouseButton::ALL.len()],
}

impl Translator {
    /// A translator for a window of `size`, at the scale factor `scale`,
    /// that has seen no events yet.
    pub(super) fn new(size: UVec2, scale: f32) -> Self {
        Self {
            scale,
            scale_announced: false,
            size,
            focused: false,
            pointer: None,
            keys_down: [false; 256],
            buttons_down: [false; MouseButton::ALL.len()],
        }
    }

    /// The size of the inside of the window, in pixels.
    pub(super) fn size(&self) -> UVec2 {
        self.size
    }

    /// The scale factor of the window.
    pub(super) fn scale(&self) -> f32 {
        self.scale
    }

    /// Call this at the start of every pump, before the events that came
    /// since the last. The first time, it reports the scale factor, which
    /// therefore comes before every other event.
    pub(super) fn begin_pump(&mut self, out: &mut Vec<Event>) {
        if !self.scale_announced {
            self.scale_announced = true;
            out.push(Event::ScaleFactorChanged(self.scale));
        }
    }

    /// The window is at a new scale factor now. It is reported at once if
    /// it changed, unless the first pump has not come yet: that one reports
    /// the latest.
    pub(super) fn set_scale(&mut self, scale: f32, out: &mut Vec<Event>) {
        if scale == self.scale {
            return;
        }
        self.scale = scale;
        if self.scale_announced {
            out.push(Event::ScaleFactorChanged(scale));
        }
    }

    /// The user asked to close the window, or to quit.
    pub(super) fn close(&mut self, out: &mut Vec<Event>) {
        out.push(Event::CloseRequested);
    }

    /// Appends the events that `incoming` amounts to, which may be none.
    pub(super) fn handle(&mut self, incoming: Incoming, out: &mut Vec<Event>) {
        match incoming {
            Incoming::Key { pressed, code } => {
                if let Some(key) = keymap::from_key_code(code) {
                    self.key(key, pressed, out);
                }
            }
            Incoming::FlagsChanged { code, flags } => self.flags_changed(code, flags, out),
            Incoming::Button {
                pressed,
                button,
                position,
            } => {
                self.move_pointer(position, out);
                self.button(pressed, button, out);
            }
            Incoming::Motion { position } => self.move_pointer(position, out),
            Incoming::Wheel(lines) => {
                if lines != Vec2::ZERO {
                    out.push(Event::MouseWheel(lines));
                }
            }
        }
    }

    fn key(&mut self, key: Key, pressed: bool, out: &mut Vec<Event>) {
        let down = &mut self.keys_down[usize::from(key.usage())];
        // A release of a key we never saw go down, which happens when the
        // key was pressed while another window had the focus, is not
        // reported: every release follows a press.
        if !pressed && !*down {
            return;
        }
        // A held key repeats as presses only.
        let repeat = pressed && *down;
        *down = pressed;
        out.push(Event::Key {
            key,
            pressed,
            repeat,
        });
    }

    fn flags_changed(&mut self, code: u16, flags: NSUInteger, out: &mut Vec<Event>) {
        let Some(key) = keymap::from_key_code(code) else {
            return;
        };
        if key == Key::CapsLock {
            // Caps Lock reports a change of its state for each press, and
            // nothing when it comes up: a press and a release, at once.
            self.key(key, true, out);
            self.key(key, false, out);
            return;
        }
        let Some(pressed) = modifier_down(key, flags) else {
            return;
        };
        if pressed != self.keys_down[usize::from(key.usage())] {
            self.key(key, pressed, out);
        }
    }

    fn button(&mut self, pressed: bool, button: MouseButton, out: &mut Vec<Event>) {
        let down = &mut self.buttons_down[button as usize];
        if pressed == *down {
            // A second press without a release, or a release with no press:
            // see `key`.
            return;
        }
        *down = pressed;
        out.push(Event::MouseButton { button, pressed });
    }

    fn move_pointer(&mut self, position: Vec2, out: &mut Vec<Event>) {
        if self.pointer == Some(position) {
            return;
        }
        self.pointer = Some(position);
        out.push(Event::MouseMoved(position));
    }

    /// The window got the keyboard focus, or lost it.
    pub(super) fn focus(&mut self, gained: bool, out: &mut Vec<Event>) {
        if !gained {
            self.release_everything(out);
        }
        if gained != self.focused {
            self.focused = gained;
            out.push(Event::Focused(gained));
        }
    }

    /// Reports a release for every key and button that is down. Without
    /// the focus we will not see them go up.
    fn release_everything(&mut self, out: &mut Vec<Event>) {
        for &key in Key::ALL {
            let down = &mut self.keys_down[usize::from(key.usage())];
            if *down {
                *down = false;
                out.push(Event::Key {
                    key,
                    pressed: false,
                    repeat: false,
                });
            }
        }
        for &button in MouseButton::ALL {
            let down = &mut self.buttons_down[button as usize];
            if *down {
                *down = false;
                out.push(Event::MouseButton {
                    button,
                    pressed: false,
                });
            }
        }
    }

    /// The inside of the window has this size now, in physical pixels.
    pub(super) fn resize(&mut self, size: UVec2, out: &mut Vec<Event>) {
        if size.x == 0 || size.y == 0 || size == self.size {
            return;
        }
        self.size = size;
        out.push(Event::Resized(size));
    }
}

#[cfg(test)]
mod tests {
    use nadir_sys::macos::appkit::{
        NSEventModifierFlagCapsLock, NSEventTypeKeyDown, NSEventTypeLeftMouseDown,
        NSEventTypeLeftMouseUp, NSEventTypeOtherMouseDown, NSEventTypeOtherMouseUp,
        NSEventTypeRightMouseDown, NSEventTypeRightMouseUp,
    };
    use nadir_sys::macos::hitoolbox::{
        kVK_ANSI_A, kVK_ANSI_W, kVK_CapsLock, kVK_Command, kVK_RightShift, kVK_Shift,
    };

    use super::*;

    /// A key with a code and no [`Key`]. `HIToolbox/Events.h`: `kVK_VolumeUp
    /// = 0x48,`
    const VOLUME_UP: u16 = 0x48;

    fn translator() -> Translator {
        Translator::new(UVec2::new(1280, 720), 2.0)
    }

    /// Feeds the events to a translator and returns what comes out.
    fn run(translator: &mut Translator, incoming: &[Incoming]) -> Vec<Event> {
        let mut out = Vec::new();
        for &event in incoming {
            translator.handle(event, &mut out);
        }
        out
    }

    fn press(code: u16) -> Incoming {
        Incoming::Key {
            pressed: true,
            code,
        }
    }

    fn release(code: u16) -> Incoming {
        Incoming::Key {
            pressed: false,
            code,
        }
    }

    fn flags(code: u16, flags: NSUInteger) -> Incoming {
        Incoming::FlagsChanged { code, flags }
    }

    fn key_event(key: Key, pressed: bool, repeat: bool) -> Event {
        Event::Key {
            key,
            pressed,
            repeat,
        }
    }

    // -- helpers ------------------------------------------------------------

    #[test]
    fn button_events_are_told_from_the_others() {
        for event_type in [
            NSEventTypeLeftMouseDown,
            NSEventTypeLeftMouseUp,
            NSEventTypeRightMouseDown,
            NSEventTypeRightMouseUp,
            NSEventTypeOtherMouseDown,
            NSEventTypeOtherMouseUp,
        ] {
            assert!(is_button_event(event_type), "{event_type}");
            // Every button event names a button for some number.
            assert!((0..5).any(|number| mouse_button(event_type, number).is_some()));
        }
        // Mouse moved, a drag, a key, a scroll.
        for event_type in [5, 6, 10, 22, 27] {
            assert!(!is_button_event(event_type), "{event_type}");
        }
    }

    #[test]
    fn mouse_events_name_their_buttons() {
        assert_eq!(
            mouse_button(NSEventTypeLeftMouseDown, 0),
            Some((true, MouseButton::Left))
        );
        assert_eq!(
            mouse_button(NSEventTypeLeftMouseUp, 0),
            Some((false, MouseButton::Left))
        );
        assert_eq!(
            mouse_button(NSEventTypeRightMouseDown, 1),
            Some((true, MouseButton::Right))
        );
        assert_eq!(
            mouse_button(NSEventTypeRightMouseUp, 1),
            Some((false, MouseButton::Right))
        );
        let other = |pressed: bool, number| {
            let event_type = if pressed {
                NSEventTypeOtherMouseDown
            } else {
                NSEventTypeOtherMouseUp
            };
            mouse_button(event_type, number)
        };
        assert_eq!(other(true, 2), Some((true, MouseButton::Middle)));
        assert_eq!(other(true, 3), Some((true, MouseButton::Back)));
        assert_eq!(other(false, 4), Some((false, MouseButton::Forward)));
        assert_eq!(other(true, 5), None);
        assert_eq!(mouse_button(NSEventTypeKeyDown, 0), None);
    }

    #[test]
    fn a_wheel_scrolls_in_lines_and_a_trackpad_in_points() {
        // A wheel turned away from the user, without natural scrolling.
        assert_eq!(wheel_lines(0.0, 1.0, false, false), Vec2::new(0.0, 1.0));
        // A trackpad, 20 points to the left.
        assert_eq!(wheel_lines(20.0, 0.0, true, false), Vec2::new(-2.0, 0.0));
    }

    #[test]
    fn natural_scrolling_does_not_change_the_direction_reported() {
        assert_eq!(wheel_lines(0.0, -1.0, false, true), Vec2::new(0.0, 1.0));
        assert_eq!(wheel_lines(-20.0, 30.0, true, true), Vec2::new(-2.0, -3.0));
    }

    #[test]
    fn a_point_is_scaled_to_pixels() {
        let point = NSPoint { x: 10.5, y: 20.0 };
        assert_eq!(to_pixels(point, 2.0), Vec2::new(21.0, 40.0));
        assert_eq!(to_pixels(point, 1.0), Vec2::new(10.5, 20.0));
    }

    #[test]
    fn a_modifier_is_down_by_its_own_side_bit() {
        let left_shift = NSEventModifierFlagShift | NX_DEVICELSHIFTKEYMASK;
        assert_eq!(modifier_down(Key::LeftShift, left_shift), Some(true));
        assert_eq!(modifier_down(Key::RightShift, left_shift), Some(false));
        let right_command = NSEventModifierFlagCommand | NX_DEVICERCMDKEYMASK;
        assert_eq!(modifier_down(Key::RightSuper, right_command), Some(true));
        assert_eq!(modifier_down(Key::LeftSuper, right_command), Some(false));
        assert_eq!(
            modifier_down(Key::RightControl, NX_DEVICERCTLKEYMASK),
            Some(true)
        );
        assert_eq!(
            modifier_down(Key::LeftAlt, NX_DEVICELALTKEYMASK),
            Some(true)
        );
        assert_eq!(
            modifier_down(Key::RightAlt, NX_DEVICELALTKEYMASK),
            Some(false)
        );
        assert_eq!(modifier_down(Key::A, left_shift), None);
    }

    #[test]
    fn without_side_bits_the_flag_for_either_side_decides() {
        assert_eq!(
            modifier_down(Key::LeftControl, NSEventModifierFlagControl),
            Some(true)
        );
        assert_eq!(
            modifier_down(Key::RightControl, NSEventModifierFlagControl),
            Some(true)
        );
        assert_eq!(modifier_down(Key::LeftAlt, 0), Some(false));
    }

    // -- translating --------------------------------------------------------

    #[test]
    fn the_scale_factor_is_reported_once_before_anything_else() {
        let mut translator = translator();
        let mut out = Vec::new();
        translator.begin_pump(&mut out);
        translator.handle(press(kVK_ANSI_A), &mut out);
        translator.begin_pump(&mut out);
        assert_eq!(
            out,
            [
                Event::ScaleFactorChanged(2.0),
                key_event(Key::A, true, false)
            ]
        );
    }

    #[test]
    fn a_new_scale_is_reported_when_it_changes() {
        let mut translator = translator();
        let mut out = Vec::new();
        translator.set_scale(1.0, &mut out);
        assert!(out.is_empty());
        translator.begin_pump(&mut out);
        translator.set_scale(1.0, &mut out);
        translator.set_scale(2.0, &mut out);
        assert_eq!(
            out,
            [
                Event::ScaleFactorChanged(1.0),
                Event::ScaleFactorChanged(2.0)
            ]
        );
    }

    #[test]
    fn a_key_goes_down_repeats_and_comes_up() {
        let mut translator = translator();
        let out = run(
            &mut translator,
            &[press(kVK_ANSI_W), press(kVK_ANSI_W), release(kVK_ANSI_W)],
        );
        assert_eq!(
            out,
            [
                key_event(Key::W, true, false),
                key_event(Key::W, true, true),
                key_event(Key::W, false, false),
            ]
        );
    }

    #[test]
    fn a_release_without_a_press_and_a_code_with_no_key_are_ignored() {
        let mut translator = translator();
        let out = run(&mut translator, &[release(kVK_ANSI_A), press(VOLUME_UP)]);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn modifiers_go_down_and_up_by_their_flags() {
        let mut translator = translator();
        let left = NSEventModifierFlagShift | NX_DEVICELSHIFTKEYMASK;
        let both = left | NX_DEVICERSHIFTKEYMASK;
        let right = NSEventModifierFlagShift | NX_DEVICERSHIFTKEYMASK;
        let out = run(
            &mut translator,
            &[
                flags(kVK_Shift, left),
                flags(kVK_RightShift, both),
                flags(kVK_Shift, right),
                flags(kVK_RightShift, 0),
            ],
        );
        assert_eq!(
            out,
            [
                key_event(Key::LeftShift, true, false),
                key_event(Key::RightShift, true, false),
                key_event(Key::LeftShift, false, false),
                key_event(Key::RightShift, false, false),
            ]
        );
    }

    #[test]
    fn a_flags_change_that_changes_nothing_reports_nothing() {
        let mut translator = translator();
        let command = NSEventModifierFlagCommand | NX_DEVICELCMDKEYMASK;
        let out = run(
            &mut translator,
            &[flags(kVK_Command, command), flags(kVK_Command, command)],
        );
        assert_eq!(out, [key_event(Key::LeftSuper, true, false)]);
        // A release of a modifier we never saw go down is not reported.
        assert!(run(&mut translator, &[flags(kVK_Shift, 0)]).is_empty());
    }

    #[test]
    fn caps_lock_is_a_tap_on_each_change_of_its_state() {
        let mut translator = translator();
        let out = run(
            &mut translator,
            &[
                flags(kVK_CapsLock, NSEventModifierFlagCapsLock),
                flags(kVK_CapsLock, 0),
            ],
        );
        let tap = [
            key_event(Key::CapsLock, true, false),
            key_event(Key::CapsLock, false, false),
        ];
        assert_eq!(out, [tap, tap].concat());
    }

    #[test]
    fn a_button_goes_down_and_up_where_the_pointer_is() {
        let mut translator = translator();
        let position = Vec2::new(10.0, 20.0);
        let button = |pressed| Incoming::Button {
            pressed,
            button: MouseButton::Left,
            position,
        };
        let out = run(
            &mut translator,
            &[button(true), button(false), button(false)],
        );
        assert_eq!(
            out,
            [
                Event::MouseMoved(position),
                Event::MouseButton {
                    button: MouseButton::Left,
                    pressed: true
                },
                Event::MouseButton {
                    button: MouseButton::Left,
                    pressed: false
                },
            ]
        );
    }

    #[test]
    fn motion_is_reported_when_the_position_changes() {
        let mut translator = translator();
        let motion = |x, y| Incoming::Motion {
            position: Vec2::new(x, y),
        };
        let out = run(
            &mut translator,
            &[motion(1.0, 2.0), motion(1.0, 2.0), motion(3.5, 2.0)],
        );
        assert_eq!(
            out,
            [
                Event::MouseMoved(Vec2::new(1.0, 2.0)),
                Event::MouseMoved(Vec2::new(3.5, 2.0)),
            ]
        );
    }

    #[test]
    fn a_scroll_of_nothing_is_not_reported() {
        let mut translator = translator();
        let out = run(
            &mut translator,
            &[
                Incoming::Wheel(Vec2::ZERO),
                Incoming::Wheel(Vec2::new(0.0, -1.0)),
            ],
        );
        assert_eq!(out, [Event::MouseWheel(Vec2::new(0.0, -1.0))]);
    }

    #[test]
    fn losing_the_focus_releases_what_is_held() {
        let mut translator = translator();
        let mut out = Vec::new();
        translator.focus(true, &mut out);
        translator.handle(press(kVK_ANSI_A), &mut out);
        translator.handle(
            flags(kVK_Shift, NSEventModifierFlagShift | NX_DEVICELSHIFTKEYMASK),
            &mut out,
        );
        translator.handle(
            Incoming::Button {
                pressed: true,
                button: MouseButton::Right,
                position: Vec2::ZERO,
            },
            &mut out,
        );
        out.clear();
        translator.focus(false, &mut out);
        assert_eq!(
            out,
            [
                key_event(Key::A, false, false),
                key_event(Key::LeftShift, false, false),
                Event::MouseButton {
                    button: MouseButton::Right,
                    pressed: false
                },
                Event::Focused(false),
            ]
        );
        out.clear();
        translator.focus(false, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn a_new_size_is_reported_once_and_close_asks_to_close() {
        let mut translator = translator();
        let mut out = Vec::new();
        translator.resize(UVec2::new(1280, 720), &mut out);
        translator.resize(UVec2::new(1600, 900), &mut out);
        translator.resize(UVec2::new(1600, 900), &mut out);
        translator.resize(UVec2::new(0, 900), &mut out);
        translator.close(&mut out);
        assert_eq!(
            out,
            [Event::Resized(UVec2::new(1600, 900)), Event::CloseRequested]
        );
        assert_eq!(translator.size(), UVec2::new(1600, 900));
    }
}
