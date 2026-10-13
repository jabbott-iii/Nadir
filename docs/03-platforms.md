# 03 — Platforms

This file collects every OS-specific decision. Code for all of it lives in
`nadir-sys` (declarations) and `nadir-platform` (behavior). The GPU
backends live in `nadir-gfx`.

## Targets

| OS | Rust target(s) | Minimum version | Toolchain notes |
| :- | :- | :- | :- |
| Windows | `x86_64-pc-windows-msvc` | Windows 10 1809 | MSVC build tools and Windows SDK (also supply `rc.exe`, `signtool.exe`) |
| macOS | `aarch64-apple-darwin`, `x86_64-apple-darwin` → universal via `lipo` | macOS 12 | Xcode command-line tools (`codesign`, `notarytool`, `lipo`, `iconutil`, `hdiutil`); full Xcode not required |
| Linux | `x86_64-unknown-linux-gnu` | glibc 2.31 (Ubuntu 20.04 era) | Build release binaries in the oldest supported distro container to set the glibc floor |

`aarch64-pc-windows-msvc` and `aarch64-unknown-linux-gnu` are backlog items.
The code should not assume x86_64, so adding them is a packaging task.

## Development order

**Linux → Windows → macOS** for every OS-facing milestone.

- Linux comes first because CI headless rendering (Mesa llvmpipe) and Claude
  Code sessions run there.
- Each milestone's exit criteria require all three OSes, so no platform falls
  more than one milestone behind.

---

## Windows

### Window and event loop

- Call `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)`
  at startup. Also declare it in the embedded manifest, so it applies even
  before code runs.
- Create the window with `RegisterClassExW` and `CreateWindowExW` (UTF-16
  strings via a small `to_wide()` helper).
- Pump messages with `PeekMessageW(PM_REMOVE)`, then `TranslateMessage` and
  `DispatchMessageW`.
- The window procedure forwards to a per-window state struct, stored with
  `SetWindowLongPtrW(GWLP_USERDATA)`.
- **Modal loop problem.** Dragging or resizing the window makes
  `DefWindowProcW` enter a modal loop, so our `PeekMessageW` loop stops.
  - On `WM_ENTERSIZEMOVE`, start a `SetTimer` at about 1 ms.
  - On `WM_TIMER`, run one engine iteration through a callback registered by
    the facade.
  - On `WM_EXITSIZEMOVE`, stop the timer.
  - If this proves insufficient, the fallback design is to run the game loop
    on a second thread with the message pump on the main thread. Record that
    as an ADR if it's needed.
- Handle `WM_DPICHANGED` by resizing to the suggested rect and emitting
  `Event::ScaleFactorChanged`.
- Fullscreen is borderless: switch to `WS_POPUP` covering the monitor rect
  from `MonitorFromWindow` + `GetMonitorInfoW`. Exclusive fullscreen is not
  supported (ADR-0001 notes).
- Cursor: `ShowCursor` for hide/show; `ClipCursor` for confinement.

### Input

- **Keyboard:** handle `WM_KEYDOWN`/`WM_KEYUP`/`WM_SYSKEYDOWN`/`WM_SYSKEYUP`.
  - The physical key comes from the scan code: `lParam` bits 16–23, plus the
    extended flag in bit 24. A table maps it to `nadir_core::Key` (USB HID
    usage based).
  - Text comes from `WM_CHAR`. Join UTF-16 surrogate pairs.
- **Mouse:** position and buttons from `WM_MOUSEMOVE`, the button messages and
  `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL`. Relative motion (for future aim modes)
  uses Raw Input, `RegisterRawInputDevices` on usage page 0x01, usage 0x02.
- **Gamepads:** XInput, loaded with
  `LoadLibraryW("xinput1_4.dll")` (fallback `xinput9_1_0.dll`).
  - Poll `XInputGetState` for 4 slots each frame.
  - **Polling a disconnected slot is slow.** Re-probe disconnected slots at
    most once per second.
  - XInput only covers Xbox-protocol pads. DualSense and Switch Pro on
    Windows need HID; that's backlog.

### GPU (OpenGL)

1. Create a hidden dummy window with a legacy context (`ChoosePixelFormat`,
   `SetPixelFormat`, `wglCreateContext`), make it current, and load
   `wglCreateContextAttribsARB` and `wglChoosePixelFormatARB`.
2. Destroy the dummy. A window's pixel format can be set only once, which is
   why a dummy window is needed.
