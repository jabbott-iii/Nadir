//! From X events to [`Event`]s.
//!
//! [`Incoming::decode`] reads an X event out of Xlib's union, and
//! [`Translator`] turns it into the events of this crate. The translator
//! needs no X server, so its rules are tested here.

use core::ffi::{c_int, c_long, c_uint};

use nadir_core::{Key, MouseButton, UVec2, Vec2};
// The constants are used as `xlib::KeyPress`, not imported by name: in a
// pattern, a bare name that is not upper case reads as a new variable, and
// the compiler warns about it.
use nadir_sys::linux::x11::{self as xlib, Atom, XEvent};

use super::super::keymap;
use crate::Event;

/// The X button numbers above [`xlib::Button5`], which `X11/X.h` does not
/// name. X.Org's input drivers use them like this by convention.
mod extra_buttons {
    use core::ffi::c_uint;

    /// One step of the wheel to the left.
    pub(super) const WHEEL_LEFT: c_uint = 6;
    /// One step of the wheel to the right.
    pub(super) const WHEEL_RIGHT: c_uint = 7;
    /// The first side button.
    pub(super) const BACK: c_uint = 8;
    /// The second side button.
    pub(super) const FORWARD: c_uint = 9;
}

/// An X event, reduced to the kinds and fields we use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Incoming {
    /// A key went down or up.
    Key { pressed: bool, keycode: c_uint },
    /// A pointer button went down or up, with the pointer at (`x`, `y`).
    Button {
        pressed: bool,
        button: c_uint,
        x: c_int,
        y: c_int,
    },
    /// The pointer moved to (`x`, `y`).
    Motion { x: c_int, y: c_int },
    /// The keyboard focus came or went. `mode` says why.
    Focus { gained: bool, mode: c_int },
    /// The window has this size now, or moved.
    Configure { width: c_int, height: c_int },
    /// A message from another client. `first` is the first item of its
    /// data.
    ClientMessage {
        message_type: Atom,
        format: c_int,
        first: c_long,
    },
    /// An event we have no use for.
    Other,
}

impl Incoming {
    /// Reads an event out of the union.
    ///
    /// # Safety
    ///
    /// The event's type must be initialized, and so must the member that
    /// the type selects. For a client message, that includes all 40 bytes of
    /// its data, whatever its format. An event that Xlib wrote is like that.
    pub(super) unsafe fn decode(event: &XEvent) -> Self {
        // SAFETY: the caller promises that the type is initialized. Every
        // member starts with it.
        let type_ = unsafe { event.type_ };
        match type_ {
            xlib::KeyPress | xlib::KeyRelease => {
                // SAFETY: the type selects `xkey`, which the caller promises
                // is initialized.
                let key = unsafe { event.xkey };
                Self::Key {
                    pressed: type_ == xlib::KeyPress,
                    keycode: key.keycode,
                }
            }
            xlib::ButtonPress | xlib::ButtonRelease => {
                // SAFETY: the type selects `xbutton`, which the caller
                // promises is initialized.
                let button = unsafe { event.xbutton };
                Self::Button {
                    pressed: type_ == xlib::ButtonPress,
                    button: button.button,
                    x: button.x,
                    y: button.y,
                }
            }
            xlib::MotionNotify => {
                // SAFETY: the type selects `xmotion`, which the caller
                // promises is initialized.
                let motion = unsafe { event.xmotion };
                Self::Motion {
                    x: motion.x,
                    y: motion.y,
                }
            }
            xlib::FocusIn | xlib::FocusOut => {
                // SAFETY: the type selects `xfocus`, which the caller
                // promises is initialized.
                let focus = unsafe { event.xfocus };
                Self::Focus {
                    gained: type_ == xlib::FocusIn,
                    mode: focus.mode,
                }
            }
            xlib::ConfigureNotify => {
                // SAFETY: the type selects `xconfigure`, which the caller
                // promises is initialized.
                let configure = unsafe { event.xconfigure };
                Self::Configure {
                    width: configure.width,
                    height: configure.height,
                }
            }
            xlib::ClientMessage => {
                // SAFETY: the type selects `xclient`, which the caller
                // promises is initialized.
                let client = unsafe { event.xclient };
                // SAFETY: the caller promises that all 40 bytes of the data
                // are initialized, and any bytes are five valid `long`s.
                // Whether they mean anything depends on the format, which
                // the translator checks.
                let longs = unsafe { client.data.l };
                Self::ClientMessage {
                    message_type: client.message_type,
                    format: client.format,
                    first: longs[0],
                }
            }
            _ => Self::Other,
        }
    }
}

