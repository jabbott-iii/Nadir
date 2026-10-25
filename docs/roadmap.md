# Roadmap

How to use this file:

- Work top to bottom. A task is **unblocked** when everything in its
  `Depends` line is checked.
- Tick a box only after its acceptance criteria are met **and** its
  verification commands were run and passed (see "Definition of done" in
  `AGENTS.md`).
- If a criterion needs an OS you can't run, leave the box unchecked. Add an
  indented note saying exactly what remains:
  `  - ⏳ needs macOS: run example events checklist`.
- Exit criteria for a milestone are checked by a human.
- OS order within every milestone: **Linux → Windows → macOS**.

---

## M0 — Foundation and guardrails

**Goal:** an empty but fully guarded workspace. The constraints are
enforced by tooling from day one.

- [x] **M0.1 — Workspace skeleton**
  - Note: toolchain pinned to `1.99.0` (b940084d7 2026-09-28), the current
    stable on 2026-10-12.
  - Note: members have no workspace dependencies or features yet; each
    later task adds the edges it needs. `cargo dev` fails until the sandbox
    declares its `dev` feature.
  - Note: `nadir-cook` is lib + bin; `xtask`, `sandbox` and
    `nadir-bench` are binaries; the rest are libraries.
  - Note: the root `.gitignore` already covered `target`. Its `debug`
    pattern also matched `crates/debug`, so `!/crates/debug/` was added.
  - Depends: —
  - Do:
    - Create every crate in the workspace map (`AGENTS.md`) as an empty
      library or binary with a crate-level doc comment.
    - Root `Cargo.toml`: `[workspace]`, `[workspace.package]` (edition 2024,
      version 0.1.0, license), and `[workspace.lints]` exactly as in
      `docs/01-architecture.md`.
    - `rust-toolchain.toml`: pin the current stable version, with
      components `rustfmt` and `clippy`. Record the version in the task
      note.
    - `clippy.toml`: `allow-unwrap-in-tests = true`,
      `allow-expect-in-tests = true`.
    - `.cargo/config.toml` with the aliases.
    - `../.gitignore` covering `target/`.
    - `#![allow(unsafe_code)]` in the `sys`, `platform` and `gfx` roots only.
  - Accept:
    - `cargo build --workspace` and
      `cargo clippy --workspace --all-targets -- -D warnings` succeed.
    - `Cargo.lock` lists only workspace packages.
  - Verify: `cargo build --workspace`;
    `cargo clippy --workspace --all-targets -- -D warnings`;
    `grep -c 'source = ' Cargo.lock` (expect 0)

- [x] **M0.2 — core: math and color**
  - Note: property tests use a test-only SplitMix64 generator
    (`crates/core/src/test_support.rs`), because `Pcg32` (M0.3) and
    `testing::check` (M0.4) do not exist yet. Port them in M0.4 and delete
    the stand-in.
  - Note: `CoreError` was created here with one variant
    (`InvalidHexColor`), since hex parsing needs an error type.
    `ErrorContext` is still to come.
  - Note: conventions chosen here: `Rect`/`IRect` store `min`/`max` and
    are half-open; `Mat3` is column-major; `Color` and `LinearColor` have
    no `Default`.
  - Depends: M0.1
  - Do: `Vec2`, `IVec2`, `UVec2`, `Mat3`, `Affine2`, `Rect`, `IRect`,
    `Color`, `LinearColor`, and the sRGB conversions, per
    `docs/subsystems/core.md`.
  - Accept:
    - Unit tests and property tests (inverse round-trip, rect intersection
      symmetry, sRGB round-trip ±1).
    - Every public item has docs.
  - Verify: `cargo test -p nadir-core`

- [x] **M0.3 — core: handles, hashing, RNG**
  - Note: `Handle<T>` is defined in `nadir-core::handle`, as this task and
    `core.md` say, with the shape sketched under `nadir-assets` in
    `assets-and-codecs.md`. M3.2 should re-export it, not redefine it.
  - Note: `Pcg32::range` is generic over `SampleRange` (`u32`, `i32` and
    `f32` ranges). An empty range returns its start.
  - Note: `Pcg32` reproduces the PCG reference demo output for seed
    `(42, 54)`; `FxLikeHasher` output is pinned by a test, because it
    decides `FastHashMap` iteration order.
  - Note: case generation still uses the M0.2 stand-in
    (`test_support.rs`); port to `testing::check` in M0.4.
  - Depends: M0.1
  - Do: `GenIndex`, `GenArena<T>`, `Handle<T>`, `fnv1a64`,
    `FxLikeHasher` + `FastHashMap`, `crc32`, `Pcg32`.
  - Accept:
    - `GenArena` model-based property test.
    - `Pcg32` matches fixed test vectors.
    - `crc32("123456789") == 0xCBF43926`.
  - Verify: `cargo test -p nadir-core`

- [x] **M0.4 — core: logging, civil time, CLI, test harness, profile scopes**
  - Note: the harness is `check` plus `check_sized` (user decision,
    2026-10-12). Only `check_sized` shrinks. `docs/04-quality.md` was
    updated to match. The M0.2/M0.3 property tests now use it, and the
    stand-in generator is gone.
  - Note: the log macros are `nadir_core::log::{error, warn, info, debug,
    trace}`. The file sink can rotate by size and on open
    (`FileConfig`); the defaults are 4 MiB, 3 copies, append. M1.4 picks
    the policy for the game.
  - Note: `profile::take_frame(&mut Vec<ScopeRecord>)` drains the calling
    thread only, and each thread keeps at most 65 536 records. Collecting
    from other threads is left to M7.3.
  - Note: the stderr sink has no automated test.
  - Note: `assert_approx!` and the golden comparison helpers from
    `core.md`'s `testing` row are not part of this task. The golden
    helpers fit M2.4 (golden harness); no task lists either explicitly.
    (M2.4 put the golden comparison in `xtask` instead, ADR-0017.)
  - Depends: M0.2, M0.3
  - Do: the `log` macros and sinks (stderr, file rotation, ring buffer),
    `civil`, `cli`, `testing::check`, and `profile_scope!` (feature-gated).
  - Accept:
    - Log rotation works (tested in a temp dir under `target/`).
    - `civil` matches known timestamps, including leap years.
    - A failing `check` prints a reproducible seed.
  - Verify: `cargo test -p nadir-core --all-features`

- [x] **M0.5 — codec: JSON/JSONC parser and writer**
  - Note: the supported subset is documented in
    `docs/subsystems/assets-and-codecs.md` ("JSON and JSONC subset").
    Numbers split into `Int(i64)` and `Float(f64)`, as KBJ needs;
    duplicate keys are an error.
  - Note: `Limits` has only the three fields JSON uses (`max_depth`,
    `max_string_bytes`, `max_entries`). Later decoders add theirs.
  - Note: `nadir-codec` depends on `nadir-core` as a dev-dependency only,
    for the property-test harness.
  - Note: the parser and writers were cross-checked against Python's
    `json` module on 6,000 random documents, in both directions, with no
    differences. That check is not part of the test suite.
  - Note: the `xtask fuzz-smoke` target and `fuzz/corpus/json/` that the
    parser rules require are M0.7's job.
  - Depends: M0.1
  - Do: `nadir_codec::json` per `docs/subsystems/assets-and-codecs.md`:
    `Value`, `parse(&str, Limits)`, `to_string_pretty`, `to_string_compact`.
  - Accept:
    - RFC 8259 examples, the JSONC extensions, and the depth limit all
      behave as specified.
    - Malformed input returns errors with line and column; it never panics.
  - Verify: `cargo test -p nadir-codec`

- [x] **M0.6 — xtask: guardrail checks**
  - Note: negative demonstrations, run in a scratch copy on 2026-10-12 and
    reverted. Each made its check exit with status 1:
    - `nadir-core` given a path dependency on `nadir-render`: `layers`
      reports the edge.
    - an `unsafe` block added to `nadir-ecs`: `unsafe-audit` reports the
      file and line.
    - `left-pad-demo = "1"` added to `nadir-ecs`: `deps` reports the
      package. crates.io was replaced by a local directory source for
      this, so nothing was downloaded; cargo still reports the crates.io
      source for the package.
    - `fmt`, `clippy`, `test` and `release-no-dev` were also made to fail
      once each.
  - Note: stricter than the one-line descriptions, and now recorded in
    `docs/04-quality.md`: `deps` also rejects a path package from outside
    the workspace and requires the exact version; `layers` counts dev and
    build dependencies and fails a workspace crate that has no row;
    `unsafe-audit` fails any `unsafe` outside the three FFI crates.
  - Note: `layers` cannot tell that `sandbox` reaches `nadir-debug` only
    behind `dev`; `release-no-dev` covers that.
  - Note: `cargo metadata` runs without `--locked` or `--offline`, as
    specified. M0.8 may want `--locked` in CI.
  - Note: run on Linux only. Nothing in `xtask` is OS-specific, but the
    Windows and macOS runs first happen in M0.8's CI.
  - Depends: M0.4, M0.5
  - Do: `xtask` subcommands `fmt`, `clippy`, `test`, `deps` (+
    `xtask/deps-allowlist.txt`, empty), `layers` (matrix as a `const`),
    `unsafe-audit`, `release-no-dev`, and `ci`. Use `nadir_core::cli` and
    `nadir_codec::json`.
  - Accept:
    - `cargo xtask ci` passes on a clean tree.
    - Each check fails when deliberately violated: add a fake path dep that
      breaks the matrix, an `unsafe` block in `ecs`, an unpinned registry
      dep in a scratch copy. Revert after demonstrating.
  - Verify: `cargo xtask ci`, plus the three negative demonstrations
    (describe the results in the summary)

- [x] **M0.7 — fuzz smoke and bench harness skeletons**
  - Note: measured on 2026-10-13 (Linux, debug build of `xtask`): 60 s on
    the `json` target ran clean over 8.7 million inputs. `json_parse`:
    median 1.5 ms, p95 1.5 ms.
  - Note: the planted-panic check ran in a scratch copy and was reverted.
    Three bugs were planted in the JSON parser: a slice that was not
    bounds-checked, a subtraction that could underflow, and a `panic!` in
    place of an error. A 20 s run reported all three, with one saved
    input each. A planted endless loop was reported as a hang after the
    10 s limit, with its input saved.
  - Note: `--secs N` is the time for each target, not for the whole run,
    and defaults to 60. `docs/04-quality.md` records this and the other
    details: seeds run first and unchanged, one finding per panic
    location, a hang ends the run, `NADIR_SEED` sets the seed.
  - Note: failing inputs go to `fuzz/crashes/<target>/`, which Git now
    ignores. Copying one into `fuzz/corpus/<target>/` makes it a
    regression test.
  - Note: not caught: the allocation counter is not wired in until M3.5,
    and a stack overflow or an aborted allocation ends the fuzzer without
    saving the input.
  - Note: the bench compares against `bench/baseline.json` when that file
    exists. None is recorded yet; M3.5 records the first.
  - Depends: M0.6
  - Do:
    - `xtask fuzz-smoke [--secs N] [--target T]` with a mutation engine,
      `catch_unwind`, a hang watchdog, and crash saving. Register `json` as
      the first target, with a seed corpus in `fuzz/corpus/json/`.
    - `nadir-bench` harness with one sample benchmark (JSON parse).
  - Accept:
    - 60 s of fuzz smoke runs clean.
    - A planted panic (temporary) is detected and saved.
    - The bench prints median/p95 and writes `target/bench/latest.json`.
  - Verify: `cargo xtask fuzz-smoke --secs 60 --target json`;
    `cargo xtask bench json`

- [x] **M0.8 — CI workflow**
  - Note: no `TODO(pin)` placeholders. The pins were looked up on
    2026-10-13 and were then the latest release of each action:
    - `actions/checkout` v7.0.1 = `3d3c42e5aac5ba805825da76410c181273ba90b1`
    - `actions/cache` v6.1.0 = `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`
  - Note: verified by parsing with Python's `yaml` module (PyYAML 6.0.1),
    by a script that checks the structure and compares each pin with the
    upstream tag, and by running the workflow's own commands in order in a
    clean clone on Linux (`cargo xtask ci` passed; 120 s of fuzz smoke ran
    clean).
  - Note: **the workflow has not run on GitHub yet**, so the Windows and
    macOS legs are untested. The first push will show whether the M0 exit
    criterion holds.
  - Note: the workflow turns `core.autocrlf` off before the checkout. In a
    CRLF checkout, which is Git's default on Windows, one test fails:
    `layers::tests::the_matrix_mirrors_the_architecture_document`.
  - Note: the Mesa and golden steps of the Linux leg belong to M2.5. There
    is no nightly workflow yet.
  - Depends: M0.6
  - Do: `.github/workflows/ci.yml` per `docs/04-quality.md`:
    - matrix of 3 OSes, `permissions: contents: read`
    - actions pinned by full SHA with the tag in a comment
    - cache keyed on the lockfile + toolchain
    - runs `cargo xtask ci`
    - plus a Linux fuzz-smoke step
  - Accept: valid YAML; no unpinned actions. If SHAs can't be looked up in
    this session, use `# TODO(pin): actions/checkout@vX` placeholders and
    list them in the summary for a human to fill in.
  - Verify: parse the YAML (e.g. with Python's `yaml` module if available,
    else visual review). State which applies.

**Exit criteria (M0):** `cargo xtask ci` is green locally and on all three
CI runners. `Cargo.lock` contains zero third-party packages.

---

## M1 — A window and events on every desktop

**Goal:** `Platform` opens a window, pumps events and runs the fixed-tick
loop on Linux, Windows and macOS. No GPU yet.

