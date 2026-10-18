//! The window and its events, on AppKit.
//!
//! Three classes of ours, built at run time, connect AppKit to the backend:
//!
//! - a content view, which takes the keyboard and swallows key events so
//!   that AppKit does not beep at them, and whose coordinates are flipped,
//!   so that its origin is the top left;
//! - a window delegate, which hears about closing, resizing, the scale
//!   factor, the focus and fullscreen;
//! - an app delegate, which turns Quit (⌘Q, or the Dock) into a request to
//!   close.
//!
//! Each has a variable that points to the [`State`] it shares with the
//! backend. The backend reads key and mouse events itself, in the pump,
//! before it hands each to AppKit: AppKit does not pass the release of a key
//! to the view while ⌘ is held.
//!
//! While the user resizes the window, AppKit runs a loop of its own inside
//! `sendEvent:`, and the pump does not return. Each resize step then runs
//! the modal frame callback, so frames come only while the window changes
//! size. Moving a window does not stop the loop.
//!
//! What this backend does not do yet, because nothing has needed it:
//!
//! - It reports no typed text.
//! - It does not notice the window being minimized.
//! - It does not put back the two keys that ISO keyboards swap (see the key
//!   table).

use core::any::Any;
use core::cell::{Cell, RefCell};
use core::ptr;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};

use nadir_core::log::{debug, error};
use nadir_core::{UVec2, Vec2};
// The event types are used as `appkit::NSEventTypeKeyDown`: see `input`.
use nadir_sys::macos::appkit::{
    self, NSApplicationActivationPolicyRegular, NSBackingStoreBuffered, NSEventMaskAny,
    NSTerminateCancel, NSWindowCollectionBehaviorFullScreenPrimary, NSWindowStyleMaskClosable,
    NSWindowStyleMaskMiniaturizable, NSWindowStyleMaskResizable, NSWindowStyleMaskTitled,
};
use nadir_sys::macos::foundation::{
    CGFloat, NSDefaultRunLoopMode, NSInteger, NSPoint, NSRect, NSSize, NSUInteger,
};
use nadir_sys::macos::objc::{BOOL, Id, Imp, NO, Sel, YES};
use nadir_sys::msg_send;

use super::input::{self, Incoming, Translator};
use super::runtime::{
    self, AutoreleasePool, Method, Owned, RETURNS_BOOL, as_id, class, ns_string, sel,
};
use crate::{Event, ModalFrame, PlatformError, WindowConfig};

/// The modal frame callback, as the platform takes it.
type ModalFrameCallback = Box<dyn FnMut(ModalFrame)>;

/// Whether the application has finished launching, which it does once.
static LAUNCHED: AtomicBool = AtomicBool::new(false);

/// Declares the selectors we send, looked up once.
macro_rules! selectors {
    ($($field:ident = $name:literal,)*) => {
        /// The selectors the backend sends.
        #[derive(Clone, Copy)]
        struct Selectors {
            $($field: Sel,)*
        }

        impl Selectors {
            fn new() -> Self {
                Self {
                    $($field: sel($name),)*
                }
            }
        }
    };
}

selectors! {
    alloc = c"alloc",
    init = c"init",
    shared_application = c"sharedApplication",
    set_activation_policy = c"setActivationPolicy:",
    activate_ignoring_other_apps = c"activateIgnoringOtherApps:",
    finish_launching = c"finishLaunching",
    set_main_menu = c"setMainMenu:",
    set_delegate = c"setDelegate:",
    delegate = c"delegate",
    next_event = c"nextEventMatchingMask:untilDate:inMode:dequeue:",
    send_event = c"sendEvent:",
    distant_past = c"distantPast",
    add_item = c"addItem:",
    set_submenu = c"setSubmenu:",
    init_menu_item = c"initWithTitle:action:keyEquivalent:",
    terminate = c"terminate:",
    init_window = c"initWithContentRect:styleMask:backing:defer:",
    init_with_frame = c"initWithFrame:",
    set_released_when_closed = c"setReleasedWhenClosed:",
    set_title = c"setTitle:",
    set_content_view = c"setContentView:",
    make_first_responder = c"makeFirstResponder:",
    set_accepts_mouse_moved_events = c"setAcceptsMouseMovedEvents:",
    set_content_min_size = c"setContentMinSize:",
    set_collection_behavior = c"setCollectionBehavior:",
    set_background_color = c"setBackgroundColor:",
    black_color = c"blackColor",
    center = c"center",
    make_key_and_order_front = c"makeKeyAndOrderFront:",
    toggle_full_screen = c"toggleFullScreen:",
    backing_scale_factor = c"backingScaleFactor",
    close = c"close",
    bounds = c"bounds",
    convert_point = c"convertPoint:fromView:",
    in_live_resize = c"inLiveResize",
    event_type = c"type",
    key_code = c"keyCode",
    modifier_flags = c"modifierFlags",
    location_in_window = c"locationInWindow",
    button_number = c"buttonNumber",
    scrolling_delta_x = c"scrollingDeltaX",
    scrolling_delta_y = c"scrollingDeltaY",
    has_precise_scrolling_deltas = c"hasPreciseScrollingDeltas",
    is_direction_inverted_from_device = c"isDirectionInvertedFromDevice",
    window = c"window",
    hide = c"hide",
    unhide = c"unhide",
}