3. On the real window, pick the format with `wglChoosePixelFormatARB` and
   create a 3.3 core context with `WGL_CONTEXT_PROFILE_MASK_ARB = CORE`.
4. Load GL functions with `wglGetProcAddress`. It returns null (or 1, 2, 3,
   −1) for GL 1.1 functions; load those from `opengl32.dll` with
   `GetProcAddress`.
5. Set vsync with `wglSwapIntervalEXT(1)` when `WGL_EXT_swap_control` is
   present.

### Audio (WASAPI, shared mode, event-driven)

1. `CoInitializeEx(COINIT_MULTITHREADED)` on the audio thread.
2. `CoCreateInstance(CLSID_MMDeviceEnumerator)` →
   `GetDefaultAudioEndpoint(eRender, eConsole)` → `Activate(IAudioClient)`.
3. Call `Initialize` with `AUDCLNT_SHAREMODE_SHARED`,
   `AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM`,
   requesting 48 kHz stereo `f32`.
4. `SetEventHandle`, then `GetService(IAudioRenderClient)`, then `Start`.
5. Loop: `WaitForSingleObject` → `GetCurrentPadding` → `GetBuffer` → mix →
   `ReleaseBuffer`.
6. Boost the thread with `AvSetMmThreadCharacteristicsW(L"Games")` from
   `avrt.dll`.
7. On `AUDCLNT_E_DEVICE_INVALIDATED`, tear down and reopen the default
   device. `IMMNotificationClient` for proactive default-device switching is
   a later task.

COM interfaces are hand-written `#[repr(C)]` vtable structs: only the methods
we call, with the full slot ordering preserved.

### Timing

- `std::time::Instant` uses `QueryPerformanceCounter`, so it's fine.
- For the frame limiter, sleep with
  `CreateWaitableTimerExW(CREATE_WAITABLE_TIMER_HIGH_RESOLUTION)` (Windows 10
  1803+). Fall back to `timeBeginPeriod(1)` + `Sleep`.

### Paths

- Config: `%APPDATA%\<Studio>\<Game>\`
- Data/saves/logs: `%LOCALAPPDATA%\<Studio>\<Game>\`
- Resolve them with `SHGetKnownFolderPath` (`FOLDERID_RoamingAppData`,
  `FOLDERID_LocalAppData`). Free the result with `CoTaskMemFree`.

### Release specifics

- Release builds use `#![windows_subsystem = "windows"]`, so no console
  window appears.
- Embed the manifest (DPI awareness, `activeCodePage` UTF-8) from `build.rs`
  with `cargo:rustc-link-arg=/MANIFEST:EMBED` and
  `/MANIFESTINPUT:<path>`.
- The icon and version info are compiled by `rc.exe` during `xtask` packaging.

---

## macOS

### Objective-C runtime glue (in `nadir-sys::objc` + `nadir-platform::macos`)

- Use `objc_getClass`, `sel_registerName` (cache selectors in a struct
  initialized once), and `objc_msgSend` **cast to the exact function pointer
  type for each call site**. Varargs-style calls are undefined behavior on
  arm64.
- **Struct returns:** on x86_64, methods returning structs larger than 16
  bytes (e.g. `NSRect`) need `objc_msgSend_stret`. On arm64 there is no
  `_stret`. The `msg_send!` macro selects per architecture; layout tests
  cover `NSRect`/`NSPoint`/`NSSize`.
- Build classes at runtime (`objc_allocateClassPair`, `class_addMethod`,
  `class_addIvar`, `objc_registerClassPair`) for:
  - the app delegate
  - the window delegate
  - a content `NSView` subclass that returns `YES` from
    `acceptsFirstResponder` and receives key events
- Push and pop an autorelease pool around every loop iteration
  (`objc_autoreleasePoolPush` / `objc_autoreleasePoolPop`) via an RAII guard.

### Window and event loop

- Startup:
  - `NSApplication sharedApplication`
  - `setActivationPolicy: NSApplicationActivationPolicyRegular`
  - a minimal main menu (Quit item with ⌘Q)
  - `finishLaunching`
- Poll with
  `nextEventMatchingMask:NSEventMaskAny untilDate:[NSDate distantPast] inMode:NSDefaultRunLoopMode dequeue:YES`
  followed by `sendEvent:`.
- `windowShouldClose:` emits `Event::CloseRequested` and returns `NO`; the
  engine decides when to close.
- **Live resize** blocks our poll loop while the user drags. Render from the
  view's `drawRect:`/`viewWillStartLiveResize` path via the same engine
  callback used on Windows. Accept a reduced frame rate during live resize.