- [x] **M1.1 — sys: dynlib + Linux libc + X11 declarations**
  - Note: `Xlib` has 23 functions, chosen from the `Do` list of M1.2 and
    the fields of `WindowConfig`. Left for the task that first calls them:
    text input (`XOpenIM`, `XCreateIC`, `Xutf8LookupString`), visuals
    (`XGetVisualInfo`, M2.2), pointer grab, and minimize detection.
  - Note: the layout numbers come from the headers of libx11-dev 1.8.7,
    printed by a C program built with gcc 13.3 for x86_64 Linux. The
    assertions are compile-time and were built on Linux only.
  - Note: a throwaway script also compared every function type, constant
    value, field type and field offset with the headers, through gcc static
    assertions. All matched. The script is not in the repository.
  - Note: `dynlib!` is compiled only on Linux and in tests, until another
    OS has a table: an unused macro would fail `clippy -D warnings` there.
  - Note: the test that looks every function up in the installed
    `libX11.so.6` passes without checking anything on a machine that has
    no Xlib.
  - Depends: M0.6
  - Do: the `dynlib!` macro; `linux/libc.rs` (`dlopen`/`dlsym`/`dlerror`);
    `linux/x11.rs` with only the calls needed for M1.2. Use the
    `/add-ffi-binding` procedure.
  - Accept:
    - Layout asserts pass: e.g. `XEvent` is 192 bytes on x86_64 Linux.
    - Each declaration cites its header.
    - `dynlib!` reports every missing symbol by name.
  - Verify: `cargo test -p nadir-sys` (Linux)

- [x] **M1.2 — platform (Linux): X11 window and events**
  - Note: confirmed by hand on 2026-10-13, on XWayland under COSMIC: real
    key presses and releases (`Q`, `C`), `repeat: true` while `W` is held,
    real clicks, `C` hiding and showing the pointer, `Resized` while an
    edge is dragged, and `CloseRequested` from the close button.
  - Note: the automated checks passed in the same session. The example ran on
    2026-10-13 on XWayland under COSMIC. Real events seen: the scale
    factor first, focus in and out, pointer motion, and resizes, including
    the ones for entering and leaving fullscreen. Key, button and wheel
    events, the focus rules and the close message were sent to the window
    as synthetic events with `XSendEvent`, and came out right. `xprop`
    showed the title, `WM_PROTOCOLS`, the size limits and the fullscreen
    state as intended.
  - Note: `nadir_core::keys` is new here, because no earlier task made it:
    `Key` (118 keys, numbered by USB HID usage) and `MouseButton`.
    `GamepadButton`, `GamepadAxis` and `Modifiers` wait for their first
    user.
  - Note: the key table has no exceptions: every `Key` has an evdev code.
    A test checks that against the kernel's own HID table.
  - Note: `nadir-sys` gained `linux/evdev.rs`: the 118 `KEY_*` codes the
    table uses, compared with the kernel header through gcc static
    assertions.
  - Note: not done here, and left to the task that first needs each:
    `Event::Text`, `Event::Minimized` and `is_minimized`, pointer grab,
    and the `vsync` and `gpu` fields of `WindowConfig` (M2.2). The scale
    factor is read once, when the window opens.
  - Note: on Windows and macOS, `Platform::new` returns
    `PlatformError::Unsupported` until M1.5 and M1.6.
  - Depends: M1.1, M0.2
  - Do: following `docs/03-platforms.md`:
    - window creation, `WM_DELETE_WINDOW`, the event pump, the evdev → `Key`
      table, mouse, focus, resize
    - `Xft.dpi` scale factor, borderless fullscreen, cursor hide,
      `XkbSetDetectableAutoRepeat`
    - an `examples/events.rs` that prints every event
  - Accept: the key table covers every `Key` variant (or lists its
    exceptions); unit tests for the mapping and the DPI parsing.
  - Verify: `cargo test -p nadir-platform`;
    `cargo run -p nadir-platform --example events` (manual, on an X11 or
    XWayland session; describe the observed events)

- [x] **M1.3 — facade: App skeleton and fixed-tick loop**
  - Note: measured with `cargo run -p sandbox` on 2026-10-13 (Linux,
    XWayland, debug build): 606 ticks in 10.1 s, which is 60.00 Hz. Each
    one-second report was between 59.96 and 60.01 Hz.
  - Note: the hitch test stalls one tick for a real second, through the
    real loop. The next frame runs 15 ticks, not 60, and the lost three
    quarters of a second are never made up.
  - Note: `nadir_core::time` is new here, because no earlier task made it:
    `FixedClock`, `Tick`, `Frame`, `Stopwatch`. With `precise_sleep` in
    `nadir-platform` and the sandbox, the task touched four crates.
  - Note: systems are closures for now: `add_fixed_system(name, FnMut(&Tick))`
    and `add_frame_system(name, FnMut(&Frame))`. M4.4 gives them the world.
  - Note: `Config::frame_rate_limit` (default 60) is all that paces the
    loop until M2.2 brings vsync. The loop does not call `is_minimized`:
    the limiter also paces a hidden window.
  - Depends: M1.2, M0.4
  - Do:
    - `nadir::App` with `Config`, `run()` and `FixedClock` integration, as
      in the loop in `docs/01-architecture.md` minus the GPU
    - frame limiter via `platform::precise_sleep`
    - quit on `CloseRequested`
    - the sandbox logs its tick rate once per second
  - Accept:
    - The measured tick rate is 60 Hz ± 1% over 10 s.
    - A hitch (sleep 1 s, injected in a test) clamps instead of spiraling.
  - Verify: `cargo test -p nadir`; `cargo run -p sandbox` (observe the log)

- [x] **M1.4 — paths, error dialog, file logging**
  - Note: checked with the sandbox on 2026-10-17 (Linux, XWayland, debug
    build), with `XDG_DATA_HOME` in a scratch folder. Two runs left
    `nadir.log` and the first run's `nadir.log.1`. Without `DISPLAY`, the
    error reached the log file and stderr, then the dialog line, with exit
    code 1. An unwritable data folder and a relative `HOME` each gave a
    warning and no file, and the game went on.
  - Note: the folder names come from two new fields, `Config::studio` and
    `Config::game`, both `"Nadir"` by default; the sandbox sets `game`.
    `user_dirs` rejects a name that some target OS would refuse, on every
    OS.
  - Note: `user_dirs` creates no folder; `App::run` creates the logs folder.
    Each run starts a new `nadir.log` and keeps the three before it. If no
    log file can be opened, the game warns and goes on.
  - Note: `App::run` logs a window that fails to open and passes the error
    to `show_error_dialog`. On Linux that writes `<title>: <message>` to
    stderr.
  - Note: on Windows and macOS, `user_dirs` returns
    `PlatformError::Unsupported` until M1.5 and M1.6, so the game runs
    without a log file there.
  - Depends: M1.3
  - Do:
    - `platform::user_dirs` (Linux first, with XDG env var handling)
    - `show_error_dialog` (Linux: stderr only; the game never shells out to
      external tools at runtime)
    - wire the log file sink to the data directory
  - Accept: unit tests for path resolution with fake env vars.
  - Verify: `cargo test -p nadir-platform`

- [ ] **M1.5 — sys + platform (Windows)**
  - Note: CI, which runs `cargo xtask ci` (tests included) on
    `windows-latest`, was green on 2026-10-18, as the user reported; the
    session could not see CI itself.
  - ⏳ needs Windows: the events example checklist (`cargo run -p
    nadir-platform --example events`):
    - keys by position on a non-QWERTY layout, repeats, side buttons, the
      wheel
    - pointer positions in physical pixels from the top left
    - resizes, and the size limit of `--fixed`
    - a DPI change when the window moves to a monitor of another scale
    - focus loss releasing held keys
    - F, C and T
    - Alt alone not freezing the window, and Alt+F4 and the close button
      giving `CloseRequested`
    - "modal frames ran" after dragging the window
  - ⏳ needs Windows: `cargo run -p sandbox` ticks at 60 Hz ± 1% over 10 s
    (`precise_sleep` relies on `std::thread::sleep`, which on Windows 10
    1803+ should use a high-resolution waitable timer), and keeps ticking
    while the window is dragged.
  - Note: written on Linux on 2026-10-17, without a Windows target or SDK
    headers. A copy of the workspace with the OS `cfg`s swapped compiled the
    Windows code on Linux:
    - `cargo check`, `clippy -D warnings` and `rustdoc -D warnings` passed
      on it
    - the 31 tests that need no Windows (key table, message decoding, the
      translator, styles, sizes, UTF-16) passed there; the four that call
      Windows were skipped
    - it showed nothing about the Win32 calls themselves
  - Note: the struct layouts, constants and signatures come from
    Microsoft's documented declarations; the layout assertions were worked
    out from them, not from Windows. The scan codes follow Microsoft's
    table in "Keyboard Input Overview", read on 2026-10-17.
  - Note: the modal-loop timer runs frames through a new
    `Platform::set_modal_frame_callback`. `ModalFrame` has the size and the
    scale, and no lifetime until it can present (M2.6). The facade keeps
    its loop state in an `Rc<RefCell<EngineCore>>`, which the callback
    shares; Linux never calls it. Three crates: sys, platform, nadir.
  - Note: not done here, and left to the task that first needs each: text
    (`WM_CHAR`), `Minimized`, `ClipCursor`, Raw Input, the embedded
    manifest, and the fake Left Control that comes with Alt Gr.
  - Depends: M1.3
  - Do:
    - Win32 declarations with layout tests
    - window, message pump, DPI awareness (V2), `WM_DPICHANGED`
    - the scan-code → `Key` table, mouse, focus
    - modal-loop timer callback, borderless fullscreen
    - `user_dirs` via `SHGetKnownFolderPath`, `MessageBoxW` error dialog
  - Accept: same acceptance as M1.2 and M1.3, on Windows.
  - Verify: on Windows: `cargo test -p nadir-sys -p nadir-platform`;
    the events example checklist; the sandbox tick rate. Cross-checks
    through CI.

- [ ] **M1.6 — sys + platform (macOS)**
  - Note: CI, which runs `cargo xtask ci` (tests included) on
    `macos-latest`, was green on 2026-10-18 after the user's fix of an
    arm64-only clippy error in `sender` (commit 177d736), as the user
    reported; the session could not see CI itself. The swapped-`cfg` build
    could not have caught that error: it compiles the x86_64 path only.
  - ⏳ needs macOS: the events example checklist (`cargo run -p
    nadir-platform --example events`):
    - keys by position on a non-QWERTY layout, ⌘ held while another key
      goes up, left and right modifiers, Caps Lock
    - on an ISO keyboard, how the keys left of `1` and right of the left
      Shift come out (expected: swapped, see the notes)
    - the wheel, a trackpad, and the scroll direction with natural
      scrolling on and off
    - pointer positions in physical pixels from the top left; a click on
      the title bar not reported
    - resizes, and the size limit of `--fixed`
    - the scale factor when the window moves between a Retina and a
      standard display
    - focus loss releasing held keys
    - F (native fullscreen), C and T
    - ⌘Q, Quit from the Dock, and the close button giving `CloseRequested`
      without quitting
    - "modal frames ran" after resizing the window
  - ⏳ needs macOS: `cargo run -p sandbox` ticks at 60 Hz ± 1% over 10 s,
    on Apple Silicon at least; Intel through the CI build.
  - Note: written on Linux on 2026-10-17, without a macOS target or SDK
    headers. A copy of the workspace with the OS `cfg`s swapped compiled the
    macOS code on Linux, on its x86_64 path only (the arm64 `BOOL` and the
    plain struct return are not compiled there):
    - `cargo check`, `clippy -D warnings` and `rustdoc -D warnings` passed
      on it
    - the 25 tests that need no macOS (key table, flags, buttons, scrolling,
      the translator, paths) passed there; 18 bugs planted in them were all
      caught
    - the `nadir-sys` tests could not run there
  - Note: the layouts, constants and selectors come from Apple's documented
    declarations, not from SDK headers. `msg_send!` spells out every
    signature, and one generic cast picks `objc_msgSend_stret` for results
    over 16 bytes on x86_64.
  - Note: the pump reads key and mouse events itself, before `sendEvent:`,
    because AppKit does not pass key releases to the view while ⌘ is held.
    Live resize runs modal frames from `windowDidResize:`, not `drawRect:`.
  - Note: no key code on macOS, listed as exceptions: Print Screen, Scroll
    Lock, Pause, Menu, F21–F24. Not done: text, `Minimized`, and putting
    back the two keys that ISO keyboards swap (needs HIToolbox's keyboard
    type).
  - Note: new `PlatformError::NotMainThread`: AppKit windows only open on
    the main thread. Paths come from `HOME`, which in a sandbox is the
    container. Two crates, plus a doc line in `nadir`.
  - Depends: M1.3
  - Do:
    - the objc runtime declarations and the `msg_send!` macro with
      per-architecture `_stret`
    - runtime class building
    - `NSApplication` setup and the event poll
    - the content view subclass, `kVK` → `Key`, `flagsChanged`, Y flip
    - backing scale, the live-resize callback, the autorelease pool guard,
      the Quit menu
    - user dirs and `NSAlert` error dialog
  - Accept: same acceptance as M1.2 and M1.3, on macOS (Apple Silicon at
    least; Intel via CI build).
  - Verify: on macOS: `cargo test -p nadir-sys -p nadir-platform`;
    the events checklist; the sandbox tick rate.

**Exit criteria (M1)** — on each OS, a human runs the events example
checklist:

- Physical keys are correct on a non-QWERTY layout.
- Mouse coordinates are in physical px with a top-left origin.
- Resize is reported.
- DPI changes are reported (move the window between monitors, if available).
- Focus loss releases held keys.
- Close produces `CloseRequested`.

The sandbox ticks at 60 Hz ± 1%.

---

## M2 — GPU on every desktop

**Goal:** the `gfx` RHI on OpenGL 3.3 core everywhere, with golden-image
testing in CI.

