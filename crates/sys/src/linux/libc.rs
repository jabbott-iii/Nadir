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

//! The C library on Linux (glibc): the dynamic loader, the locale, `ioctl`,
//! `inotify`, and the error numbers that ALSA's functions and event devices
//! return and the engine tells apart.
//!
//! Sources: `<dlfcn.h>`, and `<bits/dlfcn.h>` for the flag values;
//! `<locale.h>`, and `<bits/locale.h>` for the category's value;
//! `<sys/ioctl.h>`; `<sys/inotify.h>`, and the kernel's `<linux/inotify.h>`
//! for its struct and masks; the kernel's `<asm-generic/fcntl.h>` for the
//! file flags, `<asm-generic/errno-base.h>` and `<asm-generic/errno.h>` for
//! the error numbers, and `<linux/time_types.h>` with glibc's
//! `<bits/types/struct_timeval.h>` for `struct timeval`. x86_64 and aarch64
//! both use the generic headers' values for everything declared here.
//!
//! The engine keeps every library it loads until the process exits, so
//! `dlclose` is not declared. Files are opened, read and closed through the
//! standard library, so `open`, `read` and `close` are not declared either.

use core::ffi::{c_char, c_int, c_long, c_ulong, c_void};
use core::mem::offset_of;

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

/// The direction of an `ioctl` request that reads from the driver into the
/// caller's buffer.
/// `asm-generic/ioctl.h`: `# define _IOC_READ 2U`
pub const _IOC_READ: c_ulong = 2;

/// Where a request's direction starts in its number.
/// `asm-generic/ioctl.h`: `#define _IOC_DIRSHIFT
/// (_IOC_SIZESHIFT+_IOC_SIZEBITS)`, which is 30.
pub const _IOC_DIRSHIFT: u32 = 30;

/// Where a request's size, in bytes, starts in its number.
/// `asm-generic/ioctl.h`: `#define _IOC_SIZESHIFT
/// (_IOC_TYPESHIFT+_IOC_TYPEBITS)`, which is 16.
pub const _IOC_SIZESHIFT: u32 = 16;

/// How many bits a request's size has.
/// `asm-generic/ioctl.h`: `# define _IOC_SIZEBITS 14`
pub const _IOC_SIZEBITS: u32 = 14;

/// A file flag: reads and writes do not wait, and fail with [`EAGAIN`]
/// instead. For `std::os::unix::fs::OpenOptionsExt::custom_flags`.
/// `asm-generic/fcntl.h`: `#define O_NONBLOCK 00004000`
pub const O_NONBLOCK: c_int = 0o4000;

/// An [`inotify_init1`] flag: reads of the instance do not wait.
/// `sys/inotify.h`: `IN_NONBLOCK = 00004000`, which is `O_NONBLOCK`.
pub const IN_NONBLOCK: c_int = 0o4000;

/// An [`inotify_init1`] flag: the instance is closed in a program that
/// `exec` starts.
/// `sys/inotify.h`: `IN_CLOEXEC = 02000000`, which is `O_CLOEXEC`
/// (`asm-generic/fcntl.h`: `#define O_CLOEXEC 02000000`).
pub const IN_CLOEXEC: c_int = 0o2000000;

/// An inotify mask bit: a file's metadata changed, its permissions among
/// them. udev gives a new device node its permissions after it creates it.
/// `linux/inotify.h`: `#define IN_ATTRIB 0x00000004 /* Metadata changed */`
pub const IN_ATTRIB: u32 = 0x0000_0004;

/// An inotify mask bit: a file was created in the watched folder.
/// `linux/inotify.h`: `#define IN_CREATE 0x00000100 /* Subfile was created
/// */`
pub const IN_CREATE: u32 = 0x0000_0100;

/// An inotify mask bit, always reported: the queue of events overflowed,
/// and some were lost.
/// `linux/inotify.h`: `#define IN_Q_OVERFLOW 0x00004000 /* Event queued
/// overflowed */`
pub const IN_Q_OVERFLOW: u32 = 0x0000_4000;

/// An inotify mask bit, always reported: the watch was removed, because the
/// watched folder was deleted or its file system unmounted.
/// `linux/inotify.h`: `#define IN_IGNORED 0x00008000 /* File was ignored */`
pub const IN_IGNORED: u32 = 0x0000_8000;

/// The fixed part of an event read from an inotify instance. The name of
/// the file, `len` bytes, NUL-padded, follows it; the next event follows
/// the name.
/// `linux/inotify.h`:
///
/// ```c
/// struct inotify_event {
///     __s32 wd;       /* watch descriptor */
///     __u32 mask;     /* watch mask */
///     __u32 cookie;   /* cookie to synchronize two events */
///     __u32 len;      /* length (including nulls) of name */
///     char name[];    /* stub for possible name */
/// };
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct inotify_event {
    /// The watch the event is of, as [`inotify_add_watch`] returned it.
    pub wd: c_int,
    /// What happened: `IN_*` bits.
    pub mask: u32,
    /// Ties the two halves of a rename together.
    pub cookie: u32,
    /// The bytes of the name that follows, its NUL padding among them.
    pub len: u32,
}

const _: () = assert!(size_of::<inotify_event>() == 16);
const _: () = assert!(align_of::<inotify_event>() == 4);
const _: () = assert!(offset_of!(inotify_event, mask) == 4);
const _: () = assert!(offset_of!(inotify_event, len) == 12);

