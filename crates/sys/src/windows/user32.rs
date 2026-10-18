//! The window manager's API: windows, their messages, the pointer, and
//! message boxes (`user32.dll`).
//!
//! Source: `WinUser.h` of the Windows SDK, and `windowsx.h` for how message
//! parameters are taken apart. Each item names its header and quotes the
//! declaration there.
//!
//! # Constants
//!
//! A constant keeps its header's name and value. One that the header gives a
//! type has that type. One that the header leaves as a bare number
//! (`#define WM_SIZE 0x0005`) has the type of the parameter or field it is
//! used with: a message is a `UINT`, a style a `DWORD`.
//!
//! # Calling these functions
//!
//! What every function here requires of its caller, besides what its own
//! documentation says:
//!
//! - A window is used only from the thread that created it. Messages for it
//!   arrive on that thread, which must pump them.
//! - A pointer argument is null only where the function allows it, and is
//!   otherwise valid for what the function reads or writes through it. A
//!   string argument ends with a NUL.
//! - A handle that is stale or of the wrong kind is not a memory error: the
//!   function fails, and says so through its result and `GetLastError`.
//!
//! Many of these functions send messages to the window procedure before they
//! return (`SetWindowPos` sends `WM_SIZE`, for example). The window procedure
//! must allow for being entered again while it is running.
//!
//! See also the layout notes in [`super::types`].

// The constants, fields and functions keep the names their headers give
// them (`cbSize`, `GetDpiForWindow`), so that each one can be compared with
// its header.
#![allow(non_snake_case)]

use core::ffi::c_void;
use core::mem::offset_of;

use super::types::{
    ATOM, BOOL, DPI_AWARENESS_CONTEXT, HBRUSH, HCURSOR, HICON, HINSTANCE, HMENU, HMONITOR, HWND,
    LONG_PTR, LPARAM, LPCWSTR, LRESULT, POINT, RECT, UINT_PTR, WPARAM,
};

// ---------------------------------------------------------------------------
// Window classes and windows
// ---------------------------------------------------------------------------

/// A window procedure: the function that handles a window's messages.
/// `WinUser.h`: `typedef LRESULT (CALLBACK* WNDPROC)(HWND, UINT, WPARAM,
/// LPARAM);`
pub type WNDPROC = Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>;

/// A function that a timer calls. We pass none: our timers post
/// [`WM_TIMER`] instead.
/// `WinUser.h`: `typedef VOID (CALLBACK* TIMERPROC)(HWND, UINT, UINT_PTR,
/// DWORD);`
pub type TIMERPROC = Option<unsafe extern "system" fn(HWND, u32, UINT_PTR, u32)>;

/// A window class, for [`RegisterClassExW`].
/// `WinUser.h`: `typedef struct tagWNDCLASSEXW { UINT cbSize; UINT style;
/// WNDPROC lpfnWndProc; int cbClsExtra; int cbWndExtra; HINSTANCE hInstance;
/// HICON hIcon; HCURSOR hCursor; HBRUSH hbrBackground; LPCWSTR lpszMenuName;
/// LPCWSTR lpszClassName; HICON hIconSm; } WNDCLASSEXW;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WNDCLASSEXW {
    /// The size of this struct, in bytes.
    pub cbSize: u32,
    /// The class styles, `CS_*`.
    pub style: u32,
    /// The window procedure of every window of the class.
    pub lpfnWndProc: WNDPROC,
    /// Extra bytes to keep with the class.
    pub cbClsExtra: i32,
    /// Extra bytes to keep with each window.
    pub cbWndExtra: i32,
    /// The module that holds the window procedure.
    pub hInstance: HINSTANCE,
    /// The large icon, or null for the default.
    pub hIcon: HICON,
    /// The pointer shape over the windows, or null to leave it to them.
    pub hCursor: HCURSOR,
    /// What fills a window before it is painted, or null for nothing.
    pub hbrBackground: HBRUSH,
    /// The default menu, or null for none.
    pub lpszMenuName: LPCWSTR,
    /// The name of the class.
    pub lpszClassName: LPCWSTR,
    /// The small icon, or null to make one from the large icon.
    pub hIconSm: HICON,
}