- [x] **M2.1 — sys: GL types/enums, EGL, WGL, NSOpenGL declarations**
  - Note: `gl.rs` (shared by every system) declares 11 types, 66 constants
    and 60 function types (`PFNGL…PROC`, `extern "system"`, each with its
    GL version; 17 are GL 1.0/1.1, which Windows takes from
    `opengl32.dll`). `linux/egl.rs` declares 10 types, 41 constants and the
    `Egl` table of 18 functions, all of EGL 1.4 or earlier.
    `eglGetPlatformDisplay` (EGL 1.5) is a function type, to look up with
    `eglGetProcAddress` after a version check.
  - Note: the list is what M2.2–M2.4 will call. Four independent passes
    (EGL; GL by feature, by API method, and by pitfall) and a critic chose
    it, and a review then cut five items that no spec calls for. Deferred
    to the task that first calls them:
    - WGL to M2.6, and NSOpenGL to M2.7
    - the X11 visual declarations (`XGetVisualInfo`, `XVisualInfo`, a
      colormap) to M2.2
    - GPU timer queries to M7.3
    - the check of an sRGB default framebuffer, which no spec asks for
  - Note: generated on 2026-10-18 from the installed headers (libglvnd
    1.7.0: `glcorearb.h`, `egl.h`, `eglext.h`, `eglplatform.h`,
    `khrplatform.h`), with hand-written descriptions. The checks, which
    are not in the repository:
    - gcc static assertions, against the headers, on every type alias,
      function type, table signature and constant value, and on the type
      of the five constants whose header gives one
    - a separate comparison of all 78 signatures with the header text,
      parameter by parameter, by alias name and position, which catches a
      swapped `GLenum` and `GLuint` that gcc cannot
    - planted errors, which both checks caught
    - an adversarial review (five angles, each finding refuted twice):
      its 43 confirmed findings, all about the documentation's accuracy
      or scope, were fixed
  - Note: no struct is declared, so the only layout assertion is that an
    `EGLNativeWindowType` is the size of an X `Window`. The test loads
    `Egl` from the installed `libEGL.so.1`, checks that `eglGetProcAddress`
    returns the exported `eglGetPlatformDisplay`, and that the client
    library reports EGL 1.5 or later.
  - Depends: M1.1
  - Do: GL types and fn-pointer types (shared); `linux/egl.rs` dynlib;
    WGL and NSOpenGL declarations as they are needed.
  - Accept: layout tests; header citations.
  - Verify: `cargo test -p nadir-sys`

- [x] **M2.2 — platform (Linux): EGL context, vsync, headless GL**
  - Note: verified on 2026-10-18 on Linux under XWayland, on a machine
    whose libglvnd has both Mesa and NVIDIA EGL. The acceptance was run with
    no display server and Mesa forced:
    `env -u DISPLAY -u WAYLAND_DISPLAY
    __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
    LIBGL_ALWAYS_SOFTWARE=1 NADIR_EXPECT_GL_RENDERER=llvmpipe cargo test -p
    nadir-platform --features headless-test`. The renderer was `llvmpipe
    (LLVM 20.1.2, 256 bits)`, giving OpenGL 4.6 core. Without the vendor
    override, NVIDIA takes Mesa's surfaceless platform and ignores
    `LIBGL_ALWAYS_SOFTWARE`. The tests then pass on the GPU, and
    `NADIR_EXPECT_GL_RENDERER` is what catches it.
  - Note: the window's context, run with `--ignored`, three windows in a
    row each time, passed on Mesa iris (4.6 core), llvmpipe, and NVIDIA
    with PRIME offload (exactly 3.3 core). Without PRIME offload, NVIDIA's
    EGL takes no X display, and `Platform::new` fails with a message that
    says so. `xwininfo` showed the window at depth 24, in the logged
    TrueColor visual, with a colormap of its own.
  - Note: the API, chosen by the user, follows ADR-0015:
    - `GlContext` is an `unsafe trait`.
    - A thread has one context at a time. A second gives the new error
      `PlatformError::GlContextExists`.
    - `HeadlessGl` is neither `Send` nor `Sync` on every system.
    - `GpuSurface` is `#[non_exhaustive]`.
    - `WindowConfig` gained `vsync`, and `gpu` waits for M10.
    - Windows and macOS return a stand-in context with no functions, and
      `create_headless_gl` returns `Unsupported` there, until M2.6 and
      M2.7.

    There are four new errors: `GpuCallFailed`, `NoGlConfig`, `GlTooOld`
    and `GlContextExists`.
  - Note: `set_swap_interval` returns `true` only for an interval that the
    config allows, because EGL clamps any other one silently. llvmpipe
    allows only 1, and does not pace even then. Under XWayland (COSMIC,
    144 Hz), interval 1 held iris and NVIDIA to about 288 swaps a second.
    The facade passes `vsync: true`, but nothing swaps yet, so the M1.3
    frame limiter still paces the loop. It stays on as a cap once there is
    rendering.
  - Note: changes to `nadir-sys`:
    - Xlib gained `XGetVisualInfo`, `XFree`, `XCreateColormap` and
      `XFreeColormap`, now 27 functions, plus `XVisualInfo` with layout
      asserts from gcc.
    - Also new in Xlib: `VisualID`, `VisualIDMask`, `TrueColor`,
      `AllocNone`, `CWBorderPixel`, `CWColormap`, and offset asserts for the
      two new fields Xlib reads.
    - Removed, since nothing calls them now: `CopyFromParent`, and
      `eglQuerySurface`, which leaves 17 functions in the `Egl` table. The
      drawable size is the window size from `ConfigureNotify`.
  - Note: CI installs `libegl1` and `libgl1-mesa-dri` on Linux from here,
    and not from M2.5, because `cargo xtask ci` turns on `headless-test`.
    It runs with `LIBGL_ALWAYS_SOFTWARE=1`, and on Linux with
    `NADIR_EXPECT_GL_RENDERER=llvmpipe`. CI, on all three systems, was
    green on 2026-10-18, as the user reported; the session could not see it
    itself. The Windows and macOS code
    compiled in copies with the OS `cfg`s swapped. Clippy, rustdoc and the
    stand-in tests passed there, and so did the `compile_fail` doctests,
    which fail once the marker that keeps `HeadlessGl` on its thread is
    removed.
  - Note: an adversarial review (five angles, two refuters for each
    finding) confirmed nine findings, all about docs or tests, and all were
    fixed. One of them showed that the threaded test caught a planted
    `eglTerminate` in the headless drop only by chance. A two-thread test
    with a fixed order now catches it in 10 runs of 10. Three crates were
    touched: sys, platform and nadir.
  - Depends: M2.1, M1.2
  - Do:
    - EGL on X11 with a matching visual
    - a 3.3 core context, swap interval
    - `create_headless_gl()` via surfaceless or pbuffer
  - Accept: headless context creation works under Mesa llvmpipe.
  - Verify: `LIBGL_ALWAYS_SOFTWARE=1 NADIR_EXPECT_GL_RENDERER=llvmpipe cargo test -p nadir-platform --features headless-test`
    (or the equivalent test name). Where libglvnd also has NVIDIA's EGL,
    add `__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json`.

- [x] **M2.3 — gfx: GL backend core**
  - Note: verified on 2026-10-18 on headless contexts, each on llvmpipe
    (`llvmpipe (LLVM 20.1.2, 256 bits)`, OpenGL 4.6 core, uniform offset
    alignment 16) and NVIDIA (`3.3.0 NVIDIA 595.104.02`, alignment 256):
    - in debug and `--release`: 35 unit, 36 GL and 5 doc tests
    - on llvmpipe, 10 runs of 10
    - `cargo test -p nadir-gfx` without the feature: the unit and doc tests,
      with the GL tests not built

    The Mesa run used no display and the vendor override, as M2.2's did.
  - Note: the API follows ADR-0016, which the user chose:
    - the device holds no borrow, and checks its context's
      `GlContextId` before each call that reaches GL
    - one device per context
    - `begin_frame` and `end_frame` take the surface
    - `begin_pass`, the setters that take ids or data, and the draws return
      `Result`
    - per-call `glGetError` under `cfg(debug_assertions)`
  - Note: the backend:
    - a `gl_fns!` loader of the 60 functions, with a compile-time check of
      each name against its type
    - one safe wrapper per function, on a token made after the liveness
      check; only the two draws are `unsafe`
    - a state cache, and a VAO per pipeline whose bindings are recorded by
      buffer id
    - base-instance emulation, and a 64 KiB uniform ring
    - four sampler objects, `GL_UNPACK_ALIGNMENT` 1, and
      `GL_FRAMEBUFFER_SRGB` always on

    Draws check every read the device sets up (`check_draw`), and refuse
    if any GL call failed unseen. Shader code is trusted, and `#extension`
    is refused.
  - Note: three crates:
    - `nadir-gfx`
    - `nadir-platform`: `GlContextId`, and the claim moved to `gpu.rs`
    - `nadir-sys`: `GL_ACTIVE_UNIFORM_BLOCKS`
  - Note: an adversarial review (five angles, two refuters a finding)
    confirmed 30 findings, all fixed, each with a test. Two were soundness
    holes, reachable from safe code:
    - In release builds, refilling the uniform ring inside a draw drained
      the error of a failed attribute-pointer call, and the draw ran on a
      vertex array that was never set: a segfault on NVIDIA. Calls that
      allocate now fail on an error an earlier call left. A test plants an
      error before a refill, and fails on the old code.
    - `B` and `B[0]` named one uniform block, which left another block
      without a binding. Two names for one block are now refused.
  - Note: not run here: Windows and macOS. There the GL tests skip on
    `Unsupported`, and `Device::new` on the stand-in gives
    `MissingFunction("glGetError")`. The copies with the OS `cfg`s swapped
    passed clippy, rustdoc and the tests that can run there. The window's
    default framebuffer (`Surface` passes, swaps) is not drawn by any test
    yet. CI, on all three systems, was green on 2026-10-18, as the user
    reported; the session could not see it itself.
  - Note (from M2.2): under libglvnd, `get_proc_address` gives a stub for
    every name that starts with `gl`. A null check therefore never finds a
    missing function on Linux, so the version check is the guard, and
    nothing beyond 3.3 core may be called without an extension check. Set
    the viewport in every pass: `GL_VIEWPORT` does not follow a resize. The
    platform's context is current from creation until it is dropped
    (ADR-0015).
  - Depends: M2.2
  - Do: following `docs/subsystems/gfx.md`:
    - the `gl_fns!` loader and the `Device`
    - textures, buffers, render targets, pipelines (UBO bindings, sampler
      units, sampler objects)
    - passes, draws, instancing, scissor/viewport
    - `read_pixels`, the state cache
    - dev-mode `glGetError` checking, `Pod` for primitives
  - Accept:
    - Using a destroyed resource returns `StaleHandle`.
    - Shader errors include the driver log.
  - Verify: `LIBGL_ALWAYS_SOFTWARE=1 NADIR_EXPECT_GL_RENDERER=llvmpipe cargo test -p nadir-gfx --features headless-test`
    (headless). Where libglvnd also has NVIDIA's EGL, add
    `__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json`.

- [x] **M2.4 — codec QOI + golden harness**
  - Note: verified on 2026-10-18:
    - `cargo test -p nadir-codec`: 69 unit, 16 JSON, 3 QOI property and 2
      doc tests.
    - `cargo xtask golden` passed on llvmpipe (`llvmpipe (LLVM 20.1.2, 256
      bits)`, Mesa forced, no display) and on NVIDIA (RTX 3070, exactly
      3.3 core), which the literal Verify line picks on this machine.
      Against the llvmpipe references, NVIDIA is off by at most 1 in any
      channel (alpha_blend 17 600 pixels, textured_quad 6 444, the rest
      exact).
    - A deliberately altered shader (`.bgra` in `TEXTURED_FS`, reverted)
      failed textured_quad (24 072 pixels over the tolerance) and
      render_target_round_trip (980), with diff images that are magenta
      exactly where red and blue differ.
    - `cargo xtask fuzz-smoke --secs 60 --target qoi`: 10.3 million inputs,
      clean. `cargo xtask ci` passed.

    The references were drawn on llvmpipe and blessed by the agent, after
    it looked at each one, converted to PNG. A human should look at them
    before they are committed. CI run 8, for the M2.4 commit `8ca3d15`,
    passed on all three systems, as the user reported. The session saw
    that on the public Actions page; the logs need a sign-in.
  - Note: the user chose two things on 2026-10-18:
    - The golden comparison lives in `xtask`, not in
      `nadir_core::testing` as `core.md` had it. The scenes are tests in
      `crates/gfx/tests/headless/golden.rs`, and write their pixels as PAM
      files when `NADIR_GOLDEN_DIR` is set; xtask compares those with the
      QOI references. Three crates: codec, gfx (tests only), xtask.
      ADR-0017 records this.
    - The QOI decoder is strict. It rejects a run past the last pixel,
      chunks that run out before it, an end marker that is missing or
      wrong, and bytes after it, all of which `qoi.h` accepts. The QOI
      subset in `assets-and-codecs.md` records this.
  - Note: the codec:
    - `qoi::decode(&[u8], Limits)` gives an `Image` (its `Header`, and
      RGBA8 pixels from the top row, whatever the channel count)
    - `qoi::encode(&Header, &[u8])` writes what `qoi.h` writes
    - new errors `CodecError::Qoi { offset, kind }` and
      `CodecError::QoiEncode { kind }`
    - new limits `max_texture_dimension` (8192) and `max_decoded_bytes`
      (256 MiB)
    - the decoder refuses a file too short for its size,
      `ceil(pixels / 62) > len − 14`, before it allocates

    The byte-exact vectors were worked out by hand. A design review
    compared the encoder with its own port of `qoi.h`, written from the
    specification, on 3000 random images: byte-identical. The fuzz seeds
    were made by another port, and `qoi::encode` writes each back byte for
    byte. Neither port is the real `qoi.h`.
  - Note: `cargo xtask golden`:
    - `--bless` writes only the references of scenes that fail, and only
      on llvmpipe; the others keep their bytes.
    - Every scene of a run must come from one renderer, the one
      `NADIR_EXPECT_GL_RENDERER` names if it is set.
    - A scene saves its pixels before its own pixel checks can fail it, and
      xtask still compares and writes the images when the scene tests fail.
    - A reference that no scene drew fails the run, so a skipped GL test
      cannot pass as empty.
    - `--backend` waits for M10.4; a `<scene>.metal.qoi` is listed and
      skipped until then.
  - Note: the six scenes (320×180), each with pixel checks of its own:
    - clear, with an alpha of 200
    - textured quad: nearest at 6× with an `update_texture` marker, a
      linear gradient at 24×, and repeat sampling through an indexed draw
      from a base vertex
    - alpha blend: all four modes, over a translucent background and a
      band, checked against a model of the blend table
    - 1000 instanced quads in two draws
    - scissor (inside, clipped, empty) and viewport
    - render-target round trip

    Colors are sRGB bytes, alphas are whole bytes, and filtered channels
    stay at 64 or above. A design review measured these choices on both
    vendors: below 64, a filtered gradient differed by up to 7, and
    stacked blends hit ±2 on 880 pixels.
  - Note: an adversarial review (five angles, two refuters a finding)
    confirmed 17 findings and left 5 contested; all 22 were acted on. The
    one that mattered most: a scene that panicked before saving, or ran on
    the wrong renderer, was reported as "its scene is gone, remove its
    reference". The renderer is now checked after the scene saves, so xtask
    can say how to choose Mesa, and a scene whose test failed before saving
    is reported as such. The rest: tests (translucent chunks, the end
    marker's every byte, other sizes, the run's wiring), stricter PAM
    parsing, dotted reference names other than `.metal` now fail, and docs.
    The allocation order of the size check waits for M3.5's counter.
  - Note (from M2.3):
    - Render targets are stored from the bottom row: sampled, `v = 0` is
      clip space `y = -1`.
    - Compare sRGB within ±1 and blends within ±2, and no value half way
      between two.
    - A `Surface` pass on a headless context draws nothing, so goldens
      render into render targets.
    - `xtask` may depend only on core and codec, so the scenes render in
      gfx tests that `xtask golden` runs.
  - Note (from M2.2): where libglvnd also has NVIDIA's EGL,
    `LIBGL_ALWAYS_SOFTWARE=1` does not choose llvmpipe: NVIDIA takes the
    surfaceless platform. Choose Mesa with
    `__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json`.
    The vendors round differently: NVIDIA reads 0.5 back as 127, and Mesa
    as 128.
  - Depends: M2.3, M0.7
  - Do:
    - `nadir_codec::qoi` (encode/decode)
    - `xtask golden [--bless]`
    - golden scenes: clear, textured quad, alpha blend, 1000 instanced
      quads, scissor, RT round trip
    - references in `tests/golden/*.qoi`
    - register a QOI fuzz target
  - Accept:
    - QOI round-trips random images.
    - Goldens pass.
    - A deliberately altered shader makes the goldens fail with a diff
      image.
  - Verify: `cargo test -p nadir-codec`;
    `LIBGL_ALWAYS_SOFTWARE=1 cargo xtask golden`. Where libglvnd also has
    NVIDIA's EGL, add
    `__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
    NADIR_EXPECT_GL_RENDERER=llvmpipe`; without them the goldens run on
    NVIDIA.

