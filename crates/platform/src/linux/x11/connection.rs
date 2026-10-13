//! The connection to the X server: one safe method for each Xlib call.

use core::ffi::{CStr, c_char, c_int, c_long, c_uchar, c_uint, c_ulong};
use core::ptr::{self, NonNull};
use std::env;
use std::ffi::OsStr;

use nadir_core::UVec2;
use nadir_sys::linux::x11::{
    Atom, CWBackPixel, ClientMessage, CopyFromParent, Cursor, Display, False, InputOutput,
    PropModeReplace, True, Window, XClientMessageData, XClientMessageEvent, XColor, XEvent,
    XSetWindowAttributes, XSizeHints, Xlib,
};

use super::super::library::Library;
use crate::PlatformError;

/// The file name of Xlib, for the loader.
const XLIB: &CStr = c"libX11.so.6";

/// The file name of Xlib, for messages.
const XLIB_NAME: &str = "libX11.so.6";

/// An open connection to the X server.
///
/// The `SAFETY` comments below rely on these invariants:
///
/// - `display` came from `XOpenDisplay`, and is closed only when this value
///   is dropped.
/// - `xlib` was loaded from Xlib itself, which is never unloaded.
/// - This type holds a raw pointer, so it is neither `Send` nor `Sync`.
///   Every call for the display therefore comes from the thread that opened
///   it, which Xlib requires because we never call `XInitThreads`.
///
/// A window, cursor or atom that does not belong to the display is not a
/// memory error: the server rejects the request, and Xlib then ends the
/// process. The methods that take one are therefore safe to call.
pub(super) struct Connection {
    xlib: Xlib,
    display: NonNull<Display>,
}

impl Connection {
    /// Loads Xlib and connects to the server that `DISPLAY` names.
    pub(super) fn open() -> Result<Self, PlatformError> {
        let display_name = env::var_os("DISPLAY");
        let wayland = env::var_os("WAYLAND_DISPLAY");
        if let Some(error) = missing_display(display_name.as_deref(), wayland.as_deref()) {
            return Err(error);
        }

        // SAFETY: Xlib is a system library that is sound to load into any
        // process.
        let library =
            unsafe { Library::open(XLIB) }.map_err(|reason| PlatformError::LibraryNotFound {
                library: XLIB_NAME,
                reason,
            })?;
        // SAFETY: the library is Xlib, which defines these functions as its
        // headers declare them, and a `Library` is never unloaded.
        let xlib =
            unsafe { Xlib::load(&library) }.map_err(|missing| PlatformError::MissingSymbols {
                library: XLIB_NAME,
                symbols: missing.names,
            })?;

        // SAFETY: a null name tells Xlib to read `DISPLAY`.
        let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
        match NonNull::new(display) {
            Some(display) => Ok(Self { xlib, display }),
            None => Err(PlatformError::ConnectionFailed {
                display: display_name
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            }),
        }
    }

    /// Returns the root window of the default screen.
    pub(super) fn root(&self) -> Window {
        // SAFETY: the display is open.
        let screen = unsafe { (self.xlib.XDefaultScreen)(self.display.as_ptr()) };
        // SAFETY: the display is open, and Xlib just gave this screen number.
        unsafe { (self.xlib.XRootWindow)(self.display.as_ptr(), screen) }
    }

    /// Asks for detectable auto-repeat, and returns whether we have it. With
    /// it, a held key repeats as presses only, with no releases between.
    pub(super) fn set_detectable_auto_repeat(&self) -> bool {
        let mut supported = False;
        // SAFETY: the display is open, and `supported` is a live `Bool`.
        let on = unsafe {
            (self.xlib.XkbSetDetectableAutoRepeat)(self.display.as_ptr(), True, &raw mut supported)
        };
        on != False && supported != False
    }

    /// Returns the user's X resources, as they were when the display was
    /// opened.
    pub(super) fn resources(&self) -> Option<String> {
        // SAFETY: the display is open.
        let text = unsafe { (self.xlib.XResourceManagerString)(self.display.as_ptr()) };
        if text.is_null() {
            return None;
        }
        // SAFETY: Xlib returned a NUL-terminated string that it owns, and
        // that lives until the display is closed. It is copied here.
        let text = unsafe { CStr::from_ptr(text) };
        Some(text.to_string_lossy().into_owned())
    }

    /// Returns the atom for `name`, making it if the server has none.
    pub(super) fn intern(&self, name: &CStr) -> Atom {
        // SAFETY: the display is open, and `name` is a C string.
        unsafe { (self.xlib.XInternAtom)(self.display.as_ptr(), name.as_ptr(), False) }
    }

