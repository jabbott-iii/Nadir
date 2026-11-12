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

//! The connection to the X server: one safe method for each Xlib call.
//!
//! It also keeps the input method and the window's input context, which turn
//! key presses into text, so that their pointers never leave this file.

use core::cell::Cell;
use core::ffi::{CStr, c_char, c_int, c_long, c_uchar, c_uint, c_ulong};
use core::ptr::{self, NonNull};
use core::str;
use core::sync::atomic::{AtomicBool, Ordering};
use std::env;
use std::ffi::OsStr;

use nadir_core::UVec2;
use nadir_core::log::{debug, warn};
use nadir_sys::linux::libc::{LC_CTYPE, setlocale};
// The lookup statuses are used as `xlib::XLookupChars`, not imported by name:
// in a pattern, a bare name that is not upper case reads as a new variable.
use nadir_sys::linux::x11::{
    self as xlib, AllocNone, Atom, CWBackPixel, CWBorderPixel, CWColormap, ClientMessage, Colormap,
    Cursor, Display, False, InputOutput, PropModeReplace, Status, True, VisualID, VisualIDMask,
    Window, XClientMessageData, XClientMessageEvent, XColor, XEvent, XICRec, XIM, XIMCallback,
    XIMPreeditNothing, XIMRec, XIMStatusNothing, XNClientWindow, XNDestroyCallback, XNFilterEvents,
    XNFocusWindow, XNInputStyle, XPointer, XSetWindowAttributes, XSizeHints, XVisualInfo, Xlib,
};

use super::super::library::Library;
use crate::PlatformError;

/// The file name of Xlib, for the loader.
const XLIB: &CStr = c"libX11.so.6";

/// The file name of Xlib, for messages.
const XLIB_NAME: &str = "libX11.so.6";

/// The bytes a key press's text is looked up in, at first. A longer text,
/// which an input method can commit at once, makes it grow.
const LOOKUP_START: usize = 64;

/// The input methods to try, by their locale modifiers, and what each is
/// for the log: the one that `XMODIFIERS` names, such as an input method
/// server, then Xlib's own, which composes from the locale's Compose file.
const INPUT_METHODS: [(&CStr, &str); 2] = [
    (c"", "the one XMODIFIERS names"),
    (c"@im=none", "Xlib's own"),
];

/// Whether a connection has an input method open. Xlib keeps the state of
/// locales and input methods for the whole process, with no lock unless
/// `XInitThreads` was called, which we never do; so one connection at a
/// time sets the locale and opens, uses and closes an input method. A window
/// opened on another thread meanwhile types nothing.
static TEXT_INPUT_TAKEN: AtomicBool = AtomicBool::new(false);

/// An input method and the window's input context in it.
struct TextInput {
    im: NonNull<XIMRec>,
    ic: NonNull<XICRec>,
    /// Set by the input method's destroy callback when it stopped working:
    /// Xlib has then freed it and its context, which must be neither used
    /// nor freed again. Made by [`new_flag`], the callback's data, and freed
    /// by [`free_flag`] only when this is dropped: in
    /// [`Connection::close_text_input`], once the input method is closed,
    /// or lost, and cannot call back any more.
    lost: NonNull<Cell<bool>>,
}

impl TextInput {
    /// Whether the input method stopped working.
    fn is_lost(&self) -> bool {
        // SAFETY: the flag is freed only when this value goes, and only
        // shared references to it are made, on this thread.
        unsafe { self.lost.as_ref() }.get()
    }
}

/// A flag for an input method's destroy callback, on the heap, where its
/// address does not change.
fn new_flag() -> NonNull<Cell<bool>> {
    NonNull::from(Box::leak(Box::new(Cell::new(false))))
}