- HiDPI:
  - set `wantsBestResolutionOpenGLSurface = YES` (GL)
  - for Metal, set `contentsScale` on the layer
  - read `backingScaleFactor` and emit `Event::ScaleFactorChanged` from
    `windowDidChangeBackingProperties:`
- Fullscreen: native `toggleFullScreen:` (separate Space). Borderless
  fullscreen in the current Space is a later option.

### Input

- **Keyboard:**
  - `keyDown:`/`keyUp:` provide `keyCode`, a hardware virtual key code
    (`kVK_*` from HIToolbox `Events.h`), which maps to `Key`.
  - Modifiers arrive only through `flagsChanged:`; diff the flags to produce
    press and release events.
  - Text comes from `characters`. Full IME through `NSTextInputClient` is
    backlog.
- **Mouse:** `mouseMoved:`/`mouseDragged:`/`scrollWheel:`. Enable
  `acceptsMouseMovedEvents`. Flip Y: Cocoa's origin is bottom-left; ours is
  top-left.
- **Gamepads:** `GameController.framework`.
  - Observe `GCControllerDidConnectNotification` and
    `GCControllerDidDisconnectNotification`.
  - Read `extendedGamepad` element values each frame.
  - This covers Xbox, PlayStation and Switch Pro controllers on supported
    macOS versions.

### GPU

- **OpenGL (M2–M9):**
  - `NSOpenGLPixelFormat` with `NSOpenGLPFAOpenGLProfile = NSOpenGLProfileVersion3_2Core`
    (yields a 4.1 core context), `NSOpenGLPFADoubleBuffer` and
    `NSOpenGLPFAAccelerated`.
  - Load functions with `dlopen("/System/Library/Frameworks/OpenGL.framework/OpenGL")`
    + `dlsym`.
  - Vsync via `NSOpenGLContextParameterSwapInterval`.
  - OpenGL is deprecated on macOS (frozen at 4.1). Silence deprecation only at
    the binding site.
- **Metal (M10):**
  - `MTLCreateSystemDefaultDevice`.
  - A `CAMetalLayer` set as the view's layer (`wantsLayer = YES`).
  - Compile MSL at runtime with `newLibraryWithSource:options:error:`, so no
    Xcode toolchain is needed at build time.
  - Per frame: `nextDrawable` per frame, command buffer, render encoder,
    `presentDrawable:`, `commit`.
  - Vsync via `displaySyncEnabled`.

### Audio (AudioUnit)

1. `AudioComponentFindNext` with `{kAudioUnitType_Output, kAudioUnitSubType_DefaultOutput, kAudioUnitManufacturer_Apple}`.
2. `AudioComponentInstanceNew`.
3. Set `kAudioUnitProperty_StreamFormat` (input scope, element 0) to 48 kHz
   interleaved `f32` stereo.
4. Set `kAudioUnitProperty_SetRenderCallback`.
5. `AudioUnitInitialize`, then `AudioOutputUnitStart`.

The render callback runs on CoreAudio's real-time thread. It calls our mixer
directly: no locks that can block, and no allocation.

### Paths

- Config and data: `~/Library/Application Support/<Game>/`.
- Logs: `~/Library/Logs/<Game>/`.

### Release specifics

- Universal binary via `lipo -create`.
- Assemble the `.app` bundle with a hand-written `Info.plist`:
  - `CFBundleIdentifier`
  - `NSHighResolutionCapable = true`
  - `LSMinimumSystemVersion = 12.0`
- `codesign` with hardened runtime, then `notarytool submit --wait`, then
  `stapler staple`. See `docs/05-release.md`.

---

## Linux

### Window and event loop (X11 via `dlopen("libX11.so.6")`)

- Setup:
  - `XOpenDisplay`
  - choose the EGL config first, then create the window with the matching
    visual (`EGL_NATIVE_VISUAL_ID` → `XGetVisualInfo`)
  - `XCreateWindow`, `XSelectInput`, `XMapWindow`
- Window close: `XInternAtom("WM_DELETE_WINDOW")` + `XSetWMProtocols`, then
  watch for `ClientMessage`.
- Poll with `XPending` / `XNextEvent`. Handle `ConfigureNotify` (resize),
  `FocusIn`/`FocusOut`, key, button and motion events.
- **Auto-repeat:** call `XkbSetDetectableAutoRepeat(display, True, ...)` so
  held keys don't generate fake release/press pairs.