    /// Creates a top-level window with a black background. It is not shown
    /// until [`Connection::map_window`].
    pub(super) fn create_window(&self, root: Window, size: UVec2) -> Window {
        let mut attributes = XSetWindowAttributes {
            background_pixmap: 0,
            // Black on every visual a desktop uses.
            background_pixel: 0,
            border_pixmap: 0,
            border_pixel: 0,
            bit_gravity: 0,
            win_gravity: 0,
            backing_store: 0,
            backing_planes: 0,
            backing_pixel: 0,
            save_under: False,
            event_mask: 0,
            do_not_propagate_mask: 0,
            override_redirect: False,
            colormap: 0,
            cursor: 0,
        };
        // SAFETY: the display is open, and `attributes` is a live struct, of
        // which the mask names one field. The depth and the visual (a null
        // pointer) are both "copy from the parent".
        unsafe {
            (self.xlib.XCreateWindow)(
                self.display.as_ptr(),
                root,
                0,
                0,
                size.x as c_uint,
                size.y as c_uint,
                0,
                CopyFromParent as c_int,
                InputOutput,
                ptr::null_mut(),
                CWBackPixel as c_ulong,
                &raw mut attributes,
            )
        }
    }

    /// Destroys a window.
    pub(super) fn destroy_window(&self, window: Window) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XDestroyWindow)(self.display.as_ptr(), window) };
    }

    /// Shows a window.
    pub(super) fn map_window(&self, window: Window) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XMapWindow)(self.display.as_ptr(), window) };
    }

    /// Chooses which events the server reports for a window.
    pub(super) fn select_input(&self, window: Window, event_mask: c_long) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XSelectInput)(self.display.as_ptr(), window, event_mask) };
    }

    /// Tells the window manager which messages it may send the window.
    /// Returns whether that worked.
    pub(super) fn set_protocols(&self, window: Window, protocols: &mut [Atom]) -> bool {
        let Ok(count) = c_int::try_from(protocols.len()) else {
            return false;
        };
        // SAFETY: the display is open, and `protocols` points to `count`
        // live atoms.
        let status = unsafe {
            (self.xlib.XSetWMProtocols)(
                self.display.as_ptr(),
                window,
                protocols.as_mut_ptr(),
                count,
            )
        };
        status != 0
    }

    /// Sets a property of a window to some bytes. Returns `false`, having
    /// done nothing, if there are more bytes than a request can count.
    pub(super) fn set_bytes(
        &self,
        window: Window,
        property: Atom,
        type_: Atom,
        data: &[u8],
    ) -> bool {
        let Ok(len) = c_int::try_from(data.len()) else {
            return false;
        };
        // SAFETY: the display is open, and `data` points to `len` live
        // bytes, which is what format 8 reads. Xlib gives the result no
        // meaning.
        unsafe {
            (self.xlib.XChangeProperty)(
                self.display.as_ptr(),
                window,
                property,
                type_,
                8,
                PropModeReplace,
                data.as_ptr(),
                len,
            )
        };
        true
    }

    /// Sets a property of a window to a list of atoms.
    pub(super) fn set_atoms(&self, window: Window, property: Atom, type_: Atom, atoms: &[Atom]) {
        let Ok(len) = c_int::try_from(atoms.len()) else {
            return;
        };
        // SAFETY: the display is open. With format 32, Xlib reads `len`
        // items of the size of a C `long` from `data`. An `Atom` is an
        // `unsigned long`, which has that size, so `atoms` is exactly that.
        // Xlib gives the result no meaning.
        unsafe {
            (self.xlib.XChangeProperty)(
                self.display.as_ptr(),
                window,
                property,
                type_,
                32,
                PropModeReplace,
                atoms.as_ptr().cast::<c_uchar>(),
                len,
            )
        };
    }

    /// Tells the window manager which sizes a window accepts.
    pub(super) fn set_normal_hints(&self, window: Window, mut hints: XSizeHints) {
        // SAFETY: the display is open, and `hints` is a live struct that
        // Xlib copies from.
        unsafe { (self.xlib.XSetWMNormalHints)(self.display.as_ptr(), window, &raw mut hints) };
    }

    /// Returns how many events are waiting, after sending every queued
    /// request if none are. Does not block.
    pub(super) fn pending(&self) -> c_int {
        // SAFETY: the display is open.
        unsafe { (self.xlib.XPending)(self.display.as_ptr()) }
    }

    /// Removes the next event from the queue, waiting for one if the queue
    /// is empty.
    ///
    /// In the event returned, the member that its type selects is the one
    /// the server filled in, which is what `Incoming::decode` requires.
    pub(super) fn next_event(&self) -> XEvent {
        let mut event = XEvent { pad: [0; 24] };
        // SAFETY: the display is open, and `event` is a live `XEvent`, which
        // has room for any event. Xlib gives the result no meaning.
        unsafe { (self.xlib.XNextEvent)(self.display.as_ptr(), &raw mut event) };
        event
    }

    /// Sends a client message about `window` to the window manager, by way
    /// of the root window `root`. Returns whether the message could be
    /// sent.
    pub(super) fn send_to_window_manager(
        &self,
        root: Window,
        window: Window,
        message_type: Atom,
        data: [c_long; 5],
        event_mask: c_long,
    ) -> bool {
        let mut event = XEvent { pad: [0; 24] };
        event.xclient = XClientMessageEvent {
            type_: ClientMessage,
            serial: 0,
            send_event: True,
            display: self.display.as_ptr(),
            window,
            message_type,
            format: 32,
            data: XClientMessageData { l: data },
        };
        // SAFETY: the display is open, and `event` is a live `XEvent` that
        // holds a complete client message.
        let status = unsafe {
            (self.xlib.XSendEvent)(
                self.display.as_ptr(),
                root,
                False,
                event_mask,
                &raw mut event,
            )
        };
        status != 0
    }

    /// Creates a pointer shape with nothing in it: one transparent pixel.
    /// The caller frees it with [`Connection::free_cursor`].
    pub(super) fn create_blank_cursor(&self, window: Window) -> Option<Cursor> {
        // One row of one pixel, which is 0. As the mask, a 0 makes the
        // pixel transparent.
        let bits: [c_char; 1] = [0];
        // SAFETY: the display is open, and `bits` holds the one byte that a
        // bitmap of 1 by 1 needs.
        let bitmap = unsafe {
            (self.xlib.XCreateBitmapFromData)(self.display.as_ptr(), window, bits.as_ptr(), 1, 1)
        };
        if bitmap == 0 {
            return None;
        }
        let black = XColor {
            pixel: 0,
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
            pad: 0,
        };
        let (mut foreground, mut background) = (black, black);
        // SAFETY: the display is open, the bitmap was just created, and the
        // two colors are live structs.
        let cursor = unsafe {
            (self.xlib.XCreatePixmapCursor)(
                self.display.as_ptr(),
                bitmap,
                bitmap,
                &raw mut foreground,
                &raw mut background,
                0,
                0,
            )
        };
        // SAFETY: the display is open, and the cursor no longer needs the
        // bitmap. Xlib gives the result no meaning.
        unsafe { (self.xlib.XFreePixmap)(self.display.as_ptr(), bitmap) };
        (cursor != 0).then_some(cursor)
    }

    /// Shows `cursor` while the pointer is over a window.
    pub(super) fn define_cursor(&self, window: Window, cursor: Cursor) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XDefineCursor)(self.display.as_ptr(), window, cursor) };
    }

    /// Shows the usual pointer over a window again.
    pub(super) fn undefine_cursor(&self, window: Window) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XUndefineCursor)(self.display.as_ptr(), window) };
    }

    /// Frees a cursor.
    pub(super) fn free_cursor(&self, cursor: Cursor) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XFreeCursor)(self.display.as_ptr(), cursor) };
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: the display is open, and nothing uses it after this: the
        // pointer goes away with `self`. Closing sends every queued request
        // first. Xlib gives the result no meaning.
        unsafe { (self.xlib.XCloseDisplay)(self.display.as_ptr()) };
    }
}