const _: () = assert!(size_of::<WNDCLASSEXW>() == 80);
const _: () = assert!(align_of::<WNDCLASSEXW>() == 8);
const _: () = assert!(offset_of!(WNDCLASSEXW, lpfnWndProc) == 8);
const _: () = assert!(offset_of!(WNDCLASSEXW, hInstance) == 24);
const _: () = assert!(offset_of!(WNDCLASSEXW, hCursor) == 40);
const _: () = assert!(offset_of!(WNDCLASSEXW, hbrBackground) == 48);
const _: () = assert!(offset_of!(WNDCLASSEXW, lpszClassName) == 64);

/// A class style: redraw the whole window when its width changes.
/// `WinUser.h`: `#define CS_HREDRAW 0x0002`
pub const CS_HREDRAW: u32 = 0x0002;

/// A class style: redraw the whole window when its height changes.
/// `WinUser.h`: `#define CS_VREDRAW 0x0001`
pub const CS_VREDRAW: u32 = 0x0001;

/// The standard arrow pointer, for [`LoadCursorW`].
/// `WinUser.h`: `#define IDC_ARROW MAKEINTRESOURCE(32512)`
pub const IDC_ARROW: LPCWSTR = core::ptr::without_provenance(32512);

/// Let Windows choose the position or the size, in [`CreateWindowExW`].
/// `WinUser.h`: `#define CW_USEDEFAULT ((int)0x80000000)`
pub const CW_USEDEFAULT: i32 = i32::MIN;

/// A window style: a window with no frame.
/// `WinUser.h`: `#define WS_POPUP 0x80000000L`
pub const WS_POPUP: u32 = 0x8000_0000;

/// A window style: the window is shown.
/// `WinUser.h`: `#define WS_VISIBLE 0x10000000L`
pub const WS_VISIBLE: u32 = 0x1000_0000;

/// A window style: a top-level window with a title bar and a border.
/// `WinUser.h`: `#define WS_OVERLAPPED 0x00000000L`
pub const WS_OVERLAPPED: u32 = 0x0000_0000;

/// A window style: a title bar, and the thin border that comes with it.
/// `WinUser.h`: `#define WS_CAPTION 0x00C00000L`
pub const WS_CAPTION: u32 = 0x00C0_0000;

/// A window style: the window menu in the title bar, with the close button.
/// `WinUser.h`: `#define WS_SYSMENU 0x00080000L`
pub const WS_SYSMENU: u32 = 0x0008_0000;

/// A window style: a border that the user can drag to resize the window.
/// `WinUser.h`: `#define WS_THICKFRAME 0x00040000L`
pub const WS_THICKFRAME: u32 = 0x0004_0000;

/// A window style: the minimize button.
/// `WinUser.h`: `#define WS_MINIMIZEBOX 0x00020000L`
pub const WS_MINIMIZEBOX: u32 = 0x0002_0000;

/// A window style: the maximize button.
/// `WinUser.h`: `#define WS_MAXIMIZEBOX 0x00010000L`
pub const WS_MAXIMIZEBOX: u32 = 0x0001_0000;

/// A window style: an ordinary top-level window.
/// `WinUser.h`: `#define WS_OVERLAPPEDWINDOW (WS_OVERLAPPED | WS_CAPTION |
/// WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX)`
pub const WS_OVERLAPPEDWINDOW: u32 =
    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;

/// As a [`WINDOWPLACEMENT`] state: hidden. `WinUser.h`: `#define SW_HIDE 0`
pub const SW_HIDE: u32 = 0;

/// As a [`WINDOWPLACEMENT`] state: shown at its normal size and place.
/// `WinUser.h`: `#define SW_SHOWNORMAL 1`
pub const SW_SHOWNORMAL: u32 = 1;

/// For [`ShowWindow`]: show the window and activate it.
/// `WinUser.h`: `#define SW_SHOW 5`
pub const SW_SHOW: i32 = 5;

/// An index for [`GetWindowLongPtrW`]: the window style.
/// `WinUser.h`: `#define GWL_STYLE (-16)`
pub const GWL_STYLE: i32 = -16;

/// An index for [`GetWindowLongPtrW`]: a value that belongs to the program.
/// `WinUser.h`: `#define GWLP_USERDATA (-21)`
pub const GWLP_USERDATA: i32 = -21;

/// For [`SetWindowPos`]: keep the size. `WinUser.h`: `#define SWP_NOSIZE
/// 0x0001`
pub const SWP_NOSIZE: u32 = 0x0001;

