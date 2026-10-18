//! From window messages to [`Event`]s.
//!
//! [`Incoming::decode`] takes a message's parameters apart, and
//! [`Translator`] turns it into the events of this crate. Neither needs a
//! window, so their rules are tested here.

use nadir_core::{Key, MouseButton, UVec2, Vec2};
use nadir_sys::windows::types::{LPARAM, WPARAM};
use nadir_sys::windows::user32::{
    SIZE_MINIMIZED, WHEEL_DELTA, WM_CLOSE, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETFOCUS, WM_SIZE, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN,
    WM_XBUTTONUP, XBUTTON1, XBUTTON2,
};

use super::keymap;
use crate::Event;

/// A window message, reduced to the kinds and fields we use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Incoming {
    /// A key went down or up. Its scan code has `0xE0` in the high byte for
    /// an extended key.
    Key { pressed: bool, scan_code: u16 },
    /// A button went down or up, with the pointer at (`x`, `y`).
    Button {
        pressed: bool,
        button: MouseButton,
        x: i32,
        y: i32,
    },
    /// The pointer moved to (`x`, `y`).
    Motion { x: i32, y: i32 },
    /// The wheel turned, by this many steps.
    Wheel(Vec2),
    /// The keyboard focus came or went.
    Focus { gained: bool },
    /// The inside of the window has this size now.
    Size { width: u32, height: u32 },
    /// The user asked to close the window.
    Close,
    /// A message we have no use for here.
    Other,
}

impl Incoming {
    /// Takes a message apart.
    pub(super) fn decode(message: u32, wparam: WPARAM, lparam: LPARAM) -> Self {
        let button = |pressed, button| {
            let (x, y) = position(lparam);
            Self::Button {
                pressed,
                button,
                x,
                y,
            }
        };
        match message {
            WM_KEYDOWN | WM_SYSKEYDOWN => Self::Key {
                pressed: true,
                scan_code: keymap::scan_code(lparam),
            },
            WM_KEYUP | WM_SYSKEYUP => Self::Key {
                pressed: false,
                scan_code: keymap::scan_code(lparam),
            },
            WM_MOUSEMOVE => {
                let (x, y) = position(lparam);
                Self::Motion { x, y }
            }
            WM_LBUTTONDOWN => button(true, MouseButton::Left),
            WM_LBUTTONUP => button(false, MouseButton::Left),
            WM_RBUTTONDOWN => button(true, MouseButton::Right),
            WM_RBUTTONUP => button(false, MouseButton::Right),
            WM_MBUTTONDOWN => button(true, MouseButton::Middle),
            WM_MBUTTONUP => button(false, MouseButton::Middle),
            WM_XBUTTONDOWN | WM_XBUTTONUP => {
                let pressed = message == WM_XBUTTONDOWN;
                // `windowsx.h`: `GET_XBUTTON_WPARAM(wParam) (HIWORD(wParam))`
                match high_word(wparam) {
                    XBUTTON1 => button(pressed, MouseButton::Back),
                    XBUTTON2 => button(pressed, MouseButton::Forward),
                    _ => Self::Other,
                }
            }
            // Away from the user is +y, and to the right +x, for both.
            WM_MOUSEWHEEL => Self::Wheel(Vec2::new(0.0, wheel_steps(wparam))),
            WM_MOUSEHWHEEL => Self::Wheel(Vec2::new(wheel_steps(wparam), 0.0)),
            WM_SETFOCUS => Self::Focus { gained: true },
            WM_KILLFOCUS => Self::Focus { gained: false },
            // A minimized window reports a size of zero, which is not one it
            // can be drawn at.
            WM_SIZE if wparam != SIZE_MINIMIZED => Self::Size {
                width: u32::from(low_word(lparam as WPARAM)),
                height: u32::from(high_word(lparam as WPARAM)),
            },
            WM_CLOSE => Self::Close,
            _ => Self::Other,
        }
    }
}

/// `minwindef.h`: `LOWORD(l) ((WORD)(((DWORD_PTR)(l)) & 0xffff))`
fn low_word(value: WPARAM) -> u16 {
    (value & 0xFFFF) as u16
}

/// `minwindef.h`: `HIWORD(l) ((WORD)((((DWORD_PTR)(l)) >> 16) & 0xffff))`
fn high_word(value: WPARAM) -> u16 {
    ((value >> 16) & 0xFFFF) as u16
}

