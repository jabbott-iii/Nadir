//! Xlib, the client library of the X Window System (`libX11.so.6`).
//!
//! Sources: `<X11/X.h>`, `<X11/Xlib.h>`, `<X11/Xutil.h>`, `<X11/Xatom.h>` and
//! `<X11/XKBlib.h>`. Each item names the header it comes from and quotes the
//! declaration there.
//!
//! Only what `nadir-platform` uses is declared. A struct has all the fields
//! of its header, in the header's order. The [`XEvent`] union has only the
//! members we use, and the padding that fixes its size.
//!
//! # Constants
//!
//! A constant keeps its header's name and value. One that the header gives a
//! type (`1L<<0`, `((Atom) 4)`) has that type. One that the header leaves as
//! a bare integer (`#define KeyPress 2`) has the type of the field or
//! parameter it is used with.
//!
//! # Layouts
//!
//! The sizes, alignments and offsets asserted below are those a C compiler
//! gives for the headers on x86_64 Linux. A C program printed them with
//! `sizeof`, `_Alignof` and `offsetof`; they were not read off the Rust
//! types. They hold wherever `long` and pointers are 8 bytes. On any other
//! target the assertions fail the build.
//!
//! # Calling Xlib
//!
//! What every function in [`Xlib`] requires of its caller:
//!
//! - A `display` argument came from `XOpenDisplay` and is not closed yet.
//! - All calls for one display come from one thread. We never call
//!   `XInitThreads`, so Xlib does no locking of its own.
//! - A window, pixmap, cursor or atom argument belongs to that display.
//! - A pointer argument is valid for what the function reads or writes
//!   through it.
//!
//! Many Xlib functions are declared to return `int`, and the Xlib manual
//! gives that result no meaning. They only queue a request. If the server
//! rejects it, an X error arrives later, and Xlib's default error handler
//! then ends the process. Where a result does mean something, the function's
//! documentation here says what.

// The constants keep the names their headers give them (`KeyPress`, `True`),
// so that each one can be compared with its header.
#![allow(non_upper_case_globals)]

use core::ffi::{c_char, c_int, c_long, c_short, c_uchar, c_uint, c_ulong, c_ushort, c_void};
use core::marker::{PhantomData, PhantomPinned};
use core::mem::offset_of;

use crate::dynlib::dynlib;

// ---------------------------------------------------------------------------
// Basic types
// ---------------------------------------------------------------------------

/// The id of a resource that the server holds for us.
/// `X11/X.h`: `typedef unsigned long XID;`
pub type XID = c_ulong;

/// A window. `X11/X.h`: `typedef XID Window;`
pub type Window = XID;

/// A window or a pixmap. `X11/X.h`: `typedef XID Drawable;`
pub type Drawable = XID;

/// An image held by the server. `X11/X.h`: `typedef XID Pixmap;`
pub type Pixmap = XID;

/// A pointer shape. `X11/X.h`: `typedef XID Cursor;`
pub type Cursor = XID;

/// A color table. `X11/X.h`: `typedef XID Colormap;`
pub type Colormap = XID;

/// The id of a visual: a way of turning pixel values into colors.
/// `X11/X.h`: `typedef unsigned long VisualID;`
pub type VisualID = c_ulong;

/// A number the server assigns to a name.
/// `X11/X.h`: `typedef unsigned long Atom;`
pub type Atom = c_ulong;

/// A server timestamp, in milliseconds.
/// `X11/X.h`: `typedef unsigned long Time;`
pub type Time = c_ulong;

/// A truth value: [`True`] or [`False`]. `X11/Xlib.h`: `#define Bool int`
pub type Bool = c_int;

/// The result of some Xlib functions: zero for failure.
/// `X11/Xlib.h`: `#define Status int`
pub type Status = c_int;

/// `X11/Xlib.h`: `#define True 1`
pub const True: Bool = 1;

/// `X11/Xlib.h`: `#define False 0`
pub const False: Bool = 0;