/// For [`SetWindowPos`]: keep the position. `WinUser.h`: `#define SWP_NOMOVE
/// 0x0002`
pub const SWP_NOMOVE: u32 = 0x0002;

/// For [`SetWindowPos`]: keep the place in the stack of windows.
/// `WinUser.h`: `#define SWP_NOZORDER 0x0004`
pub const SWP_NOZORDER: u32 = 0x0004;

/// For [`SetWindowPos`]: do not activate the window.
/// `WinUser.h`: `#define SWP_NOACTIVATE 0x0010`
pub const SWP_NOACTIVATE: u32 = 0x0010;

/// For [`SetWindowPos`]: apply a style that changed, and recompute the
/// frame. `WinUser.h`: `#define SWP_FRAMECHANGED 0x0020`
pub const SWP_FRAMECHANGED: u32 = 0x0020;

/// For [`SetWindowPos`]: leave the owner's place in the stack alone.
/// `WinUser.h`: `#define SWP_NOOWNERZORDER 0x0200`
pub const SWP_NOOWNERZORDER: u32 = 0x0200;

/// For [`SetWindowPos`]: put the window on top of the others.
/// `WinUser.h`: `#define HWND_TOP ((HWND)0)`
pub const HWND_TOP: HWND = core::ptr::null_mut();

/// Where a window is and how it is shown, for [`GetWindowPlacement`].
/// `WinUser.h`: `typedef struct tagWINDOWPLACEMENT { UINT length; UINT flags;
/// UINT showCmd; POINT ptMinPosition; POINT ptMaxPosition; RECT
/// rcNormalPosition; } WINDOWPLACEMENT;` (`rcDevice` exists only for
/// `_MAC`.)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WINDOWPLACEMENT {
    /// The size of this struct, in bytes.
    pub length: u32,
    /// How the minimized position is used.
    pub flags: u32,
    /// How the window is shown, `SW_*`.
    pub showCmd: u32,
    /// Where the window goes when minimized.
    pub ptMinPosition: POINT,
    /// Where the window goes when maximized.
    pub ptMaxPosition: POINT,
    /// The window's rectangle when it is neither minimized nor maximized.
    pub rcNormalPosition: RECT,
}

const _: () = assert!(size_of::<WINDOWPLACEMENT>() == 44);
const _: () = assert!(align_of::<WINDOWPLACEMENT>() == 4);
const _: () = assert!(offset_of!(WINDOWPLACEMENT, showCmd) == 8);
const _: () = assert!(offset_of!(WINDOWPLACEMENT, rcNormalPosition) == 28);

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// A message, as [`PeekMessageW`] takes it from the queue.
/// `WinUser.h`: `typedef struct tagMSG { HWND hwnd; UINT message; WPARAM
/// wParam; LPARAM lParam; DWORD time; POINT pt; } MSG;` (`lPrivate` exists
/// only for `_MAC`.)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MSG {
    /// The window it is for, or null for a message to the thread.
    pub hwnd: HWND,
    /// What kind of message it is, `WM_*`.
    pub message: u32,
    /// The first parameter.
    pub wParam: WPARAM,
    /// The second parameter.
    pub lParam: LPARAM,
    /// When it was posted, in milliseconds since the system started.
    pub time: u32,
    /// Where the pointer was, in screen coordinates.
    pub pt: POINT,
}

const _: () = assert!(size_of::<MSG>() == 48);
const _: () = assert!(align_of::<MSG>() == 8);
const _: () = assert!(offset_of!(MSG, message) == 8);
const _: () = assert!(offset_of!(MSG, wParam) == 16);
const _: () = assert!(offset_of!(MSG, lParam) == 24);
const _: () = assert!(offset_of!(MSG, time) == 32);
const _: () = assert!(offset_of!(MSG, pt) == 36);

/// For [`PeekMessageW`]: take the message off the queue.
/// `WinUser.h`: `#define PM_REMOVE 0x0001`
pub const PM_REMOVE: u32 = 0x0001;

/// The window's size changed. `wParam` says how, and `lParam` holds the
/// new width (low word) and height (high word) of the inside.
/// `WinUser.h`: `#define WM_SIZE 0x0005`
pub const WM_SIZE: u32 = 0x0005;

