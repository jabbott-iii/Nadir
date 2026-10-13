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

- [ ] **M1.3 — facade: App skeleton and fixed-tick loop**
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

- [ ] **M1.4 — paths, error dialog, file logging**
  - Depends: M1.3
  - Do:
    - `platform::user_dirs` (Linux first, with XDG env var handling)
    - `show_error_dialog` (Linux: stderr only; the game never shells out to
      external tools at runtime)
    - wire the log file sink to the data directory
  - Accept: unit tests for path resolution with fake env vars.
  - Verify: `cargo test -p nadir-platform`

- [ ] **M1.5 — sys + platform (Windows)**
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

- [ ] **M2.1 — sys: GL types/enums, EGL, WGL, NSOpenGL declarations**
  - Depends: M1.1
  - Do: GL types and fn-pointer types (shared); `linux/egl.rs` dynlib;
    WGL and NSOpenGL declarations as they are needed.
  - Accept: layout tests; header citations.
  - Verify: `cargo test -p nadir-sys`

- [ ] **M2.2 — platform (Linux): EGL context, vsync, headless GL**
  - Depends: M2.1, M1.2
  - Do:
    - EGL on X11 with a matching visual
    - a 3.3 core context, swap interval
    - `create_headless_gl()` via surfaceless or pbuffer
  - Accept: headless context creation works under Mesa llvmpipe.
  - Verify: `LIBGL_ALWAYS_SOFTWARE=1 cargo test -p nadir-platform --features headless-test`
    (or the equivalent test name)

- [ ] **M2.3 — gfx: GL backend core**
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
  - Verify: `cargo test -p nadir-gfx` (headless)

- [ ] **M2.4 — codec QOI + golden harness**
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
    `LIBGL_ALWAYS_SOFTWARE=1 cargo xtask golden`

- [ ] **M2.5 — CI golden job (Linux)**
  - Depends: M2.4, M0.8
  - Do: install Mesa and run `cargo xtask golden` in the Linux CI job.
    Investigate whether the hosted Windows and macOS runners expose usable
    GL, and record the findings in `docs/04-quality.md`.
  - Accept: the workflow is updated; findings are recorded.
  - Verify: workflow YAML review; CI results, if visible to the session

- [ ] **M2.6 — platform (Windows): WGL context**
  - Depends: M2.3, M1.5
  - Do: the dummy-window dance, `wglChoosePixelFormatARB`, a 3.3 core
    context, `wglSwapIntervalEXT`, GL 1.1 symbols from `opengl32.dll`.
  - Accept: the golden scenes render correctly on Windows hardware
    (`cargo xtask golden` locally).
  - Verify: on Windows: `cargo xtask golden`

- [ ] **M2.7 — platform (macOS): NSOpenGL context**
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

- [ ] **M3.1 — input crate**
  - Depends: M1.3, M0.2
  - Do: device state, edge latching, `ActionMap`/`ActionState`, composite
    WASD, text mode, recorder/player (format only), per
    `docs/subsystems/input.md`.
  - Accept: every test listed in the input spec passes.
  - Verify: `cargo test -p nadir-input`

- [ ] **M3.2 — minimal assets: handles + synchronous loose loading**
  - Depends: M0.3, M2.4
  - Do:
    - `Handle<T>` and `Assets<T>`
    - an `AssetServer` with synchronous loading from loose files
    - the `Asset` trait with `decode`/`finalize`
    - a `Texture` asset in `render` (QOI → GPU)
  - Accept: loading the same path twice returns the same handle; a missing
    file shows the placeholder in dev.
  - Verify: `cargo test -p nadir-assets -p nadir-render`

- [ ] **M3.3 — render: sprite batcher, sort keys, camera, upscale**
  - Depends: M3.2
  - Do: following `docs/subsystems/render.md`:
    - the `sprite` and `blit` shaders, with an interface parity test
    - instance packing, sort keys, batching
    - `Camera2D`, pixel-perfect virtual resolution, letterboxed integer
      upscale, `screen_to_world`
  - Accept:
    - Sort-key property test; batching tests.
    - Goldens: y-sort scene, upscale at 3 window sizes.
  - Verify: `cargo test -p nadir-render`; `cargo xtask golden`

- [ ] **M3.4 — facade + sandbox: moving character**
  - Depends: M3.1, M3.3
  - Do:
    - the `App` integrates input latching, interpolation (prev/current
      positions) and render submission
    - the sandbox: a character sprite moved with the `Move` action,
      following camera, a few static props
    - plain structs for now; the ECS arrives in M4
  - Accept: movement is smooth at 60, 144 and uncapped fps (interpolation
    works); a tap between ticks moves the character exactly one tick.
  - Verify: `cargo test -p nadir`; `cargo dev` (manual)