/// A connection to an X server. Only Xlib looks inside it.
/// `X11/Xlib.h`: `typedef struct _XDisplay Display;`
#[repr(C)]
pub struct Display {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// A way of turning pixel values into colors. The header defines its fields,
/// but we only pass pointers to it, so it is opaque here.
/// `X11/Xlib.h`: `typedef struct { ... } Visual;`
#[repr(C)]
pub struct Visual {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

// ---------------------------------------------------------------------------
// Creating a window
// ---------------------------------------------------------------------------

/// The `class` of a window that can be drawn to, for `XCreateWindow`.
/// `X11/X.h`: `#define InputOutput 1`
pub const InputOutput: c_uint = 1;

/// A bit of the `valuemask` of `XCreateWindow`: `background_pixel` is set.
/// `X11/X.h`: `#define CWBackPixel (1L<<1)`
pub const CWBackPixel: c_long = 1 << 1;

/// A bit of the `valuemask` of `XCreateWindow`: `border_pixel` is set. A
/// window of a depth other than its parent's needs it (or a border pixmap),
/// because the default copies the parent's border, which needs the same
/// depth. Without one the server refuses the window.
/// `X11/X.h`: `#define CWBorderPixel (1L<<3)`
pub const CWBorderPixel: c_long = 1 << 3;

/// A bit of the `valuemask` of `XCreateWindow`: `colormap` is set. A window
/// of a visual other than its parent's needs a colormap of that visual, or
/// the server refuses the window.
/// `X11/X.h`: `#define CWColormap (1L<<13)`
pub const CWColormap: c_long = 1 << 13;

/// The attributes of a new window, for `XCreateWindow`. The function reads
/// only the fields that its `valuemask` names.
/// `X11/Xlib.h`: `typedef struct { ... } XSetWindowAttributes;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XSetWindowAttributes {
    /// The background as a pixmap.
    pub background_pixmap: Pixmap,
    /// The background as one pixel value.
    pub background_pixel: c_ulong,
    /// The border as a pixmap.
    pub border_pixmap: Pixmap,
    /// The border as one pixel value.
    pub border_pixel: c_ulong,
    /// Where the contents stay when the window is resized.
    pub bit_gravity: c_int,
    /// Where the window stays when its parent is resized.
    pub win_gravity: c_int,
    /// Whether the server should keep the contents of hidden parts.
    pub backing_store: c_int,
    /// The planes to keep, if the server keeps contents.
    pub backing_planes: c_ulong,
    /// The value for the planes that are not kept.
    pub backing_pixel: c_ulong,
    /// Whether the server should save what the window covers.
    pub save_under: Bool,
    /// The events the window wants.
    pub event_mask: c_long,
    /// The events that must not pass on to the parent.
    pub do_not_propagate_mask: c_long,
    /// Whether the window manager is to leave the window alone.
    pub override_redirect: Bool,
    /// The color table of the window.
    pub colormap: Colormap,
    /// The pointer shape over the window.
    pub cursor: Cursor,
}

const _: () = assert!(size_of::<XSetWindowAttributes>() == 112);
const _: () = assert!(align_of::<XSetWindowAttributes>() == 8);
const _: () = assert!(offset_of!(XSetWindowAttributes, background_pixel) == 8);
const _: () = assert!(offset_of!(XSetWindowAttributes, border_pixel) == 24);
const _: () = assert!(offset_of!(XSetWindowAttributes, colormap) == 96);

// ---------------------------------------------------------------------------
// Visuals and colormaps
// ---------------------------------------------------------------------------

/// A visual and what it is like, as `XGetVisualInfo` describes it.
/// `X11/Xutil.h`: `typedef struct { ... } XVisualInfo;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XVisualInfo {
    /// The visual. It belongs to the display, and lives until the display
    /// is closed.
    pub visual: *mut Visual,
    /// The id of the visual.
    pub visualid: VisualID,
    /// The number of the screen the visual is on.
    pub screen: c_int,
    /// The bits in a pixel of a window of this visual.
    pub depth: c_int,
    /// How pixel values become colors: [`TrueColor`] and others. `class` in
    /// C, and `c_class` in C++.
    pub class: c_int,
    /// The bits of a pixel value that hold red.
    pub red_mask: c_ulong,
    /// The bits that hold green.
    pub green_mask: c_ulong,
    /// The bits that hold blue.
    pub blue_mask: c_ulong,
    /// The number of entries in a colormap of the visual.
    pub colormap_size: c_int,
    /// The bits of each of red, green and blue.
    pub bits_per_rgb: c_int,
}

const _: () = assert!(size_of::<XVisualInfo>() == 64);
const _: () = assert!(align_of::<XVisualInfo>() == 8);
const _: () = assert!(offset_of!(XVisualInfo, visual) == 0);
const _: () = assert!(offset_of!(XVisualInfo, visualid) == 8);
const _: () = assert!(offset_of!(XVisualInfo, screen) == 16);
const _: () = assert!(offset_of!(XVisualInfo, depth) == 20);
const _: () = assert!(offset_of!(XVisualInfo, class) == 24);
const _: () = assert!(offset_of!(XVisualInfo, red_mask) == 32);
const _: () = assert!(offset_of!(XVisualInfo, colormap_size) == 56);
const _: () = assert!(offset_of!(XVisualInfo, bits_per_rgb) == 60);

/// A bit of the `vinfo_mask` of `XGetVisualInfo`: match the `visualid` of
/// the template.
/// `X11/Xutil.h`: `#define VisualIDMask 0x1`
pub const VisualIDMask: c_long = 0x1;

/// A [`XVisualInfo::class`]: a pixel value is red, green and blue bits,
/// which mean fixed intensities. Desktops draw in visuals of this class.
/// `X11/X.h`: `#define TrueColor 4`
pub const TrueColor: c_int = 4;

/// The `alloc` of `XCreateColormap`: allocate no entries, which is what a
/// colormap of a [`TrueColor`] visual needs.
/// `X11/X.h`: `#define AllocNone 0`
pub const AllocNone: c_int = 0;

