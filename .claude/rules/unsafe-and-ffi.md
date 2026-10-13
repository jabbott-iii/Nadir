---
paths:
  - "crates/sys/**"
  - "crates/platform/**"
  - "crates/gfx/**"
---

# Rules for `unsafe` and FFI code

These three crates are the only ones allowed to contain `unsafe`.

- **Declarations belong in `nadir-sys`.** `platform` and `gfx` call them.
  Never write `extern` blocks in `platform` or `gfx`.
- **To add bindings, follow `/add-ffi-binding`.** In short:
  - declare only what you call, and cite the header or doc for each item
  - assert `size_of`/`align_of` for every `#[repr(C)]` type, plus
    `offset_of!` for fields we read
- **Edition 2024 syntax:** `unsafe extern "system" { ... }` for Win32/COM,
  `unsafe extern "C" { ... }` elsewhere.
- **One unsafe operation per `unsafe` block,** each preceded by
  `// SAFETY:` explaining why it is sound: pointer validity, lifetimes,
  thread affinity, initialization.
- **Every `pub unsafe fn`** gets a `# Safety` doc section listing the
  caller's obligations.
- **Objective-C:**
  - Always call `objc_msgSend` through `msg_send!` with the exact
    signature, never through a varargs prototype.
  - On x86_64, use `_stret` for struct returns larger than 16 bytes.
  - Wrap each loop iteration in the autorelease-pool guard.
  - AppKit calls happen on the main thread only.
- **COM:** a vtable struct must reproduce **every slot in order** up to the
  last method we call. Type unused slots as `usize`, and document the
  method name of each.
- **Ownership:** document who frees every pointer an OS call returns, and
  how (`XFree`, `CoTaskMemFree`, `release`, `CFRelease`, ...). Wrap it in
  RAII.
- **Errors:** convert HRESULT, `errno`, EGL/GL errors and `NSError` into our
  error enums with context. Never ignore a return code. If ignoring one is
  truly correct, add a comment saying why.
- **Before reporting done,** run `cargo xtask unsafe-audit` (once it
  exists).
- If layout assertions can only compile on another OS, say which OS must
  verify them. Do not claim they passed.
