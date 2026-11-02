# nadir-platform

The OS layer of the Nadir engine: a window and its events, OpenGL contexts,
the user's folders, an error dialog and a precise sleep, with one safe API
on Linux, Windows and macOS.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. It wraps the raw declarations of
[nadir-sys](https://crates.io/crates/nadir-sys). Games use it through
[nadir_engine](https://crates.io/crates/nadir_engine), which opens the
window and runs the loop with it, and re-exports its `Event` as
`nadir::Event`; depend on it directly to use it on its own, for a window and
its events without the rest of the engine.

## Status

Version 0.2: the API will change before 1.0.

Built today:

- A window (`Platform`): borderless fullscreen (native fullscreen on
  macOS), the title, hiding the pointer, size limits, and the scale factor.
- Its events, polled without waiting: close requests, resizes, scale
  factors, focus, physical keys with repeats, mouse motion, buttons and the
  wheel. When the window loses the focus, every key and button still down
  gets a release first.
- Frames while Windows drags or resizes the window, or macOS resizes it
  (`set_modal_frame_callback`).
- On Linux, an OpenGL 3.3 core context for the window, and one that draws
  offscreen with no display (`create_headless_gl`).
- `user_dirs`, the folders for settings, saves and logs; `show_error_dialog`;
  `precise_sleep`, for a frame limiter.
- With the `dev` feature, an allocation counter.

Not yet: OpenGL on Windows and macOS (roadmap tasks M2.6 and M2.7), typed
text (M7.2), gamepads (M8.1 to M8.3), audio output (M6.3 to M6.5), Metal
(M10.1), noticing that the window was minimized, holding the pointer inside
the window, and native Wayland.

## Usage

```toml
[dependencies]
nadir-platform = "0.2"
nadir-core = "0.2"
```

`nadir-core` has the `UVec2` of the window's size, the `Key` of a key
event and the log. Open a window and run until it is closed:

```rust,no_run
use std::time::{Duration, Instant};

use nadir_core::log::info;
use nadir_core::{Key, UVec2};
use nadir_platform::{Event, Platform, PlatformError, WindowConfig, precise_sleep};

fn main() -> Result<(), PlatformError> {
    let mut platform = Platform::new(&WindowConfig {
        title: "My game".to_string(),
        size: UVec2::new(640, 360),
        min_size: UVec2::new(320, 180),
        resizable: true,
        fullscreen: false,
        vsync: true,
    })?;
    let mut events = Vec::new();
    let mut next_frame = Instant::now();
    loop {
        events.clear();
        platform.pump_events(&mut events);
        for &event in &events {
            match event {
                Event::CloseRequested => return Ok(()),
                Event::Key { key: Key::Escape, pressed: true, .. } => return Ok(()),
                Event::Resized(size) => info!("{}x{} physical pixels", size.x, size.y),
                _ => {}
            }
        }
        // Run a frame here, then wait for the next one.
        next_frame += Duration::from_micros(16_667);
        precise_sleep(next_frame);
    }
}
```

Find the folders where a game keeps what it writes. Nothing is created:
make a folder before writing into it.

```rust
use nadir_platform::{PlatformError, user_dirs};

fn main() -> Result<(), PlatformError> {
    let dirs = user_dirs("My Studio", "My Game")?;
    assert!(dirs.config.ends_with("My Game"));
    assert!(dirs.data.ends_with("My Game"));
    assert!(dirs.logs.ends_with("My Game"));

    // A name must be a valid folder name on all three systems.
    let invalid = user_dirs("My Studio", "Game?");
    assert!(matches!(invalid, Err(PlatformError::InvalidName { .. })));
    Ok(())
}
```

## Features

- `dev`: the allocation counter. The global allocator of every binary that
  links this crate counts each thread's allocations and the process's, and
  `thread_alloc_counts` and `process_alloc_counts` read them. No other crate
  in the binary may declare a global allocator. Turn it on only from a
  `[features]` table or a `[dev-dependencies]` entry, so that no release
  build carries it. `nadir_engine`'s own `dev` feature turns it on.
- `headless-test` is for the workspace's own tests, which need an EGL
  driver.

## Platforms

- **Linux** (X11): all of the above works, and was checked by hand on
  XWayland. Xlib (`libX11.so.6`) and EGL (`libEGL.so.1`) are
  loaded while the program runs, not linked, so a machine without them
  gets an error that says which library or function is missing. A Wayland
  session runs the window through XWayland; without XWayland, the error
  says how to turn it on. The window draws with OpenGL 3.3 core through
  EGL, and `create_headless_gl` needs the surfaceless platform
  (`EGL_MESA_platform_surfaceless`), which Mesa and NVIDIA have. The scale
  factor is read once, from `Xft.dpi`, when the window opens.
- **Windows** 10 and later: a Win32 window, aware of each monitor's DPI,
  with scale-factor changes, borderless fullscreen and an error message
  box. It builds and passes its tests in CI, but still awaits its checks by
  hand (roadmap task M1.5). It has no OpenGL context yet (M2.6): its GPU
  surface has no functions, and swapping fails with
  `PlatformError::Unsupported`, as does `create_headless_gl`.
- **macOS** 12 and later: an AppKit window, opened on the main thread only,
  with Retina scale factors, native fullscreen and an error alert. It
  builds and passes its tests in CI, but still awaits its checks by hand
  (M1.6). It has no OpenGL context yet (M2.7), as on Windows.

On any other system, opening a window, `user_dirs` and `create_headless_gl`
return `PlatformError::Unsupported`.

## Documentation

- API reference: https://docs.rs/nadir-platform
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/sys-and-platform.md
- Each system's details: https://github.com/jabbott-iii/Nadir/blob/main/docs/03-platforms.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