- [x] **M2.5 — CI golden job (Linux)**
  - Note: the Linux leg runs `cargo xtask golden` after the checks, even
    when they fail, with `LIBGL_ALWAYS_SOFTWARE=1` and
    `NADIR_EXPECT_GL_RENDERER=llvmpipe` set on its own step.
    - If that step fails, the job uploads `target/golden/*.qoi` as the
      artifact `golden-failures`, with `actions/upload-artifact`.
    - The pin is v7.0.2, `cf430e030ddbb5b0abf93d22962f4752f3646cd9`. It
      was looked up on 2026-10-18, when it was the latest release; the
      tag points at the commit directly.
    - The checkout and cache pins were still the latest releases.
  - Note: verified on 2026-10-18:
    - The YAML was parsed with PyYAML 6.0.1. A script checked the order,
      conditions and inputs of the steps, and compared each pin with its
      upstream tag.
    - The Linux leg's commands ran in order in a clean clone (Mesa forced,
      no display): `cargo xtask ci`, `cargo xtask golden` (6 scenes) and
      120 s of fuzz smoke per target. All passed.
    - A planted `.bgra` in `TEXTURED_FS`, in that clone: the golden step
      exits 1, and the upload's pattern matches exactly the four `actual`
      and `diff` images.
    - With a reference removed, its `actual` image has the bytes of the
      committed reference.

    The `if:` conditions cannot run off GitHub. CI run 9, for the M2.5
    commit `7119361`, passed on all three systems, as the user reported.
    The session saw that on the public Actions page; the logs need a
    sign-in. On Linux the golden step runs whenever the steps before it
    pass, so CI's llvmpipe matched the references within the tolerance.
    Which Ubuntu image the job ran on is in the logs.
  - Note: an adversarial review (three angles, two refuters a finding)
    confirmed four findings and contested one; all five are fixed. The one
    that mattered: `cargo xtask ci` draws the same scenes, so a scene whose
    own pixel checks failed (the planted `.bgra` above, say) stopped the
    job at the checks, and the golden step never ran to upload its images.
    It now runs after failed checks too. The rest corrected the docs:
    - the GL tests do not skip on every context failure (below)
    - a runner log was cited for what came from a developer's machine
    - which DLLs each Mesa build needs
    - the macOS GL points are scoped to the arm64 images
  - Note: the runner findings are in `docs/04-quality.md`, under "GPU
    availability on hosted runners".
    - Windows runners have only GDI Generic 1.1.
    - macOS runners have Apple's software renderer for GL, and Metal on a
      paravirtual GPU.
    - So goldens run in CI on Linux only.

    The findings come from docs and other projects' reports, not from a
    job of ours. A research pass made 51 claims, and two skeptics checked
    each one. Five were refuted as overstated or out of date; their
    corrections are what the doc says.
  - Note: CI draws on Ubuntu's Mesa, not the 26.1.6 the references were
    drawn on: 25.2.8 on Ubuntu 24.04, and 26.0.8 on 26.04.
    `ubuntu-latest` moves to 26.04 between 2026-10-19 and 2026-11-19. If a
    run fails on the tolerance, the artifact holds what CI drew. Blessing
    from it needs the user's agreement, as any re-bless does.
  - Note (from M2.2): CI already installs EGL and Mesa (`libegl1`,
    `libgl1-mesa-dri`), with `LIBGL_ALWAYS_SOFTWARE=1` on `cargo xtask
    ci`. What is left here is the golden step.
  - Note (from M2.4): the step needs its own `LIBGL_ALWAYS_SOFTWARE: "1"`
    and `NADIR_EXPECT_GL_RENDERER: llvmpipe`, because `ci.yml` sets them
    on the checks step only. On failure, upload `target/golden/*.qoi` (the
    `actual` and `diff` images) with `actions/upload-artifact`, pinned by
    SHA.
  - Depends: M2.4, M0.8
  - Do: install Mesa and run `cargo xtask golden` in the Linux CI job.
    Investigate whether the hosted Windows and macOS runners expose usable
    GL, and record the findings in `docs/04-quality.md`.
  - Accept: the workflow is updated; findings are recorded.
  - Verify: workflow YAML review; CI results, if visible to the session

- [ ] **M2.6 — platform (Windows): WGL context**
  - Note (from M2.3):
    - The context takes `gpu::Claim` and overrides
      `GlContext::context_id`, or gfx refuses it with `NoContext`
      (ADR-0016).
    - `ModalFrame` gets a `GpuSurface`, so that the callback can call
      `begin_frame` and `end_frame`.
    - Until then, `Device::new` on the stand-in fails with
      `MissingFunction`, and the gfx GL tests skip.
  - Note (from M2.5): the hosted Windows runners have only GDI Generic 1.1,
    with no `WGL_ARB_create_context` (`docs/04-quality.md`).
    - The context must fail there, not fall back to a legacy one: check
      that `wglCreateContextAttribsARB` exists and that `GL_VERSION` is at
      least 3.3.
    - The gfx GL tests skip only on `Unsupported` with
      `NADIR_EXPECT_GL_RENDERER` empty (`tests/headless/common.rs`,
      `src/tests.rs`). Any other failure fails them, and so the Windows
      leg of CI. Widen the skip, or bring llvmpipe.
    - Mesa's llvmpipe could run them there, as third-party DLLs beside the
      test binary. That is the user's decision.
    - Mesa's WGL reports every pixel format as fully accelerated, llvmpipe
      included, so only `GL_RENDERER` tells it from a GPU.
  - Note (from M3.2): two more tests decide whether to skip:
    `headless_device` in `crates/gfx/tests/headless/offscreen.rs` and
    `device` in `crates/render/tests/headless/common.rs`, which `Stage::new`
    and so every render golden scene use. They skip only on
    `GfxError::NoOffscreenContext`, and render cannot name `PlatformError`,
    so widening them means matching a `GfxError`, such as
    `OffscreenContext(_)`. The same holds for M2.7.
  - Note (from M3.4): the sandbox draws now. Without this context,
    `App::run` logs "no GPU device" and runs the game without drawing.
    - Once it works, check `cargo dev` here: M3's exit criterion is a
      character that moves smoothly on all three OSes.
    - Modal frames call `EngineCore::frame(&[], None)`
      (`crates/engine/src/app.rs`). Pass the `ModalFrame`'s surface and the
      window size there, so that a title-bar drag draws.
  - Note (from M3.5): the facade's `steady_frames` test runs without
    drawing where there is no offscreen context. Once this one exists, it
    draws, and it fails rather than skips on a context that fails another
    way, as the gfx and render tests do.
  - Depends: M2.3, M1.5
  - Do: the dummy-window dance, `wglChoosePixelFormatARB`, a 3.3 core
    context, `wglSwapIntervalEXT`, GL 1.1 symbols from `opengl32.dll`.
  - Accept: the golden scenes render correctly on Windows hardware
    (`cargo xtask golden` locally).
  - Verify: on Windows: `cargo xtask golden`

- [ ] **M2.7 — platform (macOS): NSOpenGL context**
  - Note (from M2.3): as for M2.6: take `gpu::Claim`, override
    `context_id`, and give `ModalFrame` a surface.
  - Note (from M2.4), for M2.6 too: `cargo xtask golden` compares with the
    llvmpipe references, and `--bless` refuses any other renderer. Until
    the context exists, the scenes skip and the run fails with every
    reference "not drawn".
  - Note (from M2.5): on the hosted arm64 macOS runners, GL is only
    Apple's software renderer, which refuses a pixel format that requires
    `NSOpenGLPFAAccelerated` (`docs/04-quality.md`).
    `docs/03-platforms.md` asks for that attribute.
    - With it, the context fails there. The gfx GL tests skip only on
      `Unsupported` (see M2.6), so unless the skip is widened, the macOS
      leg of CI fails.
    - Without it, they run on a renderer whose pixels are not llvmpipe's,
      and whose rendering has known defects.
    - Choose one, and record it.
  - Note (from M3.4): the sandbox draws now. Without this context,
    `App::run` logs "no GPU device" and runs the game without drawing.
    - Once it works, check `cargo dev` here: M3's exit criterion is a
      character that moves smoothly on all three OSes.
    - Modal frames call `EngineCore::frame(&[], None)`
      (`crates/engine/src/app.rs`). Pass the `ModalFrame`'s surface and the
      window size there, so that a live resize draws.
  - Note (from M3.5): the facade's `steady_frames` test runs without
    drawing where there is no offscreen context. Once this one exists, it
    draws, and it fails rather than skips on a context that fails another
    way, as the gfx and render tests do.
  - Depends: M2.3, M1.6
  - Do: the pixel format, a 3.2-core-profile (4.1) context, Retina backing,
    swap interval, `dlopen` of the OpenGL framework.
  - Accept: golden scenes render correctly on macOS (`cargo xtask golden`
    locally).
  - Verify: on macOS: `cargo xtask golden`

**Exit criteria (M2):**

- The sandbox shows a textured, alpha-blended quad on all three OSes, with
  vsync on and off.
- Window resize keeps the image correct.
- Goldens pass in Linux CI.

---

## M3 — Sprites, camera, actions: a moving character

**Goal:** the first playable pixel: a character moved by actions, rendered
by the batched sprite renderer.

- [x] **M3.1 — input crate**
  - Note: the user chose three things on 2026-10-19:
    - Text mode is on the input side only. The platform has no
      `Event::Text` yet; it comes with the first text field (M7.2).
    - Keyboard and mouse bindings, with rebind conflict detection. The
      `begin_capture` flow waits for M9.2, and pad bindings and deadzones
      for M8.4, whose Accept already lists the deadzone property test.
    - Replays are fixed records, and live axes are rounded when latched,
      so that a replay is bit-identical to the live run.
  - Note: the crate, in `docs/subsystems/input.md`:
    - `ActionSet` carries each action's name, kind and `ui_allowed`, which
      the spec had in an `ActionDef`. The map holds only bindings: four
      slots an action.
    - `Input` takes events and `set_text_mode`, and `latch(&map, &mut
      state)` makes the tick's `ActionState`.
    - `InputRecorder` and `InputPlayer` write and read format version 1.
    - One error enum, `InputError`. One crate: `nadir-input`, which now
      depends on core and platform.
  - Note: the latch keeps the changes between two ticks in order, not the
    spec's counts per button. With one binding the result is the spec's
    `ButtonTrack`, and a property test checks that against the model. With
    several, an action goes down with the first input and up with the last.
  - Note: the replay reader has no fuzz target: `xtask` may depend only on
    core and codec. Property tests stand in: a replay cut inside its header
    or a tick fails where it ends, one cut at the end of a tick plays the
    ticks before the cut, and damaged ones never panic.
  - Note: verified on 2026-10-19: `cargo test -p nadir-input` gave 42 unit,
    5 property and 2 doc tests, all passing. Four planted bugs each failed
    the property tests:
    - a tap not held for its tick
    - the recorder dropping the just-released bit
    - edges counted per input, not per action
    - a button's value taken from any binding pressed during the tick

    `cargo xtask ci` passed, and rustdoc with `-D warnings` was clean. CI
    run 10, for the M3.1 commit `3e3e885`, passed on all three systems, as
    the user reported; the session saw it on the public Actions page.
  - Note: a design critique (three lenses, two refuters a point) confirmed
    two defects before any code was written:
    - With two bindings, a button's live value could be 1 while it was not
      held. The replay derives the value from held, so a replay would have
      differed from the live run. A button's value now follows held.
    - With no tick count in the format, "every prefix fails" could not
      hold. It now reads: a cut inside a tick fails, and a cut at the end
      of one plays the ticks before it.

    From the contested points, directions are shortened to length 1 after
    rounding, and the recorder is typed by the set. The adversarial review
    (three angles, two refuters a finding) then confirmed three findings,
    all about docs: the map's "strongest binding" wording, the player's
    "anything latching cannot make" claim, and a mouse binding hidden by
    text mode. All are fixed. The refuters of the third angle did not run
    (a session limit), so its six findings were checked by hand: all held,
    and all are fixed, with two new unit tests. A direction from a key
    tapped while the action is released was refuted twice as the
    documented rule, and is now spelled out.
  - Depends: M1.3, M0.2
  - Do: device state, edge latching, `ActionMap`/`ActionState`, composite
    WASD, text mode, recorder/player (format only), per
    `docs/subsystems/input.md`.
  - Accept: every test listed in the input spec passes.
  - Verify: `cargo test -p nadir-input`