/// What the backend and the objects of our classes share.
struct State {
    selectors: Selectors,
    /// The window and its content view. The backend owns both.
    window: Id,
    view: Id,
    translator: RefCell<Translator>,
    /// The events since the last pump.
    events: RefCell<Vec<Event>>,
    fullscreen: Cell<bool>,
    modal_frame: Cell<Option<ModalFrameCallback>>,
    /// A panic caught in a method of ours, which must not unwind into
    /// AppKit. The pump resumes it.
    panic: Cell<Option<Box<dyn Any + Send>>>,
}

impl State {
    /// The window's scale factor.
    fn backing_scale(&self) -> f32 {
        // SAFETY: `-[NSWindow backingScaleFactor]` takes nothing and returns
        // a `CGFloat`.
        let scale =
            unsafe { msg_send![self.window, self.selectors.backing_scale_factor, () -> CGFloat] };
        scale as f32
    }

    /// The content view's bounds, in points.
    fn bounds(&self) -> NSRect {
        // SAFETY: `-[NSView bounds]` takes nothing and returns an `NSRect`.
        unsafe { msg_send![self.view, self.selectors.bounds, () -> NSRect] }
    }

    /// The size of the content view, in physical pixels.
    fn pixel_size(&self) -> UVec2 {
        let size = self.bounds().size;
        let scale = CGFloat::from(self.backing_scale());
        // The casts saturate, and a NaN gives 0.
        UVec2::new(
            (size.width * scale).round() as u32,
            (size.height * scale).round() as u32,
        )
    }

    /// Adds the events of `f` to those of the next pump.
    fn record(&self, f: impl FnOnce(&mut Translator, &mut Vec<Event>)) {
        f(
            &mut self.translator.borrow_mut(),
            &mut self.events.borrow_mut(),
        );
    }

    fn resized(&self) {
        let size = self.pixel_size();
        self.record(|translator, events| translator.resize(size, events));
        // SAFETY: `-[NSView inLiveResize]` takes nothing and returns a
        // `BOOL`.
        let live = unsafe { msg_send![self.view, self.selectors.in_live_resize, () -> BOOL] };
        if live != NO {
            self.run_modal_frame();
        }
    }

    fn rescaled(&self) {
        let scale = self.backing_scale();
        let size = self.pixel_size();
        self.record(|translator, events| {
            translator.set_scale(scale, events);
            translator.resize(size, events);
        });
    }

    /// Runs the modal frame callback, if there is one.
    fn run_modal_frame(&self) {
        let frame = {
            let translator = self.translator.borrow();
            ModalFrame {
                size: translator.size(),
                scale_factor: translator.scale(),
            }
        };
        // The callback is taken out while it runs. If it panics, it is
        // dropped, and no more modal frames run.
        let Some(mut callback) = self.modal_frame.take() else {
            return;
        };
        callback(frame);
        let replaced = self.modal_frame.take();
        self.modal_frame.set(replaced.or(Some(callback)));
    }
}

/// Runs `f` with the state of `this`, an object of one of our classes, if
/// it has one, and keeps any panic for the pump.
///
/// # Safety
///
/// `this` is an object of a class from [`runtime::build_class`] whose state
/// variable is null or points at a live [`State`].
unsafe fn with_state<R>(this: Id, otherwise: R, f: impl FnOnce(&State) -> R) -> R {
    // SAFETY: the caller promises what `runtime::state` needs.
    let Some(state) = (unsafe { runtime::state::<State>(this) }) else {
        return otherwise;
    };
    match panic::catch_unwind(AssertUnwindSafe(|| f(state))) {
        Ok(result) => result,
        Err(payload) => {
            let first = state.panic.take().unwrap_or(payload);
            state.panic.set(Some(first));
            otherwise
        }
    }
}

// -- the content view -------------------------------------------------------

/// `-[NSResponder acceptsFirstResponder]`: the view takes the keyboard.
unsafe extern "C" fn accepts_first_responder(_this: Id, _cmd: Sel) -> BOOL {
    YES
}