/// A `wParam` of [`WM_SIZE`]: the window was minimized.
/// `WinUser.h`: `#define SIZE_MINIMIZED 1`
pub const SIZE_MINIMIZED: WPARAM = 1;

/// The window got the keyboard focus. `WinUser.h`: `#define WM_SETFOCUS
/// 0x0007`
pub const WM_SETFOCUS: u32 = 0x0007;

/// The window is about to lose the keyboard focus.
/// `WinUser.h`: `#define WM_KILLFOCUS 0x0008`
pub const WM_KILLFOCUS: u32 = 0x0008;

/// The user asked to close the window. `WinUser.h`: `#define WM_CLOSE
/// 0x0010`
pub const WM_CLOSE: u32 = 0x0010;

/// Windows asks for the window's size limits; `lParam` points to a
/// [`MINMAXINFO`] to adjust. `WinUser.h`: `#define WM_GETMINMAXINFO 0x0024`
pub const WM_GETMINMAXINFO: u32 = 0x0024;

/// A key went down. `WinUser.h`: `#define WM_KEYDOWN 0x0100`
pub const WM_KEYDOWN: u32 = 0x0100;

/// A key went up. `WinUser.h`: `#define WM_KEYUP 0x0101`
pub const WM_KEYUP: u32 = 0x0101;

/// A key went down while Alt was held, or F10 or Alt itself went down.
/// `WinUser.h`: `#define WM_SYSKEYDOWN 0x0104`
pub const WM_SYSKEYDOWN: u32 = 0x0104;

/// The release that matches [`WM_SYSKEYDOWN`].
/// `WinUser.h`: `#define WM_SYSKEYUP 0x0105`
pub const WM_SYSKEYUP: u32 = 0x0105;

/// A command from the window menu, or a key that stands for one.
/// `WinUser.h`: `#define WM_SYSCOMMAND 0x0112`
pub const WM_SYSCOMMAND: u32 = 0x0112;

/// A [`WM_SYSCOMMAND`]: open the window menu from the keyboard (Alt or F10
/// alone). Only the high 12 bits of `wParam` say which command it is.
/// `WinUser.h`: `#define SC_KEYMENU 0xF100`
pub const SC_KEYMENU: WPARAM = 0xF100;

/// A timer of the window ran out; `wParam` is its id.
/// `WinUser.h`: `#define WM_TIMER 0x0113`
pub const WM_TIMER: u32 = 0x0113;

/// The pointer moved; `lParam` holds its position in the window.
/// `WinUser.h`: `#define WM_MOUSEMOVE 0x0200`
pub const WM_MOUSEMOVE: u32 = 0x0200;

/// `WinUser.h`: `#define WM_LBUTTONDOWN 0x0201`
pub const WM_LBUTTONDOWN: u32 = 0x0201;

/// `WinUser.h`: `#define WM_LBUTTONUP 0x0202`
pub const WM_LBUTTONUP: u32 = 0x0202;

/// `WinUser.h`: `#define WM_RBUTTONDOWN 0x0204`
pub const WM_RBUTTONDOWN: u32 = 0x0204;

/// `WinUser.h`: `#define WM_RBUTTONUP 0x0205`
pub const WM_RBUTTONUP: u32 = 0x0205;

/// `WinUser.h`: `#define WM_MBUTTONDOWN 0x0207`
pub const WM_MBUTTONDOWN: u32 = 0x0207;

/// `WinUser.h`: `#define WM_MBUTTONUP 0x0208`
pub const WM_MBUTTONUP: u32 = 0x0208;

/// The wheel turned; the high word of `wParam` says how far.
/// `WinUser.h`: `#define WM_MOUSEWHEEL 0x020A`
pub const WM_MOUSEWHEEL: u32 = 0x020A;

/// A side button went down; the high word of `wParam` says which.
/// `WinUser.h`: `#define WM_XBUTTONDOWN 0x020B`
pub const WM_XBUTTONDOWN: u32 = 0x020B;

/// A side button went up. `WinUser.h`: `#define WM_XBUTTONUP 0x020C`
pub const WM_XBUTTONUP: u32 = 0x020C;

/// The wheel tilted sideways. `WinUser.h`: `#define WM_MOUSEHWHEEL 0x020E`
pub const WM_MOUSEHWHEEL: u32 = 0x020E;

