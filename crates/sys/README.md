# nadir-sys

Hand-written raw FFI declarations for the systems the Nadir engine runs on:
Win32, the Objective-C runtime and AppKit, Xlib, EGL, ALSA, the Linux key
codes, and the types of OpenGL 3.3. It declares and does nothing else, and almost
every function in it is `unsafe` to call.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Games do not use it directly: the engine's
[nadir-platform](https://crates.io/crates/nadir-platform) and
[nadir-gfx](https://crates.io/crates/nadir-gfx) call it and wrap it in safe
APIs, and games reach those through
[nadir_engine](https://crates.io/crates/nadir_engine). Depend on it only to
write OS code of your own on top of the same declarations.

## Status

Version 0.3: the API will change before 1.0. Only what the engine calls is
declared, so each module is far from a full binding of its API.

Declared today:

- On every system: the OpenGL 3.3 core types, constants and function types
  (`gl`), and the symbol tables' `Library` trait and `MissingSymbols` error
  (`dynlib`).
- Linux: the dynamic loader (`dlopen`, `dlsym`, `dlerror`), Xlib
  (`libX11.so.6`), EGL (`libEGL.so.1`), ALSA's PCM output
  (`libasound.so.2`) and the kernel's key codes.
- Windows: the Win32 types and functions of `kernel32`, `user32`, `gdi32`,
  `shell32` and `ole32` that a window, its messages, the user's folders and
  a message box need.
- macOS: the Objective-C runtime with a `msg_send!` macro, and the numbers
  of Foundation, AppKit and HIToolbox's key codes.

Not declared yet: WGL and NSOpenGL (roadmap tasks M2.6 and M2.7), WASAPI
and CoreAudio (M6.4 and M6.5), the gamepad APIs (M8.1 to M8.3), Metal
(M10.1), and X input methods for typed text (M7.2).

## Usage

```toml
[dependencies]
nadir-sys = "0.3"
```

GL functions are never linked. A program looks each one up through the
loader of its context and casts it to the type that `gl` declares for it:

```rust
use core::ffi::{CStr, c_void};
use core::mem;

use nadir_sys::gl::{GL_COLOR_BUFFER_BIT, GLfloat, PFNGLCLEARCOLORPROC, PFNGLCLEARPROC};

/// Fills the bound framebuffer with `color`. Returns `false`, and draws
/// nothing, if the loader lacks either function.
///
/// # Safety
///
/// An OpenGL 3.3 core context is current on this thread, and `load`
/// returns its function of each name, or null.
unsafe fn clear(load: impl Fn(&CStr) -> *const c_void, color: [GLfloat; 4]) -> bool {
    let (clear_color, clear) = (load(c"glClearColor"), load(c"glClear"));
    if clear_color.is_null() || clear.is_null() {
        return false;
    }
    // SAFETY: the pointer is not null, and is `glClearColor`, of this type.
    let clear_color =
        unsafe { mem::transmute::<*const c_void, PFNGLCLEARCOLORPROC>(clear_color) };
    // SAFETY: as above, for `glClear`.
    let clear = unsafe { mem::transmute::<*const c_void, PFNGLCLEARPROC>(clear) };
    let [red, green, blue, alpha] = color;
    // SAFETY: the context is current, and the call takes only numbers.
    unsafe { clear_color(red, green, blue, alpha) };
    // SAFETY: as above.
    unsafe { clear(GL_COLOR_BUFFER_BIT) };
    true
}
```

## Platforms

Each system's module is built only on that system: `linux` on Linux,
`windows` on Windows and `macos` on macOS. `gl` and `dynlib` are built on
all three. docs.rs documents the crate for each of them.

- **Linux:** Xlib and EGL are not linked. They are loaded while the program
  runs, through the tables `Xlib` and `Egl`, so that a machine without them
  gets an error that names what is missing. This crate links only
  `libdl`. The layouts were measured by a C program on x86_64 Linux, and
  hold wherever `long` and pointers are 8 bytes, as on aarch64 too.
- **Windows:** the functions are linked through the Windows SDK's import
  libraries, and all are in Windows 10.
- **macOS:** `libobjc`, Foundation and AppKit are linked.

The Windows and macOS layouts and constants come from Microsoft's and
Apple's documented declarations. They compile in CI on those systems, but
have not been checked against the SDK headers yet.

## Documentation

- API reference: https://docs.rs/nadir-sys
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/sys-and-platform.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