/// The pointer position in a mouse message, in pixels from the top-left of
/// the inside of the window. It can be negative, or past the size, while
/// the pointer is captured.
/// `windowsx.h`: `GET_X_LPARAM(lp) ((int)(short)LOWORD(lp))`, and
/// `GET_Y_LPARAM(lp) ((int)(short)HIWORD(lp))`
fn position(lparam: LPARAM) -> (i32, i32) {
    let x = low_word(lparam as WPARAM).cast_signed();
    let y = high_word(lparam as WPARAM).cast_signed();
    (i32::from(x), i32::from(y))
}

/// How far a wheel message turned the wheel, in steps. A wheel with fine
/// steps reports parts of one.
/// `WinUser.h`: `GET_WHEEL_DELTA_WPARAM(wParam) ((short)HIWORD(wParam))`
fn wheel_steps(wparam: WPARAM) -> f32 {
    let delta = high_word(wparam).cast_signed();
    f32::from(delta) / f32::from(WHEEL_DELTA)
}

/// Turns window messages into [`Event`]s, and remembers what that takes:
/// which keys and buttons are down, where the pointer is, and the window's
/// size and scale factor.
pub(super) struct Translator {
    scale: f32,
    /// Whether the scale factor has been reported.
    scale_announced: bool,
    size: UVec2,
    focused: bool,
    pointer: Option<(i32, i32)>,
    /// Which keys are down, by [`Key::usage`].
    keys_down: [bool; 256],
    /// Which buttons are down, in the order of [`MouseButton::ALL`].
    buttons_down: [bool; MouseButton::ALL.len()],
}

impl Translator {
    /// A translator for a window of `size`, at the scale factor `scale`,
    /// that has seen no messages yet.
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

    /// Whether any mouse button is down.
    pub(super) fn any_button_down(&self) -> bool {
        self.buttons_down.contains(&true)
    }

    /// Call this at the start of every pump, before the events of the
    /// messages that came since the last. The first time, it reports the
    /// scale factor, which therefore comes before every other event.
    pub(super) fn begin_pump(&mut self, out: &mut Vec<Event>) {
        if !self.scale_announced {
            self.scale_announced = true;
            out.push(Event::ScaleFactorChanged(self.scale));
        }
    }

    /// The window is at a new scale factor now. It is reported at once,
    /// unless the first pump has not come yet: that one reports the latest.
    pub(super) fn set_scale(&mut self, scale: f32, out: &mut Vec<Event>) {
        self.scale = scale;
        if self.scale_announced {
            out.push(Event::ScaleFactorChanged(scale));
        }
    }

    /// Appends the events that `incoming` amounts to, which may be none.
    pub(super) fn handle(&mut self, incoming: Incoming, out: &mut Vec<Event>) {
        match incoming {
            Incoming::Key { pressed, scan_code } => self.key(pressed, scan_code, out),
            Incoming::Button {
                pressed,
                button,
                x,
                y,
            } => {
                self.move_pointer(x, y, out);
                self.button(pressed, button, out);
            }
            Incoming::Motion { x, y } => self.move_pointer(x, y, out),
            Incoming::Wheel(steps) => out.push(Event::MouseWheel(steps)),
            Incoming::Focus { gained } => self.focus(gained, out),
            Incoming::Size { width, height } => self.resize(width, height, out),
            Incoming::Close => out.push(Event::CloseRequested),
            Incoming::Other => {}
        }
    }

