//! The window and its events, on X11.
//!
//! What this backend does not do yet, because nothing has needed it:
//!
//! - It reports no typed text. Key events say which physical keys moved,
//!   and nothing about the characters they stand for.
//! - It reads the scale factor once, when the window opens, from the
//!   `Xft.dpi` resource. A change of the desktop's scale while the game runs
//!   goes unnoticed. Under XWayland the compositor decides what it tells X
//!   clients, so the value can differ from the compositor's own scale.
//! - It does not notice the window being minimized.
//!
//! One case can leave a key stuck down. While a window manager holds a
//! keyboard grab, for a shortcut of its own, the window gets no key events.
//! A key that goes up during the grab is seen as held until it is pressed
//! again or the window loses the focus.

mod connection;
mod input;

use core::ffi::{c_int, c_long};

use nadir_core::UVec2;
use nadir_core::log::{debug, error, warn};
use nadir_sys::linux::x11::{
    Atom, ButtonPressMask, ButtonReleaseMask, Cursor, FocusChangeMask, KeyPressMask,
    KeyReleaseMask, PMaxSize, PMinSize, PointerMotionMask, StructureNotifyMask,
    SubstructureNotifyMask, SubstructureRedirectMask, Window, XA_ATOM, XA_STRING, XA_WM_NAME,
    XAspectRatio, XSizeHints,
};

use self::connection::Connection;
use self::input::{CloseMessage, Incoming, Translator};
use super::xresources;
use crate::{Event, PlatformError, WindowConfig};

/// The events we ask the server for.
const EVENT_MASK: c_long = KeyPressMask
    | KeyReleaseMask
    | ButtonPressMask
    | ButtonReleaseMask
    | PointerMotionMask
    | FocusChangeMask
    | StructureNotifyMask;

/// The widest and tallest window we ask for. The X protocol carries a size
/// in 16 bits, and servers stop at half of that.
const MAX_DIMENSION: u32 = 32767;

/// The first item of a `_NET_WM_STATE` message: take the state away.
/// Extended Window Manager Hints, "_NET_WM_STATE": `_NET_WM_STATE_REMOVE 0`.
const NET_WM_STATE_REMOVE: c_long = 0;

/// The first item of a `_NET_WM_STATE` message: add the state.
/// Extended Window Manager Hints, "_NET_WM_STATE": `_NET_WM_STATE_ADD 1`.
const NET_WM_STATE_ADD: c_long = 1;

/// The fourth item of a `_NET_WM_STATE` message: the sender is an ordinary
/// application. Extended Window Manager Hints, "Source indication".
const SOURCE_APPLICATION: c_long = 1;

/// The atoms we use, looked up once.
struct Atoms {
    wm_protocols: Atom,
    wm_delete_window: Atom,
    net_wm_name: Atom,
    net_wm_state: Atom,
    net_wm_state_fullscreen: Atom,
    utf8_string: Atom,
}

impl Atoms {
    fn intern(connection: &Connection) -> Self {
        Self {
            wm_protocols: connection.intern(c"WM_PROTOCOLS"),
            wm_delete_window: connection.intern(c"WM_DELETE_WINDOW"),
            net_wm_name: connection.intern(c"_NET_WM_NAME"),
            net_wm_state: connection.intern(c"_NET_WM_STATE"),
            net_wm_state_fullscreen: connection.intern(c"_NET_WM_STATE_FULLSCREEN"),
            utf8_string: connection.intern(c"UTF8_STRING"),
        }
    }
}

/// The X11 window of the game.
pub(crate) struct Backend {
    // The connection is declared last so that it is dropped last: `Drop`
    // below uses it, and so would any field that held server resources.
    root: Window,
    window: Window,
    atoms: Atoms,
    translator: Translator,
    /// The size the window was asked to have, in physical pixels.
    requested_size: UVec2,
    /// The smallest size the user may give the window, in physical pixels.
    /// A zero means no limit in that direction.
    min_size: UVec2,
    resizable: bool,
    fullscreen: bool,
    cursor_visible: bool,
    /// The empty pointer shape, once it has been needed.
    blank_cursor: Option<Cursor>,
    connection: Connection,
}

