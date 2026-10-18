//! The window and its messages, on Win32.
//!
//! Windows delivers a window's messages by calling its window procedure:
//! from [`DispatchMessageW`] in our pump, and from inside many other calls,
//! such as [`SetWindowPos`]. The procedure turns them into events and keeps
//! them in the window's [`WindowState`] until the next pump hands them out.
//! The state is shared with the procedure through a pointer that Windows
//! keeps with the window ([`GWLP_USERDATA`]), so it is only ever borrowed
//! shared, and its parts change through cells: the procedure can be entered
//! again while it runs.
//!
//! While the user drags or resizes the window, Windows runs a loop of its
//! own inside [`DispatchMessageW`], and our pump does not return. A timer
//! then runs the modal frame callback, so that the game goes on.
//!
//! What this backend does not do yet, because nothing has needed it:
//!
//! - It reports no typed text (`WM_CHAR`).
//! - It does not notice the window being minimized, except that a minimized
//!   window reports no size.
//! - It does not hold the pointer inside the window (`ClipCursor`).
//! - The Alt Gr key of some layouts comes with a Left Control press that
//!   Windows makes up, and that is reported as one.

use core::any::Any;
use core::cell::{Cell, RefCell};
use core::ptr;
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};

use nadir_core::UVec2;
use nadir_core::log::{debug, error, warn};
use nadir_sys::windows::gdi32::{BLACK_BRUSH, GetStockObject};
use nadir_sys::windows::kernel32::GetModuleHandleW;
use nadir_sys::windows::types::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, ERROR_ACCESS_DENIED, FALSE, HINSTANCE, HWND,
    LONG_PTR, LPARAM, LRESULT, POINT, RECT, TRUE, UINT_PTR, WPARAM,
};
use nadir_sys::windows::user32::{
    AdjustWindowRectExForDpi, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW,
    DefWindowProcW, DestroyWindow, DispatchMessageW, GWL_STYLE, GWLP_USERDATA, GetDpiForWindow,
    GetMonitorInfoW, GetWindowLongPtrW, GetWindowPlacement, HWND_TOP, IDC_ARROW, KillTimer,
    LoadCursorW, MINMAXINFO, MONITOR_DEFAULTTONEAREST, MONITORINFO, MSG, MonitorFromWindow,
    PM_REMOVE, PeekMessageW, RegisterClassExW, ReleaseCapture, SC_KEYMENU, SW_HIDE, SW_SHOW,
    SW_SHOWNORMAL, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_NOZORDER, SetCapture, SetProcessDpiAwarenessContext, SetTimer, SetWindowLongPtrW,
    SetWindowPlacement, SetWindowPos, SetWindowTextW, ShowCursor, ShowWindow, TranslateMessage,
    USER_DEFAULT_SCREEN_DPI, UnregisterClassW, WINDOWPLACEMENT, WM_DPICHANGED, WM_ENTERSIZEMOVE,
    WM_EXITSIZEMOVE, WM_GETMINMAXINFO, WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER,
    WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSEXW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED,
    WS_OVERLAPPEDWINDOW, WS_POPUP, WS_SYSMENU,
};

use super::input::{Incoming, Translator};
use super::wide;
use crate::no_gl::NO_GL;
use crate::{Event, GpuSurface, ModalFrame, PlatformError, WindowConfig};

/// The widest and tallest window we ask for, in pixels.
const MAX_DIMENSION: u32 = 32767;

/// The id of the timer that runs modal frames.
const MODAL_TIMER: UINT_PTR = 1;

/// How often the modal timer fires, in milliseconds. Windows makes it at
/// least 10, and the system clock rounds it up further, to about 16.
const MODAL_TIMER_INTERVAL: u32 = 1;

/// Numbers the window classes, so that each window has a class of its own.
static NEXT_CLASS: AtomicU32 = AtomicU32::new(1);

/// The modal frame callback, as the platform takes it.
type ModalFrameCallback = Box<dyn FnMut(ModalFrame)>;