- [x] **M3.2 — minimal assets: handles + synchronous loose loading**
  - Note: the user chose on 2026-10-19 that placeholders are a runtime
    setting of the server (`OnFailure::Placeholder` or `Nothing`), not a
    cargo feature. The facade picks it under its own `dev` feature, and
    assets gets a `dev` feature when something must be compiled out
    (M5.7/M5.8).
  - Note: three crates:
    - `nadir-assets`:
      - `Handle` (core's), `Assets<T>`, `LoadState`, and the `Asset`
        trait with `decode`, `finalize` and `placeholder`
      - `DecodeCtx`, and `FinalizeCtx`: up to four resources by type,
        with no allocation
      - `AssetError`, and `AssetServer`
    - `nadir-render`: the `Texture` asset, a QOI on the GPU, with a
      magenta and black 8×8 checker as its placeholder
    - `nadir-gfx`: `Device::new_headless`, `begin_offscreen_frame`,
      `end_offscreen_frame`, and `GfxError::NoOffscreenContext` and
      `OffscreenContext`
  - Note: `load` reads and decodes at once, and the asset is made by the
    next `finish_completed_loads`, as it will be once workers decode
    (M5.3). The same path gives the same handle in every state.
  - Note: paths are ASCII-lowercased, as `.claude/rules/parsers.md` asks,
    ahead of M5.3's normalization. They are then checked as one byte
    grammar on every system: no drive prefix (`:`), no `\`, no empty, `.`
    or `..` part, no Windows device name, no control character.
  - Note: verified on 2026-10-19:
    - `cargo test -p nadir-assets -p nadir-render` (the Verify line, plain)
      passed: assets 8 unit, 8 integration and 1 doc test; render 3 unit
      and 1 integration test.
    - The GPU half of the line also passed: gfx 36 unit and 45 GL tests,
      and render's 3 GL tests. The run used llvmpipe with Mesa forced and no
      display. Render's GL tests and the gfx offscreen tests also passed on
      NVIDIA.
    - Render's GL tests draw two textures and read them back: a loaded 2×2
      texture with a color in each corner, the right way up, and a missing
      file's placeholder, magenta and black texel for texel.
    - Three planted bugs each failed the tests: no dedup by path, the
      placeholder policy ignored, and the checker inverted.
    - `cargo xtask ci` passed, and rustdoc with `-D warnings` was clean.
      CI run 11, for the M3.2 commit `89de9cd`, passed on all three
      systems, as the user reported; the session saw it on the public
      Actions page.
  - Note: a design critique (three lenses, two refuters a point) confirmed
    five defects before any code was written:
    - A headless device could not run a frame, since `begin_frame` needs a
      surface that render cannot name. Hence the offscreen frames.
    - `FinalizeCtx` allocated every frame. It is now a fixed array, and
      there is `has_pending()`.
    - A decode failure that is not a codec's had no variant. Hence
      `AssetError::Invalid`.
    - The Verify line did not build render's GL tests.
    - The path check let `C:x.qoi` replace the root on Windows.

    From the contested points, a failed offscreen context carries the
    platform's error.
  - Note: the adversarial review (three angles, two refuters a finding)
    confirmed seven findings and contested two; all nine are fixed:
    - the device-name check missed `con .qoi`, `CONIN$` and the
      superscript ports
    - "`None` for a handle from another server" was false, and its test
      empty; handles are plain ids
    - `Assets::handle` did not lowercase
    - `new_headless`'s doc gave the wrong error for an old driver
    - the M2.6/M2.7 notes missed the new skip sites
    - gfx.md said assets could use `new_headless`
    - a loaded texture's pixels were not read back
    - stale wording in the M5.3 note
  - Note (from M2.3): render and assets may not depend on platform, even
    for tests, so their GPU tests need a `Device::new_headless` that owns
    its context.
  - Depends: M0.3, M2.4
  - Do:
    - `Handle<T>` and `Assets<T>`
    - an `AssetServer` with synchronous loading from loose files
    - the `Asset` trait with `decode`/`finalize`
    - a `Texture` asset in `render` (QOI → GPU)
  - Accept: loading the same path twice returns the same handle; a missing
    file shows the placeholder in dev.
  - Verify: `cargo test -p nadir-assets -p nadir-render`; for the GPU
    tests, `LIBGL_ALWAYS_SOFTWARE=1 NADIR_EXPECT_GL_RENDERER=llvmpipe cargo
    test -p nadir-gfx -p nadir-assets -p nadir-render --features
    nadir-gfx/headless-test,nadir-render/headless-test`. Where libglvnd also
    has NVIDIA's EGL, add
    `__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json`.

- [x] **M3.3 — render: sprite batcher, sort keys, camera, upscale**
  - Note: the user chose two things on 2026-10-19, recorded as ADR-0018:
    - Pixel-perfect mode snaps each unrotated sprite's corner to a whole
      virtual pixel, not only the camera.
    - It rounds the zoom to a whole number, at least 1.
  - Note: two crates, render and xtask (the golden scene list), plus four
    new references in `tests/golden/`:
    - `Renderer` with `render(device, assets, frame, output)`, `upscale`,
      `screen_to_world` and `world_to_screen`
    - `RenderFrame` (camera, clear, sprites), `SpriteDraw`, `Camera2D`,
      `SortMode`, `Flip`, `Output`, `FrameStats` and `RenderError`
    - the `sprite` and `blit` GLSL, in `crates/render/shaders`, with a
      parity test
    - render's GL tests moved to `tests/headless/`, as gfx's are
  - Note: departures from `render.md`, which now records them:
    - Ties sort by the full submission index, not `seq`. The design
      critique showed that `seq` swaps two tied sprites submitted at 255
      and 256, so tied decals would flicker.
    - The view's top-left corner snaps, with halves rounding up, not the
      centre: a camera step across zero would otherwise move the view by
      two pixels.
    - The unit quad's corner is location 6, after the spec's six instance
      attributes.
    - The blit's clip-space corners and uv come from Rust (`BLIT_QUAD`),
      as `.claude/rules/shaders.md` asks.
    - That rule also asks for both sources of a program in one task. Only
      the GLSL is written; the `.metal` sources are M10.3's, as the
      roadmap schedules them.
  - Note: verified on 2026-10-20:
    - `cargo test -p nadir-render`: 29 unit tests, including the sort-key
      property test and the batching tests.
    - With `headless-test` on llvmpipe (Mesa forced, no display): 17 GL
      tests, among them order by feet, batches, snapping, placeholders,
      zoom, shake, rotation, tint and buffer growth.
    - `cargo xtask golden`: 10 scenes ok on llvmpipe and on NVIDIA, the 4
      new ones among them.

    The new references were drawn on llvmpipe and blessed by the agent,
    after it looked at each one as a PNG. A human should look at them
    before they are committed. CI run 12, for the M3.3 commit `18d2660`,
    passed on all three systems, as the user reported; the session saw it
    on the public Actions page. Its golden step drew the new scenes on
    Ubuntu's Mesa, within the tolerance of the references.
  - Note: nine planted bugs each failed the tests:
    - sorting by the top
    - an ignored flip
    - a factor taken from the width alone
    - an upside-down blit
    - the zoom left out of the uniform
    - an unrounded zoom
    - no buffer growth
    - an unconverted tint
    - counter-clockwise rotation

    `cargo xtask ci` passed, and rustdoc with `-D warnings` was clean.
  - Note: a design critique (three lenses, two refuters a point) confirmed
    nine findings, all taken:
    - unsnapped sprite corners, ADR-0018 after the user's choice
    - the `seq` tie flicker
    - halves rounding away from zero at the camera
    - a planner that needed a GPU to test
    - golden sprites at non-whole texel scales
    - upscale sizes that could not catch a width-only factor, and edges
      the scenes could not show

    The adversarial review (three angles, two refuters a finding) confirmed
    four findings and contested four; all eight are fixed:
    - `skipped` and the placeholder rule under `OnFailure::Nothing`
    - the architecture doc's `render` call
    - stale test paths
    - the fragment shaders' alpha comments
    - tests for zoom, buffer growth, tint and rotation, which four of the
      planted bugs had survived before
  - Note (from M2.3):
    - Each pass call returns a `Result`.
    - Uniforms are std140 blocks of `[f32; 4 * n]`, set again in each
      pass.
    - Shaders read instances through attributes, never `gl_InstanceID`.
    - The blit samples a render target bottom-up on GL.
  - Note (from M3.2):
    - Render's GPU tests make their device with `Device::new_headless`, and
      run frames with `begin_offscreen_frame` and `end_offscreen_frame`.
      `crates/render/tests/headless/textures.rs` draws a texture that way.
    - A sprite whose texture failed gets its placeholder from
      `AssetServer::get`, an 8×8 checker. A sprite's `uv` names a small
      region of an atlas page, which would show one texel of it: draw a
      sprite whose texture is `LoadState::Failed` with `uv` 0 to 1.
  - Note (from M2.4): render's golden scenes are tests too (ADR-0017). Add
    their `cargo test` command to `SCENES` in `xtask/src/golden.rs`, write
    PAM as `crates/gfx/tests/headless/golden.rs` does, and define them the
    same way, so that each saves before its checks.
  - Depends: M3.2
  - Do: following `docs/subsystems/render.md`:
    - the `sprite` and `blit` shaders, with an interface parity test
    - instance packing, sort keys, batching
    - `Camera2D`, pixel-perfect virtual resolution, letterboxed integer
      upscale, `screen_to_world`
  - Accept:
    - Sort-key property test; batching tests.
    - Goldens: y-sort scene, upscale at 3 window sizes.
  - Verify: `cargo test -p nadir-render`; `cargo xtask golden`. For the GPU
    tests and the goldens on llvmpipe, as for M3.2: `LIBGL_ALWAYS_SOFTWARE=1
    NADIR_EXPECT_GL_RENDERER=llvmpipe`, plus the Mesa vendor file where
    libglvnd also has NVIDIA's EGL, and `--features headless-test` for
    `cargo test`.

- [x] **M3.4 — facade + sandbox: moving character**
  - Depends: M3.1, M3.3
  - Note (from M3.3):
    - Make the `Renderer` once with the game's virtual resolution.
    - Inside each device frame, call `render(&mut gfx, &assets, &frame,
      Output { target: PassTarget::Surface, size })`, with `size` from
      `Platform::window_size()`: gfx does not expose the drawable size.
    - Keep one `RenderFrame`, and `clear()` it each frame.
    - Sprites snap to whole virtual pixels (ADR-0018), so motion is in
      whole pixels. Interpolation still matters, for when each step lands.
  - Note (from M3.2): make the `AssetServer` with
    `OnFailure::Placeholder` under the facade's `dev` feature and
    `OnFailure::Nothing` without it. Once a frame, when `has_pending()`,
    call `finish_completed_loads(&mut FinalizeCtx::new().with(&mut gfx))`.
  - Note (from M3.1): feed every event to `Input::handle`, and call
    `Input::latch` once for each tick, before the fixed systems. The first
    tick of a frame sees that frame's events, and the frame's other ticks
    see none: a held key stays held, and a tap counts once. `latch` starts
    the next tick, so a reader per frame needs an `Input` of its own.
  - Do:
    - the `App` integrates input latching, interpolation (prev/current
      positions) and render submission
    - the sandbox: a character sprite moved with the `Move` action,
      following camera, a few static props
    - plain structs for now; the ECS arrives in M4
  - Accept: movement is smooth at 60, 144 and uncapped fps (interpolation
    works); a tap between ticks moves the character exactly one tick.
  - Verify: `cargo test -p nadir`; `cargo dev` (manual)
  - Note: built on a typed resource map, `Resources`, until the ECS
    (ADR-0019).
    - Systems are `FnMut(&Resources, &Tick)` and
      `FnMut(&Resources, &Frame)`.
    - The engine provides `Input`, `AssetServer`, `RenderFrame`,
      `FrameStats`, and `ActionMap<A>` with `ActionState<A>` from
      `App::add_actions` (one set per app).
    - With no GPU device, the game runs without drawing.
  - Note: the sandbox takes `--fps <n>` (0 means no limit; 60 by default)
    and `--no-vsync`, and refuses any other argument. Its sprites are in
    the root `assets/sprites/`. The person walks 2 px a tick (120 px/s),
    so in pixel-perfect mode (ADR-0018) each tick's step is whole pixels.
  - Note: `cargo dev` on Linux (X11, Mesa 26.1.6, Intel UHD Graphics TGL
    GT1), 7 s at each setting. Every frame drew 203 sprites in 7 batches,
    with 0 not ready and no warnings or errors in the log.
    - `--fps 60`: 60.0 frames and 60.00 ticks a second
    - `--fps 144 --no-vsync`: 144.0 frames and 59.6 to 60.6 ticks a second
    - `--fps 0 --no-vsync`: 3,600 to 3,900 frames and 60.00 ticks a second
  - Note: each test that proves the acceptance was checked against a
    planted bug, then reverted. The tap test fails when only a frame's
    first tick latches. The interpolation test fails when frames draw
    `current` instead of the lerp. The render-frame test fails when the
    frame is not emptied. The wheel test fails when the loop latches only
    for a game's own action set.
  - Note: the user checked the movement on 2026-10-23, on the same Linux
    machine. `cargo dev --fps 60`, `cargo dev --fps 144 --no-vsync` and
    `cargo dev --fps 0 --no-vsync` were all smooth, and WASD and the arrow
    keys both moved the person. The runs logged 60.0, 144.0 and 3,400 to
    3,800 frames a second, 60 ticks a second, and 203 sprites in 7 or 8
    batches (the person's place in the y-sort splits a batch) with 0 not
    ready. Pass the sandbox's options after `cargo dev` with no second
    `--`: the alias ends in one, and the sandbox refuses the extra one.
    A press and release within one tick moving exactly one tick is shown
    by `a_tap_between_ticks_moves_exactly_one_tick`, not by eye.
  - Note: CI run 13, for the M3.4 commit `3a43a01`, passed on all three
    systems, as the user reported; the session saw it on the public
    Actions page.

- [x] **M3.5 — allocation counter + sprite stress benchmark**
  - Note (from M3.3):
    - `Renderer::render` reuses its plan and instance buffers. The buffer
      grows by doubling, so a steady frame should allocate nothing; this
      task's counter will tell.
    - The planner (`crates/render/src/plan.rs`) is generic over the
      texture, so the sort + batch bench can run without a GPU. It is
      crate-private: expose it for `nadir-bench`.
    - Each sprite's tint goes through `Color::to_linear`, which is three
      `powf` calls. A table or a white fast path may be needed for the 2 ms
      budget.
  - Note (from M2.4): with the counter in place, a test can show that
    `qoi::decode` of a file too short for its size (a 23-byte file that
    claims 8192×8192) allocates nothing. Today
    `a_file_too_short_for_its_size_is_rejected_before_decoding` checks the
    error only, and moving the allocation before the check would pass it.
  - Note (from M3.4):
    - The facade's frame now draws: `Graphics::draw` in
      `crates/engine/src/app.rs` renders, then calls
      `finish_completed_loads` while loads are pending.
    - The sandbox's `report` system logs a line once a second, so 100
      frames at 60 fps include one report.
  - Depends: M3.4, M0.7
  - Do:
    - the dev `#[global_allocator]` counter
    - a 10k-sprite stress scene in the sandbox (`--scene stress-sprites`)
    - a bench for sort + batch
    - a test asserting 0 allocations over 100 steady-state frames
  - Accept: 0 allocations/frame; bench results recorded in
    `bench/baseline.json`.
  - Verify: `cargo test -p nadir --features dev`; `cargo xtask bench sprite`
  - Note: the user decided on 2026-10-23 to keep this one task across five
    crates (platform, nadir, render, bench, sandbox). They also chose one
    new public render type for the bench, and a facade test with a scene
    of its own.
  - Note: built:
    - `nadir-platform`'s `dev` feature installs the counting global
      allocator (`alloc_counter.rs`): allocations, reallocations included,
      deallocations and bytes, per thread and per process.
    - The facade's `dev` turns it on and keeps `FrameAllocs`, the main
      thread's count from the end of one frame to the end of the next.
    - render's `SpriteBatcher<K>` does the sort, the batches and the
      instance packing for the `Renderer`, and for the bench without a GPU.
      Its `build` takes a closure that returns `Option<(K, bool)>`, so
      that it is the one new public type.
    - The bench `sprite_sort_batch` times one frame shaped like the stress
      scene.
    - The sandbox's `--scene stress-sprites` adds 10,000 people walking
      about in view, all drawn from `assets/sprites/atlas.qoi`, as are the
      props and the player. The camera stays on the view's centre.
  - Note: measured on 2026-10-23 on an i7-11800H.
    - `steady_frames`: over 100 counted frames of 10,192 sprites in 2
      batches through the real loop, 0 allocations and 0 deallocations.
      This held on llvmpipe, on the NVIDIA RTX 3070, and with no context.
    - `bench/baseline.json`, from a full run: `sprite_sort_batch` median
      738 µs (p95 825 µs), `json_parse` 1.6 ms. render.md's 2 ms budget
      also covers the upload, and is for min-spec, not this machine.
    - The stress scene on X11 (Mesa 26.1.6, Intel UHD Graphics TGL GT1)
      drew 10,203 sprites in 2 batches, with 0 allocations in every second
      after the first. The first second's 73 are loading and growth.
      - debug, uncapped: about 119 frames a second
      - debug, `--fps 60` with vsync: 60.0
      - release, uncapped: 680 to 726
  - Note: each of these planted bugs failed its test, and was reverted:
    - the batcher making a new instance list each frame
    - `FrameAllocs` never written, or ended before the draw
    - an allocation in `App::frame`
    - an allocation in the crowd's draw
    - `qoi::decode` allocating before its size check (the M2.4 note's
      test, in `crates/engine/tests/allocations.rs`)
  - Note: the counter sees only Rust's global allocator: not what Mesa or
    Xlib allocate themselves. The fuzzer does not use it, because `xtask`
    may not link `nadir-platform` (`docs/04-quality.md`). Release builds
    stay free of it by a rule, `dev` only from `[features]` or
    dev-dependencies (`docs/01-architecture.md`); `release-no-dev` does
    not check this.
  - Note: CI run 14, for the M3.5 commit `34e0ea8`, passed on all three
    systems, as the user reported; the session saw it on the public
    Actions page.

**Exit criteria (M3):**

- The character moves smoothly on all three OSes.
- The 10k-sprite stress scene runs at ≥ 60 fps on min-spec, with ≤ 30 draw
  calls and 0 allocations per frame.

---

## M4 — Simulation: ECS, reflection, collision, tilemaps

- [x] **M4.1 — ecs crate**
  - Depends: M0.3
  - Do: everything in `docs/subsystems/ecs.md`:
    - entities, `SparseSet`, `World`, `Read`/`Write`, resources
    - commands, events, schedule with stages and run conditions
    - borrow-panic messages with the system name
  - Accept: the model-based property test, the borrow-conflict test and
    the events test pass; benchmarks are recorded.
  - Verify: `cargo test -p nadir-ecs`; `cargo xtask bench ecs`
  - Note: built as `docs/subsystems/ecs.md` describes. The spec now
    records where the code settles what it left open:
    - `Entity` wraps a `GenIndex` and has no public constructor.
    - A stale entity's insert never touches its slot's newer occupant.
    - `Component` is an explicit marker (ADR-0014: the facade defines the
      components).
    - `Read`, `Write`, `Res` and `ResMut` are aliases of `Ref` and `RefMut`.
    - Commands queue small ops, with each insert's value in its own type's
      queue: nothing is boxed.
    - `World::add_event` records how the schedule clears each event type.
    - `Schedule::run_fixed` and `run_frame` take `&mut World`, apply the
      commands and clear the events.
    - A guard restores the current system's name, also on a panic.
    - `docs/01-architecture.md`'s loop is updated to match.
  - Note: tests: 19 unit tests and 3 doc tests.
    - The model-based property test runs 300 random histories of up to 300
      steps against a `BTreeMap` model, stale entities and queued commands
      included.
    - The borrow-conflict test checks the start of each message, the
      system's name and the type's, for components, resources and an
      unregistered type.
    - The events test runs frames of 3, then 0, then 1 ticks, and compares
      exact lists.
    - Each of these planted bugs failed a test, and was reverted: the
      swap-remove fix-up left out, despawn leaving components, removes
      applied before inserts, frame events cleared at the next tick, the
      command queue or the event buffers freed instead of cleared, and
      `add_event` skipping an `Events<T>` inserted by hand.
  - Note: `bench/baseline.json`, from a full run on 2026-10-24 (i7-11800H):
    `ecs_join2_5k` 8.2 µs, `ecs_join3_5k` 14.3 µs, `ecs_spawn_despawn`
    (100 entities with 2 components, through commands) 7.4 µs. The run
    re-recorded `json_parse` (1.58 ms, 2.1% faster than the earlier
    baseline) and `sprite_sort_batch` (744 µs, 0.9% slower).
  - Note: `nadir-ecs` cannot count allocations: it may depend only on
    `nadir-core`. So a test checks only that, under a steady stream of
    commands and events, the command queue, the storages and the event
    buffers keep the same non-zero capacities. The facade's count covers
    the rest in M4.4.
  - Note: CI run 15, for the M4.1 commit `9a282c8`, passed on all three
    systems, as the user reported; the session saw it on the public
    Actions page.

- [x] **M4.2 — scene: reflection**
  - Note (from M4.1): the ecs world keeps only names and storage
    operations per component type. The reflection hooks belong to this
    crate's `TypeRegistry`. `Component` is an explicit marker, so a
    `reflect_struct!` type used as a component also needs
    `impl Component for …`.
  - Depends: M4.1, M0.5
  - Do: `reflect_struct!`, `reflect_enum!`, `reflect_newtype!`, the
    built-in impls, `TypeRegistry`, `Value` round-trip, unknown-field
    warnings.
  - Accept: the reflection tests in the scene spec pass.
  - Verify: `cargo test -p nadir-scene`
  - Note: the user decided three points on 2026-10-25 (ADR-0020):
    - `to_value` and `apply_value` take a `ReflectContext`: the source's
      name, the path, the warnings.
    - `Handle` and `Entity` impls come with M5.6 and M9.4.
    - The compact encodings: `[x, y]`, `"#rrggbb(aa)"`,
      `{"min", "max"}`.
    `docs/subsystems/scene.md` has the rest. The design critique moved
    `FromValue` to a per-type impl, because `Color` has no `Default`.
  - Note: tests: 12 unit tests, 1 integration test and 3 doc tests.
    - The field lists check names, types as written, and docs: verbatim,
      across another attribute.
    - Every built-in type, and three macro types (an enum, a newtype and a
      struct), go to JSON text and back in property tests. Floats come from random bits, so all magnitudes
      are covered; strings include quotes, controls and non-BMP
      characters.
    - Errors are checked with their paths, and hostile colors are refused.
    - Unknown fields warn in nested structs, list elements, `Rect` and
      tags.
    - The registry is checked from end to end.
    - `tests/macros.rs` uses the macros among items named `Option`,
      `Result`, `String`, `usize`, `str` and `stringify!`. It found that
      the generated code named `usize` and `str` loosely; it now uses
      absolute paths.
  - Note: each of these planted bugs failed a test, and was reverted:
    unknown fields skipped silently, field docs dropped, a large `u64`
    wrapping, and colors losing their alpha.

- [ ] **M4.3 — collision crate (without pathfinding)**
  - Depends: M0.2
  - Do: shapes, `TileGrid`, spatial hash, `move_and_slide` with substeps,
    sweeps, triggers, queries.
  - Accept: the property tests and scenario tests in the collision spec
    pass; benchmarks are recorded.
  - Verify: `cargo test -p nadir-collision`; `cargo xtask bench collision`

- [ ] **M4.4 — facade: ECS integration and sandbox migration**
  - Note (from M3.4):
    - `Resources` (`crates/engine/src/resources.rs`) has the `World`'s
      resource calls: replace it with the `World` (ADR-0019). `ecs.md`
      names the borrows `Res` and `ResMut`, where `Resources` returns
      `Ref` and `RefMut`.
    - The sandbox snapshots its `Interpolated<Vec2>` in its own fixed
      system. `PrevTransform` and the snapshot glue system replace that.
    - The sandbox's systems are `fn` items with their state in resources,
      except `count_ticks`, a closure with no captures.
    - An app has one action set (`App::add_actions`): a second set's latch
      would see no edges. Until the game adds its set, the loop latches
      into a private set of one unbound action (`NoActions`), so that
      every tick ends and `Input::wheel` moves on.
  - Note (from M3.5):
    - `FrameAllocs` counts per frame. The M4 exit asks for 0 allocations a
      tick: a count around the fixed systems in `App::frame` would give it.
    - The 5k-entity steady test can follow `steady_frames` in
      `crates/engine/src/app.rs`, which drives the real loop offscreen.
    - The stress scene (`games/sandbox/src/stress.rs`) moves its crowd
      with `Interpolated` too, and has a test of its own systems.
  - Note (from M4.1):
    - `Schedule::run_fixed(&mut World, &Tick)` and `run_frame` apply the
      commands and clear the events themselves: the loop calls nothing
      else (`docs/01-architecture.md`).
    - `Res` and `ResMut` are aliases of `Ref` and `RefMut`, so code that
      names `Ref` or `RefMut` keeps working after the swap.
    - The current system's name is private to `nadir-ecs` (`SystemScope`
      in `system.rs`). Engine steps outside the schedule, such as the latch
      or the draw, show `(none)` in borrow panics unless the guard is made
      public.
    - `Component` is explicit: the facade implements it for each component.
    - Count a steady stream of commands and events with `FrameAllocs`; the
      ecs crate checks only that its buffers do not grow.
  - Note (from M4.2): `app.register_component::<T>()` wraps
    `TypeRegistry::register_component` and `World::register`. A component
    with a `Color` field, such as a sprite's tint, needs a hand-written
    `Default`: `Color` has none (ADR-0020). A component holding a
    `Handle<T>`, such as a sprite's texture, cannot be reflected until
    M5.6 adds the `Handle` impl: register it then, or write its `Reflect`
    by hand.
  - Depends: M4.1, M4.2, M4.3, M3.4
  - Do:
    - `App` builder with stages
    - components: `Transform`, `PrevTransform`, `Velocity`, `Sprite`,
      `Collider`, `Player`, `Parent`, `SaveId`
    - glue systems: snapshot, movement + collision, triggers → events,
      parent offsets, extract
    - migrate the sandbox
  - Accept: the sandbox behaves as in M3, now through the ECS; the
    schedule prints from a test.
  - Verify: `cargo test -p nadir`; `cargo dev` (manual)