/// `-[NSView isFlipped]`: the view's origin is its top left.
unsafe extern "C" fn is_flipped(_this: Id, _cmd: Sel) -> BOOL {
    YES
}

/// `keyDown:`, `keyUp:` and `flagsChanged:`. The pump has read the event
/// already; this keeps AppKit from beeping at a key nobody handled.
unsafe extern "C" fn ignore_event(_this: Id, _cmd: Sel, _event: Id) {}

// -- the window delegate ----------------------------------------------------

/// `-windowShouldClose:`: asks the game, and leaves the window open.
unsafe extern "C" fn window_should_close(this: Id, _cmd: Sel, _sender: Id) -> BOOL {
    // SAFETY: AppKit calls this for our window delegate, whose state the
    // backend keeps alive while it is set.
    unsafe {
        with_state(this, NO, |state| {
            state.record(Translator::close);
            NO
        })
    }
}

/// `-windowDidResize:`.
unsafe extern "C" fn window_did_resize(this: Id, _cmd: Sel, _notification: Id) {
    // SAFETY: as in `window_should_close`.
    unsafe { with_state(this, (), State::resized) };
}

/// `-windowDidChangeBackingProperties:`: the scale factor may have changed.
unsafe extern "C" fn window_did_change_backing_properties(this: Id, _cmd: Sel, _notification: Id) {
    // SAFETY: as in `window_should_close`.
    unsafe { with_state(this, (), State::rescaled) };
}

/// `-windowDidBecomeKey:`: the window has the keyboard focus.
unsafe extern "C" fn window_did_become_key(this: Id, _cmd: Sel, _notification: Id) {
    // SAFETY: as in `window_should_close`.
    unsafe {
        with_state(this, (), |state| {
            state.record(|translator, events| translator.focus(true, events));
        });
    };
}

/// `-windowDidResignKey:`: the window lost the keyboard focus.
unsafe extern "C" fn window_did_resign_key(this: Id, _cmd: Sel, _notification: Id) {
    // SAFETY: as in `window_should_close`.
    unsafe {
        with_state(this, (), |state| {
            state.record(|translator, events| translator.focus(false, events));
        });
    };
}

/// `-windowDidEnterFullScreen:`.
unsafe extern "C" fn window_did_enter_full_screen(this: Id, _cmd: Sel, _notification: Id) {
    // SAFETY: as in `window_should_close`.
    unsafe { with_state(this, (), |state| state.fullscreen.set(true)) };
}

/// `-windowDidExitFullScreen:`.
unsafe extern "C" fn window_did_exit_full_screen(this: Id, _cmd: Sel, _notification: Id) {
    // SAFETY: as in `window_should_close`.
    unsafe { with_state(this, (), |state| state.fullscreen.set(false)) };
}

// -- the app delegate -------------------------------------------------------

/// `-applicationShouldTerminate:`: asks the game, and keeps the app running.
unsafe extern "C" fn application_should_terminate(this: Id, _cmd: Sel, _sender: Id) -> NSUInteger {
    // SAFETY: as in `window_should_close`, for our app delegate.
    unsafe {
        with_state(this, NSTerminateCancel, |state| {
            state.record(Translator::close);
            NSTerminateCancel
        })
    }
}

/// Casts a method's function to the untyped `IMP` that the runtime stores.
macro_rules! imp {
    ($function:expr, $type:ty) => {{
        let function: $type = $function;
        // SAFETY: all function pointers have the same size and
        // representation, and the runtime calls the method through the type
        // its encoding gives, which is this one.
        unsafe { core::mem::transmute::<$type, Imp>(function) }
    }};
}

/// Our three classes.
struct Classes {
    view: Id,
    window_delegate: Id,
    app_delegate: Id,
}