/// A bit of [`XSizeHints::flags`]: `min_width` and `min_height` are set.
/// `X11/Xutil.h`: `#define PMinSize (1L << 4)`
pub const PMinSize: c_long = 1 << 4;

/// A bit of [`XSizeHints::flags`]: `max_width` and `max_height` are set.
/// `X11/Xutil.h`: `#define PMaxSize (1L << 5)`
pub const PMaxSize: c_long = 1 << 5;

/// A ratio of width to height in [`XSizeHints`]. The header gives this
/// struct no name. `X11/Xutil.h`: `struct { int x; int y; } min_aspect,
/// max_aspect;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XAspectRatio {
    /// The numerator.
    pub x: c_int,
    /// The denominator.
    pub y: c_int,
}

const _: () = assert!(size_of::<XAspectRatio>() == 8);
const _: () = assert!(align_of::<XAspectRatio>() == 4);

/// What sizes a window accepts, for `XSetWMNormalHints`. The window manager
/// reads only the fields that `flags` names.
/// `X11/Xutil.h`: `typedef struct { ... } XSizeHints;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XSizeHints {
    /// Which of the fields below are set: [`PMinSize`], [`PMaxSize`].
    pub flags: c_long,
    /// Obsolete.
    pub x: c_int,
    /// Obsolete.
    pub y: c_int,
    /// Obsolete.
    pub width: c_int,
    /// Obsolete.
    pub height: c_int,
    /// The smallest width.
    pub min_width: c_int,
    /// The smallest height.
    pub min_height: c_int,
    /// The largest width.
    pub max_width: c_int,
    /// The largest height.
    pub max_height: c_int,
    /// The step in which the width changes.
    pub width_inc: c_int,
    /// The step in which the height changes.
    pub height_inc: c_int,
    /// The narrowest shape.
    pub min_aspect: XAspectRatio,
    /// The widest shape.
    pub max_aspect: XAspectRatio,
    /// The width the steps start from.
    pub base_width: c_int,
    /// The height the steps start from.
    pub base_height: c_int,
    /// Which corner stays in place when the window manager adds its frame.
    pub win_gravity: c_int,
}

const _: () = assert!(size_of::<XSizeHints>() == 80);
const _: () = assert!(align_of::<XSizeHints>() == 8);
const _: () = assert!(offset_of!(XSizeHints, flags) == 0);
const _: () = assert!(offset_of!(XSizeHints, min_width) == 24);
const _: () = assert!(offset_of!(XSizeHints, min_height) == 28);
const _: () = assert!(offset_of!(XSizeHints, max_width) == 32);
const _: () = assert!(offset_of!(XSizeHints, max_height) == 36);

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

/// The `mode` of `XChangeProperty` that discards the old value.
/// `X11/X.h`: `#define PropModeReplace 0`
pub const PropModeReplace: c_int = 0;

/// The property type "list of atoms".
/// `X11/Xatom.h`: `#define XA_ATOM ((Atom) 4)`
pub const XA_ATOM: Atom = 4;

/// The property type "Latin-1 text".
/// `X11/Xatom.h`: `#define XA_STRING ((Atom) 31)`
pub const XA_STRING: Atom = 31;

/// The property that holds a window's title.
/// `X11/Xatom.h`: `#define XA_WM_NAME ((Atom) 39)`
pub const XA_WM_NAME: Atom = 39;

// ---------------------------------------------------------------------------
// Cursors
// ---------------------------------------------------------------------------

/// A color, for `XCreatePixmapCursor`.
/// `X11/Xlib.h`: `typedef struct { ... } XColor;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XColor {
    /// The pixel value. `XCreatePixmapCursor` ignores it.
    pub pixel: c_ulong,
    /// Red, from 0 to 65535.
    pub red: c_ushort,
    /// Green, from 0 to 65535.
    pub green: c_ushort,
    /// Blue, from 0 to 65535.
    pub blue: c_ushort,
    /// Which of red, green and blue are set.
    pub flags: c_char,
    /// Unused.
    pub pad: c_char,
}

const _: () = assert!(size_of::<XColor>() == 16);
const _: () = assert!(align_of::<XColor>() == 8);
const _: () = assert!(offset_of!(XColor, red) == 8);
const _: () = assert!(offset_of!(XColor, green) == 10);
const _: () = assert!(offset_of!(XColor, blue) == 12);

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Event selection: keys going down. `X11/X.h`: `#define KeyPressMask (1L<<0)`
pub const KeyPressMask: c_long = 1 << 0;

/// Event selection: keys going up.
/// `X11/X.h`: `#define KeyReleaseMask (1L<<1)`
pub const KeyReleaseMask: c_long = 1 << 1;

/// Event selection: pointer buttons going down.
/// `X11/X.h`: `#define ButtonPressMask (1L<<2)`
pub const ButtonPressMask: c_long = 1 << 2;

/// Event selection: pointer buttons going up.
/// `X11/X.h`: `#define ButtonReleaseMask (1L<<3)`
pub const ButtonReleaseMask: c_long = 1 << 3;

/// Event selection: the pointer moving.
/// `X11/X.h`: `#define PointerMotionMask (1L<<6)`
pub const PointerMotionMask: c_long = 1 << 6;