/// A time, in seconds and microseconds: the `time` of an
/// [`input_event`](super::evdev::input_event).
/// `bits/types/struct_timeval.h`: `struct timeval { __time_t tv_sec;
/// __suseconds_t tv_usec; };`, both `long` on our 64-bit targets.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct timeval {
    /// Whole seconds.
    pub tv_sec: c_long,
    /// Microseconds, 0 to 999,999.
    pub tv_usec: c_long,
}

const _: () = assert!(size_of::<timeval>() == 16);
const _: () = assert!(align_of::<timeval>() == 8);
const _: () = assert!(offset_of!(timeval, tv_usec) == 8);

/// Try again: for an ALSA stream that does not block, no room to write in
/// yet (`snd_pcm_writei` returns it negated).
/// `asm-generic/errno-base.h`: `#define EAGAIN 11 /* Try again */`
pub const EAGAIN: c_int = 11;

/// No such device: an event device that was unplugged fails every `read`
/// with this, until it is closed.
/// `asm-generic/errno-base.h`: `#define ENODEV 19 /* No such device */`
pub const ENODEV: c_int = 19;

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

    /// Sends the device driver of `fd` the request `request`, with one more
    /// argument, a pointer for the requests declared here. Returns -1 and
    /// sets `errno` on failure, and otherwise a value that depends on the
    /// request.
    /// `sys/ioctl.h`: `extern int ioctl (int __fd, unsigned long int
    /// __request, ...)`.
    ///
    /// # Safety
    ///
    /// - The extra argument is what the request takes: for a request that
    ///   reads into a buffer, a pointer to as many writable bytes as the
    ///   request's size says.
    /// - `fd` is an open file whose driver the request is meant for, or the
    ///   call fails with `ENOTTY` or `EINVAL`.
    pub fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;

    /// Starts an inotify instance and returns its file descriptor, or -1
    /// and sets `errno`. `flags` is 0 or [`IN_NONBLOCK`] and
    /// [`IN_CLOEXEC`].
    /// `sys/inotify.h`: `extern int inotify_init1 (int __flags)`.
    ///
    /// The descriptor is the caller's, to close.
    pub safe fn inotify_init1(flags: c_int) -> c_int;

    /// Watches the file or folder `pathname` for the events of `mask`, and
    /// returns the watch's descriptor, which the instance's events carry, or
    /// -1 and sets `errno`.
    /// `sys/inotify.h`: `extern int inotify_add_watch (int __fd, const char
    /// *__name, uint32_t __mask)`.
    ///
    /// # Safety
    ///
    /// `pathname` points to a NUL-terminated string.
    pub fn inotify_add_watch(fd: c_int, pathname: *const c_char, mask: u32) -> c_int;
}

#[cfg(test)]
mod tests {
    use core::ffi::CStr;
    use std::ffi::CString;
    use std::fs::{self, File};
    use std::io::{self, Read};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::{env, process};

    use super::*;

    #[test]
    fn inotify_reports_a_file_made_in_a_watched_folder() {
        let folder = env::temp_dir().join(format!("nadir-sys-inotify-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("make the folder");

        let fd = inotify_init1(IN_NONBLOCK | IN_CLOEXEC);
        assert!(fd >= 0, "{}", io::Error::last_os_error());
        // SAFETY: `inotify_init1` returned a new descriptor that nothing else
        // owns.
        let mut instance = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        let path = CString::new(folder.as_os_str().as_bytes()).expect("no NUL in the path");
        // SAFETY: the path is a C string, and the descriptor an inotify
        // instance.
        let watch = unsafe { inotify_add_watch(instance.as_raw_fd(), path.as_ptr(), IN_CREATE) };
        assert!(watch >= 0, "{}", io::Error::last_os_error());

        // Nothing yet: the read does not wait.
        let mut buffer = [0u8; 256];
        let empty = instance.read(&mut buffer).expect_err("no event yet");
        assert_eq!(empty.raw_os_error(), Some(EAGAIN));

        File::create(folder.join("event7")).expect("make a file");
        let read = instance.read(&mut buffer).expect("an event");
        let header = size_of::<inotify_event>();
        assert!(read > header, "{read}");
        let field = |offset: usize| {
            u32::from_ne_bytes(buffer[offset..offset + 4].try_into().expect("four bytes"))
        };
        assert_eq!(field(offset_of!(inotify_event, wd)) as c_int, watch);
        assert_ne!(field(offset_of!(inotify_event, mask)) & IN_CREATE, 0);
        let len = field(offset_of!(inotify_event, len)) as usize;
        assert_eq!(read, header + len);
        let name = CStr::from_bytes_until_nul(&buffer[header..header + len]).expect("a NUL");
        assert_eq!(name, c"event7");

        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_evdev_request_on_a_file_that_is_no_device_fails() {
        let file = File::open("/proc/self/stat").expect("open a file");
        let mut id = [0u8; 8];
        // SAFETY: the buffer has the 8 bytes that the request writes at most.
        let result = unsafe {
            ioctl(
                file.as_raw_fd(),
                super::super::evdev::EVIOCGID,
                id.as_mut_ptr(),
            )
        };
        assert_eq!(result, -1);
        // A file with no driver of its own takes no requests: `ENOTTY`
        // (`asm-generic/errno-base.h`: `#define ENOTTY 25`).
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(25));
    }

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