/// The window started a move or resize loop of its own.
/// `WinUser.h`: `#define WM_ENTERSIZEMOVE 0x0231`
pub const WM_ENTERSIZEMOVE: u32 = 0x0231;

/// The move or resize loop ended. `WinUser.h`: `#define WM_EXITSIZEMOVE
/// 0x0232`
pub const WM_EXITSIZEMOVE: u32 = 0x0232;

/// The window's DPI changed. The low word of `wParam` is the new DPI, and
/// `lParam` points to the [`RECT`] that Windows suggests for the window.
/// `WinUser.h`: `#define WM_DPICHANGED 0x02E0`
pub const WM_DPICHANGED: u32 = 0x02E0;

/// The first side button, in the high word of `wParam`.
/// `WinUser.h`: `#define XBUTTON1 0x0001`
pub const XBUTTON1: u16 = 0x0001;

/// The second side button. `WinUser.h`: `#define XBUTTON2 0x0002`
pub const XBUTTON2: u16 = 0x0002;

/// How far one step of the wheel turns it.
/// `WinUser.h`: `#define WHEEL_DELTA 120`
pub const WHEEL_DELTA: i16 = 120;

/// The DPI of a monitor at 100% scale.
/// `WinUser.h`: `#define USER_DEFAULT_SCREEN_DPI 96`
pub const USER_DEFAULT_SCREEN_DPI: u32 = 96;

/// The size limits of a window, for [`WM_GETMINMAXINFO`].
/// `WinUser.h`: `typedef struct tagMINMAXINFO { POINT ptReserved; POINT
/// ptMaxSize; POINT ptMaxPosition; POINT ptMinTrackSize; POINT
/// ptMaxTrackSize; } MINMAXINFO;`
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MINMAXINFO {
    /// Not used.
    pub ptReserved: POINT,
    /// The size of the window when maximized.
    pub ptMaxSize: POINT,
    /// The position of the window when maximized.
    pub ptMaxPosition: POINT,
    /// The smallest size the user can drag the window to, frame included.
    pub ptMinTrackSize: POINT,
    /// The largest size the user can drag the window to, frame included.
    pub ptMaxTrackSize: POINT,
}

const _: () = assert!(size_of::<MINMAXINFO>() == 40);
const _: () = assert!(align_of::<MINMAXINFO>() == 4);
const _: () = assert!(offset_of!(MINMAXINFO, ptMinTrackSize) == 24);

// ---------------------------------------------------------------------------
// Monitors
// ---------------------------------------------------------------------------

/// For [`MonitorFromWindow`]: the monitor nearest the window, if it is on
/// none. `WinUser.h`: `#define MONITOR_DEFAULTTONEAREST 0x00000002`
pub const MONITOR_DEFAULTTONEAREST: u32 = 0x0000_0002;

/// Where a monitor is, for [`GetMonitorInfoW`].
/// `WinUser.h`: `typedef struct tagMONITORINFO { DWORD cbSize; RECT
/// rcMonitor; RECT rcWork; DWORD dwFlags; } MONITORINFO;`
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MONITORINFO {
    /// The size of this struct, in bytes.
    pub cbSize: u32,
    /// The whole monitor, in virtual-screen coordinates.
    pub rcMonitor: RECT,
    /// The part that the taskbar and docked bars leave free.
    pub rcWork: RECT,
    /// Whether it is the primary monitor.
    pub dwFlags: u32,
}

const _: () = assert!(size_of::<MONITORINFO>() == 40);
const _: () = assert!(align_of::<MONITORINFO>() == 4);
const _: () = assert!(offset_of!(MONITORINFO, rcMonitor) == 4);
const _: () = assert!(offset_of!(MONITORINFO, rcWork) == 20);

// ---------------------------------------------------------------------------
// Message boxes
// ---------------------------------------------------------------------------

/// For [`MessageBoxW`]: one OK button. `WinUser.h`: `#define MB_OK
/// 0x00000000L`
pub const MB_OK: u32 = 0x0000_0000;

/// For [`MessageBoxW`]: the error icon. `WinUser.h`: `#define MB_ICONERROR
/// MB_ICONHAND`, `#define MB_ICONHAND 0x00000010L`
pub const MB_ICONERROR: u32 = 0x0000_0010;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

