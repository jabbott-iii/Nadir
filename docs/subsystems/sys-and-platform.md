# Subsystem: `nadir-sys` and `nadir-platform`

Read `docs/03-platforms.md` for the per-OS details. This file defines the
crate structure and the API every other crate sees.

## `nadir-sys`: declarations only

- **Contains only:** `#[repr(C)]` types, constants, `unsafe extern` blocks,
  function-pointer type aliases, COM vtable structs, `dlopen` symbol tables,
  and layout tests.
- **No behavior:** no wrappers, no loops, no allocation. The only functions
  are the generated symbol-table loaders.

```
crates/sys/src/
├── lib.rs                # #![allow(unsafe_code)], cfg-gated modules
├── gl.rs                 # GL types, enums, fn-pointer types (shared by all OSes)
├── dynlib.rs             # dynlib! macro: struct of fn pointers + load(&dyn Library) -> Result<Self, MissingSymbols>
├── windows/
│   ├── types.rs          # HWND, HRESULT, GUID, RECT, MSG, WNDCLASSEXW, ...
│   ├── kernel32.rs user32.rs gdi32.rs shell32.rs winmm.rs ole32.rs
│   ├── wgl.rs            # WGL + ARB extension enums
│   ├── com.rs            # IUnknown vtable, GUID consts
│   ├── wasapi.rs         # IMMDeviceEnumerator, IMMDevice, IAudioClient, IAudioRenderClient vtables
│   ├── xinput.rs dbghelp.rs avrt.rs   # dynlib! tables
├── macos/
│   ├── objc.rs           # objc_* runtime fns, Class/Sel/Id/BOOL types, msg_send! macro
│   ├── foundation.rs appkit.rs quartzcore.rs   # NSRect/NSPoint/NSSize, enum constants
│   ├── opengl.rs metal.rs audiotoolbox.rs gamecontroller.rs
├── linux/
│   ├── libc.rs           # dlopen/dlsym/dlerror, ioctl, inotify_*, poll, errno
│   ├── x11.rs            # Display, Window, XEvent (192-byte union), XVisualInfo, ... (dynlib!)
│   ├── egl.rs alsa.rs    # dynlib! tables
│   └── evdev.rs          # input_event, input_absinfo, EVIOC* ioctl numbers, KEY_*/BTN_*/ABS_* codes
```

### Rules

- Declare **only what is called.** Each declaration cites its source: a
  header name or doc URL.
- Every struct gets `const _: () = assert!(size_of::<T>() == N && align_of::<T>() == A);`,
  with N and A taken from the header for each supported target. Use `cfg` on
  the assertion when sizes differ by architecture.
- Calling conventions: `extern "system"` for Win32/COM, `extern "C"`
  elsewhere.
- Use edition-2024 syntax: `unsafe extern "C" { ... }`. Mark items whose
  calling has no preconditions as `safe fn` only when that's true for every
  argument value (rare; default to `unsafe fn`).
- `msg_send!` must transmute `objc_msgSend` to the exact signature. Calling it
  through a varargs prototype is forbidden because of the arm64 ABI.
  - Every call spells the signature out:
    `msg_send![receiver, selector, (argument => Type, ...) -> Result]`.
  - The cast lives in one place: `MessageArguments`, implemented for tuples
    of up to four arguments. On x86_64 it sends a result of more than 16
    bytes through `objc_msgSend_stret`.
- Prefer `dlopen`/`LoadLibraryW` for anything not guaranteed present
  (XInput, X11, EGL, ALSA, avrt, dbghelp), so the game can report a clear
  error.
