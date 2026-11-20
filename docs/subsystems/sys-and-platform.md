# Subsystem: `nadir-sys` and `nadir-platform`

Read `docs/03-platforms.md` for the per-OS details. This file defines the
crate structure and the API every other crate sees.

## `nadir-sys`: declarations only

- **Contains only:** `#[repr(C)]` types, constants, `unsafe extern` blocks,
  function-pointer type aliases, COM vtable structs, `dlopen` symbol tables,
  and layout tests.
- **No behavior:** no wrappers, no loops, and no allocation but a table's
  list of missing symbols. The only code is the generated symbol-table
  loaders and, on macOS, `msg_send!`.

```
crates/sys/src/
├── lib.rs                # #![allow(unsafe_code)], cfg-gated modules
├── gl.rs                 # GL types, enums, fn-pointer types (shared by all OSes)
├── dynlib.rs             # dynlib! macro: struct of fn pointers + load(&dyn Library) -> Result<Self, MissingSymbols>
├── windows/
│   ├── types.rs          # HWND, HRESULT, GUID, RECT, POINT, ...
│   ├── kernel32.rs user32.rs gdi32.rs shell32.rs winmm.rs ole32.rs
│   ├── wgl.rs            # WGL + ARB extension enums
│   ├── com.rs            # IUnknown vtable, GUID consts
│   ├── wasapi.rs         # IMMDeviceEnumerator, IMMDevice, IAudioClient, IAudioRenderClient vtables
│   ├── xinput.rs dbghelp.rs avrt.rs   # dynlib! tables
├── macos/
│   ├── objc.rs           # objc_* runtime fns, Class/Sel/Id/BOOL types, msg_send! macro
│   ├── foundation.rs appkit.rs quartzcore.rs   # NSRect/NSPoint/NSSize, enum constants
│   ├── hitoolbox.rs      # kVK_* virtual key codes
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
- Every struct whose fields are declared gets
  `const _: () = assert!(size_of::<T>() == N && align_of::<T>() == A);`,
  with N and A taken from the header for each supported target. Use `cfg` on
  the assertion when sizes differ by architecture.
- Calling conventions: `extern "system"` for Win32/COM and GL, `extern "C"`
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
pub struct Platform { /* this OS's backend */ }

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
    GamepadAxis { id: GamepadId, axis: GamepadAxis, value: f32 },   // sticks -1..1 (+Y down), triggers 0..1; raw, no deadzone
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GamepadId(/* from 1, never given twice */);
impl GamepadId {
    pub const fn from_raw(raw: u32) -> Self;   // for tests and replays
    pub const fn raw(self) -> u32;
}

pub struct GamepadState { /* … */ }
impl GamepadState {
    pub fn name(&self) -> &str;
    pub fn pressed(&self, button: GamepadButton) -> bool;
    pub fn axis(&self, axis: GamepadAxis) -> f32;
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

// Audio output (Linux from M6.3; ADR-0047). A free function: no backend
// needs the window, and a tool can open audio without one.
pub fn open_audio(cfg: &AudioOutputConfig, render: Box<dyn AudioRender>)
    -> Result<AudioStream, PlatformError>;

pub struct AudioOutputConfig {
    pub sample_rate: u32,      // 8,000..=384,000; 48,000
    pub channels: u16,         // 1..=8; 2
    pub latency_ms: u32,       // 5..=500; 30: the device's buffer
}

pub trait AudioRender: Send + 'static {
    /// Fill all of `out` with interleaved f32 frames, which follow those of
    /// the last call; its length can change from call to call. Called on
    /// the audio thread. Must not block, allocate, free or log; a panic ends
    /// the stream.
    fn render(&mut self, out: &mut [f32], channels: u16, sample_rate: u32);
    /// Each underrun the system reports, before the next render: the
    /// facade's wrapper of the mixer calls `Mixer::count_underrun`.
    fn underrun(&mut self);
}

pub struct AudioStream { /* Send + Sync; dropping it stops the thread */ }
impl AudioStream {
    pub fn latency(&self) -> Option<Duration>;   // the device's buffer; None while there is none
}

pub fn user_dirs(studio: &str, game: &str) -> Result<UserDirs, PlatformError>;  // config, data, logs
pub fn show_error_dialog(title: &str, message: &str);
pub fn os_description() -> String;  // one line, for crash reports: system, its version where known, architecture
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
  `Unsupported`, and `create_headless_gl` returns `Unsupported`. They
  ignore `WindowConfig::vsync`.

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

### Typed text (M7.9, ADR-0057)

- `Event::Text(char)` comes after the key press that typed it: one a
  character, a composed one included, again with each repeat, never a
  control character (Enter, Tab, Backspace, Escape and Delete are keys
  only).
- A key pressed with a shortcut's modifier types nothing: on Linux Control,
  Alt (Mod1) or Super (Mod4), from the event's own state, Alt Gr (Mod5)
  typing; on Windows Control without Alt, or the Windows key, Alt alone
  being the window menu's; on macOS ⌘, or Control without Option, from the
  press's `modifierFlags`, Option typing. The rules are in the crate's
  `text` module, which every system's tests run.
- Linux: an X input method, the one `XMODIFIERS` names or else Xlib's own
  (`@im=none`), with an input context of no preedit and no status. Opening
  the window first takes `LC_CTYPE` from the environment if it is still "C"
  or "POSIX", and puts "C" back if Xlib cannot work in it: in "C", Xlib
  composes Latin-1 only. Only one window of the process at a time has an
  input method, Xlib's locale and input-method state being process-wide and
  unlocked. Each event is decoded, then given to `XFilterEvent`: a key it
  takes is still a key, its text the input method's, which comes as a key
  press of keycode 0. A press no input method took, of a keycode whose last
  taken press is as late or later, is a copy that a server sent back, and
  is left out (GLFW's rule). If the input method stops, nothing more is
  typed.
- Windows: `WM_CHAR`, its surrogates joined, its shortcut's modifiers from
  `GetKeyState`; a key an input method editor took (`VK_PROCESSKEY`) is
  still a key, by its scan code.
- macOS: a key press's `characters`, without Apple's function-key
  characters (U+F700 to U+F7FF). No input method yet (backlog).

### Gamepads (M8.1, ADR-0065)

- `poll_gamepads` appends the pads' events since the last call, without
  waiting, apart from `pump_events`; the game calls both each frame. Its
  first call reports the pads already plugged in.
- **The events add up to the state.** A pad connects at rest, and what is
  held on it then follows as events. A pad unplugged is put back at rest by
  events of their own (each held button let go, each axis back at 0) before
  `GamepadDisconnected`. `gamepad_state` gives the state the events so far
  leave, while the pad is plugged in.
- **Ids** count from 1 in the order pads connect and are never given twice
  by one `Platform`: a pad plugged in again is a new pad.
- **Values:** a button by its place on the pad (core's `GamepadButton`,
  Xbox names); a stick's axis from -1 to 1 with +Y down, linear on each
  side of the middle of the driver's range (rounded up), so that both ends
  are exact, and the rest too in a range of three values or more; a
  trigger from 0 to 1, or 0 or 1 if the pad's trigger is only a button;
  the d-pad as four buttons. No dead zone: that
  is `nadir-input`'s (M8.4).
- Pads are read whether or not the window has the focus. Whether a game
  hears them in the background is the input layer's call (M8.4).
- Windows (M8.2) and macOS (M8.3) report no pads yet: the backend is a
  stand-in, `no_gamepads`.
- **Linux** (`linux/evdev.rs`), following `docs/03-platforms.md`:
  - A device is a pad if it has `EV_KEY`, `EV_ABS` and `BTN_SOUTH`.
  - The face buttons are read by place (`BTN_NORTH` is the top one) for
    Sony's and Nintendo's vendor ids, whose drivers follow the kernel's
    documentation, and by Xbox label (`BTN_X`, the same code, is X, on the
    left) for every other pad, as `xpad` and an Xbox pad over Bluetooth
    report them. SDL decides the same way for Sony's id; Nintendo's is
    added because `hid-nintendo` reports by place too.
  - The axes' roles come from what the pad has: the right stick on
    `ABS_RX`/`ABS_RY` and analog triggers on `ABS_Z`/`ABS_RZ` (`xpad`,
    `hid-playstation`, `hid-nintendo`); or, on a pad with no `ABS_RX` and
    `ABS_RY`, the right stick on `ABS_Z`/`ABS_RZ` and analog triggers on
    `ABS_BRAKE` (left) and `ABS_GAS` (right), as an Xbox pad over
    Bluetooth through `hid-generic` or `hid-microsoft` has them.
  - `BTN_TL2` and `BTN_TR2` drive the triggers only on a pad with no analog
    trigger there. The d-pad is `ABS_HAT0X`/`ABS_HAT0Y` or `BTN_DPAD_*`.
  - Records are read without waiting into a buffer on the stack, and
    applied as they come. At the `SYN_REPORT` that ends a report after
    `SYN_DROPPED`, the rest of that read and everything the device still
    has queued is dropped, as libevdev does, and the state is read again
    with `EVIOCGABS` and `EVIOCGKEY`, the changes reported. Every record
    dropped is older than that state; and `EVIOCGKEY` takes the queued key
    records out of the kernel's queue, so one applied after it could leave
    a button stuck. A read that fails with `ENODEV` is the pad unplugged;
    any other failure is logged, and the pad taken as unplugged.
  - `inotify` on `/dev/input`, `IN_CREATE` and `IN_ATTRIB`: a node not open
    yet is tried on either, so a node that udev opens to the user after
    making it is opened then. `IN_Q_OVERFLOW` rescans the folder;
    `IN_IGNORED`, or a failed read, stops the watch, with a warning.
  - A gamepad that the first poll cannot open for want of permission, by
    what sysfs (`/sys/class/input/eventN/device/capabilities`), which
    everyone can read, says of it, is logged with a hint about the seat's
    `uaccess` udev rules and the `input` group.
  - Each `ioctl` request is checked to read exactly the size of the value
    it fills, from the size its number carries, before it is sent.
  - Polling allocates nothing while no pad is plugged in or unplugged.

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
- Focus loss emits a synthetic release for every held key and button,
  right after `Focused(false)`, so that a reader can tell them from the
  user's own (ADR-0057; they came before it until M7.9).
- `Key` values are physical. Text comes only through `Event::Text`, after
  the key press that typed it.
- A gamepad's events, from its `GamepadConnected` to its
  `GamepadDisconnected`, start and end at rest, and give its state at every
  point: no button is down and no axis off 0 at either end.
- Mouse coordinates are in physical pixels with a top-left origin on every
  OS.
- `GpuSurface` stays valid for the lifetime of `Platform`. Window recreation
  is not supported (fullscreen toggling reuses the window).
- A thread has at most one OpenGL context (ADR-0015).
- On Linux, the EGL display of headless contexts is the same for the whole
  process and is never terminated: one `eglTerminate` would end it for
  every thread. The window's X11 EGL display is terminated before its X
  connection closes.
- The audio thread is created by `open_audio`, and makes every call of the
  system's audio API. `open_audio` waits for the device to open 2 s at
  most. Dropping `AudioStream` stops the thread and joins it, waiting up to
  2 s. A device that is lost, absent at the start or slow to open is tried
  again every second while the thread renders on in time (ADR-0047).
- `user_dirs` only works out paths; it creates no folder. It rejects a
  studio or game name that is not a valid folder name on all three OSes,
  whichever OS it runs on, so a name that works on one works on all.
- `os_description` reads `PRETTY_NAME` from `/etc/os-release` (else
  `/usr/lib/os-release`) and `/proc/sys/kernel/osrelease` on Linux, as
  `Pop!_OS 24.04 LTS (Linux 7.2.15-76070215-generic, x86_64)`. On Windows
  and macOS it gives the system's name and the architecture only, until
  M1.5 and M1.6 read their versions (ADR-0071).

## Tests

- **`sys`:** layout assertions, compiled per OS in CI, and unit tests of
  the table loader, of `msg_send!`, and of a few calls into each system's
  libraries.
- **`platform`:**
  - unit tests for the key-mapping tables (every `Key` reachable on each OS
    with a documented exception list) and the path resolution logic (env-var
    based, with the variables passed in, touching no folder)
  - a manual interactive test binary, `cargo run -p nadir-platform --example events`,
    which prints every event; used in milestone exit checks on each OS
  - typed text (M7.9): the shared rules (control characters, shortcuts,
    UTF-16 surrogates, Apple's function keys) on every system; each
    translator's text after its key, repeats, composed text, shortcuts and,
    on Linux, an input method's copies; on Linux, an ignored test (run with
    `--ignored --test-threads=1` in an X session) that sends key presses to
    its own window through the server and its input method
- **Headless GL** creation is tested in Linux CI: the tests behind the
  `headless-test` feature, which `cargo xtask ci` turns on, fail without a
  working driver. `NADIR_EXPECT_GL_RENDERER` (such as `llvmpipe`) makes
  them also require that renderer.
- **Audio output** (M6.3):
  - the audio thread's loop against a fake device and a fake clock: every
    sample written once, in order; full buffers and writes that take
    nothing waited for; underruns counted before the next render; suspends
    and signals recovered; a device lost on an error of a write or a wait,
    on four errors in a row, or on a 2 s stall, though not when frames are
    taken between them; then rendered on in time and tried every retry, a
    slow try losing only its own periods, a stop seen at its period; a
    stream with no device at the start, or the first device's own period;
    a device that never makes the loop wait held a buffer ahead of the
    clock; a refusal of the format told once; a drop that waits for its
    thread only so long; no allocation in a period, with underruns and
    waits (feature `dev`);
  - with `headless-test`, libasound through ALSA's `null` and `file`
    devices, which play nothing (`file` writes what it gets to a file): the
    file holds every sample in order, and the audio thread allocates
    nothing as it plays (with `dev` too); `null` is played in time; a
    device that does not exist starts the stream without one; one slow to
    open (a FIFO with no reader) is waited for 2 s at most, and plays once
    it opens; a format the device refuses fails; a render that panics ends
    the stream, `latency()` is then `None`, and its drop returns;
    `quietly` puts the thread's own handler of alsa-lib's messages back; a
    real PCM's errors come back negative. Some print alsa-lib's own
    `ALSA lib ...` lines;
  - an ignored test on the default device, which plays a second of
    silence with one period late by three buffers, and needs the
    underrun right after it;
  - a manual example, `cargo run -p nadir-platform --example audio`, of
    beeps on the left, then the right, with `NADIR_ALSA_DEVICE` to play on
    a device past the sound server, for unplugging and replugging it.
- **The window's context:** an ignored test, run with `--ignored` on an X11
  or XWayland session, opens a window three times.
- **Gamepads** (M8.1): the state's changes and rest as events; on Linux,
  the code → button and axis table for both readings of the face buttons,
  every button and axis reached on each of four layouts (`xpad`, Switch
  Pro, an Xbox pad over Bluetooth, a DirectInput pad), the trigger
  buttons, the hat, the axes' roles; the axes' normalization (the ends and
  the rest of the xpad, DualSense and Switch ranges, a range of two values,
  and property tests that a stick stays in -1..1 with exact ends and a
  trigger in 0..1, both monotonic); the `SYN_DROPPED` decoder; records and
  inotify events read from bytes; sysfs bitmaps; requests of the wrong size
  refused; and a pad read through a socket pair standing in for its device:
  its records, the records dropped after a lost report, its unplugging
  and, with `dev`, no allocation. `nadir-sys` tests
  `inotify` and `ioctl` on real files. A pad by hand: the events example
  prints the pads' events, which `/dev/uinput` can stand in for when no pad
  is at hand (roadmap M8.1).