impl Classes {
    /// Returns our classes, and makes them the first time.
    fn get() -> Result<Self, PlatformError> {
        type Getter = unsafe extern "C" fn(Id, Sel) -> BOOL;
        type Handler = unsafe extern "C" fn(Id, Sel, Id);
        let event = |selector: &'static core::ffi::CStr| Method {
            selector,
            imp: imp!(ignore_event, Handler),
            result: "v",
            arguments: "@",
        };
        let notification = |selector, function: Handler| Method {
            selector,
            imp: imp!(function, Handler),
            result: "v",
            arguments: "@",
        };
        let view_methods = [
            Method {
                selector: c"acceptsFirstResponder",
                imp: imp!(accepts_first_responder, Getter),
                result: RETURNS_BOOL,
                arguments: "",
            },
            Method {
                selector: c"isFlipped",
                imp: imp!(is_flipped, Getter),
                result: RETURNS_BOOL,
                arguments: "",
            },
            event(c"keyDown:"),
            event(c"keyUp:"),
            event(c"flagsChanged:"),
        ];
        let window_delegate_methods = [
            Method {
                selector: c"windowShouldClose:",
                imp: imp!(
                    window_should_close,
                    unsafe extern "C" fn(Id, Sel, Id) -> BOOL
                ),
                result: RETURNS_BOOL,
                arguments: "@",
            },
            notification(c"windowDidResize:", window_did_resize),
            notification(
                c"windowDidChangeBackingProperties:",
                window_did_change_backing_properties,
            ),
            notification(c"windowDidBecomeKey:", window_did_become_key),
            notification(c"windowDidResignKey:", window_did_resign_key),
            notification(c"windowDidEnterFullScreen:", window_did_enter_full_screen),
            notification(c"windowDidExitFullScreen:", window_did_exit_full_screen),
        ];
        let app_delegate_methods = [Method {
            selector: c"applicationShouldTerminate:",
            imp: imp!(
                application_should_terminate,
                unsafe extern "C" fn(Id, Sel, Id) -> NSUInteger
            ),
            // `NSUInteger` is an `unsigned long`, 64 bits.
            result: "Q",
            arguments: "@",
        }];
        // In each class, each function has the signature its encoding
        // describes, and is sound for any instance: an instance without a
        // state does nothing.
        // SAFETY: as above.
        let view = unsafe { runtime::build_class(c"NadirView", c"NSView", &view_methods)? };
        // SAFETY: as above.
        let window_delegate = unsafe {
            runtime::build_class(
                c"NadirWindowDelegate",
                c"NSObject",
                &window_delegate_methods,
            )?
        };
        // SAFETY: as above.
        let app_delegate = unsafe {
            runtime::build_class(c"NadirAppDelegate", c"NSObject", &app_delegate_methods)?
        };
        Ok(Self {
            view: as_id(view),
            window_delegate: as_id(window_delegate),
            app_delegate: as_id(app_delegate),
        })
    }
}

/// The AppKit window of the game.
pub(crate) struct Backend {
    selectors: Selectors,
    /// The shared application. It is not ours to release.
    app: Id,
    window: Owned,
    view: Owned,
    window_delegate: Owned,
    app_delegate: Owned,
    cursor_visible: bool,
    /// Dropped after the objects stop pointing at it: see `Drop`.
    state: Box<State>,
}

