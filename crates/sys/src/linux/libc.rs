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

//! The C library on Linux (glibc): the dynamic loader, the locale, and the
//! error numbers that ALSA's functions return and the engine tells apart.
//!
//! Sources: `<dlfcn.h>`, and `<bits/dlfcn.h>` for the flag values;
//! `<locale.h>`, and `<bits/locale.h>` for the category's value; the
//! kernel's `<asm-generic/errno-base.h>` and `<asm-generic/errno.h>` for the
//! error numbers, which x86_64 and aarch64 both use.
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

/// A [`setlocale`] category: how bytes are read as characters, and which
/// characters are letters. Xlib's input methods follow it.
/// `locale.h`: `#define LC_CTYPE __LC_CTYPE`, and `bits/locale.h`: `#define
/// __LC_CTYPE 0`.
pub const LC_CTYPE: c_int = 0;

/// Try again: for an ALSA stream that does not block, no room to write in
/// yet (`snd_pcm_writei` returns it negated).
/// `asm-generic/errno-base.h`: `#define EAGAIN 11 /* Try again */`
pub const EAGAIN: c_int = 11;

/// Broken pipe: for an ALSA stream, an underrun, when the device ran out of
/// samples (`snd_pcm_writei` returns it negated).
/// `asm-generic/errno-base.h`: `#define EPIPE 32 /* Broken pipe */`
pub const EPIPE: c_int = 32;

/// Streams pipe error: for an ALSA stream, the system was suspended
/// (`snd_pcm_writei` returns it negated).
/// `asm-generic/errno.h`: `#define ESTRPIPE 86 /* Streams pipe error */`
pub const ESTRPIPE: c_int = 86;

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

// In libc itself, which every Rust program on Linux links.
unsafe extern "C" {
    /// Sets the locale of `category` for the whole process, and returns its
    /// name, or null if `locale` names one that is not installed. A null
    /// `locale` only returns the name; an empty one takes the locale from
    /// the environment (`LC_ALL`, then the category's own variable, then
    /// `LANG`).
    /// `locale.h`: `char *setlocale (int __category, const char *__locale)`.
    ///
    /// The string belongs to the C library: do not free it. It is valid
    /// until the next call of `setlocale`.
    ///
    /// # Safety
    ///
    /// - `locale` is null or points to a NUL-terminated string.
    /// - No other thread is calling `setlocale`, nor reading the locale in a
    ///   way that breaks if it changes meanwhile. glibc marks it
    ///   `MT-Unsafe const:locale env`.
    pub fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
}

#[cfg(test)]
mod tests {
    use core::ffi::CStr;

    use super::*;

    #[test]
    fn the_character_locale_has_a_name() {
        // SAFETY: a null locale only reads the name, which no test changes.
        let name = unsafe { setlocale(LC_CTYPE, core::ptr::null()) };
        assert!(!name.is_null());
        // SAFETY: the C library returned a NUL-terminated string.
        assert!(!unsafe { CStr::from_ptr(name) }.is_empty());
    }

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