/// What the window procedure and the backend share.
struct WindowState {
    translator: RefCell<Translator>,
    /// The events since the last pump.
    events: RefCell<Vec<Event>>,
    /// The window's DPI.
    dpi: Cell<u32>,
    /// The smallest size the user may give the window, in logical pixels.
    /// A zero means no limit in that direction.
    min_size: UVec2,
    fullscreen: Cell<bool>,
    modal_frame: Cell<Option<ModalFrameCallback>>,
    /// A panic caught in the window procedure, which must not unwind into
    /// Windows. The pump resumes it.
    panic: Cell<Option<Box<dyn Any + Send>>>,
}

/// The Win32 window of the game.
pub(crate) struct Backend {
    hwnd: HWND,
    instance: HINSTANCE,
    /// The name of the window's class, ending with a NUL.
    class_name: Vec<u16>,
    resizable: bool,
    /// Where the window was before it went fullscreen.
    windowed_placement: Option<WINDOWPLACEMENT>,
    cursor_visible: bool,
    /// Dropped after the window is destroyed: see `Drop`.
    state: Box<WindowState>,
}

impl Backend {
    pub(crate) fn new(config: &WindowConfig) -> Result<Self, PlatformError> {
        become_dpi_aware();

        // SAFETY: a null name asks for the program itself.
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err(PlatformError::CallFailed {
                call: "GetModuleHandleW",
            });
        }
        let class_name = wide(&format!(
            "Nadir window {}",
            NEXT_CLASS.fetch_add(1, Ordering::Relaxed)
        ));
        // SAFETY: a null module and `IDC_ARROW` name a standard shape, which
        // the system owns.
        let arrow = unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) };
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: ptr::null_mut(),
            hCursor: arrow,
            // Black until something draws, as on the other systems.
            hbrBackground: GetStockObject(BLACK_BRUSH),
            lpszMenuName: ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: ptr::null_mut(),
        };
        // SAFETY: the struct is filled in, its class name ends with a NUL,
        // and the window procedure is a function, callable for as long as
        // the program runs.
        if unsafe { RegisterClassExW(&raw const class) } == 0 {
            return Err(PlatformError::CallFailed {
                call: "RegisterClassExW",
            });
        }

        let style = windowed_style(config.resizable);
        let title = wide(&config.title);
        // SAFETY: the class is registered, and both strings end with a NUL.
        // Until the state is attached below, the window procedure leaves
        // every message to `DefWindowProcW`.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null_mut(),
            )
        };
        if hwnd.is_null() {
            // SAFETY: the name ends with a NUL, and the class has no
            // windows.
            unsafe { UnregisterClassW(class_name.as_ptr(), instance) };
            return Err(PlatformError::CallFailed {
                call: "CreateWindowExW",
            });
        }

        // From here on, dropping `backend` destroys the window.
        let mut backend = Self {
            hwnd,
            instance,
            class_name,
            resizable: config.resizable,
            windowed_placement: None,
            cursor_visible: true,
            state: Box::new(WindowState {
                translator: RefCell::new(Translator::new(config.size, 1.0)),
                events: RefCell::new(Vec::new()),
                dpi: Cell::new(USER_DEFAULT_SCREEN_DPI),
                min_size: config.min_size,
                fullscreen: Cell::new(false),
                modal_frame: Cell::new(None),
                panic: Cell::new(None),
            }),
        };
        let state: *const WindowState = &raw const *backend.state;
        // The window procedure turns the number back into the pointer.
        let address = state.expose_provenance().cast_signed();
        // SAFETY: `GWLP_USERDATA` is ours to use. The pointer is to the boxed
        // state, which stays where it is and outlives the window (see
        // `Drop`). The previous value was 0, so the result says nothing.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, address) };

        // SAFETY: the window is ours.
        let dpi = match unsafe { GetDpiForWindow(hwnd) } {
            0 => USER_DEFAULT_SCREEN_DPI,
            dpi => dpi,
        };
        let scale = scale_of(dpi);
        let size = physical_size(config.size, scale);
        backend.state.dpi.set(dpi);
        *backend.state.translator.borrow_mut() = Translator::new(size, scale);

        // The window was created at a size Windows chose. Give its inside
        // the size asked for.
        let Some(outer) = outer_size(size, style, dpi) else {
            return Err(PlatformError::CallFailed {
                call: "AdjustWindowRectExForDpi",
            });
        };
        // SAFETY: the window is ours, on this thread.
        let sized = unsafe {
            SetWindowPos(
                hwnd,
                ptr::null_mut(),
                0,
                0,
                outer.x,
                outer.y,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        if sized == FALSE {
            return Err(PlatformError::CallFailed {
                call: "SetWindowPos",
            });
        }

        if config.fullscreen {
            backend.set_fullscreen(true);
        }
        // SAFETY: the window is ours, on this thread. The result says
        // whether it was visible before, which it was not.
        unsafe { ShowWindow(hwnd, SW_SHOW) };
        debug!(
            "opened a Win32 window of {}x{} pixels at scale {scale}",
            size.x, size.y
        );
        Ok(backend)
    }

    pub(crate) fn pump_events(&mut self, out: &mut Vec<Event>) {
        let mut message = MSG {
            hwnd: ptr::null_mut(),
            message: 0,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: POINT::default(),
        };
        // SAFETY: `message` is valid for writes. A null window takes the
        // messages of every window of this thread, and of the thread.
        while unsafe { PeekMessageW(&raw mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } != FALSE {
            // The results say whether characters were posted, and what the
            // window procedure returned; neither matters here.
            // SAFETY: `PeekMessageW` just filled in `message`.
            unsafe { TranslateMessage(&raw const message) };
            // SAFETY: as above.
            unsafe { DispatchMessageW(&raw const message) };
            self.resume_panic();
        }
        // `PeekMessageW` itself calls the window procedure for messages that
        // other threads sent.
        self.resume_panic();
        self.state.translator.borrow_mut().begin_pump(out);
        out.append(&mut self.state.events.borrow_mut());
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
        if on {
            self.enter_fullscreen();
        } else {
            self.leave_fullscreen();
        }
    }

    pub(crate) fn set_title(&mut self, title: &str) {
        let title = wide(title);
        // SAFETY: the window is ours, and the title ends with a NUL.
        if unsafe { SetWindowTextW(self.hwnd, title.as_ptr()) } == FALSE {
            error!(
                "could not set the window title: {}",
                io::Error::last_os_error()
            );
        }
    }

    pub(crate) fn set_cursor_visible(&mut self, visible: bool) {
        if visible == self.cursor_visible {
            return;
        }
        // The pointer shows while the count is 0 or more. It starts at 0,
        // and this is the only code that changes it.
        ShowCursor(if visible { TRUE } else { FALSE });
        self.cursor_visible = visible;
    }

    pub(crate) fn set_modal_frame_callback(&mut self, callback: ModalFrameCallback) {
        self.state.modal_frame.set(Some(callback));
    }

    pub(crate) fn gpu_surface(&self) -> GpuSurface<'_> {
        // No OpenGL context yet.
        GpuSurface::Gl(&NO_GL)
    }

    /// Unwinds with a panic that the window procedure caught, if it caught
    /// one.
    fn resume_panic(&self) {
        if let Some(payload) = self.state.panic.take() {
            panic::resume_unwind(payload);
        }
    }

    fn enter_fullscreen(&mut self) {
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..WINDOWPLACEMENT::default()
        };
        // SAFETY: the window is ours, and `placement` is valid for reads and
        // writes, with its length set.
        if unsafe { GetWindowPlacement(self.hwnd, &raw mut placement) } == FALSE {
            error!(
                "could not go fullscreen: GetWindowPlacement failed: {}",
                io::Error::last_os_error()
            );
            return;
        }
        // SAFETY: the window is ours.
        let monitor = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..MONITORINFO::default()
        };
        // SAFETY: `info` is valid for reads and writes, with its size set.
        // A null monitor only makes the call fail.
        if unsafe { GetMonitorInfoW(monitor, &raw mut info) } == FALSE {
            error!("could not go fullscreen: GetMonitorInfoW failed");
            return;
        }
        self.windowed_placement = Some(placement);
        // Before the move, so that `WM_GETMINMAXINFO` sets no limits.
        self.state.fullscreen.set(true);
        self.set_style(WS_POPUP);
        let monitor = info.rcMonitor;
        // SAFETY: the window is ours, on this thread.
        let moved = unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOP,
                monitor.left,
                monitor.top,
                monitor.right - monitor.left,
                monitor.bottom - monitor.top,
                SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
            )
        };
        if moved == FALSE {
            error!(
                "could not cover the monitor: SetWindowPos failed: {}",
                io::Error::last_os_error()
            );
        }
    }

    fn leave_fullscreen(&mut self) {
        self.state.fullscreen.set(false);
        self.set_style(windowed_style(self.resizable));
        if let Some(mut placement) = self.windowed_placement.take() {
            // A window that went fullscreen before it was first shown was
            // hidden then, and must not be hidden again now.
            if placement.showCmd == SW_HIDE {
                placement.showCmd = SW_SHOWNORMAL;
            }
            // SAFETY: the window is ours, and `placement` came from
            // `GetWindowPlacement`, length and all.
            if unsafe { SetWindowPlacement(self.hwnd, &raw const placement) } == FALSE {
                error!(
                    "could not put the window back: SetWindowPlacement failed: {}",
                    io::Error::last_os_error()
                );
            }
        }
        // SAFETY: the window is ours, on this thread.
        let applied = unsafe {
            SetWindowPos(
                self.hwnd,
                ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
            )
        };
        if applied == FALSE {
            error!(
                "could not give the window its frame back: SetWindowPos failed: {}",
                io::Error::last_os_error()
            );
        }
    }

    /// Replaces the frame part of the window's style with `style`. It takes
    /// effect at the next `SetWindowPos` with `SWP_FRAMECHANGED`.
    fn set_style(&self, style: u32) {
        let old = window_style(self.hwnd);
        let new = (old & !(WS_OVERLAPPEDWINDOW | WS_POPUP)) | style;
        // A style is 32 bits, kept as a `LONG` and sign-extended.
        let value = new.cast_signed() as LONG_PTR;
        // SAFETY: the window is ours, `GWL_STYLE` is an index the header
        // defines, and the value is a style.
        let previous = unsafe { SetWindowLongPtrW(self.hwnd, GWL_STYLE, value) };
        // A top-level window always has some style bits, so 0 is a failure.
        if previous == 0 {
            error!(
                "could not change the window style: {}",
                io::Error::last_os_error()
            );
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        if !self.cursor_visible {
            ShowCursor(TRUE);
        }
        // The window gets its last messages here, while the state is still
        // alive: the field is dropped after this.
        // SAFETY: the window is ours, on this thread.
        if unsafe { DestroyWindow(self.hwnd) } == FALSE {
            error!(
                "could not destroy the window: {}",
                io::Error::last_os_error()
            );
            // The window procedure could still be called. Detached from the
            // state, it leaves every message to `DefWindowProcW`.
            // SAFETY: the window is ours, and 0 is what the window procedure
            // takes for "no state".
            unsafe { SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0) };
            return;
        }
        // SAFETY: the name ends with a NUL, and the class has no windows
        // left.
        if unsafe { UnregisterClassW(self.class_name.as_ptr(), self.instance) } == FALSE {
            warn!(
                "could not unregister the window class: {}",
                io::Error::last_os_error()
            );
        }
    }
}