/// Returns the error to report if the environment names no X server.
///
/// Xlib would fail too, but could not say that the session is a Wayland one
/// without XWayland, which the player can fix.
fn missing_display(display: Option<&OsStr>, wayland: Option<&OsStr>) -> Option<PlatformError> {
    let is_set = |value: Option<&OsStr>| value.is_some_and(|value| !value.is_empty());
    if is_set(display) {
        return None;
    }
    Some(PlatformError::NoDisplay {
        wayland: is_set(wayland),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(display: Option<&str>, wayland: Option<&str>) -> Option<PlatformError> {
        missing_display(display.map(OsStr::new), wayland.map(OsStr::new))
    }

    #[test]
    fn a_display_is_enough_whatever_else_is_set() {
        assert_eq!(check(Some(":0"), None), None);
        assert_eq!(check(Some(":1"), Some("wayland-1")), None);
        assert_eq!(check(Some("localhost:10.0"), Some("")), None);
    }

    #[test]
    fn no_display_and_no_wayland_is_no_display_server() {
        let expected = Some(PlatformError::NoDisplay { wayland: false });
        assert_eq!(check(None, None), expected);
        assert_eq!(check(Some(""), None), expected);
        assert_eq!(check(None, Some("")), expected);
    }

    #[test]
    fn no_display_in_a_wayland_session_says_so() {
        let expected = Some(PlatformError::NoDisplay { wayland: true });
        assert_eq!(check(None, Some("wayland-0")), expected);
        assert_eq!(check(Some(""), Some("wayland-1")), expected);
    }
}