impl Backend {
    pub(crate) fn new(config: &WindowConfig) -> Result<Self, PlatformError> {
        let _pool = AutoreleasePool::new();
        if !runtime::is_main_thread() {
            return Err(PlatformError::NotMainThread);
        }
        let sel = Selectors::new();
        let classes = Classes::get()?;

        // SAFETY: `+[NSApplication sharedApplication]` takes nothing and
        // returns the application, making it the first time.
        let app =
            unsafe { msg_send![as_id(class(c"NSApplication")?), sel.shared_application, () -> Id] };
        // SAFETY: `-setActivationPolicy:(NSApplicationActivationPolicy)`
        // returns a `BOOL`. A program started from a terminal is no app
        // until this.
        unsafe {
            msg_send![
                app,
                sel.set_activation_policy,
                (NSApplicationActivationPolicyRegular => NSInteger) -> BOOL
            ]
        };
        if !LAUNCHED.swap(true, Ordering::Relaxed) {
            set_main_menu(app, sel, &config.title)?;
            // SAFETY: `-finishLaunching` takes nothing and returns nothing.
            unsafe { msg_send![app, sel.finish_launching, () -> ()] };
        }

        let content = NSRect {
            origin: NSPoint::default(),
            size: NSSize {
                width: CGFloat::from(config.size.x.max(1)),
                height: CGFloat::from(config.size.y.max(1)),
            },
        };
        let mut style =
            NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable;
        if config.resizable {
            style |= NSWindowStyleMaskResizable;
        }
        // SAFETY: `+alloc` takes nothing and returns a new, uninitialized
        // object.
        let blank = unsafe { msg_send![as_id(class(c"NSWindow")?), sel.alloc, () -> Id] };
        // SAFETY: `-[NSWindow initWithContentRect:(NSRect)
        // styleMask:(NSWindowStyleMask)style backing:(NSBackingStoreType)
        // defer:(BOOL)]` returns the window, holding a reference for us.
        let window = unsafe {
            msg_send![
                blank,
                sel.init_window,
                (content => NSRect, style => NSUInteger, NSBackingStoreBuffered => NSUInteger, NO => BOOL) -> Id
            ]
        };
        // SAFETY: the window is new, and its reference is ours.
        let window = unsafe { Owned::new(window) };
        if window.id().is_null() {
            return Err(PlatformError::CallFailed {
                call: "-[NSWindow initWithContentRect:styleMask:backing:defer:]",
            });
        }
        let view = new_object(classes.view, sel, Some(content))?;
        let window_delegate = new_object(classes.window_delegate, sel, None)?;
        let app_delegate = new_object(classes.app_delegate, sel, None)?;

        let mut backend = Self {
            selectors: sel,
            app,
            state: Box::new(State {
                selectors: sel,
                window: window.id(),
                view: view.id(),
                translator: RefCell::new(Translator::new(config.size, 1.0)),
                events: RefCell::new(Vec::new()),
                fullscreen: Cell::new(false),
                modal_frame: Cell::new(None),
                panic: Cell::new(None),
            }),
            window,
            view,
            window_delegate,
            app_delegate,
            cursor_visible: true,
        };
        // From here on, dropping `backend` detaches the objects from the
        // state, and closes the window.
        let state: *const State = &raw const *backend.state;
        for object in [
            backend.view.id(),
            backend.window_delegate.id(),
            backend.app_delegate.id(),
        ] {
            // SAFETY: the objects are of our classes, and the boxed state
            // outlives the time they point at it: `Drop` detaches them first.
            unsafe { runtime::set_state(object, state) };
        }
        let window = backend.window.id();
        // The window does not keep its delegate, or the app its own: the
        // backend does, as long as they are set.
        // SAFETY: `-setDelegate:(id)` returns nothing; ours answers the
        // delegate methods it has, and the others are optional.
        unsafe { msg_send![window, sel.set_delegate, (backend.window_delegate.id() => Id) -> ()] };
        // SAFETY: as above, for the app.
        unsafe { msg_send![app, sel.set_delegate, (backend.app_delegate.id() => Id) -> ()] };
        backend.configure_window(config)?;

        // The window is on its screen now, at its scale. What the delegate
        // heard while it was set up is old news.
        let scale = backend.state.backing_scale();
        let size = backend.state.pixel_size();
        *backend.state.translator.borrow_mut() = Translator::new(size, scale);
        backend.state.events.borrow_mut().clear();
        // Showing the window makes it key, before any pump; the translator
        // is ready for the news.
        // SAFETY: `-makeKeyAndOrderFront:(id)sender` returns nothing.
        unsafe { msg_send![window, sel.make_key_and_order_front, (ptr::null_mut() => Id) -> ()] };
        // SAFETY: `-activateIgnoringOtherApps:(BOOL)` returns nothing. It
        // brings the game in front of the terminal that started it.
        unsafe { msg_send![app, sel.activate_ignoring_other_apps, (YES => BOOL) -> ()] };
        if config.fullscreen {
            backend.set_fullscreen(true);
        }
        debug!(
            "opened an AppKit window of {}x{} pixels at scale {scale}",
            size.x, size.y
        );
        Ok(backend)
    }

    /// Sets what the window looks like, and how it behaves, before it is
    /// shown.
    fn configure_window(&self, config: &WindowConfig) -> Result<(), PlatformError> {
        let (window, sel) = (self.window.id(), self.selectors);
        let title = ns_string(&config.title)?;
        // SAFETY: `-setReleasedWhenClosed:(BOOL)` returns nothing. The
        // backend releases the window itself.
        unsafe { msg_send![window, sel.set_released_when_closed, (NO => BOOL) -> ()] };
        // SAFETY: `-setTitle:(NSString *)` copies the string and returns
        // nothing.
        unsafe { msg_send![window, sel.set_title, (title.id() => Id) -> ()] };
        // SAFETY: `-setContentView:(NSView *)` keeps the view and returns
        // nothing.
        unsafe { msg_send![window, sel.set_content_view, (self.view.id() => Id) -> ()] };
        // SAFETY: `-makeFirstResponder:(NSResponder *)` returns a `BOOL`.
        unsafe { msg_send![window, sel.make_first_responder, (self.view.id() => Id) -> BOOL] };
        // SAFETY: `-setAcceptsMouseMovedEvents:(BOOL)` returns nothing.
        unsafe { msg_send![window, sel.set_accepts_mouse_moved_events, (YES => BOOL) -> ()] };
        let min = NSSize {
            width: CGFloat::from(config.min_size.x),
            height: CGFloat::from(config.min_size.y),
        };
        // SAFETY: `-setContentMinSize:(NSSize)` returns nothing. A zero is
        // no limit in that direction.
        unsafe { msg_send![window, sel.set_content_min_size, (min => NSSize) -> ()] };
        // A window that may not be resized may still go fullscreen.
        // SAFETY: `-setCollectionBehavior:(NSWindowCollectionBehavior)`
        // returns nothing.
        unsafe {
            msg_send![
                window,
                sel.set_collection_behavior,
                (NSWindowCollectionBehaviorFullScreenPrimary => NSUInteger) -> ()
            ]
        };
        // SAFETY: `+[NSColor blackColor]` takes nothing and returns a color
        // that we do not own.
        let black = unsafe { msg_send![as_id(class(c"NSColor")?), sel.black_color, () -> Id] };
        // SAFETY: `-setBackgroundColor:(NSColor *)` keeps the color and
        // returns nothing.
        unsafe { msg_send![window, sel.set_background_color, (black => Id) -> ()] };
        // SAFETY: `-center` takes nothing and returns nothing.
        unsafe { msg_send![window, sel.center, () -> ()] };
        Ok(())
    }

