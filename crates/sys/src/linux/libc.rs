//! The C library on Linux (glibc): the dynamic loader.
//!
//! Source: `<dlfcn.h>`, and `<bits/dlfcn.h>` for the flag values.
//!
//! The engine keeps every library it loads until the process exits, so
//! `dlclose` is not declared.

use core::ffi::{c_char, c_int, c_void};

/// A [`dlopen`] flag: resolve every undefined symbol of the library before
/// `dlopen` returns, and fail if one cannot be resolved.
/// `bits/dlfcn.h`: `#define RTLD_NOW 0x00002`.
pub const RTLD_NOW: c_int = 0x00002;

/// A [`dlopen`] flag: do not make the library's symbols available to
/// libraries loaded after it. This is the default.
/// `bits/dlfcn.h`: `#define RTLD_LOCAL 0`.
pub const RTLD_LOCAL: c_int = 0;

// Before glibc 2.34 these functions are in libdl. From 2.34 on they are in
// libc itself, and libdl is an empty library kept so that this still links.
// Our floor is glibc 2.31, so libdl is named.
#[link(name = "dl")]
unsafe extern "C" {
    /// Loads the shared library `filename` and returns a handle to it. A null
    /// `filename` gives a handle to the program itself. Returns null on
    /// failure; [`dlerror`] then says why.
    /// `dlfcn.h`: `void *dlopen (const char *__file, int __mode)`.
    ///
    /// The handle is ours. We never close it.
    ///
    /// # Safety
    ///
    /// - `filename` is null or points to a NUL-terminated string.
    /// - Loading a library runs its initializers, which can do anything. The
    ///   library must be one that is sound to load into this process.
    pub fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;

    /// Returns the address of `symbol` in the library `handle`, or null if
    /// the library and its dependencies do not define it.
    /// `dlfcn.h`: `void *dlsym (void *__restrict __handle, const
    /// char *__restrict __name)`.
    ///
    /// The address stays valid while the library is loaded.
    ///
    /// # Safety
    ///
    /// - `handle` came from [`dlopen`] and was not closed.
    /// - `symbol` points to a NUL-terminated string.
    pub fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;

    /// Describes the last failure of [`dlopen`] or [`dlsym`] on this thread,
    /// and clears it. Returns null if there was none since the last call.
    /// `dlfcn.h`: `char *dlerror (void)`.
    ///
    /// The string belongs to the C library: do not free it. It is valid
    /// until the next call of a `dl*` function on this thread.
    pub safe fn dlerror() -> *mut c_char;
}

#[cfg(test)]
mod tests {
    use core::ffi::CStr;

    use super::*;

    #[test]
    fn the_program_itself_can_be_opened_and_searched() {
        // SAFETY: a null name asks for the program itself, which loads
        // nothing new.
        let program = unsafe { dlopen(core::ptr::null(), RTLD_NOW | RTLD_LOCAL) };
        assert!(!program.is_null());

        // SAFETY: the handle is the one just opened, and the name is a C
        // string.
        let found = unsafe { dlsym(program, c"malloc".as_ptr()) };
        assert!(!found.is_null());

        // SAFETY: as above.
        let absent = unsafe { dlsym(program, c"nadir_sys_has_no_such_symbol".as_ptr()) };
        assert!(absent.is_null());
    }

    #[test]
    fn a_library_that_does_not_exist_fails_with_a_message() {
        let name = c"libnadir-sys-has-no-such-library.so.0";
        // SAFETY: the name is a C string. No such library exists, so
        // nothing is loaded.
        let handle = unsafe { dlopen(name.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        assert!(handle.is_null());

        let message = dlerror();
        assert!(!message.is_null());
        // SAFETY: `dlerror` returned a NUL-terminated string that stays
        // valid until this thread calls a `dl*` function again, which it
        // does not do while `message` is in use.
        let message = unsafe { CStr::from_ptr(message) }.to_string_lossy();
        assert!(
            message.contains("libnadir-sys-has-no-such-library.so.0"),
            "{message}"
        );
    }
}