/// What an X button number means.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pointer {
    /// A button.
    Button(MouseButton),
    /// One step of the wheel, in lines.
    Wheel(Vec2),
}

/// Returns what an X button number means, if anything. The wheel is four
/// "buttons", each pressed and released at once for one step.
fn pointer(button: c_uint) -> Option<Pointer> {
    Some(match button {
        xlib::Button1 => Pointer::Button(MouseButton::Left),
        xlib::Button2 => Pointer::Button(MouseButton::Middle),
        xlib::Button3 => Pointer::Button(MouseButton::Right),
        xlib::Button4 => Pointer::Wheel(Vec2::new(0.0, 1.0)),
        xlib::Button5 => Pointer::Wheel(Vec2::new(0.0, -1.0)),
        extra_buttons::WHEEL_LEFT => Pointer::Wheel(Vec2::new(-1.0, 0.0)),
        extra_buttons::WHEEL_RIGHT => Pointer::Wheel(Vec2::new(1.0, 0.0)),
        extra_buttons::BACK => Pointer::Button(MouseButton::Back),
        extra_buttons::FORWARD => Pointer::Button(MouseButton::Forward),
        _ => return None,
    })
}

/// The atoms that mark the window manager's "close" message.
#[derive(Clone, Copy, Debug)]
pub(super) struct CloseMessage {
    /// `WM_PROTOCOLS`: the type of the message.
    pub(super) wm_protocols: Atom,
    /// `WM_DELETE_WINDOW`: the first item of its data.
    pub(super) wm_delete_window: Atom,
}

/// Turns X events into [`Event`]s, and remembers what that takes: which
/// keys and buttons are down, where the pointer is, and the window's size.
pub(super) struct Translator {
    close: CloseMessage,
    scale: f32,
    /// Whether the scale factor has been reported.
    scale_announced: bool,
    size: UVec2,
    focused: bool,
    pointer: Option<(c_int, c_int)>,
    /// Which keys are down, by [`Key::usage`].
    keys_down: [bool; 256],
    /// Which buttons are down, in the order of [`MouseButton::ALL`].
    buttons_down: [bool; MouseButton::ALL.len()],
}