/// Event selection: changes to the window itself, such as its size.
/// `X11/X.h`: `#define StructureNotifyMask (1L<<17)`
pub const StructureNotifyMask: c_long = 1 << 17;

/// Event selection: changes to the window's children. With
/// [`SubstructureRedirectMask`], this addresses a message for the window
/// manager to the root window.
/// `X11/X.h`: `#define SubstructureNotifyMask (1L<<19)`
pub const SubstructureNotifyMask: c_long = 1 << 19;

/// Event selection: requests to change the window's children, which is what
/// a window manager selects on the root window.
/// `X11/X.h`: `#define SubstructureRedirectMask (1L<<20)`
pub const SubstructureRedirectMask: c_long = 1 << 20;

/// Event selection: the window gaining or losing the keyboard focus.
/// `X11/X.h`: `#define FocusChangeMask (1L<<21)`
pub const FocusChangeMask: c_long = 1 << 21;

/// Event type: a key went down; see [`XKeyEvent`].
/// `X11/X.h`: `#define KeyPress 2`
pub const KeyPress: c_int = 2;

/// Event type: a key went up; see [`XKeyEvent`].
/// `X11/X.h`: `#define KeyRelease 3`
pub const KeyRelease: c_int = 3;

/// Event type: a pointer button went down; see [`XButtonEvent`].
/// `X11/X.h`: `#define ButtonPress 4`
pub const ButtonPress: c_int = 4;

/// Event type: a pointer button went up; see [`XButtonEvent`].
/// `X11/X.h`: `#define ButtonRelease 5`
pub const ButtonRelease: c_int = 5;

/// Event type: the pointer moved; see [`XMotionEvent`].
/// `X11/X.h`: `#define MotionNotify 6`
pub const MotionNotify: c_int = 6;

/// Event type: the window gained the keyboard focus; see
/// [`XFocusChangeEvent`]. `X11/X.h`: `#define FocusIn 9`
pub const FocusIn: c_int = 9;

/// Event type: the window lost the keyboard focus; see
/// [`XFocusChangeEvent`]. `X11/X.h`: `#define FocusOut 10`
pub const FocusOut: c_int = 10;

/// Event type: the window's size, position or stacking changed; see
/// [`XConfigureEvent`]. `X11/X.h`: `#define ConfigureNotify 22`
pub const ConfigureNotify: c_int = 22;

/// Event type: a message from another client; see [`XClientMessageEvent`].
/// `X11/X.h`: `#define ClientMessage 33`
pub const ClientMessage: c_int = 33;

/// [`XButtonEvent::button`]: the left button. `X11/X.h`: `#define Button1 1`
pub const Button1: c_uint = 1;

/// [`XButtonEvent::button`]: the middle button.
/// `X11/X.h`: `#define Button2 2`
pub const Button2: c_uint = 2;

/// [`XButtonEvent::button`]: the right button.
/// `X11/X.h`: `#define Button3 3`
pub const Button3: c_uint = 3;

/// [`XButtonEvent::button`]: one step of the wheel away from the user.
/// `X11/X.h`: `#define Button4 4`
pub const Button4: c_uint = 4;

/// [`XButtonEvent::button`]: one step of the wheel toward the user.
/// `X11/X.h`: `#define Button5 5`
pub const Button5: c_uint = 5;

/// [`XFocusChangeEvent::mode`]: a keyboard grab started, which moves the
/// focus for as long as the grab lasts. `X11/X.h`: `#define NotifyGrab 1`
pub const NotifyGrab: c_int = 1;

/// [`XFocusChangeEvent::mode`]: a keyboard grab ended.
/// `X11/X.h`: `#define NotifyUngrab 2`
pub const NotifyUngrab: c_int = 2;

/// A key going down ([`KeyPress`]) or up ([`KeyRelease`]).
/// `X11/Xlib.h`: `typedef struct { ... } XKeyEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XKeyEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window the event is reported to.
    pub window: Window,
    /// The root window of the screen.
    pub root: Window,
    /// The child window the pointer is in, if any.
    pub subwindow: Window,
    /// When it happened.
    pub time: Time,
    /// The pointer's x in the window.
    pub x: c_int,
    /// The pointer's y in the window.
    pub y: c_int,
    /// The pointer's x on the screen.
    pub x_root: c_int,
    /// The pointer's y on the screen.
    pub y_root: c_int,
    /// The modifier keys and buttons held just before the event.
    pub state: c_uint,
    /// Which physical key. On a server with XKB this is the Linux evdev
    /// code plus 8.
    pub keycode: c_uint,
    /// Whether the pointer is on the same screen as the window.
    pub same_screen: Bool,
}

const _: () = assert!(size_of::<XKeyEvent>() == 96);
const _: () = assert!(align_of::<XKeyEvent>() == 8);
const _: () = assert!(offset_of!(XKeyEvent, keycode) == 84);

