//! The basic Win32 types: integers, handles, and the small structs that many
//! functions share.
//!
//! Sources: `minwindef.h`, `windef.h`, `winnt.h`, `basetsd.h` and
//! `guiddef.h` of the Windows SDK. Each item names its header and quotes the
//! declaration there.
//!
//! # Layouts
//!
//! The sizes, alignments and offsets asserted here are those of 64-bit
//! Windows, worked out from the declarations by the C layout rules: `LONG`
//! and `DWORD` are 4 bytes on every Windows target, and pointers and
//! handles 8. They were not read off the Rust types. 32-bit Windows is not a
//! target, and on it the assertions fail the build.
//!
//! Verify on Windows: this session had no Windows SDK headers, so the
//! numbers come from the declarations as Microsoft documents them.

// The types and fields keep the names their headers give them
// (`UINT_PTR`, `cbSize`), so that each one can be compared with its header.
#![allow(non_camel_case_types, non_snake_case)]

use core::ffi::c_void;
use core::mem::offset_of;

// ---------------------------------------------------------------------------
// Integers
// ---------------------------------------------------------------------------

/// A truth value: zero is false, anything else true.
/// `minwindef.h`: `typedef int BOOL;`
pub type BOOL = i32;

/// `minwindef.h`: `#define TRUE 1`
pub const TRUE: BOOL = 1;

/// `minwindef.h`: `#define FALSE 0`
pub const FALSE: BOOL = 0;

/// An unsigned integer of the size of a pointer.
/// `basetsd.h`: `typedef unsigned __int64 UINT_PTR;` (64-bit)
pub type UINT_PTR = usize;

/// A signed integer of the size of a pointer.
/// `basetsd.h`: `typedef __int64 LONG_PTR;` (64-bit)
pub type LONG_PTR = isize;

/// The first parameter of a window message.
/// `minwindef.h`: `typedef UINT_PTR WPARAM;`
pub type WPARAM = UINT_PTR;

/// The second parameter of a window message.
/// `minwindef.h`: `typedef LONG_PTR LPARAM;`
pub type LPARAM = LONG_PTR;

/// What a window procedure returns.
/// `minwindef.h`: `typedef LONG_PTR LRESULT;`
pub type LRESULT = LONG_PTR;

/// The number of a registered window class.
/// `minwindef.h`: `typedef WORD ATOM;`
pub type ATOM = u16;

/// The result of a COM or shell call: negative for a failure.
/// `winnt.h`: `typedef _Return_type_success_(return >= 0) long HRESULT;`
/// (`long` is 4 bytes on Windows.)
pub type HRESULT = i32;

/// `winerror.h`: `#define S_OK ((HRESULT)0L)`
pub const S_OK: HRESULT = 0;

/// The error code of a call that may not do what it was asked, as
/// `GetLastError` returns it (and `std::io::Error::raw_os_error`, which has
/// this type). `winerror.h`: `#define ERROR_ACCESS_DENIED 5L`
pub const ERROR_ACCESS_DENIED: i32 = 5;

/// A UTF-16 string that the callee may write, ending with a NUL.
/// `winnt.h`: `typedef _Null_terminated_ WCHAR *NWPSTR, *LPWSTR, *PWSTR;`
pub type PWSTR = *mut u16;

/// A UTF-16 string that the callee only reads, ending with a NUL.
/// `winnt.h`: `typedef _Null_terminated_ CONST WCHAR *LPCWSTR, *PCWSTR;`
pub type LPCWSTR = *const u16;

// ---------------------------------------------------------------------------
// Handles
// ---------------------------------------------------------------------------
//
// The headers declare each handle with `DECLARE_HANDLE`, as a pointer to a
// struct of its own, or as `void *`. Here every one is a `void` pointer, so
// they convert into each other as freely as in C, where `HMODULE` and
// `HINSTANCE` are one type.

/// A handle to an object of the kernel. `winnt.h`: `typedef void *HANDLE;`
pub type HANDLE = *mut c_void;

/// A window. `windef.h`: `DECLARE_HANDLE(HWND);`
pub type HWND = *mut c_void;

/// A loaded executable or DLL. `minwindef.h`: `DECLARE_HANDLE(HINSTANCE);`
pub type HINSTANCE = *mut c_void;

/// The same as [`HINSTANCE`]. `minwindef.h`: `typedef HINSTANCE HMODULE;`
pub type HMODULE = HINSTANCE;

/// A menu. `windef.h`: `DECLARE_HANDLE(HMENU);`
pub type HMENU = *mut c_void;

/// An icon. `windef.h`: `DECLARE_HANDLE(HICON);`
pub type HICON = *mut c_void;

/// A pointer shape. `windef.h`: `typedef HICON HCURSOR;`
pub type HCURSOR = HICON;

/// A brush, for painting. `windef.h`: `DECLARE_HANDLE(HBRUSH);`
pub type HBRUSH = *mut c_void;

/// Any GDI object: a brush, pen, font, bitmap... `windef.h`:
/// `DECLARE_HANDLE(HGDIOBJ);` (with `STRICT`; `typedef void *HGDIOBJ;`
/// without)
pub type HGDIOBJ = *mut c_void;

/// A display monitor. `windef.h`: `DECLARE_HANDLE(HMONITOR);`
pub type HMONITOR = *mut c_void;

/// How a thread or process scales for the screen's DPI.
/// `windef.h`: `DECLARE_HANDLE(DPI_AWARENESS_CONTEXT);`
pub type DPI_AWARENESS_CONTEXT = *mut c_void;

/// Scale for the DPI of each monitor, and let Windows scale the window
/// frame and dialogs too. Windows 10 1703 and later.
/// `windef.h`: `#define DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
/// ((DPI_AWARENESS_CONTEXT)-4)`
pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: DPI_AWARENESS_CONTEXT =
    core::ptr::without_provenance_mut(-4_isize as usize);

// ---------------------------------------------------------------------------
// Small structs
// ---------------------------------------------------------------------------

/// A point. `windef.h`: `typedef struct tagPOINT { LONG x; LONG y; } POINT;`
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct POINT {
    /// Pixels from the left.
    pub x: i32,
    /// Pixels from the top.
    pub y: i32,
}

const _: () = assert!(size_of::<POINT>() == 8);
const _: () = assert!(align_of::<POINT>() == 4);

/// A rectangle, by its edges. The right and bottom edges are outside it.
/// `windef.h`: `typedef struct tagRECT { LONG left; LONG top; LONG right;
/// LONG bottom; } RECT;`
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RECT {
    /// The left edge.
    pub left: i32,
    /// The top edge.
    pub top: i32,
    /// The right edge, one past the last column.
    pub right: i32,
    /// The bottom edge, one past the last row.
    pub bottom: i32,
}

const _: () = assert!(size_of::<RECT>() == 16);
const _: () = assert!(align_of::<RECT>() == 4);
const _: () = assert!(offset_of!(RECT, right) == 8);

/// A 128-bit identifier. `guiddef.h`: `typedef struct _GUID { unsigned long
/// Data1; unsigned short Data2; unsigned short Data3; unsigned char
/// Data4[ 8 ]; } GUID;`
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GUID {
    /// The first 8 hex digits.
    pub Data1: u32,
    /// The next 4.
    pub Data2: u16,
    /// The next 4.
    pub Data3: u16,
    /// The last 16, as bytes in the order they are written.
    pub Data4: [u8; 8],
}

const _: () = assert!(size_of::<GUID>() == 16);
const _: () = assert!(align_of::<GUID>() == 4);
const _: () = assert!(offset_of!(GUID, Data4) == 8);