impl Translator {
    /// A translator for a window of `size`, at the scale factor `scale`,
    /// that has seen no events yet.
    pub(super) fn new(close: CloseMessage, size: UVec2, scale: f32) -> Self {
        Self {
            close,
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

    /// Call this at the start of every pump, before any [`Translator::handle`].
    /// The first time, it reports the scale factor, which therefore comes
    /// before every other event.
    pub(super) fn begin_pump(&mut self, out: &mut Vec<Event>) {
        if !self.scale_announced {
            self.scale_announced = true;
            out.push(Event::ScaleFactorChanged(self.scale));
        }
    }

    /// Appends the events that `incoming` amounts to, which may be none.
    pub(super) fn handle(&mut self, incoming: Incoming, out: &mut Vec<Event>) {
        match incoming {
            Incoming::Key { pressed, keycode } => self.key(pressed, keycode, out),
            Incoming::Button {
                pressed,
                button,
                x,
                y,
            } => {
                // A click can come with no motion before it, when the
                // window appears under the pointer.
                self.move_pointer(x, y, out);
                self.button(pressed, button, out);
            }
            Incoming::Motion { x, y } => self.move_pointer(x, y, out),
            Incoming::Focus { gained, mode } => self.focus(gained, mode, out),
            Incoming::Configure { width, height } => self.configure(width, height, out),
            Incoming::ClientMessage {
                message_type,
                format,
                first,
            } => {
                // An atom is an `unsigned long` that travels as a `long`.
                let is_close = message_type == self.close.wm_protocols
                    && format == 32
                    && first as Atom == self.close.wm_delete_window;
                if is_close {
                    out.push(Event::CloseRequested);
                }
            }
            Incoming::Other => {}
        }
    }

    fn key(&mut self, pressed: bool, keycode: c_uint, out: &mut Vec<Event>) {
        let Some(key) = keymap::from_x_keycode(keycode) else {
            return;
        };
        let down = &mut self.keys_down[usize::from(key.usage())];
        // A release of a key we never saw go down, which happens when the
        // key was pressed while another window had the focus, is not
        // reported: every release follows a press.
        if !pressed && !*down {
            return;
        }
        // With detectable auto-repeat, a held key repeats as presses only.
        let repeat = pressed && *down;
        *down = pressed;
        out.push(Event::Key {
            key,
            pressed,
            repeat,
        });
    }

    fn button(&mut self, pressed: bool, button: c_uint, out: &mut Vec<Event>) {
        match pointer(button) {
            Some(Pointer::Button(button)) => {
                let down = &mut self.buttons_down[button as usize];
                if pressed == *down {
                    // A second press without a release, or a release with
                    // no press: see `key`.
                    return;
                }
                *down = pressed;
                out.push(Event::MouseButton { button, pressed });
            }
            // The release that follows each step at once says nothing new.
            Some(Pointer::Wheel(lines)) if pressed => out.push(Event::MouseWheel(lines)),
            Some(Pointer::Wheel(_)) | None => {}
        }
    }

    fn move_pointer(&mut self, x: c_int, y: c_int, out: &mut Vec<Event>) {
        if self.pointer == Some((x, y)) {
            return;
        }
        self.pointer = Some((x, y));
        out.push(Event::MouseMoved(Vec2::new(x as f32, y as f32)));
    }

    fn focus(&mut self, gained: bool, mode: c_int, out: &mut Vec<Event>) {
        // A keyboard grab, such as a window manager takes for a shortcut or
        // while it switches windows, moves the focus away and back. The
        // window is still the one the user works in, so this is not a
        // change of focus. If another window gets the focus while the grab
        // lasts, a normal event says so.
        if mode == xlib::NotifyGrab || mode == xlib::NotifyUngrab {
            return;
        }
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

    fn configure(&mut self, width: c_int, height: c_int, out: &mut Vec<Event>) {
        let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
            return;
        };
        let size = UVec2::new(width, height);
        // The same event also reports moves, which do not change the size.
        if width == 0 || height == 0 || size == self.size {
            return;
        }
        self.size = size;
        out.push(Event::Resized(size));
    }
}

#[cfg(test)]
mod tests {
    use core::ptr;

    use nadir_sys::linux::x11::{
        ButtonPress, ButtonRelease, ClientMessage, ConfigureNotify, False, FocusIn, FocusOut,
        KeyPress, KeyRelease, MotionNotify, NotifyGrab, NotifyUngrab, XButtonEvent,
        XClientMessageData, XClientMessageEvent, XConfigureEvent, XFocusChangeEvent, XKeyEvent,
        XMotionEvent,
    };

    use super::*;

    const WM_PROTOCOLS: Atom = 300;
    const WM_DELETE_WINDOW: Atom = 301;
    /// `X11/X.h`: `#define NotifyNormal 0`
    const NOTIFY_NORMAL: c_int = 0;
    /// `X11/X.h`: `#define NotifyWhileGrabbed 3`
    const NOTIFY_WHILE_GRABBED: c_int = 3;

    // X keycodes: the evdev code plus 8.
    const KEYCODE_W: c_uint = 17 + 8;
    const KEYCODE_A: c_uint = 30 + 8;
    const KEYCODE_LEFT_SHIFT: c_uint = 42 + 8;
    const KEYCODE_MUTE: c_uint = 113 + 8;

    fn translator() -> Translator {
        let close = CloseMessage {
            wm_protocols: WM_PROTOCOLS,
            wm_delete_window: WM_DELETE_WINDOW,
        };
        Translator::new(close, UVec2::new(640, 360), 1.5)
    }

    /// Feeds the events to a translator and returns what comes out.
    fn run(translator: &mut Translator, incoming: &[Incoming]) -> Vec<Event> {
        let mut out = Vec::new();
        for &event in incoming {
            translator.handle(event, &mut out);
        }
        out
    }

    fn press(keycode: c_uint) -> Incoming {
        Incoming::Key {
            pressed: true,
            keycode,
        }
    }

    fn release(keycode: c_uint) -> Incoming {
        Incoming::Key {
            pressed: false,
            keycode,
        }
    }

    fn button(pressed: bool, button: c_uint) -> Incoming {
        Incoming::Button {
            pressed,
            button,
            x: 10,
            y: 20,
        }
    }

    fn focus(gained: bool) -> Incoming {
        Incoming::Focus {
            gained,
            mode: NOTIFY_NORMAL,
        }
    }

    fn key_event(key: Key, pressed: bool, repeat: bool) -> Event {
        Event::Key {
            key,
            pressed,
            repeat,
        }
    }

    fn button_event(button: MouseButton, pressed: bool) -> Event {
        Event::MouseButton { button, pressed }
    }

    const AT_10_20: Event = Event::MouseMoved(Vec2::new(10.0, 20.0));

    // -- the scale factor --------------------------------------------------

    #[test]
    fn the_scale_factor_is_reported_once_before_anything_else() {
        let mut translator = translator();
        assert_eq!(translator.scale(), 1.5);
        let mut out = Vec::new();
        translator.begin_pump(&mut out);
        translator.handle(focus(true), &mut out);
        translator.begin_pump(&mut out);
        translator.handle(press(KEYCODE_W), &mut out);
        assert_eq!(
            out,
            [
                Event::ScaleFactorChanged(1.5),
                Event::Focused(true),
                key_event(Key::W, true, false),
            ]
        );
    }

    // -- keys ---------------------------------------------------------------

    #[test]
    fn a_key_goes_down_and_up() {
        let out = run(&mut translator(), &[press(KEYCODE_W), release(KEYCODE_W)]);
        assert_eq!(
            out,
            [
                key_event(Key::W, true, false),
                key_event(Key::W, false, false)
            ]
        );
    }

    #[test]
    fn presses_of_a_key_that_is_down_are_repeats() {
        let mut translator = translator();
        let out = run(
            &mut translator,
            &[
                press(KEYCODE_W),
                press(KEYCODE_W),
                press(KEYCODE_A),
                press(KEYCODE_W),
                release(KEYCODE_W),
                press(KEYCODE_W),
            ],
        );
        assert_eq!(
            out,
            [
                key_event(Key::W, true, false),
                key_event(Key::W, true, true),
                key_event(Key::A, true, false),
                key_event(Key::W, true, true),
                key_event(Key::W, false, false),
                // Down again after a release: a new press.
                key_event(Key::W, true, false),
            ]
        );
    }

    #[test]
    fn a_release_without_a_press_is_not_reported() {
        let mut translator = translator();
        assert_eq!(run(&mut translator, &[release(KEYCODE_W)]), []);
        // Nor is a second release.
        let out = run(
            &mut translator,
            &[press(KEYCODE_W), release(KEYCODE_W), release(KEYCODE_W)],
        );
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn a_keycode_with_no_key_is_ignored() {
        let out = run(
            &mut translator(),
            &[
                press(KEYCODE_MUTE),
                release(KEYCODE_MUTE),
                press(0),
                press(7),
            ],
        );
        assert_eq!(out, []);
    }

    // -- the pointer --------------------------------------------------------

    #[test]
    fn x_button_numbers_map_to_buttons_and_wheel_steps() {
        let expected = [
            (1, Pointer::Button(MouseButton::Left)),
            (2, Pointer::Button(MouseButton::Middle)),
            (3, Pointer::Button(MouseButton::Right)),
            (4, Pointer::Wheel(Vec2::new(0.0, 1.0))),
            (5, Pointer::Wheel(Vec2::new(0.0, -1.0))),
            (6, Pointer::Wheel(Vec2::new(-1.0, 0.0))),
            (7, Pointer::Wheel(Vec2::new(1.0, 0.0))),
            (8, Pointer::Button(MouseButton::Back)),
            (9, Pointer::Button(MouseButton::Forward)),
        ];
        for (number, meaning) in expected {
            assert_eq!(pointer(number), Some(meaning), "button {number}");
        }
        for number in [0, 10, 11, 255, c_uint::MAX] {
            assert_eq!(pointer(number), None, "button {number}");
        }
    }

    #[test]
    fn every_mouse_button_can_be_reported() {
        for &wanted in MouseButton::ALL {
            let numbers = (0..32).filter(|&n| pointer(n) == Some(Pointer::Button(wanted)));
            assert_eq!(numbers.count(), 1, "{wanted:?}");
        }
    }

    #[test]
    fn a_button_goes_down_and_up_where_the_pointer_is() {
        let out = run(&mut translator(), &[button(true, 1), button(false, 1)]);
        assert_eq!(
            out,
            [
                // The position comes first, and once.
                AT_10_20,
                button_event(MouseButton::Left, true),
                button_event(MouseButton::Left, false),
            ]
        );
    }

    #[test]
    fn a_button_release_without_a_press_is_not_reported() {
        let mut translator = translator();
        assert_eq!(run(&mut translator, &[button(false, 3)]), [AT_10_20]);
        let out = run(&mut translator, &[button(true, 3), button(true, 3)]);
        assert_eq!(out, [button_event(MouseButton::Right, true)]);
    }

    #[test]
    fn a_wheel_step_is_one_event() {
        let out = run(
            &mut translator(),
            &[
                button(true, 4),
                button(false, 4),
                button(true, 5),
                button(false, 5),
                button(true, 6),
                button(false, 6),
                button(true, 7),
                button(false, 7),
            ],
        );
        assert_eq!(
            out,
            [
                AT_10_20,
                Event::MouseWheel(Vec2::new(0.0, 1.0)),
                Event::MouseWheel(Vec2::new(0.0, -1.0)),
                Event::MouseWheel(Vec2::new(-1.0, 0.0)),
                Event::MouseWheel(Vec2::new(1.0, 0.0)),
            ]
        );
    }

    #[test]
    fn an_unknown_button_reports_only_the_position() {
        let out = run(&mut translator(), &[button(true, 12), button(false, 12)]);
        assert_eq!(out, [AT_10_20]);
    }

    #[test]
    fn motion_is_reported_when_the_position_changes() {
        let out = run(
            &mut translator(),
            &[
                Incoming::Motion { x: 0, y: 0 },
                Incoming::Motion { x: 0, y: 0 },
                Incoming::Motion { x: 3, y: -2 },
                Incoming::Motion { x: 3, y: 5 },
            ],
        );
        assert_eq!(
            out,
            [
                // The first position is news, even if it is the origin.
                Event::MouseMoved(Vec2::new(0.0, 0.0)),
                // A held button keeps reporting when the pointer leaves the
                // window, so a coordinate can be negative.
                Event::MouseMoved(Vec2::new(3.0, -2.0)),
                Event::MouseMoved(Vec2::new(3.0, 5.0)),
            ]
        );
    }

    // -- focus --------------------------------------------------------------

    #[test]
    fn focus_is_reported_when_it_changes() {
        let mut translator = translator();
        let out = run(
            &mut translator,
            &[
                focus(true),
                focus(true),
                focus(false),
                focus(false),
                focus(true),
            ],
        );
        assert_eq!(
            out,
            [
                Event::Focused(true),
                Event::Focused(false),
                Event::Focused(true)
            ]
        );
    }

    #[test]
    fn gaining_the_focus_releases_nothing() {
        let mut translator = translator();
        let out = run(
            &mut translator,
            &[
                // Down before any focus event, then the focus arrives, twice.
                press(KEYCODE_W),
                button(true, 1),
                focus(true),
                focus(true),
                press(KEYCODE_W),
                button(false, 1),
            ],
        );
        assert_eq!(
            out,
            [
                key_event(Key::W, true, false),
                AT_10_20,
                button_event(MouseButton::Left, true),
                Event::Focused(true),
                // Both were still down.
                key_event(Key::W, true, true),
                button_event(MouseButton::Left, false),
            ]
        );
    }

    #[test]
    fn losing_the_focus_releases_what_is_held() {
        let mut translator = translator();
        let out = run(
            &mut translator,
            &[
                focus(true),
                press(KEYCODE_W),
                press(KEYCODE_LEFT_SHIFT),
                press(KEYCODE_A),
                release(KEYCODE_A),
                button(true, 3),
                button(true, 1),
                focus(false),
            ],
        );
        assert_eq!(
            out[out.len() - 5..],
            [
                // Keys in the order of `Key::ALL`, then buttons in the
                // order of `MouseButton::ALL`, then the news itself.
                key_event(Key::W, false, false),
                key_event(Key::LeftShift, false, false),
                button_event(MouseButton::Left, false),
                button_event(MouseButton::Right, false),
                Event::Focused(false),
            ]
        );

        // The real releases, which may still arrive, add nothing.
        let late = run(
            &mut translator,
            &[release(KEYCODE_W), button(false, 1), focus(false)],
        );
        assert_eq!(late, []);

        // And a key that goes down after that is a new press, not a repeat.
        let again = run(&mut translator, &[focus(true), press(KEYCODE_W)]);
        assert_eq!(
            again,
            [Event::Focused(true), key_event(Key::W, true, false)]
        );
    }

    #[test]
    fn a_focus_loss_we_did_not_see_coming_still_releases() {
        // Keys can go down with no focus event before them, for example
        // when the focus follows the pointer.
        let out = run(&mut translator(), &[press(KEYCODE_W), focus(false)]);
        assert_eq!(
            out,
            [
                key_event(Key::W, true, false),
                key_event(Key::W, false, false)
            ]
        );
    }

    #[test]
    fn a_keyboard_grab_is_not_a_change_of_focus() {
        let mut translator = translator();
        let grab = |gained, mode| Incoming::Focus { gained, mode };
        let out = run(
            &mut translator,
            &[
                focus(true),
                press(KEYCODE_W),
                grab(false, NotifyGrab),
                grab(true, NotifyUngrab),
                grab(false, NotifyUngrab),
                grab(true, NotifyGrab),
                press(KEYCODE_W),
            ],
        );
        assert_eq!(
            out,
            [
                Event::Focused(true),
                key_event(Key::W, true, false),
                // Still held, so this is a repeat.
                key_event(Key::W, true, true),
            ]
        );

        // The focus moving to another window during a grab does count.
        let out = run(&mut translator, &[grab(false, NOTIFY_WHILE_GRABBED)]);
        assert_eq!(
            out,
            [key_event(Key::W, false, false), Event::Focused(false)]
        );
    }

    // -- size and closing ---------------------------------------------------

    #[test]
    fn a_new_size_is_reported_once() {
        let mut translator = translator();
        assert_eq!(translator.size(), UVec2::new(640, 360));
        let configure = |width, height| Incoming::Configure { width, height };
        let out = run(
            &mut translator,
            &[
                // The window was only moved.
                configure(640, 360),
                configure(800, 600),
                configure(800, 600),
                configure(800, 601),
            ],
        );
        assert_eq!(
            out,
            [
                Event::Resized(UVec2::new(800, 600)),
                Event::Resized(UVec2::new(800, 601)),
            ]
        );
        assert_eq!(translator.size(), UVec2::new(800, 601));
    }

    #[test]
    fn a_size_that_cannot_be_is_ignored() {
        let mut translator = translator();
        let configure = |width, height| Incoming::Configure { width, height };
        let out = run(
            &mut translator,
            &[
                configure(0, 600),
                configure(800, 0),
                configure(-1, 600),
                configure(800, -600),
            ],
        );
        assert_eq!(out, []);
        assert_eq!(translator.size(), UVec2::new(640, 360));
    }

    #[test]
    fn the_window_managers_close_message_asks_to_close() {
        let message = |message_type, format, first: Atom| Incoming::ClientMessage {
            message_type,
            format,
            first: first as c_long,
        };
        let close = message(WM_PROTOCOLS, 32, WM_DELETE_WINDOW);
        assert_eq!(run(&mut translator(), &[close]), [Event::CloseRequested]);

        let others = [
            // Another protocol, such as a ping.
            message(WM_PROTOCOLS, 32, WM_DELETE_WINDOW + 1),
            // Another kind of message with the same number in it.
            message(WM_DELETE_WINDOW, 32, WM_DELETE_WINDOW),
            message(WM_PROTOCOLS + 5, 32, WM_DELETE_WINDOW),
            // The right atoms in the wrong format.
            message(WM_PROTOCOLS, 8, WM_DELETE_WINDOW),
            message(WM_PROTOCOLS, 16, WM_DELETE_WINDOW),
            Incoming::Other,
        ];
        assert_eq!(run(&mut translator(), &others), []);
    }

    // -- decoding -----------------------------------------------------------

    /// An event with every byte set, so that any member can be read.
    fn blank() -> XEvent {
        XEvent { pad: [0; 24] }
    }

    fn decode(event: &XEvent) -> Incoming {
        // SAFETY: the events of these tests start from `blank`, and then
        // have the member written that their type selects.
        unsafe { Incoming::decode(event) }
    }

    fn x_key(type_: c_int, keycode: c_uint) -> XEvent {
        let mut event = blank();
        event.xkey = XKeyEvent {
            type_,
            serial: 1,
            send_event: False,
            display: ptr::null_mut(),
            window: 2,
            root: 3,
            subwindow: 4,
            time: 5,
            x: 6,
            y: 7,
            x_root: 8,
            y_root: 9,
            state: 10,
            keycode,
            same_screen: 11,
        };
        event
    }

    fn x_button(type_: c_int, button: c_uint) -> XEvent {
        let mut event = blank();
        event.xbutton = XButtonEvent {
            type_,
            serial: 1,
            send_event: False,
            display: ptr::null_mut(),
            window: 2,
            root: 3,
            subwindow: 4,
            time: 5,
            x: 6,
            y: 7,
            x_root: 8,
            y_root: 9,
            state: 10,
            button,
            same_screen: 11,
        };
        event
    }

    #[test]
    fn key_events_are_decoded() {
        assert_eq!(decode(&x_key(KeyPress, 25)), press(25));
        assert_eq!(decode(&x_key(KeyRelease, 38)), release(38));
    }

    #[test]
    fn button_events_are_decoded() {
        let expected = |pressed| Incoming::Button {
            pressed,
            button: 3,
            x: 6,
            y: 7,
        };
        assert_eq!(decode(&x_button(ButtonPress, 3)), expected(true));
        assert_eq!(decode(&x_button(ButtonRelease, 3)), expected(false));
    }

    #[test]
    fn motion_events_are_decoded() {
        let mut event = blank();
        event.xmotion = XMotionEvent {
            type_: MotionNotify,
            serial: 1,
            send_event: False,
            display: ptr::null_mut(),
            window: 2,
            root: 3,
            subwindow: 4,
            time: 5,
            x: -6,
            y: 7,
            x_root: 8,
            y_root: 9,
            state: 10,
            is_hint: 0,
            same_screen: 11,
        };
        assert_eq!(decode(&event), Incoming::Motion { x: -6, y: 7 });
    }

    #[test]
    fn focus_events_are_decoded() {
        for (type_, gained) in [(FocusIn, true), (FocusOut, false)] {
            let mut event = blank();
            event.xfocus = XFocusChangeEvent {
                type_,
                serial: 1,
                send_event: False,
                display: ptr::null_mut(),
                window: 2,
                mode: NotifyUngrab,
                detail: 3,
            };
            let expected = Incoming::Focus {
                gained,
                mode: NotifyUngrab,
            };
            assert_eq!(decode(&event), expected);
        }
    }

    #[test]
    fn configure_events_are_decoded() {
        let mut event = blank();
        event.xconfigure = XConfigureEvent {
            type_: ConfigureNotify,
            serial: 1,
            send_event: False,
            display: ptr::null_mut(),
            event: 2,
            window: 2,
            x: 3,
            y: 4,
            width: 800,
            height: 600,
            border_width: 5,
            above: 6,
            override_redirect: False,
        };
        let expected = Incoming::Configure {
            width: 800,
            height: 600,
        };
        assert_eq!(decode(&event), expected);
    }

    #[test]
    fn client_messages_are_decoded() {
        let mut event = blank();
        event.xclient = XClientMessageEvent {
            type_: ClientMessage,
            serial: 1,
            send_event: False,
            display: ptr::null_mut(),
            window: 2,
            message_type: WM_PROTOCOLS,
            format: 32,
            data: XClientMessageData {
                l: [WM_DELETE_WINDOW as c_long, 7, 0, 0, 0],
            },
        };
        let expected = Incoming::ClientMessage {
            message_type: WM_PROTOCOLS,
            format: 32,
            first: WM_DELETE_WINDOW as c_long,
        };
        assert_eq!(decode(&event), expected);
        assert_eq!(
            run(&mut translator(), &[decode(&event)]),
            [Event::CloseRequested]
        );
    }

    #[test]
    fn other_events_are_decoded_as_nothing() {
        // 0 and 1 are errors and replies, 12 is `Expose`, 19 is `MapNotify`,
        // and 36 is past the last core event.
        for type_ in [0, 1, 12, 19, 36, -1, c_int::MAX] {
            let mut event = blank();
            event.type_ = type_;
            assert_eq!(decode(&event), Incoming::Other, "type {type_}");
        }
    }
}