- [ ] **M3.5 — allocation counter + sprite stress benchmark**
  - Depends: M3.4, M0.7
  - Do:
    - the dev `#[global_allocator]` counter
    - a 10k-sprite stress scene in the sandbox (`--scene stress-sprites`)
    - a bench for sort + batch
    - a test asserting 0 allocations over 100 steady-state frames
  - Accept: 0 allocations/frame; bench results recorded in
    `bench/baseline.json`.
  - Verify: `cargo test -p nadir --features dev`; `cargo xtask bench sprite`

**Exit criteria (M3):**

- The character moves smoothly on all three OSes.
- The 10k-sprite stress scene runs at ≥ 60 fps on min-spec, with ≤ 30 draw
  calls and 0 allocations per frame.

---

## M4 — Simulation: ECS, reflection, collision, tilemaps

- [ ] **M4.1 — ecs crate**
  - Depends: M0.3
  - Do: everything in `docs/subsystems/ecs.md`:
    - entities, `SparseSet`, `World`, `Read`/`Write`, resources
    - commands, events, schedule with stages and run conditions
    - borrow-panic messages with the system name
  - Accept: the model-based property test, the borrow-conflict test and
    the events test pass; benchmarks are recorded.
  - Verify: `cargo test -p nadir-ecs`; `cargo xtask bench ecs`

- [ ] **M4.2 — scene: reflection**
  - Depends: M4.1, M0.5
  - Do: `reflect_struct!`, `reflect_enum!`, `reflect_newtype!`, the
    built-in impls, `TypeRegistry`, `Value` round-trip, unknown-field
    warnings.
  - Accept: the reflection tests in the scene spec pass.
  - Verify: `cargo test -p nadir-scene`

- [ ] **M4.3 — collision crate (without pathfinding)**
  - Depends: M0.2
  - Do: shapes, `TileGrid`, spatial hash, `move_and_slide` with substeps,
    sweeps, triggers, queries.
  - Accept: the property tests and scenario tests in the collision spec
    pass; benchmarks are recorded.
  - Verify: `cargo test -p nadir-collision`; `cargo xtask bench collision`

- [ ] **M4.4 — facade: ECS integration and sandbox migration**
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
  - Depends: M4.4, M3.1
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
  - Do: worker threads; decode on workers and finalize on main; load
    states; scopes (global/level/transient); `Source`, `LooseSource`; path
    normalization.
  - Accept: a load storm of 1,000 assets completes with no main-thread
    stall over 4 ms per frame; unloading a scope frees its assets.
  - Verify: `cargo test -p nadir-assets`

- [ ] **M5.4 — cook: CLI, cache, textures, atlas packer**
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
  - Do: IDs, hot/active state, layout, windows (drag, collapse, scroll),
    the widget set, input capture, position persistence.
  - Accept: the UI logic tests in the debug spec pass.
  - Verify: `cargo test -p nadir-debug`

- [ ] **M7.3 — stats overlay and profiler**
  - Depends: M7.2, M0.4
  - Do: the overlay; profiler ring, graph and scope tree; Chrome trace
    export; GL timer queries.
  - Accept: the exported trace parses with our JSON parser; manual: it
    opens in Perfetto UI.
  - Verify: `cargo test -p nadir-debug`; manual check described

- [ ] **M7.4 — inspector**
  - Depends: M7.2, M4.2
  - Do: the entity list with filter; component editors via `Reflect`;
    add/remove; despawn; save as prefab.
  - Accept: the reflect round-trip edit test passes.
  - Verify: `cargo test -p nadir-debug`

- [ ] **M7.5 — console**
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
  - Do: in the facade, focusable buttons, sliders and toggles with
    keyboard, mouse and gamepad navigation; the main menu, pause menu and
    settings (display, audio, controls with rebinding).
  - Accept: focus-navigation unit tests; manual: every menu is fully usable
    with gamepad only and with mouse only.
  - Verify: `cargo test -p nadir`; `cargo dev` (manual)

- [ ] **M9.3 — settings file**
  - Depends: M9.2
  - Do: `settings.jsonc` load, validate and save; unknown keys preserved;
    invalid values fall back to defaults with a warning; fuzz target.
  - Accept: a test for each fallback rule; the fuzz target is registered.
  - Verify: `cargo test -p nadir`;
    `cargo xtask fuzz-smoke --target settings --secs 120`

- [ ] **M9.4 — saves**
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
  - Depends: M10.1, M2.3
  - Do: resources, pipelines (blend baked in), passes,
    `setVertexBytes`/`setFragmentBytes` uniforms, triple-buffered dynamic
    buffers, readback.
  - Accept: the stale-handle test and RHI unit tests pass on Metal.
  - Verify: on macOS: `cargo test -p nadir-gfx`

- [ ] **M10.3 — MSL for every shader + parity test**
  - Depends: M10.2
  - Do: write a `.metal` source for every program; extend the parity test
    to parse MSL attributes.
  - Accept: the parity test passes for every program.
  - Verify: `cargo test -p nadir-render` (the parity test is pure Rust
    and runs on any OS)

- [ ] **M10.4 — goldens on Metal; Metal is the macOS default**
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