impl WindowState {
    /// Handles a message, and returns its result. `None` leaves it to
    /// `DefWindowProcW`.
    ///
    /// # Safety
    ///
    /// The parameters are those of a message that Windows sent or posted to
    /// `hwnd`, which is this state's window, on its thread. Any pointer in
    /// `lparam` is therefore what the message says it is.
    unsafe fn handle(
        &self,
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        match message {
            WM_DPICHANGED => {
                // The low word is the DPI across, which is also the DPI down.
                let dpi = u32::from((wparam & 0xFFFF) as u16);
                self.dpi.set(dpi);
                self.translator
                    .borrow_mut()
                    .set_scale(scale_of(dpi), &mut self.events.borrow_mut());
                // A fullscreen window keeps covering its monitor, which the
                // suggested rectangle, made for a framed window, would not.
                if self.fullscreen.get() {
                    return Some(0);
                }
                // SAFETY: for this message, `lparam` points to the rectangle
                // Windows suggests for the window, valid while the message is
                // handled.
                let suggested = unsafe { *(lparam as *const RECT) };
                // The new size arrives as `WM_SIZE`, from inside this call.
                // No cell is borrowed across it.
                // SAFETY: the window is ours, on this thread.
                let moved = unsafe {
                    SetWindowPos(
                        hwnd,
                        ptr::null_mut(),
                        suggested.left,
                        suggested.top,
                        suggested.right - suggested.left,
                        suggested.bottom - suggested.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                };
                if moved == FALSE {
                    warn!("could not resize the window for its new DPI");
                }
                Some(0)
            }
            WM_GETMINMAXINFO => {
                if let Some(min) = self.min_track_size(hwnd) {
                    // SAFETY: for this message, `lparam` points to the
                    // limits to fill in, valid while the message is handled.
                    let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
                    if min.x > 0 {
                        info.ptMinTrackSize.x = min.x;
                    }
                    if min.y > 0 {
                        info.ptMinTrackSize.y = min.y;
                    }
                }
                Some(0)
            }
            // Alt or F10 alone would open the window menu, and the game would
            // stop until the menu closed. Alt+F4 and Alt+Space still work:
            // they are other commands.
            WM_SYSCOMMAND if (wparam & 0xFFF0) == SC_KEYMENU => Some(0),
            WM_ENTERSIZEMOVE => {
                // SAFETY: the window is ours, and there is no timer function:
                // the timer posts `WM_TIMER`.
                if unsafe { SetTimer(hwnd, MODAL_TIMER, MODAL_TIMER_INTERVAL, None) } == 0 {
                    warn!("the game stops while the window is moved: SetTimer failed");
                }
                None
            }
            WM_EXITSIZEMOVE => {
                // This fails only if the timer did not start.
                // SAFETY: the window is ours.
                unsafe { KillTimer(hwnd, MODAL_TIMER) };
                None
            }
            WM_TIMER if wparam == MODAL_TIMER => {
                self.run_modal_frame();
                Some(0)
            }
            _ => {
                let incoming = Incoming::decode(message, wparam, lparam);
                let any_button_down = {
                    let mut translator = self.translator.borrow_mut();
                    translator.handle(incoming, &mut self.events.borrow_mut());
                    translator.any_button_down()
                };
                if let Incoming::Button { .. } = incoming {
                    // While a button is down, the pointer's messages come to
                    // this window even from outside it, so that the release
                    // is seen. No cell is borrowed here: both calls send
                    // messages.
                    if any_button_down {
                        // SAFETY: the window is ours, on this thread.
                        unsafe { SetCapture(hwnd) };
                    } else {
                        ReleaseCapture();
                    }
                }
                result(message, incoming)
            }
        }
    }

    /// The smallest size, frame included, that the user may drag the window
    /// to, at its current DPI and style. A zero means no limit in that
    /// direction. `None` means no limits at all.
    fn min_track_size(&self, hwnd: HWND) -> Option<POINT> {
        if self.fullscreen.get() || self.min_size == UVec2::ZERO {
            return None;
        }
        let dpi = self.dpi.get();
        let min = physical_min_size(self.min_size, scale_of(dpi));
        let outer = outer_size(min.max(UVec2::ONE), window_style(hwnd), dpi)?;
        Some(POINT {
            x: if min.x == 0 { 0 } else { outer.x },
            y: if min.y == 0 { 0 } else { outer.y },
        })
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
        // dropped, and the timer runs nothing more.
        let Some(mut callback) = self.modal_frame.take() else {
            return;
        };
        callback(frame);
        let replaced = self.modal_frame.take();
        self.modal_frame.set(replaced.or(Some(callback)));
    }
}

/// What the window procedure returns for a message that the translator
/// took, or `None` to leave the message to `DefWindowProcW`.
fn result(message: u32, incoming: Incoming) -> Option<LRESULT> {
    match incoming {
        Incoming::Other => None,
        // Windows uses these for Alt+F4, Alt+Tab and the window menu.
        Incoming::Key { .. } if message == WM_SYSKEYDOWN || message == WM_SYSKEYUP => None,
        // "An application should return TRUE from this message if it
        // processes it."
        Incoming::Button { .. } if message == WM_XBUTTONDOWN || message == WM_XBUTTONUP => {
            Some(TRUE as LRESULT)
        }
        _ => Some(0),
    }
}

/// The window procedure of every window of ours.
///
/// # Safety
///
/// Only Windows calls it, for a window of a class that `Backend::new`
/// registered, on that window's thread.
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: `hwnd` is the window this message is for, and
    // `GWLP_USERDATA` is an index the header defines.
    let address = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    let state = ptr::with_exposed_provenance::<WindowState>(address.cast_unsigned());
    // SAFETY: the value is 0, before `Backend::new` attaches the state, or
    // a pointer to the boxed state, which outlives the window.
    let Some(state) = (unsafe { state.as_ref() }) else {
        // SAFETY: these are the parameters of the message Windows sent.
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };
    // A panic must not unwind into Windows, which would abort the process.
    // It is kept, and the pump resumes it.
    let handled = panic::catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the parameters are those of the message Windows sent to
        // the state's window.
        unsafe { state.handle(hwnd, message, wparam, lparam) }
    }));
    match handled {
        Ok(Some(result)) => result,
        // SAFETY: as above.
        Ok(None) => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        Err(payload) => {
            let first = state.panic.take().unwrap_or(payload);
            state.panic.set(Some(first));
            0
        }
    }
}

