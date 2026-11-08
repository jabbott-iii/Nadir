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

//! ALSA's PCM interface, through which sound goes to the output device
//! (`libasound.so.2`).
//!
//! Sources: `<alsa/pcm.h>` and `<alsa/error.h>` of alsa-lib 1.2.11, and its
//! `src/pcm/pcm.c` for what each function does. Each item quotes its
//! declaration there. Every function of [`Asound`] is in alsa-lib 1.0.16
//! (2008) and later.
//!
//! Only what `nadir-platform` uses is declared: the "simple" setup of
//! `snd_pcm_set_params`, writes of interleaved frames that do not block,
//! with `snd_pcm_wait` for room, and the thread's own handler of alsa-lib's
//! messages.
//!
//! # The table and the error handler
//!
//! [`Asound`] holds functions that every `libasound.so.2` since 1.0.16
//! exports. `snd_lib_error_set_local`, which sets a handler of alsa-lib's
//! messages for the calling thread only, is newer: it is looked up apart,
//! with `dlsym`, and called through [`snd_lib_error_set_local_fn`], so that
//! an older library still loads.
//!
//! # Errors
//!
//! A function that returns an `int` or a [`snd_pcm_sframes_t`] returns a
//! negative `errno` value on failure, such as `-EPIPE` (see
//! [`libc`](super::libc)), and [`snd_strerror`](Asound::snd_strerror)
//! describes it. alsa-lib also prints its own messages about some failures
//! to stderr.
//!
//! # Calling ALSA
//!
//! What every function in [`Asound`] requires of its caller, besides what
//! each says:
//!
//! - A `pcm` argument came from `snd_pcm_open` and has not been closed.
//! - A PCM is used by one thread at a time. alsa-lib locks a PCM's calls
//!   when it is built to, but this crate does not count on it.
//! - A pointer argument is valid for what the function reads or writes
//!   through it.

// The types keep the names their header gives them, so that each one can be
// compared with the header.
#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
use core::marker::{PhantomData, PhantomPinned};

use crate::dynlib::dynlib;