- A symbol table holds only functions that every version of its library we
  support exports. A newer one (EGL 1.5's `eglGetPlatformDisplay`) gets a
  function-pointer type and is looked up apart, after a version check, so an
  older library still loads.
- GL functions are never linked or tabled in `nadir-sys`: `gl.rs` declares
  the type of each one we call, named like the header's typedef
  (`PFNGLCLEARPROC`), with `extern "system"`, and says which GL version
  brought it. `nadir-gfx` loads them through `GlContext::get_proc_address`.
- `nadir-sys` declares symbol tables but never opens a library:
  - `dynlib.rs` declares the `Library` trait, whose one method looks a
    symbol up by name. `nadir-platform` implements it for what `dlopen` and
    `LoadLibraryW` return.
  - A table's `load` is `unsafe`: the caller promises that the library is
    the one the table was written for. If symbols are missing, the
    `MissingSymbols` error names every one of them.
  - A loaded library stays loaded until the process exits, so the function
    pointers in a table never dangle. `dlclose` is not declared.

## `nadir-platform`: safe OS behavior

### Public API

```rust
pub struct Platform { /* per-OS backend enum */ }

pub struct WindowConfig {
    pub title: String,
    pub size: UVec2,           // logical size
    pub min_size: UVec2,
    pub resizable: bool,
    pub fullscreen: bool,      // borderless
    pub vsync: bool,           // swap interval 1, or else 0
    pub gpu: GpuPreference,    // Gl33 | Metal | Auto; comes with the Metal backend (M10)
}

impl Platform {
    pub fn new(cfg: &WindowConfig) -> Result<Self, PlatformError>;
    pub fn pump_events(&mut self, out: &mut Vec<Event>);        // non-blocking; appends
    pub fn window_size(&self) -> UVec2;                          // physical pixels
    pub fn scale_factor(&self) -> f32;
    pub fn is_minimized(&self) -> bool;
    pub fn set_fullscreen(&mut self, on: bool);
    pub fn set_title(&mut self, title: &str);
    pub fn set_cursor_visible(&mut self, visible: bool);
    pub fn set_cursor_grab(&mut self, grab: bool);
    pub fn gpu_surface(&self) -> GpuSurface<'_>;
    pub fn poll_gamepads(&mut self, out: &mut Vec<Event>);       // connect/disconnect + state changes
    pub fn gamepad_state(&self, id: GamepadId) -> Option<&GamepadState>;
    pub fn set_modal_frame_callback(&mut self, cb: Box<dyn FnMut(ModalFrame)>);
    pub fn open_audio(&mut self, cfg: AudioConfig, render: Box<dyn AudioRender>)
        -> Result<AudioStream, PlatformError>;
}

pub enum Event {
    CloseRequested,
    Resized(UVec2),
    ScaleFactorChanged(f32),
    Focused(bool),
    Minimized(bool),
    Key { key: Key, pressed: bool, repeat: bool },
    Text(char),
    MouseMoved(Vec2),            // physical px, top-left origin
    MouseButton { button: MouseButton, pressed: bool },
    MouseWheel(Vec2),            // lines; +y = away from user
    GamepadConnected(GamepadId),
    GamepadDisconnected(GamepadId),
    GamepadButton { id: GamepadId, button: GamepadButton, pressed: bool },
    GamepadAxis { id: GamepadId, axis: GamepadAxis, value: f32 },   // -1..1 raw, no deadzone
}

#[non_exhaustive]
pub enum GpuSurface<'a> {
    Gl(&'a dyn GlContext),
    #[cfg(target_os = "macos")]
    Metal(MetalLayerRef<'a>),
}

/// # Safety: the promises of ADR-0015 (real functions, a 3.3 core context
/// current on the creating thread, neither Send nor Sync).
pub unsafe trait GlContext {
    fn get_proc_address(&self, name: &core::ffi::CStr) -> *const core::ffi::c_void;
    fn swap_buffers(&self) -> Result<(), PlatformError>;
    fn set_swap_interval(&self, interval: i32) -> bool;   // true only if the driver took exactly this interval
    fn drawable_size(&self) -> UVec2;                     // physical px; zero if there is no default framebuffer
    fn context_id(&self) -> Option<GlContextId> { None }  // ADR-0016: Some for nadir-platform's contexts
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlContextId(/* never reused */);
impl GlContextId {
    pub fn is_current(self) -> bool;   // the context exists, and is current on this thread
}

pub trait AudioRender: Send + 'static {
    /// Fill `out` with interleaved f32 frames. Called on the audio thread.
    /// Must not block, allocate, or panic.
    fn render(&mut self, out: &mut [f32], channels: u16, sample_rate: u32);
}

pub fn user_dirs(studio: &str, game: &str) -> Result<UserDirs, PlatformError>;  // config, data, logs
pub fn show_error_dialog(title: &str, message: &str);
pub fn precise_sleep(until: std::time::Instant);
pub fn create_headless_gl() -> Result<HeadlessGl, PlatformError>;               // tests/goldens; HeadlessGl: GlContext, !Send, !Sync
```

### GPU surfaces

- **One OpenGL context a thread** (ADR-0015): the window's, on the thread
  that opened it, or a `HeadlessGl`. It is current there from creation
  until it is dropped. A second one fails with
  `PlatformError::GlContextExists`.
- **Context ids** (ADR-0016): every context takes the thread's claim
  (`gpu::Claim`, shared by every backend), which gives it a `GlContextId`
  that no other context of the process gets, and overrides
  `GlContext::context_id`. `is_current` is `true` while that context
  exists on the thread. `nadir-gfx` keeps the id instead of a borrow, so a
  device can live beside `Platform`'s `&mut` calls. WGL (M2.6) and
  NSOpenGL (M2.7) must take the claim too; a context without an id gets
  `NoContext` from gfx.
- **`HeadlessGl`** draws only into render targets. It has no default
  framebuffer: `drawable_size` is zero, `swap_buffers` does nothing, and
  `set_swap_interval` returns `false`.
- **`set_swap_interval`** returns `true` only if the driver accepted exactly
  that interval. EGL would quietly clamp it to the range of the config. A
  `true` promises no pacing: llvmpipe takes an interval of 1 and does not
  wait at all, and under XWayland (COSMIC, 144 Hz) an interval of 1 held
  swaps to about 288 a second, not to the display. `WindowConfig::vsync` is
  applied once, when the window opens. If
  the driver refuses it, a warning is logged and the window opens anyway.
- **Until M2.6 and M2.7**, Windows and macOS have no OpenGL context. Their
  `gpu_surface` is a context with no functions, whose swaps fail with
  `Unsupported`, and `create_headless_gl` returns `Unsupported`.

### Modal frames (Windows move/resize, macOS live resize)

- While the OS runs a modal loop, the platform calls the registered callback
  with a `ModalFrame` (current size and scale; a way to present comes with
  the GPU surface, and with it a lifetime). This lets the
  facade run one simulation/render iteration.
- The facade keeps its state in a structure the callback can reach without
  borrowing `Platform` mutably. For example, `Rc<RefCell<EngineCore>>` held
  by both the loop and the callback, with `Platform` passed in separately.
- If this proves fragile, fall back to the two-thread design described in
  `docs/03-platforms.md` and record an ADR.

### Allocation counter (feature `dev`)

The counter wraps `std::alloc::System` with a `#[global_allocator]`. It keeps
per-thread counters (allocations, deallocations, bytes) in `thread_local!`
`Cell`s, plus process totals in atomics. `nadir-platform` owns this because
it requires `unsafe impl GlobalAlloc`.

- A reallocation counts as an allocation of its new size, and not as a
  deallocation. The bytes are the bytes asked for.
- `thread_alloc_counts()` and `process_alloc_counts()` read them, as an
  `AllocCounts`; `AllocCounts::since` takes the difference of two readings.
- Inside the allocator nothing allocates, locks, logs or panics: the
  thread-local has a constant start and no destructor, and is read with
  `try_with`.

## Invariants

- Each window has its own event sequence. Events are delivered in OS order.
- Focus loss emits a synthetic release for every held key and button.
- `Key` values are physical. Text comes only through `Event::Text`.
- Mouse coordinates are in physical pixels with a top-left origin on every
  OS.
- `GpuSurface` stays valid for the lifetime of `Platform`. Window recreation
  is not supported (fullscreen toggling reuses the window).
- A thread has at most one OpenGL context (ADR-0015).
- On Linux, the EGL display of headless contexts is the same for the whole
  process and is never terminated: one `eglTerminate` would end it for
  every thread. The window's X11 EGL display is terminated before its X
  connection closes.
- The audio thread is created by `open_audio`. Dropping `AudioStream` stops
  and joins it.
- `user_dirs` only works out paths; it creates no folder. It rejects a
  studio or game name that is not a valid folder name on all three OSes,
  whichever OS it runs on, so a name that works on one works on all.

## Tests

- **`sys`:** layout assertions, compiled per OS in CI.
- **`platform`:**
  - unit tests for the key-mapping tables (every `Key` reachable on each OS
    with a documented exception list) and the path resolution logic (env-var
    based, in temp dirs)
  - a manual interactive test binary, `cargo run -p nadir-platform --example events`,
    which prints every event; used in milestone exit checks on each OS
- **Headless GL** creation is tested in Linux CI: the tests behind the
  `headless-test` feature, which `cargo xtask ci` turns on, fail without a
  working driver. `NADIR_EXPECT_GL_RENDERER` (such as `llvmpipe`) makes
  them also require that renderer.
- **The window's context:** an ignored test, run with `--ignored` on an X11
  or XWayland session, opens a window three times.