/// Makes the process aware of each monitor's DPI, so that Windows does not
/// scale its windows, and sizes and positions are physical pixels.
fn become_dpi_aware() {
    // SAFETY: the value is one of the `DPI_AWARENESS_CONTEXT` constants.
    let set = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    if set == FALSE {
        let error = io::Error::last_os_error();
        // The program's manifest, or an earlier window, set it already.
        if error.raw_os_error() != Some(ERROR_ACCESS_DENIED) {
            warn!("could not make the process DPI aware: {error}");
        }
    }
}

/// The window style of `hwnd`.
fn window_style(hwnd: HWND) -> u32 {
    // SAFETY: `GWL_STYLE` is an index the header defines. A handle that is
    // not a window gives 0.
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    // A style is 32 bits, kept as a `LONG`.
    style as u32
}

/// The style of a window with a frame.
fn windowed_style(resizable: bool) -> u32 {
    if resizable {
        WS_OVERLAPPEDWINDOW
    } else {
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX
    }
}

/// The scale factor at a DPI.
fn scale_of(dpi: u32) -> f32 {
    dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32
}

/// The size of a window, frame included, whose inside is `inner`.
fn outer_size(inner: UVec2, style: u32, dpi: u32) -> Option<POINT> {
    let int = |value: u32| i32::try_from(value).unwrap_or(i32::MAX);
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: int(inner.x),
        bottom: int(inner.y),
    };
    // SAFETY: `rect` is valid for reads and writes.
    let adjusted = unsafe { AdjustWindowRectExForDpi(&raw mut rect, style, FALSE, 0, dpi) };
    (adjusted != FALSE).then(|| POINT {
        x: rect.right - rect.left,
        y: rect.bottom - rect.top,
    })
}