    pub(crate) fn pump_events(&mut self, out: &mut Vec<Event>) {
        let _pool = AutoreleasePool::new();
        let sel = self.selectors;
        if let Ok(date) = class(c"NSDate") {
            self.dispatch(date, sel);
        }
        self.state.translator.borrow_mut().begin_pump(out);
        out.append(&mut self.state.events.borrow_mut());
    }

    /// Takes every waiting event off the queue, reads it, and hands it to
    /// AppKit.
    fn dispatch(&mut self, date: nadir_sys::macos::objc::Class, sel: Selectors) {
        // SAFETY: `+[NSDate distantPast]` takes nothing and returns a date
        // that the pool owns.
        let past = unsafe { msg_send![as_id(date), sel.distant_past, () -> Id] };
        // SAFETY: Foundation defines the constant, and nothing changes it.
        let mode = unsafe { NSDefaultRunLoopMode };
        loop {
            // SAFETY: `-[NSApplication nextEventMatchingMask:(NSEventMask)
            // untilDate:(NSDate *) inMode:(NSRunLoopMode)
            // dequeue:(BOOL)]` returns an event that the pool owns, or nil.
            // A date in the past makes it wait for nothing.
            let event = unsafe {
                msg_send![
                    self.app,
                    sel.next_event,
                    (NSEventMaskAny => u64, past => Id, mode => Id, YES => BOOL) -> Id
                ]
            };
            if event.is_null() {
                break;
            }
            if let Some(incoming) = self.decode(event) {
                self.state
                    .record(|translator, events| translator.handle(incoming, events));
            }
            // SAFETY: `-sendEvent:(NSEvent *)` returns nothing. It is where
            // AppKit runs its own loop while the window is resized.
            unsafe { msg_send![self.app, sel.send_event, (event => Id) -> ()] };
            self.resume_panic();
        }
    }

    /// Reads a key or mouse event for our window. `None` for any other.
    fn decode(&self, event: Id) -> Option<Incoming> {
        let sel = self.selectors;
        // SAFETY: `-[NSEvent window]` takes nothing and returns the window
        // the event is for, or nil.
        let window = unsafe { msg_send![event, sel.window, () -> Id] };
        if window != self.window.id() {
            return None;
        }
        // SAFETY: `-[NSEvent type]` takes nothing and returns an
        // `NSEventType`, which is an `NSUInteger`.
        let event_type = unsafe { msg_send![event, sel.event_type, () -> NSUInteger] };
        match event_type {
            appkit::NSEventTypeKeyDown | appkit::NSEventTypeKeyUp => {
                // SAFETY: a key event's `-keyCode` returns an `unsigned
                // short`.
                let code = unsafe { msg_send![event, sel.key_code, () -> u16] };
                Some(Incoming::Key {
                    pressed: event_type == appkit::NSEventTypeKeyDown,
                    code,
                })
            }
            appkit::NSEventTypeFlagsChanged => {
                // SAFETY: as above, for a flags-changed event.
                let code = unsafe { msg_send![event, sel.key_code, () -> u16] };
                // SAFETY: `-modifierFlags` takes nothing and returns an
                // `NSEventModifierFlags`, which is an `NSUInteger`.
                let flags = unsafe { msg_send![event, sel.modifier_flags, () -> NSUInteger] };
                Some(Incoming::FlagsChanged { code, flags })
            }
            appkit::NSEventTypeMouseMoved
            | appkit::NSEventTypeLeftMouseDragged
            | appkit::NSEventTypeRightMouseDragged
            | appkit::NSEventTypeOtherMouseDragged => {
                let (position, inside) = self.position(event);
                // Outside the view, only a drag counts: the button that went
                // down inside is still down.
                (inside || event_type != appkit::NSEventTypeMouseMoved)
                    .then_some(Incoming::Motion { position })
            }
            appkit::NSEventTypeScrollWheel => {
                // SAFETY: a scroll event's `-scrollingDeltaX` and
                // `-scrollingDeltaY` return `CGFloat`s, and
                // `-hasPreciseScrollingDeltas` and
                // `-isDirectionInvertedFromDevice` `BOOL`s.
                let dx = unsafe { msg_send![event, sel.scrolling_delta_x, () -> CGFloat] };
                // SAFETY: as above.
                let dy = unsafe { msg_send![event, sel.scrolling_delta_y, () -> CGFloat] };
                // SAFETY: as above.
                let precise =
                    unsafe { msg_send![event, sel.has_precise_scrolling_deltas, () -> BOOL] };
                // SAFETY: as above.
                let inverted =
                    unsafe { msg_send![event, sel.is_direction_inverted_from_device, () -> BOOL] };
                Some(Incoming::Wheel(input::wheel_lines(
                    dx,
                    dy,
                    precise != NO,
                    inverted != NO,
                )))
            }
            _ if input::is_button_event(event_type) => {
                // SAFETY: a button event's `-buttonNumber` returns an
                // `NSInteger`.
                let number = unsafe { msg_send![event, sel.button_number, () -> NSInteger] };
                let (pressed, button) = input::mouse_button(event_type, number)?;
                let (position, inside) = self.position(event);
                // A click on the title bar is the window's, not the game's.
                if pressed && !inside {
                    return None;
                }
                Some(Incoming::Button {
                    pressed,
                    button,
                    position,
                })
            }
            _ => None,
        }
    }