/// Frees a flag of [`new_flag`].
///
/// # Safety
///
/// No input method calls back with it any more: it was closed, or it was
/// lost, after which Xlib does not call the callback again.
unsafe fn free_flag(flag: NonNull<Cell<bool>>) {
    // SAFETY: the flag came from a box, and nothing uses it after this.
    drop(unsafe { Box::from_raw(flag.as_ptr()) });
}

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
/// process. The methods that take one are therefore safe to call. A
/// `Visual *`, which is a pointer into the display, never leaves this file.
///
/// The input method and its context are freed before the display is closed,
/// and the context before its window ([`Connection::close_text_input`]).
pub(super) struct Connection {
    xlib: Xlib,
    display: NonNull<Display>,
    /// The input method and the window's context, once opened.
    text: Option<TextInput>,
    /// Where a key press's text is looked up: its length is the room.
    lookup: Vec<u8>,
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
            Some(display) => Ok(Self {
                xlib,
                display,
                text: None,
                lookup: vec![0; LOOKUP_START],
            }),
            None => Err(PlatformError::ConnectionFailed {
                display: display_name
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            }),
        }
    }

    /// The display, for EGL. It is open for as long as this value exists.
    pub(super) fn display(&self) -> NonNull<Display> {
        self.display
    }

    /// Returns the depth and class of the visual `id`, or `None` if the
    /// server has no such visual.
    pub(super) fn visual_format(&self, id: VisualID) -> Option<VisualFormat> {
        let info = self.visual_info(id)?;
        Some(VisualFormat {
            depth: info.depth,
            class: info.class,
        })
    }

    /// Returns what Xlib says of the visual `id`, or `None` if the server
    /// has no such visual. Its `visual` lives until the display is closed.
    fn visual_info(&self, id: VisualID) -> Option<XVisualInfo> {
        let mut template = XVisualInfo {
            visual: ptr::null_mut(),
            visualid: id,
            screen: 0,
            depth: 0,
            class: 0,
            red_mask: 0,
            green_mask: 0,
            blue_mask: 0,
            colormap_size: 0,
            bits_per_rgb: 0,
        };
        let mut count = 0;
        // SAFETY: the display is open, and the template and the count are
        // live. With this mask, Xlib reads only the template's `visualid`.
        let infos = unsafe {
            (self.xlib.XGetVisualInfo)(
                self.display.as_ptr(),
                VisualIDMask,
                &raw mut template,
                &raw mut count,
            )
        };
        if infos.is_null() {
            return None;
        }
        let info = if count > 0 {
            // SAFETY: Xlib returned an array of `count` visuals.
            Some(unsafe { infos.read() })
        } else {
            None
        };
        // SAFETY: Xlib allocated the array for us, and it is not used after
        // this. The visual that `info` points to belongs to the display, not
        // to the array. Xlib gives the result no meaning.
        unsafe { (self.xlib.XFree)(infos.cast()) };
        info
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

    /// Creates a top-level window of the visual `visual`, with a black
    /// background, and a colormap of that visual for it. Returns `None` if
    /// the server has no such visual. The window is not shown until
    /// [`Connection::map_window`]. The caller frees the colormap with
    /// [`Connection::free_colormap`] once the window is destroyed.
    pub(super) fn create_window(
        &self,
        root: Window,
        size: UVec2,
        visual: VisualID,
    ) -> Option<(Window, Colormap)> {
        let info = self.visual_info(visual)?;
        // SAFETY: the display is open, and the visual came from Xlib for it
        // and lives until it is closed.
        let colormap = unsafe {
            (self.xlib.XCreateColormap)(self.display.as_ptr(), root, info.visual, AllocNone)
        };
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
            colormap,
            cursor: 0,
        };
        // A window of a visual other than the root's needs a colormap of its
        // visual, and one of a depth other than the root's (24 on a root of
        // 30) a border pixel. The server refuses the window otherwise, so
        // both are always set.
        let mask = CWBackPixel | CWBorderPixel | CWColormap;
        // SAFETY: the display is open, `attributes` is a live struct, of which
        // the mask names three fields, and the visual came from Xlib for the
        // display. The depth is the visual's.
        let window = unsafe {
            (self.xlib.XCreateWindow)(
                self.display.as_ptr(),
                root,
                0,
                0,
                size.x as c_uint,
                size.y as c_uint,
                0,
                info.depth,
                InputOutput,
                info.visual,
                mask as c_ulong,
                &raw mut attributes,
            )
        };
        Some((window, colormap))
    }

    /// Frees a colormap.
    pub(super) fn free_colormap(&self, colormap: Colormap) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XFreeColormap)(self.display.as_ptr(), colormap) };
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

    /// Has the server send a key event to `window`, as if `keycode` went
    /// down or up with the modifiers `state` held: for the tests of typed
    /// text, which must not need a person at the keyboard.
    #[cfg(test)]
    pub(super) fn send_key(&self, window: Window, keycode: c_uint, pressed: bool, state: c_uint) {
        let mut event = XEvent { pad: [0; 24] };
        event.xkey = nadir_sys::linux::x11::XKeyEvent {
            type_: if pressed {
                xlib::KeyPress
            } else {
                xlib::KeyRelease
            },
            serial: 0,
            send_event: True,
            display: self.display.as_ptr(),
            window,
            root: self.root(),
            subwindow: 0,
            // `CurrentTime`.
            time: 0,
            x: 0,
            y: 0,
            x_root: 0,
            y_root: 0,
            state,
            keycode,
            same_screen: True,
        };
        let mask = if pressed {
            xlib::KeyPressMask
        } else {
            xlib::KeyReleaseMask
        };
        // SAFETY: the display is open, and `event` is a live `XEvent` that
        // holds a complete key event.
        let status = unsafe {
            (self.xlib.XSendEvent)(self.display.as_ptr(), window, False, mask, &raw mut event)
        };
        assert_ne!(status, 0, "XSendEvent could not send a key event");
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

// -- typed text ----------------------------------------------------------------

impl Connection {
    /// Opens an input method and an input context for `window`, so that
    /// key presses type text ([`Connection::lookup_text`]), and returns the
    /// events the context needs the window to select besides the keys. If
    /// none opens, or another connection has one ([`TEXT_INPUT_TAKEN`]), it
    /// says so in the log, and returns 0: the keys still work, and nothing is
    /// typed.
    ///
    /// It first takes the C library's character locale from the environment
    /// if the program left it at the default (see [`use_environment_ctype`]):
    /// in the C locale, Xlib composes Latin-1 characters only.
    pub(super) fn open_text_input(&mut self, window: Window) -> c_long {
        self.close_text_input();
        if TEXT_INPUT_TAKEN.swap(true, Ordering::Acquire) {
            warn!("another window has the X input method: typed text is not reported in this one");
            return 0;
        }
        use_environment_ctype(&self.xlib);
        for (modifiers, which) in INPUT_METHODS {
            // SAFETY: the modifiers are a C string, which Xlib copies. A null
            // result, a failure, leaves the modifiers as they were.
            let set = unsafe { (self.xlib.XSetLocaleModifiers)(modifiers.as_ptr()) };
            if set.is_null() {
                continue;
            }
            if let Some(text) = self.open_input_method(window) {
                let mask = self.filter_events(&text);
                debug!("typed text comes from an X input method: {which}");
                self.text = Some(text);
                return mask;
            }
        }
        TEXT_INPUT_TAKEN.store(false, Ordering::Release);
        warn!("no X input method opened: the keys work, and typed text is not reported");
        0
    }

    /// Opens the input method that the locale modifiers choose, watches it,
    /// and makes a context for `window` in it. Closes it again if any of
    /// that fails.
    fn open_input_method(&self, window: Window) -> Option<TextInput> {
        // SAFETY: the display is open, and the database and the names may
        // be null.
        let im = unsafe {
            (self.xlib.XOpenIM)(
                self.display.as_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        let im = NonNull::new(im)?;
        let lost = new_flag();
        let close = |im: NonNull<XIMRec>| {
            // SAFETY: the input method is open and has no context, and is
            // not used again. Xlib gives the result no meaning here.
            unsafe { (self.xlib.XCloseIM)(im.as_ptr()) };
            // SAFETY: the input method that could call back is closed.
            unsafe { free_flag(lost) };
        };
        let mut callback = XIMCallback {
            client_data: lost.as_ptr().cast::<c_char>(),
            callback: Some(input_method_lost),
        };
        // SAFETY: the input method is open. The name is followed by a
        // pointer to an `XIMCallback`, which Xlib copies, and the list ends
        // with a null pointer. The callback's data is the flag, which is
        // freed only after the input method is closed or lost.
        let failed = unsafe {
            (self.xlib.XSetIMValues)(
                im.as_ptr(),
                XNDestroyCallback.as_ptr(),
                &raw mut callback,
                ptr::null::<c_char>(),
            )
        };
        if !failed.is_null() {
            // Without the callback, a server that went away would leave the
            // input method dangling.
            close(im);
            return None;
        }
        // SAFETY: the input method is open. Each name is followed by a
        // value of the size Xlib reads it at, a pointer's: the style, then
        // the window twice, all `unsigned long`. The list ends with a null
        // pointer.
        let ic = unsafe {
            (self.xlib.XCreateIC)(
                im.as_ptr(),
                XNInputStyle.as_ptr(),
                XIMPreeditNothing | XIMStatusNothing,
                XNClientWindow.as_ptr(),
                window,
                XNFocusWindow.as_ptr(),
                window,
                ptr::null::<c_char>(),
            )
        };
        let Some(ic) = NonNull::new(ic) else {
            close(im);
            return None;
        };
        Some(TextInput { im, ic, lost })
    }

    /// Whether an input method and a context for the window are open.
    #[cfg(test)]
    pub(super) fn has_text_input(&self) -> bool {
        self.text.is_some()
    }

    /// The events that the input context needs the window to select.
    fn filter_events(&self, text: &TextInput) -> c_long {
        let mut mask: c_ulong = 0;
        // SAFETY: the context is live. The name is followed by a pointer to
        // an `unsigned long`, where Xlib writes the mask, and the list ends
        // with a null pointer.
        let failed = unsafe {
            (self.xlib.XGetICValues)(
                text.ic.as_ptr(),
                XNFilterEvents.as_ptr(),
                &raw mut mask,
                ptr::null::<c_char>(),
            )
        };
        if failed.is_null() {
            // An event mask is a `long` that the header reads as `unsigned`.
            mask as c_long
        } else {
            0
        }
    }

    /// The input context, while it works. If the input method stopped
    /// working, says so in the log and forgets it.
    fn live_context(&mut self) -> Option<NonNull<XICRec>> {
        if self.text.as_ref()?.is_lost() {
            warn!("the X input method stopped: typed text is not reported any more");
            self.close_text_input();
            return None;
        }
        self.text.as_ref().map(|text| text.ic)
    }

    /// Gives `event` to the input methods that asked for it, and returns
    /// whether one took it: a step of a Compose sequence, or a key press
    /// that goes to an input method server. A key press it took types
    /// nothing now; the key still went down. Read what the event says
    /// before: Xlib's own input method changes the press that ends a
    /// Compose sequence.
    ///
    /// # Safety
    ///
    /// `event` came from [`Connection::next_event`] of this connection.
    pub(super) unsafe fn filter_event(&self, event: &mut XEvent) -> bool {
        // SAFETY: the display is open, and the caller promises that Xlib
        // wrote the event for it. A window of 0 stands for the event's own.
        unsafe { (self.xlib.XFilterEvent)(event, 0) != False }
    }

    /// Returns what a key press typed, which may be nothing, until the next
    /// call.
    ///
    /// # Safety
    ///
    /// `event` came from [`Connection::next_event`] of this connection, is
    /// a `KeyPress`, and [`Connection::filter_event`] did not take it.
    pub(super) unsafe fn lookup_text(&mut self, event: &mut XEvent) -> &str {
        let Some(ic) = self.live_context() else {
            return "";
        };
        // Once for the text, and once more if it did not fit.
        for _ in 0..2 {
            let room = c_int::try_from(self.lookup.len()).unwrap_or(c_int::MAX);
            let mut status: Status = 0;
            // SAFETY: the context is live. The caller promises that the event
            // is a key press that Xlib wrote for this display, and that no
            // input method took. The buffer has `room` bytes, the key symbol
            // may be null, and the status is a live `Status`.
            let length = unsafe {
                (self.xlib.Xutf8LookupString)(
                    ic.as_ptr(),
                    &raw mut event.xkey,
                    self.lookup.as_mut_ptr().cast::<c_char>(),
                    room,
                    ptr::null_mut(),
                    &raw mut status,
                )
            };
            let length = usize::try_from(length).unwrap_or(0);
            match status {
                xlib::XLookupChars | xlib::XLookupBoth => {
                    let typed = &self.lookup[..length.min(self.lookup.len())];
                    // Xlib writes UTF-8 here.
                    return str::from_utf8(typed).unwrap_or("");
                }
                xlib::XBufferOverflow if length > self.lookup.len() => {
                    self.lookup.resize(length, 0);
                }
                _ => return "",
            }
        }
        ""
    }

    /// Tells the input method whether the window has the keyboard focus.
    pub(super) fn set_text_focus(&mut self, focused: bool) {
        let Some(ic) = self.live_context() else {
            return;
        };
        if focused {
            // SAFETY: the context is live.
            unsafe { (self.xlib.XSetICFocus)(ic.as_ptr()) };
        } else {
            // SAFETY: the context is live.
            unsafe { (self.xlib.XUnsetICFocus)(ic.as_ptr()) };
        }
    }

    /// Destroys the input context, then closes its input method, unless the
    /// input method stopped working, which freed both, and lets another
    /// connection have one. Call it before the context's window is
    /// destroyed. Nothing is typed after it.
    pub(super) fn close_text_input(&mut self) {
        let Some(text) = self.text.take() else {
            return;
        };
        if !text.is_lost() {
            // SAFETY: the context is live, and goes before its input method.
            unsafe { (self.xlib.XDestroyIC)(text.ic.as_ptr()) };
            // SAFETY: the input method is open, and has no context left.
            // Xlib gives the result no meaning here.
            unsafe { (self.xlib.XCloseIM)(text.im.as_ptr()) };
        }
        // SAFETY: the input method was closed just now, or was lost, and
        // calls back no more either way.
        unsafe { free_flag(text.lost) };
        TEXT_INPUT_TAKEN.store(false, Ordering::Release);
    }
}

/// What Xlib calls when an input method stops working, as one whose server
/// exited does: sets its flag.
///
/// # Safety
///
/// Only Xlib calls it, for an input method that [`Connection`] opened, with
/// that input method's flag as `client_data`, on the thread that uses the
/// display.
unsafe extern "C" fn input_method_lost(_im: XIM, client_data: XPointer, _call_data: XPointer) {
    // SAFETY: the data is the flag, which is freed only after its input
    // method is closed or lost, and this is the flag's thread.
    let lost = unsafe { &*client_data.cast::<Cell<bool>>() };
    lost.set(true);
}

/// Takes the C library's character locale (`LC_CTYPE`) from the environment
/// if it is the default, "C" (or "POSIX"), which a Rust program never
/// changes: Xlib's input methods read and compose text in it, and in "C"
/// they type Latin-1 characters only. A locale that is not installed, or
/// that Xlib cannot work in, leaves "C".
///
/// The locale is the whole process's, and so is Xlib's state of it. Only the
/// connection that took [`TEXT_INPUT_TAKEN`] calls this, so no other window
/// changes either meanwhile. Another thread's C code that reads the locale
/// would race with it: [`Platform::new`](crate::Platform::new) asks to open
/// the window first, and the engine opens it before its audio thread starts;
/// its other threads are Rust code, which does not read it.
fn use_environment_ctype(xlib: &Xlib) {
    // SAFETY: a null locale only returns the current name.
    let current = unsafe { setlocale(LC_CTYPE, ptr::null()) };
    if !current.is_null() {
        // SAFETY: the C library returned a NUL-terminated name, valid until
        // the next `setlocale`, which comes after this use.
        let name = unsafe { CStr::from_ptr(current) };
        if name != c"C" && name != c"POSIX" {
            return;
        }
    }
    // SAFETY: the empty name is a C string, and reads the environment. No
    // other window sets the locale meanwhile, and see the function's
    // documentation about other threads.
    let set = unsafe { setlocale(LC_CTYPE, c"".as_ptr()) };
    if set.is_null() {
        debug!("the environment names no installed character locale: typed text stays Latin-1");
        return;
    }
    // SAFETY: Xlib reads only the locale.
    if unsafe { (xlib.XSupportsLocale)() } == False {
        // SAFETY: as above; "C" is always installed.
        unsafe { setlocale(LC_CTYPE, c"C".as_ptr()) };
        debug!("Xlib cannot work in the environment's character locale: typed text stays Latin-1");
        return;
    }
    // Asked again: `XSupportsLocale` itself calls `setlocale`, after which
    // the name it returned before may be gone.
    // SAFETY: a null locale only returns the current name.
    let name = unsafe { setlocale(LC_CTYPE, ptr::null()) };
    if !name.is_null() {
        // SAFETY: the C library returned a NUL-terminated name, valid until
        // the next `setlocale`, which comes after this use.
        let name = unsafe { CStr::from_ptr(name) };
        debug!(
            "the character locale is the environment's from now on, {:?}, for typed text",
            name.to_string_lossy()
        );
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close_text_input();
        // SAFETY: the display is open, and nothing uses it after this: the
        // pointer goes away with `self`. Closing sends every queued request
        // first. Xlib gives the result no meaning.
        unsafe { (self.xlib.XCloseDisplay)(self.display.as_ptr()) };
    }
}

/// The depth and class of a visual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VisualFormat {
    /// The bits in a pixel.
    pub(super) depth: c_int,
    /// [`TrueColor`](nadir_sys::linux::x11::TrueColor) and others.
    pub(super) class: c_int,
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