- [ ] **M4.5 — tilemap rendering and collision grid**
  - Note (from M3.3): tile chunks draw in the renderer's world pass, into
    the virtual image, before or among the sprites by layer. Tall tiles are
    sprites (`render.md`). Draw each tile at whole pixels per texel, as the
    golden rules in `crates/render/tests/headless/golden.rs` require.
  - Depends: M4.4
  - Do:
    - the `tile_chunk` shader + chunk cache, culling, animated tiles
    - tall tiles emitted as y-sorted sprites
    - a hand-built test map in code, with a collision grid
  - Accept: tilemap goldens (including animated tiles at fixed times); the
    player slides along walls without snagging.
  - Verify: `cargo xtask golden`; `cargo test -p nadir-render -p nadir`

- [ ] **M4.6 — animation**
  - Depends: M4.4
  - Do: the `Animator` component and clips (hand-defined until M5), a
    frame-stage system, a flip by `Facing`.
  - Accept: unit tests for clip timing, looping and one-shot completion
    events.
  - Verify: `cargo test -p nadir`

- [ ] **M4.7 — determinism test infrastructure**
  - Note (from M4.2): hashing the world through reflection needs every
    component of every entity in a fixed order. The registry has per-entity
    hooks only: add a per-type walk in registration order. Components that
    hold a `Handle` are not reflected before M5.6 (ADR-0020): hash them
    another way, or leave them out and say so.
  - Depends: M4.4, M3.1
  - Note (from M3.1): `InputRecorder` and `InputPlayer` hold only the
    actions' states, tick after tick: no seed, no pointer, no wheel. Keep
    the seed beside the recording. The player replaces `Input::latch`.
  - Do: a headless run mode; input recording in the sandbox; a world hash
    via reflection; `nadir/tests/determinism.rs` with a stored recording
    and hash sequence.
  - Accept: the test passes and fails when a system is deliberately made
    nondeterministic (demonstrate, then revert).
  - Verify: `cargo test -p nadir --test determinism`