// [`SND_PCM_FORMAT_FLOAT_LE`] is then the layout of `f32`, so a buffer of
// `f32`s goes to `snd_pcm_writei` as it is. Both Linux targets, x86_64 and
// aarch64, are little-endian.
const _: () = assert!(cfg!(target_endian = "little"));

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// An open PCM: a stream of samples to or from a device. Only alsa-lib
/// looks inside it.
/// `alsa/pcm.h`: `typedef struct _snd_pcm snd_pcm_t;`
#[repr(C)]
pub struct snd_pcm_t {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// A count of frames: one sample for each channel.
/// `alsa/pcm.h`: `typedef unsigned long snd_pcm_uframes_t;`
pub type snd_pcm_uframes_t = c_ulong;

/// A count of frames, or a negative error code.
/// `alsa/pcm.h`: `typedef long snd_pcm_sframes_t;`
pub type snd_pcm_sframes_t = c_long;

/// The direction of a stream. A C enum: with no negative value, GCC and
/// Clang make it an `unsigned int`.
/// `alsa/pcm.h`: `typedef enum _snd_pcm_stream { ... } snd_pcm_stream_t;`
pub type snd_pcm_stream_t = c_uint;

/// How the samples are passed. A C enum with no negative value, so an
/// `unsigned int`.
/// `alsa/pcm.h`: `typedef enum _snd_pcm_access { ... } snd_pcm_access_t;`
pub type snd_pcm_access_t = c_uint;

/// The format of a sample. A C enum with a negative value
/// (`SND_PCM_FORMAT_UNKNOWN = -1`), so an `int`.
/// `alsa/pcm.h`: `typedef enum _snd_pcm_format { ... } snd_pcm_format_t;`
pub type snd_pcm_format_t = c_int;

/// A handler of alsa-lib's messages for one thread: the source file, line
/// and function that reported it, an `errno` value or 0, and a `printf`
/// format with its arguments. Null is none: alsa-lib then uses its global
/// handler, which prints to stderr.
/// `alsa/error.h`: `typedef void (*snd_local_error_handler_t)(const char
/// *file, int line, const char *func, int err, const char *fmt, va_list
/// arg);`
///
/// A `va_list` is passed as a pointer on both Linux targets: on x86_64 it
/// is an array of one `__va_list_tag`, which decays to a pointer, and on
/// aarch64 a struct of 32 bytes, which AAPCS64 passes by reference. A
/// handler that does not read the arguments can take it as `*mut c_void`.
pub type snd_local_error_handler_t = Option<
    unsafe extern "C" fn(
        file: *const c_char,
        line: c_int,
        func: *const c_char,
        err: c_int,
        fmt: *const c_char,
        arg: *mut c_void,
    ),
>;

/// `snd_lib_error_set_local`, as `dlsym` finds it: sets the calling
/// thread's handler of alsa-lib's messages, or none, and returns the one it
/// replaces. Not in every libasound: see the module documentation.
/// `alsa/error.h`: `snd_local_error_handler_t
/// snd_lib_error_set_local(snd_local_error_handler_t func);`
///
/// # Safety
///
/// The pointer came from `dlsym(libasound, "snd_lib_error_set_local")`, and
/// a handler set is a function of [`snd_local_error_handler_t`]'s type that
/// stays valid for as long as it is set.
pub type snd_lib_error_set_local_fn =
    unsafe extern "C" fn(func: snd_local_error_handler_t) -> snd_local_error_handler_t;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// A stream of samples to the device: playback.
/// `alsa/pcm.h`: `SND_PCM_STREAM_PLAYBACK = 0,`
pub const SND_PCM_STREAM_PLAYBACK: snd_pcm_stream_t = 0;

/// A mode of `snd_pcm_open`: the kernel's device does not wait for one
/// that another program holds, which fails the open with `-EBUSY` at once,
/// and reads and writes do not wait. A plugin's own open, as of a sound
/// server, can still wait for the server.
/// `alsa/pcm.h`: `#define SND_PCM_NONBLOCK 0x00000001`
pub const SND_PCM_NONBLOCK: c_int = 0x0000_0001;

/// Interleaved frames, passed with `snd_pcm_writei` (the enum's fourth
/// value, after the three of mmap access).
/// `alsa/pcm.h`: `SND_PCM_ACCESS_RW_INTERLEAVED,`
pub const SND_PCM_ACCESS_RW_INTERLEAVED: snd_pcm_access_t = 3;

/// 32-bit floats, little-endian, from −1.0 to 1.0 (the enum's fifteenth
/// value, from `SND_PCM_FORMAT_S8 = 0`).
/// `alsa/pcm.h`: `SND_PCM_FORMAT_FLOAT_LE,`
pub const SND_PCM_FORMAT_FLOAT_LE: snd_pcm_format_t = 14;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

dynlib! {
    /// The functions we call in `libasound.so.2`.
    ///
    /// The module documentation lists what every one of them requires.
    pub struct Asound: extern "C" {
        /// Opens the PCM called `name`, such as `default`, and writes its
        /// handle to `pcm`. A `mode` of 0 makes the open, the reads and the
        /// writes block; with [`SND_PCM_NONBLOCK`], the reads, the writes
        /// and the kernel's device do not.
        /// Returns 0, or a negative error code. The handle is the caller's,
        /// to close with `snd_pcm_close`.
        /// `alsa/pcm.h`: `int snd_pcm_open(snd_pcm_t **pcm, const char *name,
        /// snd_pcm_stream_t stream, int mode);`
        ///
        /// `name` ends with a NUL.
        fn snd_pcm_open(
            pcm: *mut *mut snd_pcm_t,
            name: *const c_char,
            stream: snd_pcm_stream_t,
            mode: c_int,
        ) -> c_int;

        /// Closes a PCM, dropping what is queued for a stream that is
        /// running, and frees it. Returns 0, or a negative error code; the
        /// handle is freed either way.
        /// `alsa/pcm.h`: `int snd_pcm_close(snd_pcm_t *pcm);`
        fn snd_pcm_close(pcm: *mut snd_pcm_t) -> c_int;

        /// Sets the format, access, channels and rate, and a buffer of about
        /// `latency` microseconds. It fails unless the device takes exactly
        /// the rate and the channels asked for; with `soft_resample` 1,
        /// alsa-lib resamples to a rate the device lacks. Returns 0, or a
        /// negative error code. The PCM is then prepared.
        /// `alsa/pcm.h`: `int snd_pcm_set_params(snd_pcm_t *pcm,
        /// snd_pcm_format_t format, snd_pcm_access_t access, unsigned int
        /// channels, unsigned int rate, int soft_resample, unsigned int
        /// latency);`
        fn snd_pcm_set_params(
            pcm: *mut snd_pcm_t,
            format: snd_pcm_format_t,
            access: snd_pcm_access_t,
            channels: c_uint,
            rate: c_uint,
            soft_resample: c_int,
            latency: c_uint,
        ) -> c_int;

        /// Writes the sizes of the ring buffer and of a period, in frames,
        /// that `snd_pcm_set_params` chose. Returns 0, or a negative error
        /// code.
        /// `alsa/pcm.h`: `int snd_pcm_get_params(snd_pcm_t *pcm,
        /// snd_pcm_uframes_t *buffer_size, snd_pcm_uframes_t *period_size);`
        fn snd_pcm_get_params(
            pcm: *mut snd_pcm_t,
            buffer_size: *mut snd_pcm_uframes_t,
            period_size: *mut snd_pcm_uframes_t,
        ) -> c_int;

        /// Writes up to `size` interleaved frames from `buffer`, and returns
        /// how many it wrote, or a negative error code: `-EPIPE` for an
        /// underrun, `-ESTRPIPE` while the system is suspended, `-ENODEV` for
        /// a device that is gone, `-EBADFD` for a PCM in another wrong state.
        /// A PCM that does not block returns at once: with what fit, or 0
        /// for a stream not started whose buffer is full, or `-EAGAIN` for a
        /// running one with less room than its minimum (a period, after
        /// `snd_pcm_set_params`). A blocking PCM waits until every frame is
        /// in the ring buffer, for as long as that takes.
        /// `alsa/pcm.h`: `snd_pcm_sframes_t snd_pcm_writei(snd_pcm_t *pcm,
        /// const void *buffer, snd_pcm_uframes_t size);`
        ///
        /// `buffer` holds `size` frames of the format set.
        fn snd_pcm_writei(
            pcm: *mut snd_pcm_t,
            buffer: *const c_void,
            size: snd_pcm_uframes_t,
        ) -> snd_pcm_sframes_t;

        /// Waits up to `timeout` milliseconds, or for ever if it is -1, for
        /// room to write. Returns 1 once there is room, 0 at the timeout,
        /// or a negative error code: `-EPIPE` for an underrun, `-ESTRPIPE`
        /// while the system is suspended.
        /// `alsa/pcm.h`: `int snd_pcm_wait(snd_pcm_t *pcm, int timeout);`
        fn snd_pcm_wait(pcm: *mut snd_pcm_t, timeout: c_int) -> c_int;

        /// Recovers the PCM from the error `err` that a read or write
        /// returned: `-EINTR` needs nothing, `-EPIPE` prepares it again, and
        /// `-ESTRPIPE` waits, a second at a time, while the system resumes,
        /// then resumes the stream with what its buffer holds, or else
        /// prepares it again. Returns 0, or the error it could not handle:
        /// any other error unchanged. With `silent` 1 it does not print the
        /// notice of an underrun; a failure to prepare the PCM again is
        /// printed whatever `silent` is.
        /// `alsa/pcm.h`: `int snd_pcm_recover(snd_pcm_t *pcm, int err, int
        /// silent);`
        fn snd_pcm_recover(pcm: *mut snd_pcm_t, err: c_int, silent: c_int) -> c_int;

        /// Describes an error code that an ALSA function returned, given
        /// either sign. The string is alsa-lib's or the C library's, never
        /// null, and must not be freed.
        /// `alsa/error.h`: `const char *snd_strerror(int errnum);`
        fn snd_strerror(errnum: c_int) -> *const c_char;
    }
}

#[cfg(test)]
mod tests {
    use core::ffi::CStr;