- Fullscreen: `_NET_WM_STATE_FULLSCREEN` via a `ClientMessage` to the root
  window.
- Hide the cursor with a blank 1×1 pixmap cursor (avoids an Xfixes
  dependency).
- DPI: read `Xft.dpi` from `XResourceManagerString`; default 96. Under
  XWayland, scaling depends on the compositor. Document the behavior and
  support an in-game UI scale setting.
- **Wayland:** native support is backlog (ADR-0009).
  - On Wayland sessions the game runs through XWayland.
  - If `DISPLAY` is unset and `WAYLAND_DISPLAY` is set, fail with a message
    explaining how to enable XWayland.

### Input

- **Keyboard:**
  - Key events carry X keycodes. On XKB servers these are evdev codes + 8.
    Map evdev codes to `Key` with a table: physical, layout-independent.
  - Text uses `Xutf8LookupString` with an input context (`XOpenIM` /
    `XCreateIC`). Full IME (preedit) is backlog.
- **Mouse:** `ButtonPress` 4/5/6/7 are wheel events; motion from
  `MotionNotify`.
- **Gamepads:** evdev, reading `/dev/input/event*` directly.
  - Detect gamepads with `ioctl(EVIOCGBIT)`: they have `EV_ABS` and
    `BTN_SOUTH`/`BTN_GAMEPAD`.
  - Read axis ranges with `EVIOCGABS`.
  - Read `struct input_event` records non-blocking.
  - Hotplug: `inotify` on `/dev/input`. Watch `IN_CREATE` **and**
    `IN_ATTRIB`, because udev applies permissions after creating the node.
  - Modern kernel drivers (`xpad`, `hid-playstation`, `hid-nintendo`) report
    standard gamepad codes, so the mapping table stays small.
  - Device access normally relies on the desktop seat's `uaccess` udev
    rules. If none are found, log a hint.

### GPU (EGL + OpenGL)

- `dlopen("libEGL.so.1")`.
- `eglGetPlatformDisplay(EGL_PLATFORM_X11_KHR, display)`, falling back to
  `eglGetDisplay`.
- `eglBindAPI(EGL_OPENGL_API)`.
- Choose a config with `EGL_RENDERABLE_TYPE = EGL_OPENGL_BIT`.
- Create the context with major 3, minor 3,
  `EGL_CONTEXT_OPENGL_PROFILE_MASK = EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT`.
- Load GL with `eglGetProcAddress`, falling back to `dlsym` on
  `libOpenGL.so.0`.
- Vsync with `eglSwapInterval(1)`.
- **Headless (CI):**
  - Use EGL with `EGL_MESA_platform_surfaceless` or a pbuffer, rendering into
    an FBO.
  - Golden tests run this way on Mesa llvmpipe, with
    `LIBGL_ALWAYS_SOFTWARE=1` in CI.

### Audio (ALSA via `dlopen("libasound.so.2")`)

1. `snd_pcm_open(&pcm, "default", SND_PCM_STREAM_PLAYBACK, 0)`.
2. `snd_pcm_set_params(FLOAT_LE, RW_INTERLEAVED, 2, 48000, soft_resample=1, latency_us=30_000)`.
3. The audio thread loops `snd_pcm_writei`.
4. On `-EPIPE`/`-ESTRPIPE`, call `snd_pcm_recover`.

On modern distros, `default` routes to PipeWire or PulseAudio through their
ALSA plugins. A native PipeWire backend is backlog.

### Paths

- Config: `$XDG_CONFIG_HOME/<game>/`, default `~/.config/<game>/`.
- Data/saves/logs: `$XDG_DATA_HOME/<game>/`, default `~/.local/share/<game>/`.

### Release specifics

- Build in the oldest supported distro container to fix the glibc floor.
- Ship a `.tar.gz` with the binary and the pack file. AppImage (via
  `appimagetool`) is optional.
- For Steam, target the Steam Linux Runtime container.
- Never link X11/EGL/ALSA at build time; `dlopen` with clear errors.

---

## Cross-platform invariants (tested on every OS)

- The same `Event` sequence for the same physical action: key codes and mouse
  coordinates in physical pixels, top-left origin.
- `ScaleFactorChanged` delivered before the first frame and on every monitor
  or DPI change.
- Closing the window produces `CloseRequested`. The OS window never vanishes
  without the engine agreeing.
- Focus loss releases all held keys and buttons (a synthetic release event for
  each), so nothing sticks "held".
- The audio device opens at 48 kHz stereo `f32`, or the backend converts, and
  reports the real rate to the mixer.