**Exit criteria (M4):**

- The player explores a hand-built tilemap, collides cleanly, and y-sorts
  correctly around tall tiles.
- The 5k-entity stress scene runs at ≤ 4 ms per tick on min-spec.
- The determinism test passes.
- 0 allocations per tick in steady state.

---

## M5 — Content pipeline

- [ ] **M5.1 — codec: adler32, inflate, PNG subset**
  - Depends: M0.5
  - Do: per the spec. Add the hand-made PNG test vectors and register fuzz
    targets for inflate and PNG.
  - Accept: all vectors decode correctly; 10 min of fuzz smoke with no
    findings.
  - Verify: `cargo test -p nadir-codec --features png`;
    `cargo xtask fuzz-smoke --secs 600 --target png`

- [ ] **M5.2 — codec: KLZ, KBJ, BMFont, WAV**
  - Depends: M0.5
  - Do: per the spec, with test vectors and fuzz targets.
  - Accept: round-trips pass; malformed input is rejected.
  - Verify: `cargo test -p nadir-codec`;
    `cargo xtask fuzz-smoke --secs 300`

- [ ] **M5.3 — assets: async server, worker pool, scopes, LooseSource**
  - Depends: M3.2
  - Note (from M3.2):
    - `load` reads and decodes at once, and `finish_completed_loads`
      finalizes, so moving decoding to workers keeps the API.
    - `load` takes `&mut self`. The spec before M3.2 had `&self`, which
      needs interior mutability: keep `&mut self` unless workers need
      otherwise.
    - Paths are already ASCII-lowercased and checked (`crates/assets/src/path.rs`).
    - Unloading a scope must destroy GPU textures, which `Asset` has no hook
      for yet.
    - The root directory is the only source so far: `AssetServer::new(root,
      ..)`.
  - Do: worker threads; decode on workers and finalize on main; load
    states; scopes (global/level/transient); `Source`, `LooseSource`; path
    normalization.
  - Accept: a load storm of 1,000 assets completes with no main-thread
    stall over 4 ms per frame; unloading a scope frees its assets.
  - Verify: `cargo test -p nadir-assets`

- [ ] **M5.4 — cook: CLI, cache, textures, atlas packer**
  - Note (from M3.5): the sandbox has a hand-made atlas,
    `assets/sprites/atlas.qoi`, with its regions as constants in
    `games/sandbox/src/stress.rs`. Cooked atlases replace both.
  - Depends: M5.1, M5.2, M2.4
  - Do: the `nadir-cook` library and CLI; the incremental cache; the
    PNG → trim → MaxRects atlas (padding + extrusion) → QOI pipeline;
    `xtask cook`.
  - Accept:
    - Cooking twice is byte-identical.
    - Changing one PNG re-cooks only its atlas group.
    - Case-colliding paths are rejected.
  - Verify: `cargo test -p nadir-cook`; `cargo xtask cook` twice and
    compare hashes

- [ ] **M5.5 — cook: Aseprite animations, data, fonts**
  - Depends: M5.4, M4.6
  - Do: the Aseprite `json-array` + tags importer → clips; JSONC → KBJ;
    BMFont → font asset.
  - Accept: sandbox animations come from Aseprite exports.
  - Verify: `cargo test -p nadir-cook`; `cargo dev` (manual)

- [ ] **M5.6 — cook: LDtk importer; scene: prefabs and level loading**
  - Note (from M4.2): reflect `Handle<T>` as its asset path here
    (ADR-0020).
    - Give `ReflectContext` the asset server: `AssetServer::path` writes
      the path, and `load` reads it.
    - `Registration::insert` goes through `Commands`. Prefab layers (base,
      derived, overrides) can each apply onto one value before the insert.
    - `TypeRegistry` keys types by `T::TYPE_NAME`, the name prefabs use.
  - Depends: M5.5, M4.5, M4.2
  - Do:
    - LDtk → `.klvl` (tile layers, IntGrid → collision, entities, tall
      tiles, external levels, `jsonVersion` check)
    - `PrefabLibrary` with `extends`
    - `load_level`
  - Accept: the sandbox level is authored in LDtk; prefab inheritance tests
    pass; an unknown component name is reported.
  - Verify: `cargo test -p nadir-cook -p nadir-scene`; `cargo dev`
    (manual)

- [ ] **M5.7 — pack files**
  - Depends: M5.4
  - Note (from M3.2): `AssetServer::new(root, ..)` reads loose files. If
    the `dev` feature compiles loose reading out of release builds, the
    facade's release path changes with it, as this task's Do says.
  - Do: the `.kpak` writer (cook) with dedup and alignment; `PackSource`
    with TOC binary search and CRC verification; release builds use the
    pack.
  - Accept: the round-trip test passes; a corrupted entry is detected and
    named; the TOC reader is fuzzed.
  - Verify: `cargo test -p nadir-assets -p nadir-cook`;
    `cargo xtask fuzz-smoke --target kpak --secs 300`

- [ ] **M5.8 — hot reload**
  - Depends: M5.6
  - Do: `xtask cook --watch`; the runtime mtime watcher; reloading
    textures, data, levels and shaders in dev.
  - Accept: an edit appears in the running game within 1 s, and existing
    handles stay valid.
  - Verify: manual demonstration in `cargo dev`; describe what was edited
    and what was observed

**Exit criteria (M5):**

- All sandbox content comes from `assets/`.
- `xtask cook` is deterministic.
- Hot reload works on all three OSes.
- All parsers pass 10 minutes of fuzz smoke.

---

## M6 — Audio

- [ ] **M6.1 — codec: QOA**
  - Depends: M0.5
  - Do: the encoder and decoder per the spec, with a fuzz target.
  - Accept: the round-trip SNR threshold is measured and asserted; the
    decoder never panics.
  - Verify: `cargo test -p nadir-codec`

- [ ] **M6.2 — audio: mixer (offline)**
  - Note (from M3.5): for the test that `render` allocates nothing, a
    dev-dependency on `nadir-platform` with `features = ["dev"]` installs the
    counter. Read `thread_alloc_counts()` around the calls. Declare no other
    `#[global_allocator]`: two in one binary do not build.
  - Depends: M6.1, M3.2
  - Do: voices, buses, constant-power pan, positional attenuation, fades,
    the resampler, stealing, the limiter, music streaming from in-memory
    QOA, the command and return queues.
  - Accept: every offline test in the audio spec passes; 0 allocations in
    `render`.
  - Verify: `cargo test -p nadir-audio`; `cargo xtask bench mixer`

- [ ] **M6.3 — platform (Linux): ALSA backend**
  - Depends: M6.2
  - Do: `dlopen` libasound, the audio thread, `snd_pcm_recover`.
  - Accept: sandbox audio plays; unplugging or replugging a device recovers
    or logs clearly.
  - Verify: `cargo dev` (manual)

- [ ] **M6.4 — platform (Windows): WASAPI backend**
  - Depends: M6.2
  - Do: COM vtables, event-driven shared mode, avrt, device invalidation
    reopen.
  - Accept: as M6.3.
  - Verify: on Windows, `cargo dev` (manual)

- [ ] **M6.5 — platform (macOS): AudioUnit backend**
  - Depends: M6.2
  - Do: the DefaultOutput unit, stream format, render callback.
  - Accept: as M6.3.
  - Verify: on macOS, `cargo dev` (manual)

- [ ] **M6.6 — audio assets and facade integration**
  - Depends: M6.3, M5.4
  - Do: cook WAV → PCM16/QOA by sidecar; `Sound`/`Music` assets;
    `AudioEmitter`/`AudioListener` components; music crossfade; bus volumes
    from settings.
  - Accept: footsteps follow the player positionally; music crossfades
    between areas.
  - Verify: `cargo test -p nadir-audio -p nadir-cook`; `cargo dev`
    (manual)

- [ ] **M6.7 — soak tooling**
  - Depends: M6.6
  - Do: `xtask soak --audio --minutes N`, which logs underruns, callback
    time p99 and memory.
  - Accept: the tool runs and reports.
  - Verify: `cargo xtask soak --audio --minutes 1`

**Exit criteria (M6):**

- Music and positional SFX on all three OSes.
- A 10-minute soak with 0 underruns on min-spec on each OS.
- Latency ≤ 50 ms, with the measurement method recorded.

---

## M7 — Developer tools

- [ ] **M7.1 — render: bitmap text**
  - Depends: M5.5, M3.3
  - Do: the font asset; layout (wrap, align, kerning, color markup); the
    `text` pipeline; the missing-glyph fallback.
  - Accept: text layout unit tests and goldens pass.
  - Verify: `cargo test -p nadir-render`; `cargo xtask golden`

- [ ] **M7.2 — debug: immediate-mode UI core**
  - Depends: M7.1, M3.1
  - Note (from M3.1): typed text does not exist yet. The platform needs
    `Event::Text`:
    - on Linux, an X input method: `XOpenIM`, `XCreateIC` and
      `Xutf8LookupString`, not yet declared in `nadir-sys`
    - on Windows, `WM_CHAR`
    - on macOS, `characters`

    `nadir-debug` may not depend on `nadir-platform`, so text, and the
    key repeats that editing needs (`Input` ignores repeats), must reach
    it through `nadir-input` or the facade. `Input::set_text_mode` hides
    every key and mouse button from gameplay actions. A capture of the
    keyboard only, or of the mouse only (`wants_mouse`), needs new API.
  - Do: IDs, hot/active state, layout, windows (drag, collapse, scroll),
    the widget set, input capture, position persistence.
  - Accept: the UI logic tests in the debug spec pass.
  - Verify: `cargo test -p nadir-debug`

- [ ] **M7.3 — stats overlay and profiler**
  - Note (from M2.3): `Caps` has no timer-query field yet. It is
    `#[non_exhaustive]`, so adding one breaks nothing.
  - Note (from M3.5): `FrameAllocs`, the last frame's allocations, is a
    facade resource holding `nadir-platform`'s `AllocCounts`, and
    `nadir-debug` may depend on neither. So the overlay takes plain numbers,
    which the facade copies in each frame. Only Rust's allocator is counted.
  - Depends: M7.2, M0.4
  - Do: the overlay; profiler ring, graph and scope tree; Chrome trace
    export; GL timer queries.
  - Accept: the exported trace parses with our JSON parser; manual: it
    opens in Perfetto UI.
  - Verify: `cargo test -p nadir-debug`; manual check described

- [ ] **M7.4 — inspector**
  - Note (from M4.2): `Registration::inspect` and `inspect_mut` hold the
    type's borrow while their callback runs: edit through `inspect_mut`
    alone. There is no remove hook yet, and `World` cannot list entities:
    the entity list needs one of those. `FieldInfo::doc` keeps each line's
    leading space; trim it for display.
  - Depends: M7.2, M4.2
  - Do: the entity list with filter; component editors via `Reflect`;
    add/remove; despawn; save as prefab.
  - Accept: the reflect round-trip edit test passes.
  - Verify: `cargo test -p nadir-debug`

- [ ] **M7.5 — console**
  - Note (from M4.1): `Schedule::describe()` lists the systems in run
    order as `SystemInfo`, which displays as `tick  Gameplay:
    player_move`, with `(conditional)` for a system with a run condition.
  - Depends: M7.2
  - Do: the command registry, history, completion, the log view, the
    initial command set.
  - Accept: unit tests for parsing and completion.
  - Verify: `cargo test -p nadir-debug`

- [ ] **M7.6 — debug draw toggles and time control**
  - Depends: M7.2, M4.3
  - Do: the F4 overlays and the F5–F7 time controls.
  - Accept: manual check on the sandbox.
  - Verify: `cargo dev` (manual); `cargo xtask release-no-dev`

**Exit criteria (M7):** all tools work on all three OSes, and
`release-no-dev` passes.

---

## M8 — Gamepads

- [ ] **M8.1 — Linux: evdev + inotify hotplug**
  - Depends: M3.1, M1.2
  - Do: the evdev declarations (`/add-ffi-binding`); device detection via
    `EVIOCGBIT`; axis ranges via `EVIOCGABS`; non-blocking reads; `inotify`
    on `/dev/input` (`IN_CREATE` + `IN_ATTRIB`); `Gamepad*` events.
  - Accept: unit tests for the code → button/axis mapping and axis
    normalization; manual: an Xbox pad works, and hotplug works without
    restart.
  - Verify: `cargo test -p nadir-platform`;
    `cargo run -p nadir-platform --example events` (manual)

