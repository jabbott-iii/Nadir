---
name: add-ffi-binding
description: Use whenever adding or changing OS bindings (Win32/COM, Objective-C/Cocoa/Metal, X11/EGL/ALSA/evdev, OpenGL). Declares items in nadir-sys with layout assertions and source citations, then wraps them safely in nadir-platform or nadir-gfx.
argument-hint: "<windows|macos|linux|gl> <API or functions needed>"
---

# Add an FFI binding

Request: `$ARGUMENTS`

We use no binding crates (ADR-0003), so correctness depends on following
this procedure exactly.

## 1. Scope it

- List the exact functions, types, constants, selectors or COM methods the
  feature calls. **Declare nothing else.**
- For each item, find the authoritative source:
  - Windows: Windows SDK headers (`um/*.h`, `shared/*.h`) or the Microsoft
    Learn reference page
  - macOS: SDK framework headers or the Apple developer documentation
  - Linux: `/usr/include/X11/*.h`, EGL/ALSA headers, kernel uapi
    `linux/input.h` and `linux/input-event-codes.h`
  - GL: the Khronos registry (`gl.xml`) or the GL 3.3 core specification
- If the header isn't available in this session (e.g. Windows headers on a
  Linux machine), use the official documentation. Mark those values
  "verify on <OS>".

## 2. Declare in `nadir-sys`

- File: `crates/sys/src/<os>/<module>.rs`. Every item carries a doc
  comment, or a `// source:` comment naming the header and declaration.
- Functions:
  - `unsafe extern "system" { ... }` for Win32/COM; `unsafe extern "C" { ... }`
    elsewhere
  - libraries we `dlopen` or `LoadLibraryW` use the `dynlib!` macro instead
    (a struct of function pointers, with every missing symbol reported by
    name)
- Types: `#[repr(C)]`, with fields in header order and exact widths:
  - `DWORD` = `u32`; `LONG` = `i32` on Windows
  - `long` = `i64` on LP64 Linux and macOS
  - `BOOL` (Win32) = `i32`; `BOOL` (objc) = `bool` on arm64, `i8` on x86_64
- Layout assertions for every struct:
  ```rust
  const _: () = assert!(core::mem::size_of::<XEvent>() == 192);
  const _: () = assert!(core::mem::align_of::<XEvent>() == 8);
  const _: () = assert!(core::mem::offset_of!(XKeyEvent, keycode) == 84);
  ```
  - Gate per target with `#[cfg(...)]` when values differ by architecture.
  - Take the numbers from the header or docs, never from the Rust compiler
    output of the struct you just wrote.
- Constants: use the header's type, and write the value exactly as the
  header does (hex stays hex).
- **COM:**
  - Reproduce every vtable slot in order, from `IUnknown`
    (`QueryInterface`, `AddRef`, `Release`) up to the last method we call.
  - Type unused slots as `usize`, with a comment naming each method.
  - Declare the GUIDs as `GUID` constants.
- **Objective-C:**
  - Cache selectors in a struct initialized once.
  - Each `msg_send!` call specifies the full fn-pointer signature.
  - On x86_64, struct returns larger than 16 bytes use `objc_msgSend_stret`.
  - Classes we create at runtime list their ivars and methods with type
    encodings in one place.

## 3. Wrap safely (in `nadir-platform` or `nadir-gfx`)

- Expose a safe Rust API: owned RAII types for handles and objects, `&str`
  instead of raw strings, our error enums instead of codes.
- One unsafe operation per block, each preceded by `// SAFETY:` explaining
  pointer validity, lifetime, initialization and thread affinity.
- Document ownership (who frees what, with which call) and the
  main-thread-only requirement where it applies.
- Convert every failure path into an error with context. Never ignore a
  return code silently.

## 4. Test

- **Layout:** the assertions are compile-time, so they're verified when the
  crate builds on that OS. Say which OS builds you actually ran.
- **Wrapper logic:** unit tests for anything that doesn't need the OS
  (mapping tables, string conversion, flag diffing).
- **Behavior:** if the change is user-visible, extend
  `examples/events.rs` or the relevant example, and describe the manual
  check.
- Run `cargo xtask unsafe-audit` and
  `cargo clippy -p <crate> --all-targets -- -D warnings`.

## 5. Report

List:

- each declared item and its source
- which layout assertions compiled on which OS in this session
- which items still need verification on another OS (also add a `⏳` note
  to the roadmap task)