impl Backend {
    pub(crate) fn new(config: &WindowConfig) -> Result<Self, PlatformError> {
        let connection = Connection::open()?;
        if !connection.set_detectable_auto_repeat() {
            warn!(
                "the X server has no detectable auto-repeat: a held key will report \
                 releases and presses in place of repeats"
            );
        }
        let scale = xresources::scale_factor(connection.resources().as_deref());
        let requested_size = physical_size(config.size, scale);
        let root = connection.root();
        let atoms = Atoms::intern(&connection);
        let window = connection.create_window(root, requested_size);
        let close = CloseMessage {
            wm_protocols: atoms.wm_protocols,
            wm_delete_window: atoms.wm_delete_window,
        };
        // From here on, dropping `backend` destroys the window.
        let mut backend = Self {
            root,
            window,
            atoms,
            translator: Translator::new(close, requested_size, scale),
            requested_size,
            min_size: physical_min_size(config.min_size, scale),
            resizable: config.resizable,
            fullscreen: config.fullscreen,
            cursor_visible: true,
            blank_cursor: None,
            connection,
        };

        backend.connection.select_input(window, EVENT_MASK);
        backend.set_title(&config.title);
        let mut protocols = [backend.atoms.wm_delete_window];
        if !backend.connection.set_protocols(window, &mut protocols) {
            return Err(PlatformError::CallFailed {
                call: "XSetWMProtocols",
            });
        }
        backend.apply_size_hints();
        if config.fullscreen {
            // Before the window is shown, the state is a property that the
            // window manager reads when it takes the window on.
            backend.connection.set_atoms(
                window,
                backend.atoms.net_wm_state,
                XA_ATOM,
                &[backend.atoms.net_wm_state_fullscreen],
            );
        }
        backend.connection.map_window(window);
        debug!(
            "opened an X11 window of {}x{} pixels at scale {scale}",
            requested_size.x, requested_size.y
        );
        Ok(backend)
    }

    pub(crate) fn pump_events(&mut self, out: &mut Vec<Event>) {
        self.translator.begin_pump(out);
        // `pending` also sends the requests that the other methods queued.
        while self.connection.pending() > 0 {
            let event = self.connection.next_event();
            // SAFETY: Xlib wrote the event, so its type and the member that
            // the type selects are initialized.
            let incoming = unsafe { Incoming::decode(&event) };
            self.translator.handle(incoming, out);
        }
    }

    pub(crate) fn window_size(&self) -> UVec2 {
        self.translator.size()
    }

    pub(crate) fn scale_factor(&self) -> f32 {
        self.translator.scale()
    }

    pub(crate) fn set_fullscreen(&mut self, on: bool) {
        if on == self.fullscreen {
            return;
        }
        self.fullscreen = on;
        // A window that may not change its size cannot go fullscreen, so
        // the limits come off first, and go back on afterwards.
        self.apply_size_hints();
        let sent = self.connection.send_to_window_manager(
            self.root,
            self.window,
            self.atoms.net_wm_state,
            fullscreen_request(on, self.atoms.net_wm_state_fullscreen),
            SubstructureNotifyMask | SubstructureRedirectMask,
        );
        if !sent {
            error!("could not ask the window manager for fullscreen: XSendEvent failed");
        }
    }

    pub(crate) fn set_title(&mut self, title: &str) {
        let bytes = title.as_bytes();
        // Current window managers read this one, which is always UTF-8.
        let modern = self.connection.set_bytes(
            self.window,
            self.atoms.net_wm_name,
            self.atoms.utf8_string,
            bytes,
        );
        // Older ones read this one.
        let legacy = self.connection.set_bytes(
            self.window,
            XA_WM_NAME,
            legacy_title_type(title, self.atoms.utf8_string),
            bytes,
        );
        if !(modern && legacy) {
            error!("the window title is too long to set");
        }
    }

    pub(crate) fn set_cursor_visible(&mut self, visible: bool) {
        if visible == self.cursor_visible {
            return;
        }
        if visible {
            self.connection.undefine_cursor(self.window);
        } else {
            if self.blank_cursor.is_none() {
                self.blank_cursor = self.connection.create_blank_cursor(self.window);
            }
            let Some(cursor) = self.blank_cursor else {
                error!("could not hide the pointer: the server made no empty cursor");
                return;
            };
            self.connection.define_cursor(self.window, cursor);
        }
        self.cursor_visible = visible;
    }

    fn apply_size_hints(&self) {
        let hints = size_hints(
            self.requested_size,
            self.min_size,
            self.resizable,
            self.fullscreen,
        );
        self.connection.set_normal_hints(self.window, hints);
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(cursor) = self.blank_cursor.take() {
            self.connection.free_cursor(cursor);
        }
        self.connection.destroy_window(self.window);
        // The connection closes when the field is dropped, after this.
    }
}