/// Converts a logical window size to physical pixels, within what we ask a
/// window to be.
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

#[cfg(test)]
mod tests {
    use nadir_core::MouseButton;
    use nadir_sys::windows::user32::{
        WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WS_MAXIMIZEBOX, WS_THICKFRAME,
    };

    use super::*;

    #[test]
    fn the_scale_factor_is_the_dpi_over_96() {
        assert_eq!(scale_of(96), 1.0);
        assert_eq!(scale_of(120), 1.25);
        assert_eq!(scale_of(144), 1.5);
        assert_eq!(scale_of(192), 2.0);
    }

    #[test]
    fn a_logical_size_is_scaled_rounded_and_kept_in_range() {
        let size = UVec2::new(640, 360);
        assert_eq!(physical_size(size, 1.0), UVec2::new(640, 360));
        assert_eq!(physical_size(size, 1.25), UVec2::new(800, 450));
        assert_eq!(physical_size(UVec2::new(101, 3), 1.25), UVec2::new(126, 4));
        assert_eq!(physical_size(UVec2::ZERO, 1.0), UVec2::new(1, 1));
        assert_eq!(
            physical_size(UVec2::new(u32::MAX, 40_000), 1.0),
            UVec2::new(32767, 32767)
        );
        assert_eq!(physical_size(size, f32::NAN), UVec2::new(1, 1));
    }