#[link(name = "user32")]
unsafe extern "system" {
    /// Registers a window class. Returns its atom, or 0 on failure.
    /// `WinUser.h`: `ATOM RegisterClassExW(const WNDCLASSEXW *)`
    ///
    /// # Safety
    ///
    /// `class` points to a filled-in struct whose strings end with a NUL.
    /// The window procedure stays callable for as long as the class exists.
    pub fn RegisterClassExW(class: *const WNDCLASSEXW) -> ATOM;

    /// Removes a window class that has no windows left. Returns `FALSE` on
    /// failure. `WinUser.h`: `BOOL UnregisterClassW(LPCWSTR lpClassName,
    /// HINSTANCE hInstance)`
    ///
    /// # Safety
    ///
    /// `lpClassName` ends with a NUL.
    pub fn UnregisterClassW(lpClassName: LPCWSTR, hInstance: HINSTANCE) -> BOOL;

    /// Creates a window. Returns null on failure. It sends messages to the
    /// window procedure before it returns.
    /// `WinUser.h`: `HWND CreateWindowExW(DWORD dwExStyle, LPCWSTR
    /// lpClassName, LPCWSTR lpWindowName, DWORD dwStyle, int X, int Y, int
    /// nWidth, int nHeight, HWND hWndParent, HMENU hMenu, HINSTANCE
    /// hInstance, LPVOID lpParam)`
    ///
    /// # Safety
    ///
    /// The strings end with a NUL, and the class is registered.
    pub fn CreateWindowExW(
        dwExStyle: u32,
        lpClassName: LPCWSTR,
        lpWindowName: LPCWSTR,
        dwStyle: u32,
        X: i32,
        Y: i32,
        nWidth: i32,
        nHeight: i32,
        hWndParent: HWND,
        hMenu: HMENU,
        hInstance: HINSTANCE,
        lpParam: *mut c_void,
    ) -> HWND;

    /// Destroys a window, after sending it its last messages. Returns
    /// `FALSE` on failure. `WinUser.h`: `BOOL DestroyWindow(HWND hWnd)`
    ///
    /// # Safety
    ///
    /// Called on the thread that created the window.
    pub fn DestroyWindow(hWnd: HWND) -> BOOL;

    /// Does what Windows does with a message by default.
    /// `WinUser.h`: `LRESULT DefWindowProcW(HWND hWnd, UINT Msg, WPARAM
    /// wParam, LPARAM lParam)`
    ///
    /// # Safety
    ///
    /// The parameters are those of a message the window procedure received,
    /// so that any pointer in them is valid.
    pub fn DefWindowProcW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;

    /// Shows, hides, minimizes or maximizes a window. Returns whether it was
    /// visible before. `WinUser.h`: `BOOL ShowWindow(HWND hWnd, int
    /// nCmdShow)`
    ///
    /// # Safety
    ///
    /// Called on the thread that created the window.
    pub fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> BOOL;

    /// Takes the next message off this thread's queue, if there is one,
    /// without waiting. Returns `FALSE` if there was none. It can call
    /// window procedures, for messages sent from other threads.
    /// `WinUser.h`: `BOOL PeekMessageW(LPMSG lpMsg, HWND hWnd, UINT
    /// wMsgFilterMin, UINT wMsgFilterMax, UINT wRemoveMsg)`
    ///
    /// # Safety
    ///
    /// `lpMsg` is valid for writes.
    pub fn PeekMessageW(
        lpMsg: *mut MSG,
        hWnd: HWND,
        wMsgFilterMin: u32,
        wMsgFilterMax: u32,
        wRemoveMsg: u32,
    ) -> BOOL;

    /// Posts the character messages that a key message stands for.
    /// `WinUser.h`: `BOOL TranslateMessage(const MSG *lpMsg)`
    ///
    /// # Safety
    ///
    /// `lpMsg` points to a message that [`PeekMessageW`] returned.
    pub fn TranslateMessage(lpMsg: *const MSG) -> BOOL;

    /// Calls the window procedure of a message's window with it.
    /// `WinUser.h`: `LRESULT DispatchMessageW(const MSG *lpMsg)`
    ///
    /// # Safety
    ///
    /// `lpMsg` points to a message that [`PeekMessageW`] returned.
    pub fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;

    /// Reads a value kept with a window, such as [`GWL_STYLE`] or
    /// [`GWLP_USERDATA`]. Returns 0 on failure, which can also be the value.
    /// `WinUser.h`: `LONG_PTR GetWindowLongPtrW(HWND hWnd, int nIndex)`
    ///
    /// # Safety
    ///
    /// `nIndex` is one of the indices that the header defines.
    pub fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> LONG_PTR;

    /// Changes a value kept with a window, and returns the old one.
    /// `WinUser.h`: `LONG_PTR SetWindowLongPtrW(HWND hWnd, int nIndex,
    /// LONG_PTR dwNewLong)`
    ///
    /// # Safety
    ///
    /// `nIndex` is one of the indices that the header defines, and the value
    /// is one that whatever reads it can use. A changed style takes effect
    /// after [`SetWindowPos`] with [`SWP_FRAMECHANGED`].
    pub fn SetWindowLongPtrW(hWnd: HWND, nIndex: i32, dwNewLong: LONG_PTR) -> LONG_PTR;

    /// Sets how the process scales for the screen's DPI. Fails if it was set
    /// already, by an earlier call or by the program's manifest. Windows 10
    /// 1703 and later. `WinUser.h`: `BOOL
    /// SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT value)`
    ///
    /// # Safety
    ///
    /// `value` is one of the `DPI_AWARENESS_CONTEXT_*` constants.
    pub fn SetProcessDpiAwarenessContext(value: DPI_AWARENESS_CONTEXT) -> BOOL;

    /// Returns the DPI of the monitor the window is on, as the window sees
    /// it, or 0 for a handle that is not a window. Windows 10 1607 and
    /// later. `WinUser.h`: `UINT GetDpiForWindow(HWND hwnd)`
    ///
    /// # Safety
    ///
    /// None beyond those of the module.
    pub fn GetDpiForWindow(hwnd: HWND) -> u32;

    /// Grows a rectangle for the inside of a window to the window's whole
    /// rectangle, frame included, at a DPI. Windows 10 1607 and later.
    /// `WinUser.h`: `BOOL AdjustWindowRectExForDpi(LPRECT lpRect, DWORD
    /// dwStyle, BOOL bMenu, DWORD dwExStyle, UINT dpi)`
    ///
    /// # Safety
    ///
    /// `lpRect` is valid for reads and writes.
    pub fn AdjustWindowRectExForDpi(
        lpRect: *mut RECT,
        dwStyle: u32,
        bMenu: BOOL,
        dwExStyle: u32,
        dpi: u32,
    ) -> BOOL;

    /// Gets the inside of a window: its top-left is 0, 0, so the right and
    /// bottom are its size. `WinUser.h`: `BOOL GetClientRect(HWND hWnd,
    /// LPRECT lpRect)`
    ///
    /// # Safety
    ///
    /// `lpRect` is valid for writes.
    pub fn GetClientRect(hWnd: HWND, lpRect: *mut RECT) -> BOOL;

    /// Moves, resizes or restacks a window. It sends messages to the window
    /// procedure, such as [`WM_SIZE`], before it returns.
    /// `WinUser.h`: `BOOL SetWindowPos(HWND hWnd, HWND hWndInsertAfter, int
    /// X, int Y, int cx, int cy, UINT uFlags)`
    ///
    /// # Safety
    ///
    /// None beyond those of the module.
    pub fn SetWindowPos(
        hWnd: HWND,
        hWndInsertAfter: HWND,
        X: i32,
        Y: i32,
        cx: i32,
        cy: i32,
        uFlags: u32,
    ) -> BOOL;

    /// Gets where a window is and how it is shown.
    /// `WinUser.h`: `BOOL GetWindowPlacement(HWND hWnd, WINDOWPLACEMENT
    /// *lpwndpl)`
    ///
    /// # Safety
    ///
    /// `lpwndpl` is valid for reads and writes, and its `length` is the
    /// size of the struct.
    pub fn GetWindowPlacement(hWnd: HWND, lpwndpl: *mut WINDOWPLACEMENT) -> BOOL;

    /// Puts a window back where a [`WINDOWPLACEMENT`] says.
    /// `WinUser.h`: `BOOL SetWindowPlacement(HWND hWnd, const
    /// WINDOWPLACEMENT *lpwndpl)`
    ///
    /// # Safety
    ///
    /// `lpwndpl` is valid for reads, and its `length` is the size of the
    /// struct.
    pub fn SetWindowPlacement(hWnd: HWND, lpwndpl: *const WINDOWPLACEMENT) -> BOOL;

    /// Returns the monitor a window is on.
    /// `WinUser.h`: `HMONITOR MonitorFromWindow(HWND hwnd, DWORD dwFlags)`
    ///
    /// # Safety
    ///
    /// None beyond those of the module.
    pub fn MonitorFromWindow(hwnd: HWND, dwFlags: u32) -> HMONITOR;

    /// Gets where a monitor is. `WinUser.h`: `BOOL GetMonitorInfoW(HMONITOR
    /// hMonitor, LPMONITORINFO lpmi)`
    ///
    /// # Safety
    ///
    /// `lpmi` is valid for reads and writes, and its `cbSize` is the size of
    /// the struct.
    pub fn GetMonitorInfoW(hMonitor: HMONITOR, lpmi: *mut MONITORINFO) -> BOOL;

    /// Starts, or restarts, a timer of the window that posts [`WM_TIMER`]
    /// with the id `nIDEvent` every `uElapse` milliseconds. Windows makes a
    /// shorter time 10 ms. Returns 0 on failure.
    /// `WinUser.h`: `UINT_PTR SetTimer(HWND hWnd, UINT_PTR nIDEvent, UINT
    /// uElapse, TIMERPROC lpTimerFunc)`
    ///
    /// # Safety
    ///
    /// A `lpTimerFunc` that is not `None` is sound to call with a timer's
    /// parameters.
    pub fn SetTimer(
        hWnd: HWND,
        nIDEvent: UINT_PTR,
        uElapse: u32,
        lpTimerFunc: TIMERPROC,
    ) -> UINT_PTR;

    /// Stops a timer. `WinUser.h`: `BOOL KillTimer(HWND hWnd, UINT_PTR
    /// uIDEvent)`
    ///
    /// # Safety
    ///
    /// None beyond those of the module.
    pub fn KillTimer(hWnd: HWND, uIDEvent: UINT_PTR) -> BOOL;

    /// Adds 1 to this thread's pointer display count, or takes 1 away, and
    /// returns the new count. The pointer shows over the thread's windows
    /// while the count is 0 or more. `WinUser.h`: `int ShowCursor(BOOL
    /// bShow)`
    pub safe fn ShowCursor(bShow: BOOL) -> i32;

    /// Loads a pointer shape. With a null `hInstance` and an `IDC_*` name,
    /// it is a standard shape that the system owns: nobody frees it.
    /// Returns null on failure. `WinUser.h`: `HCURSOR LoadCursorW(HINSTANCE
    /// hInstance, LPCWSTR lpCursorName)`
    ///
    /// # Safety
    ///
    /// `lpCursorName` is an `IDC_*` value, or a string that ends with a NUL.
    pub fn LoadCursorW(hInstance: HINSTANCE, lpCursorName: LPCWSTR) -> HCURSOR;

    /// Changes a window's title. It sends a message to the window procedure.
    /// `WinUser.h`: `BOOL SetWindowTextW(HWND hWnd, LPCWSTR lpString)`
    ///
    /// # Safety
    ///
    /// `lpString` ends with a NUL.
    pub fn SetWindowTextW(hWnd: HWND, lpString: LPCWSTR) -> BOOL;

    /// Sends the pointer's messages to this window, wherever the pointer
    /// is, until [`ReleaseCapture`]. Returns the window that had them.
    /// `WinUser.h`: `HWND SetCapture(HWND hWnd)`
    ///
    /// # Safety
    ///
    /// Called on the thread that created the window.
    pub fn SetCapture(hWnd: HWND) -> HWND;

    /// Ends what [`SetCapture`] started. `WinUser.h`: `BOOL
    /// ReleaseCapture(VOID)`
    pub safe fn ReleaseCapture() -> BOOL;

    /// Shows a message box and waits until the user closes it. Returns the
    /// button the user chose, or 0 on failure.
    /// `WinUser.h`: `int MessageBoxW(HWND hWnd, LPCWSTR lpText, LPCWSTR
    /// lpCaption, UINT uType)`
    ///
    /// # Safety
    ///
    /// The strings end with a NUL. `hWnd` is null or a window of this
    /// thread.
    pub fn MessageBoxW(hWnd: HWND, lpText: LPCWSTR, lpCaption: LPCWSTR, uType: u32) -> i32;
}