    /// Where a mouse event happened, in physical pixels from the top left of
    /// the view, and whether that is inside it.
    fn position(&self, event: Id) -> (Vec2, bool) {
        let sel = self.selectors;
        // SAFETY: a mouse event's `-locationInWindow` returns an `NSPoint`.
        let in_window = unsafe { msg_send![event, sel.location_in_window, () -> NSPoint] };
        // SAFETY: `-[NSView convertPoint:(NSPoint) fromView:(NSView *)]`
        // returns an `NSPoint`; a nil view means the window's coordinates.
        let point = unsafe {
            msg_send![
                self.view.id(),
                sel.convert_point,
                (in_window => NSPoint, ptr::null_mut() => Id) -> NSPoint
            ]
        };
        let bounds = self.state.bounds();
        let inside = (0.0..bounds.size.width).contains(&point.x)
            && (0.0..bounds.size.height).contains(&point.y);
        (input::to_pixels(point, self.state.backing_scale()), inside)
    }

    pub(crate) fn window_size(&self) -> UVec2 {
        self.state.translator.borrow().size()
    }

    pub(crate) fn scale_factor(&self) -> f32 {
        self.state.translator.borrow().scale()
    }

    pub(crate) fn set_fullscreen(&mut self, on: bool) {
        if on == self.state.fullscreen.get() {
            return;
        }
        // Fullscreen takes an animation; the delegate hears when it is done,
        // and this keeps a second request from undoing the first meanwhile.
        self.state.fullscreen.set(on);
        // SAFETY: `-toggleFullScreen:(id)sender` returns nothing.
        unsafe {
            msg_send![
                self.window.id(),
                self.selectors.toggle_full_screen,
                (ptr::null_mut() => Id) -> ()
            ]
        };
    }

    pub(crate) fn set_title(&mut self, title: &str) {
        let title = match ns_string(title) {
            Ok(title) => title,
            Err(error) => {
                error!("could not set the window title: {error}");
                return;
            }
        };
        // SAFETY: `-setTitle:(NSString *)` copies the string and returns
        // nothing.
        unsafe { msg_send![self.window.id(), self.selectors.set_title, (title.id() => Id) -> ()] };
    }

    pub(crate) fn set_cursor_visible(&mut self, visible: bool) {
        if visible == self.cursor_visible {
            return;
        }
        let Ok(cursor) = class(c"NSCursor") else {
            return;
        };
        let selector = if visible {
            self.selectors.unhide
        } else {
            self.selectors.hide
        };
        // `+[NSCursor hide]` and `+unhide` count, like `ShowCursor` on
        // Windows; this is the only code that calls them, once each way.
        // SAFETY: both take nothing and return nothing.
        unsafe { msg_send![as_id(cursor), selector, () -> ()] };
        self.cursor_visible = visible;
    }

    pub(crate) fn set_modal_frame_callback(&mut self, callback: ModalFrameCallback) {
        self.state.modal_frame.set(Some(callback));
    }