- [ ] **M8.2 — Windows: XInput with throttled probing**
  - Depends: M3.1, M1.5
  - Do: `dynlib!` for `xinput1_4.dll` (fallback `xinput9_1_0.dll`); poll
    connected slots every frame and disconnected slots at most once per
    second; diff the states into events.
  - Accept: unit tests for state diffing; manual as M8.1.
  - Verify: on Windows: `cargo test -p nadir-platform`; the events
    example (manual)

- [ ] **M8.3 — macOS: GameController.framework**
  - Depends: M3.1, M1.6
  - Do: connect/disconnect notifications; per-frame `extendedGamepad`
    sampling; diff into events.
  - Accept: manual as M8.1, plus DualSense.
  - Verify: on macOS: `cargo test -p nadir-platform`; the events example
    (manual)

- [ ] **M8.4 — input: pad bindings, deadzones, active device, prompt data**
  - Depends: M8.1
  - Note (from M3.1):
    - `Binding` is not `#[non_exhaustive]`: adding the pad variants breaks
      the workspace's matches, which is the point.
    - A replay of version 1 derives a button's value from held. An analog
      button value needs format version 2 and a reader for version 1, or
      the analog half-axis goes through an `Axis1` instead.
    - Directions are already shortened to length 1 after rounding.
      `Axis1` values need rounding and clamping to 0..1: M3.1 had no
      analog input, so it has no function for that.
    - The spec's deadzone property test is this task's.
  - Do: `PadButton`/`PadAxis`/`PadStick` bindings; radial and trigger
    deadzones; active-device tracking; prompt-glyph data table (per device
    family) in `assets/data/`.
  - Accept: deadzone property tests; active-device switching tests.
  - Verify: `cargo test -p nadir-input`

**Exit criteria (M8):**

- An Xbox pad works on all three OSes.
- DualSense and Switch Pro work on Linux and macOS.
- Hotplug works without restart.

---

## M9 — Vertical slice

- [ ] **M9.1 — game states and level transitions**
  - Depends: M5.6
  - Do: the `GameState` machine with on-enter/on-exit and run conditions;
    a loading screen; level transitions that unload the previous level's
    asset scope.
  - Accept: the state tests in the scene spec pass; no transition happens
    mid-tick.
  - Verify: `cargo test -p nadir-scene -p nadir`

- [ ] **M9.2 — menu kit and game UI**
  - Depends: M7.1, M8.4
  - Note (from M3.1): `ActionMap::conflict` returns the first other slot
    whose binding shares a key or button. A swap is an `unbind` and a
    `bind`. `begin_capture` is this task's.
  - Do: in the facade, focusable buttons, sliders and toggles with
    keyboard, mouse and gamepad navigation; the main menu, pause menu and
    settings (display, audio, controls with rebinding).
  - Accept: focus-navigation unit tests; manual: every menu is fully usable
    with gamepad only and with mouse only.
  - Verify: `cargo test -p nadir`; `cargo dev` (manual)

- [ ] **M9.3 — settings file**
  - Note (from M4.2): apply settings field by field (`fields()` and
    `field_mut()`), so that an invalid value can fall back alone. Keep the
    source `Value` to preserve unknown keys: `apply_value` warns about them
    and leaves them out.
  - Depends: M9.2
  - Note (from M3.1): bindings are saved by action name
    (`ActionSet::from_name`). `Key` and `MouseButton` have no names to
    read back yet, beyond their `Debug` text.
  - Do: `settings.jsonc` load, validate and save; unknown keys preserved;
    invalid values fall back to defaults with a warning; fuzz target.
  - Accept: a test for each fallback rule; the fuzz target is registered.
  - Verify: `cargo test -p nadir`;
    `cargo xtask fuzz-smoke --target settings --secs 120`

- [ ] **M9.4 — saves**
  - Note (from M4.2): reflect `Entity` as its `SaveId` here (ADR-0020).
    - Give `ReflectContext` the map between them. `Entity` has no
      `Default`, so implement `FromValue` for it directly.
    - Registrations need a `saved` flag (scene.md "Saves").
    - `reflect_newtype!` already makes `SaveId(u64)` transparent. Values
      too large for an `i64` are written as strings.
  - Depends: M4.2, M9.1
  - Do: the `.ksav` format; atomic write with `.bak`; CRC; migration
    scaffolding with a v1 fixture; fuzz target.
  - Accept: the save tests in the scene spec pass.
  - Verify: `cargo test -p nadir-scene`;
    `cargo xtask fuzz-smoke --target ksav --secs 120`

- [ ] **M9.5 — pathfinding and enemy AI**
  - Depends: M4.3
  - Do: A* with budget and smoothing; flow fields; sandbox enemies that
    chase and patrol.
  - Accept: the A* tests in the collision spec pass; the A* benchmark is
    recorded.
  - Verify: `cargo test -p nadir-collision`; `cargo xtask bench astar`

- [ ] **M9.6 — crash reporting basics**
  - Depends: M1.4
  - Do: the panic-hook report (message, backtrace, version, OS, GPU string,
    log tail); the native error dialog; the "open crash folder" prompt on
    the next launch.
  - Accept: a forced panic (`--debug-panic` flag, dev only) produces a
    complete report.
  - Verify: `cargo test -p nadir`;
    `cargo dev -- --debug-panic`, then inspect the report (manual)

- [ ] **M9.7 — content: one complete level of the real game**
  - Depends: M9.1–M9.6, M6.6
  - Do: build the level with LDtk and Aseprite content, enemies, pickups,
    music, a save point and a win condition.
  - Accept: a playtester outside the team finishes the level on each OS.
  - Verify: playtest notes per OS (manual, by a human)

**Exit criteria (M9):**

- One level is playable start to finish on all three OSes, with keyboard
  and mouse and with a gamepad.
- Save, quit and load resumes correctly.
- Settings persist.

---

## M10 — Metal backend (macOS)

- [ ] **M10.1 — sys + platform: Metal, QuartzCore, CAMetalLayer surface**
  - Depends: M1.6
  - Do: the Metal and QuartzCore declarations (`/add-ffi-binding`);
    `MTLCreateSystemDefaultDevice`; `CAMetalLayer` on the content view;
    `contentsScale` and `drawableSize` on resize; `GpuSurface::Metal`.
  - Accept: the layer is created and resizes correctly (manual, with a
    clear color via a minimal path).
  - Verify: on macOS: `cargo test -p nadir-sys -p nadir-platform`

- [ ] **M10.2 — gfx: Metal device**
  - Note (from M2.3): passes start with nothing bound, as encoders do. The
    blend table in `gfx.md` gives the factors to bake. The device keeps no
    borrow of its layer (ADR-0016).
  - Depends: M10.1, M2.3
  - Do: resources, pipelines (blend baked in), passes,
    `setVertexBytes`/`setFragmentBytes` uniforms, triple-buffered dynamic
    buffers, readback.
  - Accept: the stale-handle test and RHI unit tests pass on Metal.
  - Verify: on macOS: `cargo test -p nadir-gfx`

- [ ] **M10.3 — MSL for every shader + parity test**
  - Note (from M3.3):
    - The interface constants are in `crates/render/src/shader.rs`, and
      the GLSL in `crates/render/shaders/`.
    - The blit's uv, in `BLIT_QUAD` in `crates/render/src/renderer.rs`,
      samples the virtual image bottom-up, as a GL render target is stored.
      Metal's is top-down.
    - `.claude/rules/shaders.md` asks for both sources of a program in the
      same task. M3.3 wrote GLSL only, since this task is where the `.metal`
      sources are scheduled.
    - An output smaller than the virtual image sets a viewport outside the
      target. Metal's validation refuses that, so clamp the viewport and
      the uv then.
  - Depends: M10.2
  - Do: write a `.metal` source for every program; extend the parity test
    to parse MSL attributes.
  - Accept: the parity test passes for every program.
  - Verify: `cargo test -p nadir-render` (the parity test is pure Rust
    and runs on any OS)

- [ ] **M10.4 — goldens on Metal; Metal is the macOS default**
  - Note (from M2.4): `cargo xtask golden` takes only `--bless` so far. Add
    `--backend`, and check the `<scene>.metal.qoi` overrides, which it
    lists and skips until then.
  - Note (from M2.5): the arm64 macOS runners have Metal on `Apple
    Paravirtual device` (`docs/04-quality.md`), so Metal goldens could run
    in CI there.
    - Its highest family is Apple5, and it has no Metal 4.
    - The Intel runners hang on multisampled Metal work.
    - Apple's docs say the process must link CoreGraphics for
      `MTLCreateSystemDefaultDevice` to return a device, which a
      command-line tool such as a test binary usually has to do
      explicitly. `nadir-sys` links AppKit, which loads CoreGraphics, but
      no test shows yet that this is enough.
  - Depends: M10.3
  - Do: `GpuPreference::Auto` picks Metal on macOS; keep the GL fallback via
    `--gpu gl`.
  - Accept: all goldens match the GL references within tolerance, run
    locally on a Mac and recorded. Performance is ≥ GL on the test Mac.
  - Verify: on macOS: `cargo xtask golden --backend metal`; benchmark notes

**Exit criteria (M10):** macOS runs on Metal by default, with GL selectable.

---

## M11 — Production hardening and release

- [ ] **M11.1 — packaging for all three OSes**
  - Depends: M9.7
  - Do: `xtask package <os>`:
    - Windows: manifest and resources via `rc.exe`
    - macOS: `.app` + `Info.plist` + `.icns` + universal binary via `lipo`
    - Linux: tarball, plus optional AppImage
    - `LICENSES.txt`
    - no signing locally
  - Accept: the package layout matches `docs/05-release.md`; the packaged
    game launches on each OS.
  - Verify: `cargo xtask package <os>` on each OS, plus a manual launch

- [ ] **M11.2 — signing and notarization in the CI release workflow**
  - Depends: M11.1
  - Do: write `.github/workflows/release.yml` per `docs/05-release.md`,
    with protected environments and a manually approved store-upload job.
    Document the required secret **names** only. A human creates the
    secrets.
  - Accept: the workflow passes YAML review; the secret names are
    documented; no secret values appear anywhere in the repo.
  - Verify: YAML parse/review; `git diff` reviewed for secret material

- [ ] **M11.3 — Windows minidumps, symbol archiving, `xtask symbolicate`**
  - Depends: M9.6
  - Do: `SetUnhandledExceptionFilter` → `MiniDumpWriteDump`; archive symbols
    per build hash; `xtask symbolicate <report>` using external tools.
  - Accept: a forced native crash on Windows writes a `.dmp`; a panic report
    symbolicates on each OS (manual).
  - Verify: on Windows: forced crash (manual); `cargo xtask symbolicate` on
    a sample report

- [ ] **M11.4 — `--smoke-test` mode and the release smoke step**
  - Depends: M11.1
  - Do: `--smoke-test` boots, loads the first level, simulates 600 ticks
    and exits 0; add the step to the release workflow.
  - Accept: exits 0 on all three OSes from the packaged build.
  - Verify: run the packaged binary with `--smoke-test` on each OS

- [ ] **M11.5 — min-spec performance pass**
  - Depends: M9.7
  - Do: measure every budget in `docs/00-overview.md` on the recorded
    min-spec machine; fix the violations.
  - Accept: every budget is met, or the exception is recorded with an ADR.
  - Verify: measurement notes with numbers, per OS

- [ ] **M11.6 — security pass**
  - Note (from M3.5): the fuzzer does not count allocations. The counter
    is in `nadir-platform`, which `xtask` may not link. Decide where the
    targets run, or the matrix edge, with the user (`docs/04-quality.md`).
  - Depends: M9.4
  - Do: nightly 30-minute fuzz smoke per parser; review the limits in
    every loader; ask the user to decide dependency-policy item 29
    (coverage-guided fuzzing).
  - Accept: clean fuzz runs are recorded; item 29 is decided and recorded.
  - Verify: `cargo xtask fuzz-smoke --secs 1800` per target (nightly CI
    logs)

- [ ] **M11.7 — documentation**
  - Depends: M11.1
  - Do: a player-facing note on log, save and crash locations;
    `cargo doc --workspace --no-deps` with no warnings.
  - Accept: the docs build cleanly; the player note exists in the package.
  - Verify: `cargo doc --workspace --no-deps`

- [ ] **M11.8 — release-candidate dry run**
  - Depends: M11.2–M11.7
  - Do: work through the full release checklist in `docs/05-release.md`.
  - Accept: every checklist item is checked, with evidence linked.
  - Verify: the completed checklist (human sign-off)

**Exit criteria (M11):** CI produces signed, notarized packages for all
three OSes from a tag, and the release checklist is complete.

---

## Backlog (M12+, each needs a trigger or a decision)

| Item | Trigger / reason |
| :- | :- |
| Native Wayland (`libwayland-client` via `dlopen`, xdg-shell, `libxkbcommon`, client-side decorations) | A target distro ships without XWayland by default, or player reports from Wayland-only setups |
| HID gamepads on Windows (DualSense, Switch Pro) | Player demand; Steam Input covers many Steam players meanwhile |
| IME (`NSTextInputClient`, XIM preedit, Windows IMM) | Localization into CJK languages or a name-entry feature |
| TrueType rasterizer in cook (glyf outlines → bitmap atlases) | Localization beyond the hand-made bitmap fonts |
| 2D lighting pass (`light2d`) | Art direction asks for it |
| `aarch64` Windows and Linux targets | Store or platform demand |
| Native PipeWire audio | ALSA-compatibility issues reported |
| Windows installer | A non-Steam/itch distribution channel requires one |
| Opt-in crash upload | Volume of player bug reports; requires a privacy review |
| Native file watchers (inotify, FSEvents, `ReadDirectoryChangesW`) | Polling cost on large asset trees |
| Play-in-editor tooling | After the vertical slice, if level iteration is the bottleneck |
| ECS change detection | A profiled need (e.g. render extract of static sprites) |
| Lock-free audio command queue | Measured `try_lock` contention |
| Localization pipeline (string tables, plurals) | Before the first non-English release |