/// Converts a logical window size to physical pixels, within what X can
/// make a window of.
fn physical_size(logical: UVec2, scale: f32) -> UVec2 {
    let dimension = |logical: u32| {
        // The cast saturates, and a NaN gives 0.
        let physical = (logical as f32 * scale).round() as u32;
        physical.clamp(1, MAX_DIMENSION)
    };
    UVec2::new(dimension(logical.x), dimension(logical.y))
}

/// Converts a logical minimum size to physical pixels. A zero stays zero:
/// no limit in that direction.
fn physical_min_size(logical: UVec2, scale: f32) -> UVec2 {
    let dimension = |logical: u32| match logical {
        0 => 0,
        _ => physical_size(UVec2::splat(logical), scale).x,
    };
    UVec2::new(dimension(logical.x), dimension(logical.y))
}

/// Returns what to tell the window manager about the sizes the window
/// accepts. All sizes are in physical pixels.
fn size_hints(size: UVec2, min_size: UVec2, resizable: bool, fullscreen: bool) -> XSizeHints {
    let mut hints = XSizeHints {
        flags: 0,
        x: 0,
        y: 0,
        width: 0,
        height: 0,
        min_width: 0,
        min_height: 0,
        max_width: 0,
        max_height: 0,
        width_inc: 0,
        height_inc: 0,
        min_aspect: XAspectRatio { x: 0, y: 0 },
        max_aspect: XAspectRatio { x: 0, y: 0 },
        base_width: 0,
        base_height: 0,
        win_gravity: 0,
    };
    // Every size here is at most `MAX_DIMENSION`, so none saturates.
    let int = |value: u32| c_int::try_from(value).unwrap_or(c_int::MAX);
    if fullscreen {
        // No limits: the window manager chooses the size.
    } else if !resizable {
        hints.flags = PMinSize | PMaxSize;
        (hints.min_width, hints.min_height) = (int(size.x), int(size.y));
        (hints.max_width, hints.max_height) = (int(size.x), int(size.y));
    } else if min_size != UVec2::ZERO {
        hints.flags = PMinSize;
        // The hint has no "no limit", and one pixel is as good.
        (hints.min_width, hints.min_height) = (int(min_size.x.max(1)), int(min_size.y.max(1)));
    }
    hints
}

/// Returns the data of the `_NET_WM_STATE` message that asks the window
/// manager to turn fullscreen on or off: the action, the state, no second
/// state, who is asking, and an unused item.
fn fullscreen_request(on: bool, net_wm_state_fullscreen: Atom) -> [c_long; 5] {
    let action = if on {
        NET_WM_STATE_ADD
    } else {
        NET_WM_STATE_REMOVE
    };
    // An atom is an `unsigned long` that travels as a `long`.
    let state = net_wm_state_fullscreen as c_long;
    [action, state, 0, SOURCE_APPLICATION, 0]
}

