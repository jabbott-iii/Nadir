//! The core of the system: modules (`kernel32.dll`).
//!
//! Source: `libloaderapi.h` of the Windows SDK. Errors are read with
//! `std::io::Error::last_os_error`, which calls `GetLastError`, so that is
//! not declared here.

// The functions and parameters keep the names their headers give them, so
// that each one can be compared with its header.
#![allow(non_snake_case)]

use super::types::{HMODULE, LPCWSTR};

#[link(name = "kernel32")]
unsafe extern "system" {
    /// Returns the handle of a module that this process has loaded, or of
    /// the program itself for a null name. Returns null on failure. The
    /// handle is not counted: nothing frees it.
    /// `libloaderapi.h`: `HMODULE GetModuleHandleW(LPCWSTR lpModuleName)`
    ///
    /// # Safety
    ///
    /// `lpModuleName` is null or ends with a NUL.
    pub fn GetModuleHandleW(lpModuleName: LPCWSTR) -> HMODULE;
}
