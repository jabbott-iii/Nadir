//! COM's memory allocator (`ole32.dll`).
//!
//! Source: `combaseapi.h` of the Windows SDK.

// The functions keep the names their headers give them, so that each one can
// be compared with its header.
#![allow(non_snake_case)]

use core::ffi::c_void;

#[link(name = "ole32")]
unsafe extern "system" {
    /// Frees memory that COM, or a function such as `SHGetKnownFolderPath`,
    /// allocated for the caller. A null pointer is allowed, and does nothing.
    /// `combaseapi.h`: `void CoTaskMemFree(LPVOID pv)`
    ///
    /// # Safety
    ///
    /// `pv` is null, or came from COM's allocator and is not freed yet.
    pub fn CoTaskMemFree(pv: *mut c_void);
}