    /// Unwinds with a panic that a method of ours caught, if it caught one.
    fn resume_panic(&self) {
        if let Some(payload) = self.state.panic.take() {
            panic::resume_unwind(payload);
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        let _pool = AutoreleasePool::new();
        let sel = self.selectors;
        if !self.cursor_visible {
            self.set_cursor_visible(true);
        }
        // Detach everything from the state first: AppKit may keep the
        // objects past their release, and call them.
        for object in [
            self.view.id(),
            self.window_delegate.id(),
            self.app_delegate.id(),
        ] {
            // SAFETY: the objects are of our classes, and null points at no
            // state.
            unsafe { runtime::set_state::<State>(object, ptr::null()) };
        }
        let window = self.window.id();
        // SAFETY: `-setDelegate:(id)` returns nothing; nil removes ours.
        unsafe { msg_send![window, sel.set_delegate, (ptr::null_mut() => Id) -> ()] };
        // SAFETY: `-[NSApplication delegate]` takes nothing and returns the
        // delegate.
        let app_delegate = unsafe { msg_send![self.app, sel.delegate, () -> Id] };
        if app_delegate == self.app_delegate.id() {
            // SAFETY: as for the window.
            unsafe { msg_send![self.app, sel.set_delegate, (ptr::null_mut() => Id) -> ()] };
        }
        // SAFETY: `-close` takes nothing and returns nothing. The window
        // was told not to release itself.
        unsafe { msg_send![window, sel.close, () -> ()] };
        // The fields release the objects after this, and the state goes
        // last.
    }
}

/// Makes an object of one of our classes: `alloc`, then `initWithFrame:`
/// for a view, or `init`.
fn new_object(class: Id, sel: Selectors, frame: Option<NSRect>) -> Result<Owned, PlatformError> {
    // SAFETY: `+alloc` takes nothing and returns a new, uninitialized
    // object.
    let blank = unsafe { msg_send![class, sel.alloc, () -> Id] };
    let object = match frame {
        // SAFETY: `-[NSView initWithFrame:(NSRect)]` returns the view,
        // holding a reference for us.
        Some(frame) => unsafe { msg_send![blank, sel.init_with_frame, (frame => NSRect) -> Id] },
        // SAFETY: `-[NSObject init]` takes nothing and returns the object,
        // holding a reference for us.
        None => unsafe { msg_send![blank, sel.init, () -> Id] },
    };
    // SAFETY: the object is new, and its reference is ours.
    let object = unsafe { Owned::new(object) };
    if object.id().is_null() {
        return Err(PlatformError::CallFailed { call: "-init" });
    }
    Ok(object)
}

/// Gives the app a menu bar with one menu, the app's, holding Quit (⌘Q).
/// Quit asks the app delegate, which turns it into a request to close.
fn set_main_menu(app: Id, sel: Selectors, title: &str) -> Result<(), PlatformError> {
    let menu_class = as_id(class(c"NSMenu")?);
    let item_class = as_id(class(c"NSMenuItem")?);
    let menu_bar = new_object(menu_class, sel, None)?;
    let app_item = new_object(item_class, sel, None)?;
    let app_menu = new_object(menu_class, sel, None)?;
    let quit_title = ns_string(&format!("Quit {title}"))?;
    let key = ns_string("q")?;
    // SAFETY: `+alloc` takes nothing and returns a new, uninitialized
    // object.
    let blank = unsafe { msg_send![item_class, sel.alloc, () -> Id] };
    // SAFETY: `-[NSMenuItem initWithTitle:(NSString *) action:(SEL)
    // keyEquivalent:(NSString *)]` returns the item, holding a reference for
    // us. It copies the strings.
    let quit = unsafe {
        msg_send![
            blank,
            sel.init_menu_item,
            (quit_title.id() => Id, sel.terminate => Sel, key.id() => Id) -> Id
        ]
    };
    // SAFETY: the item is new, and its reference is ours.
    let quit = unsafe { Owned::new(quit) };
    // Each menu keeps its items, an item its submenu, and the app its menu
    // bar, so ours can all be released after this.
    // SAFETY: `-[NSMenu addItem:(NSMenuItem *)]` returns nothing.
    unsafe { msg_send![app_menu.id(), sel.add_item, (quit.id() => Id) -> ()] };
    // SAFETY: `-[NSMenuItem setSubmenu:(NSMenu *)]` returns nothing.
    unsafe { msg_send![app_item.id(), sel.set_submenu, (app_menu.id() => Id) -> ()] };
    // SAFETY: as for `addItem:` above.
    unsafe { msg_send![menu_bar.id(), sel.add_item, (app_item.id() => Id) -> ()] };
    // SAFETY: `-[NSApplication setMainMenu:(NSMenu *)]` returns nothing.
    unsafe { msg_send![app, sel.set_main_menu, (menu_bar.id() => Id) -> ()] };
    Ok(())
}