/// A pointer button going down ([`ButtonPress`]) or up ([`ButtonRelease`]).
/// `X11/Xlib.h`: `typedef struct { ... } XButtonEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XButtonEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window the event is reported to.
    pub window: Window,
    /// The root window of the screen.
    pub root: Window,
    /// The child window the pointer is in, if any.
    pub subwindow: Window,
    /// When it happened.
    pub time: Time,
    /// The pointer's x in the window.
    pub x: c_int,
    /// The pointer's y in the window.
    pub y: c_int,
    /// The pointer's x on the screen.
    pub x_root: c_int,
    /// The pointer's y on the screen.
    pub y_root: c_int,
    /// The modifier keys and buttons held just before the event.
    pub state: c_uint,
    /// Which button: [`Button1`] and so on.
    pub button: c_uint,
    /// Whether the pointer is on the same screen as the window.
    pub same_screen: Bool,
}

const _: () = assert!(size_of::<XButtonEvent>() == 96);
const _: () = assert!(align_of::<XButtonEvent>() == 8);
const _: () = assert!(offset_of!(XButtonEvent, x) == 64);
const _: () = assert!(offset_of!(XButtonEvent, y) == 68);
const _: () = assert!(offset_of!(XButtonEvent, button) == 84);

/// The pointer moving ([`MotionNotify`]).
/// `X11/Xlib.h`: `typedef struct { ... } XMotionEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XMotionEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window the event is reported to.
    pub window: Window,
    /// The root window of the screen.
    pub root: Window,
    /// The child window the pointer is in, if any.
    pub subwindow: Window,
    /// When it happened.
    pub time: Time,
    /// The pointer's x in the window.
    pub x: c_int,
    /// The pointer's y in the window.
    pub y: c_int,
    /// The pointer's x on the screen.
    pub x_root: c_int,
    /// The pointer's y on the screen.
    pub y_root: c_int,
    /// The modifier keys and buttons held.
    pub state: c_uint,
    /// Whether this is a hint that stands for several motions.
    pub is_hint: c_char,
    /// Whether the pointer is on the same screen as the window.
    pub same_screen: Bool,
}

const _: () = assert!(size_of::<XMotionEvent>() == 96);
const _: () = assert!(align_of::<XMotionEvent>() == 8);
const _: () = assert!(offset_of!(XMotionEvent, x) == 64);
const _: () = assert!(offset_of!(XMotionEvent, y) == 68);

/// The window gaining ([`FocusIn`]) or losing ([`FocusOut`]) the keyboard
/// focus. `X11/Xlib.h`: `typedef struct { ... } XFocusChangeEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XFocusChangeEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window.
    pub window: Window,
    /// Why the focus moved: [`NotifyGrab`], [`NotifyUngrab`], or a value
    /// for an ordinary change.
    pub mode: c_int,
    /// How the two windows the focus moved between are related.
    pub detail: c_int,
}

const _: () = assert!(size_of::<XFocusChangeEvent>() == 48);
const _: () = assert!(align_of::<XFocusChangeEvent>() == 8);
const _: () = assert!(offset_of!(XFocusChangeEvent, mode) == 40);

/// The window's size, position, border or stacking changing
/// ([`ConfigureNotify`]). `X11/Xlib.h`: `typedef struct { ... }
/// XConfigureEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XConfigureEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window the event is reported to.
    pub event: Window,
    /// The window that changed.
    pub window: Window,
    /// The new x of the window in its parent.
    pub x: c_int,
    /// The new y of the window in its parent.
    pub y: c_int,
    /// The new width of the inside of the window, in pixels.
    pub width: c_int,
    /// The new height of the inside of the window, in pixels.
    pub height: c_int,
    /// The new border width.
    pub border_width: c_int,
    /// The sibling the window is now stacked above, if any.
    pub above: Window,
    /// Whether the window manager leaves the window alone.
    pub override_redirect: Bool,
}

const _: () = assert!(size_of::<XConfigureEvent>() == 88);
const _: () = assert!(align_of::<XConfigureEvent>() == 8);
const _: () = assert!(offset_of!(XConfigureEvent, width) == 56);
const _: () = assert!(offset_of!(XConfigureEvent, height) == 60);

/// The payload of an [`XClientMessageEvent`]. Which member is in use depends
/// on the event's `format`. The header gives this union no name.
/// `X11/Xlib.h`: `union { char b[20]; short s[10]; long l[5]; } data;`
#[repr(C)]
#[derive(Clone, Copy)]
pub union XClientMessageData {
    /// For `format` 8.
    pub b: [c_char; 20],
    /// For `format` 16.
    pub s: [c_short; 10],
    /// For `format` 32. Each item is a C `long`, which is 8 bytes on 64-bit
    /// Linux, but only its low 32 bits travel to and from the server.
    pub l: [c_long; 5],
}

const _: () = assert!(size_of::<XClientMessageData>() == 40);
const _: () = assert!(align_of::<XClientMessageData>() == 8);

