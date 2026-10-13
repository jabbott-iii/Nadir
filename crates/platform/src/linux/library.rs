//! Shared libraries that are loaded while the program runs.

use core::ffi::{CStr, c_void};
use core::ptr::NonNull;

use nadir_sys::dynlib;
use nadir_sys::linux::libc::{RTLD_LOCAL, RTLD_NOW, dlerror, dlopen, dlsym};

/// A shared library, loaded with `dlopen`.
///
/// It stays loaded until the process exits: dropping this does not unload
/// it. Function pointers taken from it therefore never dangle.
pub(super) struct Library {
    handle: NonNull<c_void>,
}

impl Library {
    /// Loads the library called `name`. The loader looks for it in its usual
    /// places.
    ///
    /// # Errors
    ///
    /// If the library cannot be loaded, returns what the loader said.
    ///
    /// # Safety
    ///
    /// Loading a library runs its initializers. The library must be one that
    /// is sound to load into this process.
    pub(super) unsafe fn open(name: &CStr) -> Result<Self, String> {
        // SAFETY: `name` is a C string, and the caller vouches for the
        // library.
        let handle = unsafe { dlopen(name.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        match NonNull::new(handle) {
            Some(handle) => Ok(Self { handle }),
            None => Err(last_error()),
        }
    }
}

/// What the loader says about its last failure on this thread.
fn last_error() -> String {
    let message = dlerror();
    if message.is_null() {
        return "the loader gave no reason".to_string();
    }
    // SAFETY: `dlerror` returned a NUL-terminated string. It stays valid
    // until this thread calls a `dl*` function again, and it is copied
    // before that.
    let message = unsafe { CStr::from_ptr(message) };
    message.to_string_lossy().into_owned()
}

impl dynlib::Library for Library {
    fn symbol(&self, name: &CStr) -> *const c_void {
        // SAFETY: the handle came from `dlopen` and is never closed, and
        // `name` is a C string.
        unsafe { dlsym(self.handle.as_ptr(), name.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use nadir_sys::dynlib::Library as _;

    use super::*;

    #[test]
    fn a_loaded_library_finds_its_symbols_and_no_others() {
        // SAFETY: the C library is already part of this process.
        let libc = unsafe { Library::open(c"libc.so.6") }.unwrap();
        assert!(!libc.symbol(c"malloc").is_null());
        assert!(libc.symbol(c"nadir_platform_has_no_such_symbol").is_null());
    }

    #[test]
    fn a_missing_library_gives_the_loaders_reason() {
        // SAFETY: no such library exists, so nothing is loaded.
        let result = unsafe { Library::open(c"libnadir-platform-has-no-such-library.so.0") };
        let reason = result.err().unwrap();
        assert!(
            reason.contains("libnadir-platform-has-no-such-library.so.0"),
            "{reason}"
        );
    }
}