    #[test]
    fn a_minimum_size_is_scaled_and_zero_stays_no_limit() {
        assert_eq!(
            physical_min_size(UVec2::new(320, 180), 1.5),
            UVec2::new(480, 270)
        );
        assert_eq!(
            physical_min_size(UVec2::new(0, 180), 2.0),
            UVec2::new(0, 360)
        );
    }

    #[test]
    fn only_a_resizable_window_has_a_sizing_border_and_a_maximize_button() {
        let resizable = windowed_style(true);
        assert_eq!(resizable, WS_OVERLAPPEDWINDOW);
        let fixed = windowed_style(false);
        assert_eq!(fixed & (WS_THICKFRAME | WS_MAXIMIZEBOX), 0);
        assert_ne!(fixed & WS_CAPTION, 0);
        assert_ne!(fixed & WS_SYSMENU, 0);
        // `WS_OVERLAPPEDWINDOW` is the combination `WinUser.h` gives.
        assert_eq!(WS_OVERLAPPEDWINDOW, 0x00CF_0000);
    }

    #[test]
    fn system_keys_are_left_to_windows_and_others_are_handled() {
        let key = Incoming::Key {
            pressed: true,
            scan_code: 0x003E,
        };
        assert_eq!(result(WM_SYSKEYDOWN, key), None);
        assert_eq!(result(WM_SYSKEYUP, key), None);
        assert_eq!(result(WM_KEYDOWN, key), Some(0));
        assert_eq!(result(WM_KEYUP, key), Some(0));
    }

    #[test]
    fn side_buttons_return_true_and_other_messages_zero_or_nothing() {
        let button = |button| Incoming::Button {
            pressed: true,
            button,
            x: 0,
            y: 0,
        };
        assert_eq!(
            result(WM_XBUTTONDOWN, button(MouseButton::Back)),
            Some(TRUE as LRESULT)
        );
        assert_eq!(
            result(WM_XBUTTONUP, button(MouseButton::Forward)),
            Some(TRUE as LRESULT)
        );
        assert_eq!(result(WM_LBUTTONDOWN, button(MouseButton::Left)), Some(0));
        assert_eq!(result(0x000F, Incoming::Other), None);
    }
}