/// A message from another client ([`ClientMessage`]). The window manager
/// sends one when the user asks to close the window, and we send one to ask
/// the window manager for fullscreen.
/// `X11/Xlib.h`: `typedef struct { ... } XClientMessageEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XClientMessageEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window the message is about.
    pub window: Window,
    /// What kind of message this is.
    pub message_type: Atom,
    /// The size of the items in `data`, in bits: 8, 16 or 32.
    pub format: c_int,
    /// The payload.
    pub data: XClientMessageData,
}

const _: () = assert!(size_of::<XClientMessageEvent>() == 96);
const _: () = assert!(align_of::<XClientMessageEvent>() == 8);
const _: () = assert!(offset_of!(XClientMessageEvent, type_) == 0);
const _: () = assert!(offset_of!(XClientMessageEvent, send_event) == 16);
const _: () = assert!(offset_of!(XClientMessageEvent, window) == 32);
const _: () = assert!(offset_of!(XClientMessageEvent, message_type) == 40);
const _: () = assert!(offset_of!(XClientMessageEvent, format) == 48);
const _: () = assert!(offset_of!(XClientMessageEvent, data) == 56);

/// Any event. `type_` says which member holds it.
///
/// The header's union has a member for every kind of event. This one has
/// only those we read or write, and `pad`, which gives it the header's size.
/// `XNextEvent` can therefore write any event into it.
/// `X11/Xlib.h`: `typedef union _XEvent { ... } XEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub union XEvent {
    /// The event type: [`KeyPress`] and so on. `type` in C. Every member
    /// starts with it.
    pub type_: c_int,
    /// For [`KeyPress`] and [`KeyRelease`].
    pub xkey: XKeyEvent,
    /// For [`ButtonPress`] and [`ButtonRelease`].
    pub xbutton: XButtonEvent,
    /// For [`MotionNotify`].
    pub xmotion: XMotionEvent,
    /// For [`FocusIn`] and [`FocusOut`].
    pub xfocus: XFocusChangeEvent,
    /// For [`ConfigureNotify`].
    pub xconfigure: XConfigureEvent,
    /// For [`ClientMessage`].
    pub xclient: XClientMessageEvent,
    /// Fixes the size. `X11/Xlib.h`: `long pad[24];`
    pub pad: [c_long; 24],
}

const _: () = assert!(size_of::<XEvent>() == 192);
const _: () = assert!(align_of::<XEvent>() == 8);

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