    use super::*;
    use crate::dynlib::Library;
    use crate::linux::libc::{EAGAIN, EPIPE, ESTRPIPE, RTLD_LOCAL, RTLD_NOW, dlopen, dlsym};

    /// A library opened with `dlopen`.
    struct Opened(*mut c_void);

    impl Library for Opened {
        fn symbol(&self, name: &CStr) -> *const c_void {
            // SAFETY: the handle came from `dlopen` and is never closed, and
            // the name is a C string.
            unsafe { dlsym(self.0, name.as_ptr()) }
        }
    }

    #[test]
    fn the_installed_libasound_has_every_function_we_declare() {
        // SAFETY: the name is a C string, and libasound is sound to load
        // into any process.
        let handle = unsafe { dlopen(c"libasound.so.2".as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        if handle.is_null() {
            // This machine has no ALSA, so there is nothing to compare with.
            return;
        }
        // SAFETY: the library is libasound, which defines these functions as
        // its headers declare them, and it is never unloaded.
        let table = unsafe { Asound::load(&Opened(handle)) };
        let asound = match table {
            Ok(asound) => asound,
            Err(missing) => panic!("libasound.so.2: {missing}"),
        };
        // The error numbers, as the C library names them: a wrong one
        // would name another error.
        for (error, name) in [
            (EAGAIN, "Resource temporarily unavailable"),
            (EPIPE, "Broken pipe"),
            (ESTRPIPE, "Streams pipe error"),
        ] {
            // SAFETY: `snd_strerror` takes any number.
            let message = unsafe { (asound.snd_strerror)(-error) };
            assert!(!message.is_null());
            // SAFETY: the string is not null, ends with a NUL, and is
            // static.
            let message = unsafe { CStr::from_ptr(message) };
            assert_eq!(message.to_str(), Ok(name));
        }

        // The enums' values, as the library names them. These functions
        // are looked up here only: the engine does not call them.
        // `alsa/pcm.h`: `const char *snd_pcm_format_name(const
        // snd_pcm_format_t format);`, and the same of `snd_pcm_access_t`
        // and `snd_pcm_stream_t`.
        type Name = unsafe extern "C" fn(c_int) -> *const c_char;
        let opened = Opened(handle);
        for (function, value, name) in [
            (c"snd_pcm_format_name", SND_PCM_FORMAT_FLOAT_LE, "FLOAT_LE"),
            (
                c"snd_pcm_access_name",
                SND_PCM_ACCESS_RW_INTERLEAVED.cast_signed(),
                "RW_INTERLEAVED",
            ),
            (
                c"snd_pcm_stream_name",
                SND_PCM_STREAM_PLAYBACK.cast_signed(),
                "PLAYBACK",
            ),
        ] {
            let found = opened.symbol(function);
            assert!(!found.is_null(), "{function:?}");
            // SAFETY: the pointer is not null, and is that function, which
            // takes an enum (an `int` or `unsigned int`, passed alike) and
            // returns a static string or null.
            let function = unsafe { core::mem::transmute::<*const c_void, Name>(found) };
            // SAFETY: as above; any value is taken.
            let named = unsafe { function(value) };
            assert!(!named.is_null(), "{value}");
            // SAFETY: the string is not null, ends with a NUL, and is static.
            let named = unsafe { CStr::from_ptr(named) };
            assert_eq!(named.to_str(), Ok(name));
        }
    }
}