/// Returns the type to give the `WM_NAME` property for this title.
///
/// The property's own type, `STRING`, is Latin-1. A title in plain ASCII is
/// the same bytes in Latin-1 and in UTF-8, so it can have that type. Any
/// other title is marked as UTF-8, which window managers accept here too.
fn legacy_title_type(title: &str, utf8_string: Atom) -> Atom {
    if title.is_ascii() {
        XA_STRING
    } else {
        utf8_string
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_logical_size_is_scaled_and_rounded() {
        let size = UVec2::new(640, 360);
        assert_eq!(physical_size(size, 1.0), UVec2::new(640, 360));
        assert_eq!(physical_size(size, 2.0), UVec2::new(1280, 720));
        assert_eq!(physical_size(size, 1.5), UVec2::new(960, 540));
        assert_eq!(physical_size(size, 0.75), UVec2::new(480, 270));
        // 101 * 1.25 = 126.25, and 3 * 1.5 = 4.5, which rounds away from 0.
        assert_eq!(physical_size(UVec2::new(101, 3), 1.25), UVec2::new(126, 4));
        assert_eq!(physical_size(UVec2::new(3, 1), 1.5), UVec2::new(5, 2));
    }

    #[test]
    fn a_physical_size_is_one_that_x_can_make() {
        assert_eq!(physical_size(UVec2::ZERO, 1.0), UVec2::new(1, 1));
        assert_eq!(physical_size(UVec2::new(1, 1), 0.25), UVec2::new(1, 1));
        assert_eq!(
            physical_size(UVec2::new(u32::MAX, 40_000), 1.0),
            UVec2::new(32767, 32767)
        );
        assert_eq!(
            physical_size(UVec2::new(20_000, 100), 2.0),
            UVec2::new(32767, 200)
        );
        // A scale that is no number still gives a window.
        assert_eq!(
            physical_size(UVec2::new(640, 360), f32::NAN),
            UVec2::new(1, 1)
        );
        assert_eq!(
            physical_size(UVec2::new(640, 360), f32::INFINITY),
            UVec2::new(32767, 32767)
        );
    }

    #[test]
    fn a_minimum_size_is_scaled_and_zero_stays_no_limit() {
        assert_eq!(
            physical_min_size(UVec2::new(320, 180), 2.0),
            UVec2::new(640, 360)
        );
        assert_eq!(physical_min_size(UVec2::ZERO, 2.0), UVec2::ZERO);
        assert_eq!(
            physical_min_size(UVec2::new(0, 180), 1.5),
            UVec2::new(0, 270)
        );
        assert_eq!(physical_min_size(UVec2::new(1, 0), 0.25), UVec2::new(1, 0));
    }

    /// The flags and the four limits of some hints.
    fn limits(hints: &XSizeHints) -> (c_long, [c_int; 4]) {
        (
            hints.flags,
            [
                hints.min_width,
                hints.min_height,
                hints.max_width,
                hints.max_height,
            ],
        )
    }

    #[test]
    fn a_window_of_fixed_size_has_equal_limits() {
        let hints = size_hints(UVec2::new(640, 360), UVec2::new(320, 180), false, false);
        assert_eq!(limits(&hints), (PMinSize | PMaxSize, [640, 360, 640, 360]));
    }

    #[test]
    fn a_resizable_window_has_only_its_minimum() {
        let size = UVec2::new(640, 360);
        let hints = size_hints(size, UVec2::new(320, 180), true, false);
        assert_eq!(limits(&hints), (PMinSize, [320, 180, 0, 0]));

        let hints = size_hints(size, UVec2::ZERO, true, false);
        assert_eq!(limits(&hints), (0, [0, 0, 0, 0]));

        // No limit in one direction becomes one pixel.
        let hints = size_hints(size, UVec2::new(0, 180), true, false);
        assert_eq!(limits(&hints), (PMinSize, [1, 180, 0, 0]));
    }

    #[test]
    fn a_fullscreen_window_has_no_limits() {
        for resizable in [false, true] {
            let hints = size_hints(UVec2::new(640, 360), UVec2::new(320, 180), resizable, true);
            assert_eq!(limits(&hints), (0, [0, 0, 0, 0]));
        }
    }

    #[test]
    fn hints_set_nothing_but_the_limits() {
        let hints = size_hints(UVec2::new(640, 360), UVec2::new(320, 180), false, false);
        let rest = [
            hints.x,
            hints.y,
            hints.width,
            hints.height,
            hints.width_inc,
            hints.height_inc,
            hints.min_aspect.x,
            hints.min_aspect.y,
            hints.max_aspect.x,
            hints.max_aspect.y,
            hints.base_width,
            hints.base_height,
            hints.win_gravity,
        ];
        assert_eq!(rest, [0; 13]);
    }

    #[test]
    fn a_fullscreen_request_adds_or_removes_the_state() {
        // The values are those of the Extended Window Manager Hints.
        const FULLSCREEN: Atom = 411;
        assert_eq!(fullscreen_request(true, FULLSCREEN), [1, 411, 0, 1, 0]);
        assert_eq!(fullscreen_request(false, FULLSCREEN), [0, 411, 0, 1, 0]);
    }

    #[test]
    fn only_an_ascii_title_is_a_latin_1_string() {
        const UTF8_STRING: Atom = 400;
        assert_eq!(legacy_title_type("Nadir", UTF8_STRING), XA_STRING);
        assert_eq!(legacy_title_type("", UTF8_STRING), XA_STRING);
        assert_eq!(legacy_title_type("Nadir: level 2!", UTF8_STRING), XA_STRING);
        // Latin-1 could hold the first of these, but not as these bytes.
        assert_eq!(legacy_title_type("Café", UTF8_STRING), UTF8_STRING);
        assert_eq!(legacy_title_type("ナディア", UTF8_STRING), UTF8_STRING);
    }

    #[test]
    fn the_event_mask_selects_what_the_translator_handles() {
        // The values are those of `X11/X.h`.
        let expected = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 6) | (1 << 17) | (1 << 21);
        assert_eq!(EVENT_MASK, expected);
    }
}