dynlib! {
    /// The functions we call in `libX11.so.6`.
    ///
    /// The module documentation lists what every one of them requires.
    pub struct Xlib: extern "C" {
        /// Connects to the X server that `display_name` names, or the one in
        /// the `DISPLAY` environment variable if it is null. Returns null on
        /// failure.
        /// `X11/Xlib.h`: `Display *XOpenDisplay(_Xconst char *display_name)`
        ///
        /// The connection is ours. Close it with `XCloseDisplay`.
        fn XOpenDisplay(display_name: *const c_char) -> *mut Display;

        /// Closes the connection. The server then destroys every window,
        /// pixmap and cursor that was made through it. `display` must not
        /// be used again.
        /// `X11/Xlib.h`: `int XCloseDisplay(Display *display)`
        fn XCloseDisplay(display: *mut Display) -> c_int;

        /// Returns the number of the screen that `XOpenDisplay` chose.
        /// `X11/Xlib.h`: `int XDefaultScreen(Display *display)`
        fn XDefaultScreen(display: *mut Display) -> c_int;

        /// Returns the root window of a screen. The window belongs to the
        /// server.
        /// `X11/Xlib.h`: `Window XRootWindow(Display *display, int
        /// screen_number)`
        fn XRootWindow(display: *mut Display, screen_number: c_int) -> Window;

        /// Returns the visuals that match the fields of `vinfo_template`
        /// that `vinfo_mask` names ([`VisualIDMask`] and others), and writes
        /// their number to `nitems_return`. Returns null if none match.
        /// `X11/Xutil.h`: `XVisualInfo *XGetVisualInfo(Display *display, long
        /// vinfo_mask, XVisualInfo *vinfo_template, int *nitems_return)`
        ///
        /// The array is ours. Free it with `XFree`. The visuals it points to
        /// belong to the display, and live until it is closed.
        fn XGetVisualInfo(
            display: *mut Display,
            vinfo_mask: c_long,
            vinfo_template: *mut XVisualInfo,
            nitems_return: *mut c_int,
        ) -> *mut XVisualInfo;

        /// Frees memory that Xlib allocated for us, such as what
        /// `XGetVisualInfo` returns.
        /// `X11/Xlib.h`: `int XFree(void *data)`
        fn XFree(data: *mut c_void) -> c_int;

        /// Creates a colormap of `visual` on the screen of the window `w`.
        /// `X11/Xlib.h`: `Colormap XCreateColormap(Display *display, Window w,
        /// Visual *visual, int alloc)`
        ///
        /// The colormap is ours. Free it with `XFreeColormap`.
        fn XCreateColormap(
            display: *mut Display,
            w: Window,
            visual: *mut Visual,
            alloc: c_int,
        ) -> Colormap;

        /// Frees a colormap. A window that still uses it has no colormap
        /// after this.
        /// `X11/Xlib.h`: `int XFreeColormap(Display *display, Colormap
        /// colormap)`
        fn XFreeColormap(display: *mut Display, colormap: Colormap) -> c_int;

        /// Creates a window, which is not shown until it is mapped, and
        /// returns its id. `attributes` may be null if `valuemask` is 0.
        /// `X11/Xlib.h`: `Window XCreateWindow(Display *display, Window parent,
        /// int x, int y, unsigned int width, unsigned int height, unsigned int
        /// border_width, int depth, unsigned int class, Visual *visual,
        /// unsigned long valuemask, XSetWindowAttributes *attributes)`
        ///
        /// The window is ours. Destroy it with `XDestroyWindow`.
        fn XCreateWindow(
            display: *mut Display,
            parent: Window,
            x: c_int,
            y: c_int,
            width: c_uint,
            height: c_uint,
            border_width: c_uint,
            depth: c_int,
            class: c_uint,
            visual: *mut Visual,
            valuemask: c_ulong,
            attributes: *mut XSetWindowAttributes,
        ) -> Window;

        /// Destroys a window and all its children.
        /// `X11/Xlib.h`: `int XDestroyWindow(Display *display, Window w)`
        fn XDestroyWindow(display: *mut Display, w: Window) -> c_int;

        /// Shows a window.
        /// `X11/Xlib.h`: `int XMapWindow(Display *display, Window w)`
        fn XMapWindow(display: *mut Display, w: Window) -> c_int;

        /// Chooses which events the server reports for a window:
        /// [`KeyPressMask`] and so on.
        /// `X11/Xlib.h`: `int XSelectInput(Display *display, Window w, long
        /// event_mask)`
        fn XSelectInput(display: *mut Display, w: Window, event_mask: c_long) -> c_int;

        /// Returns the atom for a name. If the server has none: makes one
        /// when `only_if_exists` is [`False`], and returns 0 when it is
        /// [`True`]. `atom_name` is a NUL-terminated string.
        /// `X11/Xlib.h`: `Atom XInternAtom(Display *display, _Xconst
        /// char *atom_name, Bool only_if_exists)`
        fn XInternAtom(
            display: *mut Display,
            atom_name: *const c_char,
            only_if_exists: Bool,
        ) -> Atom;

        /// Sets the `WM_PROTOCOLS` property of a window to `count` atoms at
        /// `protocols`: the messages the window manager may send it. Returns
        /// zero if it could not get the `WM_PROTOCOLS` atom.
        /// `X11/Xlib.h`: `Status XSetWMProtocols(Display *display, Window w,
        /// Atom *protocols, int count)`
        fn XSetWMProtocols(
            display: *mut Display,
            w: Window,
            protocols: *mut Atom,
            count: c_int,
        ) -> Status;

        /// Sets a property of a window to `nelements` items at `data`.
        /// `format` is the size of an item in bits: 8, 16 or 32.
        ///
        /// With `format` 32, `data` must point to an array of C `long`,
        /// which is 8 bytes an item on 64-bit Linux. Xlib sends the low 32
        /// bits of each.
        /// `X11/Xlib.h`: `int XChangeProperty(Display *display, Window w, Atom
        /// property, Atom type, int format, int mode, _Xconst unsigned
        /// char *data, int nelements)`
        fn XChangeProperty(
            display: *mut Display,
            w: Window,
            property: Atom,
            type_: Atom,
            format: c_int,
            mode: c_int,
            data: *const c_uchar,
            nelements: c_int,
        ) -> c_int;

        /// Sets the `WM_NORMAL_HINTS` property of a window: the sizes it
        /// accepts. Xlib copies what it needs from `hints`.
        /// `X11/Xutil.h`: `void XSetWMNormalHints(Display *display, Window w,
        /// XSizeHints *hints)`
        fn XSetWMNormalHints(display: *mut Display, w: Window, hints: *mut XSizeHints);

        /// Returns how many events are waiting. If none are waiting, it
        /// first sends every queued request to the server and reads what
        /// has arrived. It does not block.
        /// `X11/Xlib.h`: `int XPending(Display *display)`
        fn XPending(display: *mut Display) -> c_int;

        /// Removes the next event from the queue and writes it to
        /// `event_return`. If the queue is empty, it sends every queued
        /// request and blocks until an event arrives.
        /// `X11/Xlib.h`: `int XNextEvent(Display *display,
        /// XEvent *event_return)`
        fn XNextEvent(display: *mut Display, event_return: *mut XEvent) -> c_int;

        /// Has the server deliver `event_send` to a window, as if the event
        /// had happened there, to the clients that selected any event of
        /// `event_mask`. Returns zero if the event could not be converted
        /// for sending.
        /// `X11/Xlib.h`: `Status XSendEvent(Display *display, Window w, Bool
        /// propagate, long event_mask, XEvent *event_send)`
        fn XSendEvent(
            display: *mut Display,
            w: Window,
            propagate: Bool,
            event_mask: c_long,
            event_send: *mut XEvent,
        ) -> Status;

        /// Returns the `RESOURCE_MANAGER` property of the root window as it
        /// was when the display was opened: the user's X resources, as
        /// NUL-terminated text. Returns null if there is none.
        /// `X11/Xlib.h`: `char *XResourceManagerString(Display *display)`
        ///
        /// The string belongs to Xlib: do not free it. It is valid until
        /// the display is closed.
        fn XResourceManagerString(display: *mut Display) -> *mut c_char;

        /// Creates a pixmap one bit deep from `data`, on the screen of the
        /// drawable `d`. `data` holds the rows from the top. Each row is
        /// padded to whole bytes, and the lowest bit of a byte is the
        /// leftmost pixel. Returns 0 on failure.
        /// `X11/Xlib.h`: `Pixmap XCreateBitmapFromData(Display *display,
        /// Drawable d, _Xconst char *data, unsigned int width, unsigned int
        /// height)`
        ///
        /// The pixmap is ours. Free it with `XFreePixmap`.
        fn XCreateBitmapFromData(
            display: *mut Display,
            d: Drawable,
            data: *const c_char,
            width: c_uint,
            height: c_uint,
        ) -> Pixmap;

        /// Creates a pointer shape from two pixmaps that are one bit deep.
        /// A pixel shows where `mask` has a 1. It has the foreground color
        /// where `source` has a 1, and the background color elsewhere. The
        /// point (`x`, `y`) of the shape is the one that points. The pixmaps
        /// can be freed as soon as this returns.
        /// `X11/Xlib.h`: `Cursor XCreatePixmapCursor(Display *display, Pixmap
        /// source, Pixmap mask, XColor *foreground_color,
        /// XColor *background_color, unsigned int x, unsigned int y)`
        ///
        /// The cursor is ours. Free it with `XFreeCursor`.
        fn XCreatePixmapCursor(
            display: *mut Display,
            source: Pixmap,
            mask: Pixmap,
            foreground_color: *mut XColor,
            background_color: *mut XColor,
            x: c_uint,
            y: c_uint,
        ) -> Cursor;

        /// Frees a pixmap. The server keeps the image for as long as
        /// something else still uses it.
        /// `X11/Xlib.h`: `int XFreePixmap(Display *display, Pixmap pixmap)`
        fn XFreePixmap(display: *mut Display, pixmap: Pixmap) -> c_int;

        /// Sets the pointer shape that shows while the pointer is over a
        /// window.
        /// `X11/Xlib.h`: `int XDefineCursor(Display *display, Window w, Cursor
        /// cursor)`
        fn XDefineCursor(display: *mut Display, w: Window, cursor: Cursor) -> c_int;

        /// Undoes `XDefineCursor`: the window shows its parent's pointer
        /// shape again.
        /// `X11/Xlib.h`: `int XUndefineCursor(Display *display, Window w)`
        fn XUndefineCursor(display: *mut Display, w: Window) -> c_int;

        /// Frees a cursor. The server keeps the shape for as long as a
        /// window still uses it.
        /// `X11/Xlib.h`: `int XFreeCursor(Display *display, Cursor cursor)`
        fn XFreeCursor(display: *mut Display, cursor: Cursor) -> c_int;

        /// Turns detectable auto-repeat on or off for this client. When it
        /// is on, a key that is held down produces repeated [`KeyPress`]
        /// events with no [`KeyRelease`] between them, so a release event
        /// always means the key went up.
        ///
        /// If `supported` is not null, it receives whether the server can
        /// do this. Returns whether detectable auto-repeat is on after the
        /// call.
        /// `X11/XKBlib.h`: `Bool XkbSetDetectableAutoRepeat(Display *dpy, Bool
        /// detectable, Bool *supported)`
        fn XkbSetDetectableAutoRepeat(
            display: *mut Display,
            detectable: Bool,
            supported: *mut Bool,
        ) -> Bool;
    }
}

#[cfg(test)]
mod tests {
    use core::ffi::{CStr, c_void};

    use super::*;
    use crate::dynlib::Library;
    use crate::linux::libc::{RTLD_LOCAL, RTLD_NOW, dlopen, dlsym};

    /// A library opened with `dlopen`.
    struct Opened(*mut c_void);

    impl Library for Opened {
        fn symbol(&self, name: &CStr) -> *const c_void {
            // SAFETY: the handle came from `dlopen` and is never closed, and
            // the name is a C string.
            unsafe { dlsym(self.0, name.as_ptr()) }
        }
    }

    #[test]
    fn the_installed_xlib_has_every_function_we_declare() {
        // SAFETY: the name is a C string, and Xlib is sound to load into any
        // process.
        let handle = unsafe { dlopen(c"libX11.so.6".as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        if handle.is_null() {
            // This machine has no Xlib, so there is nothing to compare with.
            return;
        }
        // SAFETY: the library is Xlib, which defines these functions as its
        // headers declare them, and it is never unloaded.
        let table = unsafe { Xlib::load(&Opened(handle)) };
        if let Err(missing) = table {
            panic!("libX11.so.6: {missing}");
        }
    }
}