    fn key(&mut self, pressed: bool, scan_code: u16, out: &mut Vec<Event>) {
        let Some(key) = keymap::from_scan_code(scan_code) else {
            return;
        };
        let down = &mut self.keys_down[usize::from(key.usage())];
        if !pressed && !*down {
            // Windows keeps the press of Print Screen for its own shortcut,
            // and posts only the release. Report both, so that the key can
            // be used at all.
            if key == Key::PrintScreen {
                out.push(Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                });
                out.push(Event::Key {
                    key,
                    pressed: false,
                    repeat: false,
                });
            }
            // Any other release we never saw go down, which happens when the
            // key was pressed while another window had the focus, is not
            // reported: every release follows a press.
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

    fn move_pointer(&mut self, x: i32, y: i32, out: &mut Vec<Event>) {
        if self.pointer == Some((x, y)) {
            return;
        }
        self.pointer = Some((x, y));
        out.push(Event::MouseMoved(Vec2::new(x as f32, y as f32)));
    }

    fn focus(&mut self, gained: bool, out: &mut Vec<Event>) {
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

    fn resize(&mut self, width: u32, height: u32, out: &mut Vec<Event>) {
        let size = UVec2::new(width, height);
        if width == 0 || height == 0 || size == self.size {
            return;
        }
        self.size = size;
        out.push(Event::Resized(size));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Scan codes.
    const W: u16 = 0x0011;
    const A: u16 = 0x001E;
    const LEFT_SHIFT: u16 = 0x002A;
    const PRINT_SCREEN: u16 = 0xE037;
    const MUTE: u16 = 0xE020;

    fn translator() -> Translator {
        Translator::new(UVec2::new(640, 360), 1.5)
    }

    /// Feeds the messages to a translator and returns what comes out.
    fn run(translator: &mut Translator, incoming: &[Incoming]) -> Vec<Event> {
        let mut out = Vec::new();
        for &message in incoming {
            translator.handle(message, &mut out);
        }
        out
    }

    fn press(scan_code: u16) -> Incoming {
        Incoming::Key {
            pressed: true,
            scan_code,
        }
    }

    fn release(scan_code: u16) -> Incoming {
        Incoming::Key {
            pressed: false,
            scan_code,
        }
    }

    fn button(pressed: bool, button: MouseButton) -> Incoming {
        Incoming::Button {
            pressed,
            button,
            x: 10,
            y: 20,
        }
    }

    fn key_event(key: Key, pressed: bool, repeat: bool) -> Event {
        Event::Key {
            key,
            pressed,
            repeat,
        }
    }

    /// An `lParam` with `x` in the low word and `y` in the high word, as
    /// `MAKELPARAM` makes it.
    fn make_lparam(x: i16, y: i16) -> LPARAM {
        let low = WPARAM::from(x.cast_unsigned());
        let high = WPARAM::from(y.cast_unsigned()) << 16;
        (low | high) as LPARAM
    }

    // -- decoding -----------------------------------------------------------

    #[test]
    fn key_messages_are_decoded() {
        for (message, pressed) in [
            (WM_KEYDOWN, true),
            (WM_SYSKEYDOWN, true),
            (WM_KEYUP, false),
            (WM_SYSKEYUP, false),
        ] {
            assert_eq!(
                Incoming::decode(message, 0x41, 0x001E_0001),
                Incoming::Key {
                    pressed,
                    scan_code: A
                }
            );
        }
        // Right Control: extended.
        assert_eq!(
            Incoming::decode(WM_KEYDOWN, 0x11, 0x011D_0001),
            Incoming::Key {
                pressed: true,
                scan_code: 0xE01D
            }
        );
    }

    #[test]
    fn pointer_messages_are_decoded_with_signed_coordinates() {
        assert_eq!(
            Incoming::decode(WM_MOUSEMOVE, 0, make_lparam(12, 34)),
            Incoming::Motion { x: 12, y: 34 }
        );
        // Left of and above the window, while the pointer is captured.
        assert_eq!(
            Incoming::decode(WM_MOUSEMOVE, 0, make_lparam(-5, -1)),
            Incoming::Motion { x: -5, y: -1 }
        );
        let cases = [
            (WM_LBUTTONDOWN, true, MouseButton::Left),
            (WM_LBUTTONUP, false, MouseButton::Left),
            (WM_RBUTTONDOWN, true, MouseButton::Right),
            (WM_RBUTTONUP, false, MouseButton::Right),
            (WM_MBUTTONDOWN, true, MouseButton::Middle),
            (WM_MBUTTONUP, false, MouseButton::Middle),
        ];
        for (message, pressed, button) in cases {
            assert_eq!(
                Incoming::decode(message, 0, make_lparam(7, 8)),
                Incoming::Button {
                    pressed,
                    button,
                    x: 7,
                    y: 8
                }
            );
        }
    }

    #[test]
    fn side_buttons_are_named_by_the_high_word() {
        let xbutton = |number: u16| WPARAM::from(number) << 16;
        assert_eq!(
            Incoming::decode(WM_XBUTTONDOWN, xbutton(XBUTTON1), make_lparam(1, 2)),
            Incoming::Button {
                pressed: true,
                button: MouseButton::Back,
                x: 1,
                y: 2
            }
        );
        assert_eq!(
            Incoming::decode(WM_XBUTTONUP, xbutton(XBUTTON2), make_lparam(1, 2)),
            Incoming::Button {
                pressed: false,
                button: MouseButton::Forward,
                x: 1,
                y: 2
            }
        );
        assert_eq!(
            Incoming::decode(WM_XBUTTONDOWN, xbutton(3), 0),
            Incoming::Other
        );
    }

    #[test]
    fn the_wheel_turns_in_steps_of_120() {
        let delta = |delta: i16| WPARAM::from(delta.cast_unsigned()) << 16;
        assert_eq!(
            Incoming::decode(WM_MOUSEWHEEL, delta(120), 0),
            Incoming::Wheel(Vec2::new(0.0, 1.0))
        );
        assert_eq!(
            Incoming::decode(WM_MOUSEWHEEL, delta(-240), 0),
            Incoming::Wheel(Vec2::new(0.0, -2.0))
        );
        assert_eq!(
            Incoming::decode(WM_MOUSEWHEEL, delta(30), 0),
            Incoming::Wheel(Vec2::new(0.0, 0.25))
        );
        assert_eq!(
            Incoming::decode(WM_MOUSEHWHEEL, delta(120), 0),
            Incoming::Wheel(Vec2::new(1.0, 0.0))
        );
        assert_eq!(
            Incoming::decode(WM_MOUSEHWHEEL, delta(-120), 0),
            Incoming::Wheel(Vec2::new(-1.0, 0.0))
        );
    }

    #[test]
    fn focus_size_and_close_messages_are_decoded() {
        assert_eq!(
            Incoming::decode(WM_SETFOCUS, 0, 0),
            Incoming::Focus { gained: true }
        );
        assert_eq!(
            Incoming::decode(WM_KILLFOCUS, 0, 0),
            Incoming::Focus { gained: false }
        );
        // SIZE_RESTORED, with the width in the low word.
        assert_eq!(
            Incoming::decode(WM_SIZE, 0, make_lparam(1280, 720)),
            Incoming::Size {
                width: 1280,
                height: 720
            }
        );
        // A size is unsigned: 40000 is not negative.
        let wide = (40_000 | (20 << 16)) as LPARAM;
        assert_eq!(
            Incoming::decode(WM_SIZE, 2, wide),
            Incoming::Size {
                width: 40_000,
                height: 20
            }
        );
        assert_eq!(
            Incoming::decode(WM_SIZE, SIZE_MINIMIZED, 0),
            Incoming::Other
        );
        assert_eq!(Incoming::decode(WM_CLOSE, 0, 0), Incoming::Close);
        // WM_PAINT.
        assert_eq!(Incoming::decode(0x000F, 0, 0), Incoming::Other);
    }

    // -- translating --------------------------------------------------------

    #[test]
    fn the_scale_factor_is_reported_once_before_anything_else() {
        let mut translator = translator();
        let mut out = Vec::new();
        translator.begin_pump(&mut out);
        translator.handle(press(A), &mut out);
        translator.begin_pump(&mut out);
        assert_eq!(
            out,
            [
                Event::ScaleFactorChanged(1.5),
                key_event(Key::A, true, false)
            ]
        );
    }

    #[test]
    fn a_new_scale_is_reported_after_the_first_pump_and_kept_before_it() {
        let mut translator = translator();
        let mut out = Vec::new();
        // Before the first pump, the first pump reports the latest scale.
        translator.set_scale(2.0, &mut out);
        assert!(out.is_empty());
        translator.begin_pump(&mut out);
        assert_eq!(out, [Event::ScaleFactorChanged(2.0)]);
        translator.set_scale(1.25, &mut out);
        assert_eq!(
            out,
            [
                Event::ScaleFactorChanged(2.0),
                Event::ScaleFactorChanged(1.25)
            ]
        );
        assert_eq!(translator.scale(), 1.25);
    }

    #[test]
    fn a_key_goes_down_repeats_and_comes_up() {
        let mut translator = translator();
        let out = run(&mut translator, &[press(W), press(W), press(W), release(W)]);
        assert_eq!(
            out,
            [
                key_event(Key::W, true, false),
                key_event(Key::W, true, true),
                key_event(Key::W, true, true),
                key_event(Key::W, false, false),
            ]
        );
    }

    #[test]
    fn a_release_without_a_press_is_not_reported() {
        let mut translator = translator();
        assert!(run(&mut translator, &[release(A)]).is_empty());
    }

    #[test]
    fn print_screen_comes_as_a_release_alone_and_is_reported_as_a_tap() {
        let mut translator = translator();
        let out = run(&mut translator, &[release(PRINT_SCREEN)]);
        assert_eq!(
            out,
            [
                key_event(Key::PrintScreen, true, false),
                key_event(Key::PrintScreen, false, false),
            ]
        );
        // Where Windows does send the press, it is an ordinary key.
        let out = run(
            &mut translator,
            &[press(PRINT_SCREEN), release(PRINT_SCREEN)],
        );
        assert_eq!(
            out,
            [
                key_event(Key::PrintScreen, true, false),
                key_event(Key::PrintScreen, false, false),
            ]
        );
    }

    #[test]
    fn a_scan_code_with_no_key_is_ignored() {
        let mut translator = translator();
        assert!(run(&mut translator, &[press(MUTE), release(MUTE), press(0)]).is_empty());
    }

    #[test]
    fn a_button_goes_down_and_up_where_the_pointer_is() {
        let mut translator = translator();
        let out = run(
            &mut translator,
            &[
                button(true, MouseButton::Left),
                button(false, MouseButton::Left),
            ],
        );
        assert_eq!(
            out,
            [
                Event::MouseMoved(Vec2::new(10.0, 20.0)),
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
    fn the_translator_knows_whether_a_button_is_down() {
        let mut translator = translator();
        assert!(!translator.any_button_down());
        run(&mut translator, &[button(true, MouseButton::Right)]);
        assert!(translator.any_button_down());
        run(&mut translator, &[button(true, MouseButton::Back)]);
        run(&mut translator, &[button(false, MouseButton::Right)]);
        assert!(translator.any_button_down());
        run(&mut translator, &[button(false, MouseButton::Back)]);
        assert!(!translator.any_button_down());
    }

    #[test]
    fn a_button_released_twice_is_reported_once() {
        let mut translator = translator();
        let out = run(
            &mut translator,
            &[
                button(true, MouseButton::Middle),
                button(false, MouseButton::Middle),
                button(false, MouseButton::Middle),
            ],
        );
        assert_eq!(out.len(), 3, "{out:?}");
    }

    #[test]
    fn motion_is_reported_when_the_position_changes() {
        let mut translator = translator();
        let motion = |x, y| Incoming::Motion { x, y };
        let out = run(&mut translator, &[motion(1, 2), motion(1, 2), motion(3, 2)]);
        assert_eq!(
            out,
            [
                Event::MouseMoved(Vec2::new(1.0, 2.0)),
                Event::MouseMoved(Vec2::new(3.0, 2.0)),
            ]
        );
    }

    #[test]
    fn a_wheel_step_is_one_event() {
        let mut translator = translator();
        let out = run(&mut translator, &[Incoming::Wheel(Vec2::new(0.0, -1.0))]);
        assert_eq!(out, [Event::MouseWheel(Vec2::new(0.0, -1.0))]);
    }

    #[test]
    fn losing_the_focus_releases_what_is_held() {
        let mut translator = translator();
        let gain = Incoming::Focus { gained: true };
        let lose = Incoming::Focus { gained: false };
        let out = run(
            &mut translator,
            &[
                gain,
                press(LEFT_SHIFT),
                press(A),
                button(true, MouseButton::Right),
                lose,
            ],
        );
        assert_eq!(
            out[out.len() - 4..],
            [
                // In the order of `Key::ALL`, which is by usage: A first.
                key_event(Key::A, false, false),
                key_event(Key::LeftShift, false, false),
                Event::MouseButton {
                    button: MouseButton::Right,
                    pressed: false
                },
                Event::Focused(false),
            ]
        );
        assert!(!translator.any_button_down());
        // Nothing is held any more, so the releases that follow are not
        // reported.
        assert!(
            run(
                &mut translator,
                &[release(A), button(false, MouseButton::Right)]
            )
            .is_empty()
        );
    }

    #[test]
    fn focus_is_reported_when_it_changes() {
        let mut translator = translator();
        let gain = Incoming::Focus { gained: true };
        let lose = Incoming::Focus { gained: false };
        let out = run(&mut translator, &[gain, gain, lose, lose, gain]);
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
    fn a_new_size_is_reported_once() {
        let mut translator = translator();
        let size = |width, height| Incoming::Size { width, height };
        let out = run(
            &mut translator,
            &[size(640, 360), size(800, 600), size(800, 600), size(0, 600)],
        );
        assert_eq!(out, [Event::Resized(UVec2::new(800, 600))]);
        assert_eq!(translator.size(), UVec2::new(800, 600));
    }

    #[test]
    fn a_close_message_asks_to_close() {
        let mut translator = translator();
        assert_eq!(
            run(&mut translator, &[Incoming::Close]),
            [Event::CloseRequested]
        );
    }
}
