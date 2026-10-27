# 01 — Architecture

## Principles

1. **Layered crates, one-way dependencies.** Lower layers never know about
   higher ones. `cargo xtask layers` enforces the matrix below.
2. **Subsystems are ECS-agnostic libraries.**
   - `render`, `audio`, `input`, `collision` and `assets` expose plain Rust
     APIs and know nothing about entities.
   - The `nadir` facade crate owns the components and the glue systems that
     connect them to the ECS.
   - This keeps each subsystem testable in isolation and swappable.
3. **The OS is quarantined.** OS declarations live in `sys`. OS behavior
   lives in `platform` (and the GPU backends in `gfx`). Nothing else touches
   an OS API, contains `unsafe`, or has a `target_os` cfg.
4. **Data-oriented hot paths.** Contiguous arrays, generational indices, sort
   keys, reused buffers, no per-frame allocation.
5. **The engine owns the loop, and it's a plain loop.** Because we only ship
   on desktops, the main loop polls the OS for events. There's no inverted
   control flow and no async runtime (ADR-0001).

## Layers

A crate may only depend on crates in lower layers, and only on the specific
crates its row in the dependency matrix allows.

| Layer | Crates |
| :- | :- |
| 6 — applications | `games/sandbox`, `tools/cook`, `xtask`, `bench` |
| 5 — facade | `nadir` (App, plugins, main loop, components, ECS glue systems) |
| 4 — tooling inside the engine | `debug` |
| 3 — composed subsystems | `render`, `audio`, `scene` |
| 2 — base subsystems | `gfx`, `ecs`, `assets`, `input`, `collision` |
| 1 — OS behavior and formats | `platform`, `codec` |
| 0 — foundations | `core`, `sys` |

## Dependency matrix

A crate may depend on the workspace crates listed in its row and nothing else.
No crate may depend on a third-party crate unless the dependency policy
approves it.

| Crate | Allowed workspace dependencies |
| :- | :- |
| `nadir-core` | — |
| `nadir-sys` | — |
| `nadir-platform` | `core`, `sys` |
| `nadir-gfx` | `core`, `sys`, `platform` |
| `nadir-codec` | `core` |
| `nadir-ecs` | `core` |
| `nadir-input` | `core`, `platform` |
| `nadir-assets` | `core`, `codec` |
| `nadir-render` | `core`, `gfx`, `assets`, `codec` |
| `nadir-audio` | `core`, `platform`, `assets`, `codec` |
| `nadir-collision` | `core` |
| `nadir-scene` | `core`, `ecs`, `assets`, `codec` |
| `nadir-debug` | `core`, `ecs`, `scene`, `render`, `input` |
| `nadir` (facade) | all crates above |
| `nadir-cook` | `core`, `codec`, `assets`, `scene` |
| `xtask` | `core`, `codec` |
| `nadir-bench` | any workspace crate |
| `sandbox` | `nadir`, `nadir-debug` (behind `dev` feature) |

## Workspace layout

```
.
├── Cargo.toml                # [workspace], [workspace.lints], [workspace.package], profiles
├── rust-toolchain.toml       # pinned stable toolchain + rustfmt, clippy
├── clippy.toml               # allow-unwrap-in-tests = true, allow-expect-in-tests = true
├── .cargo/config.toml        # aliases: xtask, dev
├── crates/{core,sys,platform,gfx,codec,ecs,input,render,audio,assets,collision,scene,debug,engine}/
├── tools/cook/
├── xtask/
├── bench/
├── games/sandbox/
├── assets/                   # source assets for the sandbox
├── tests/golden/             # golden-image references (.qoi)
├── fuzz/corpus/              # seed inputs per parser
└── docs/
```

`.cargo/config.toml`:

```toml
[alias]
xtask = "run --package xtask --"
dev   = "run --package sandbox --features dev --"
```

Workspace lints (`Cargo.toml`); every member uses `[lints] workspace = true`:

```toml
[workspace.lints.rust]
unsafe_code = "deny"              # sys, platform, gfx override with #![allow(unsafe_code)]
unsafe_op_in_unsafe_fn = "deny"
missing_docs = "warn"
unreachable_pub = "warn"

[workspace.lints.clippy]
undocumented_unsafe_blocks = "deny"
multiple_unsafe_ops_per_block = "deny"
unwrap_used = "deny"
print_stdout = "deny"
print_stderr = "deny"
dbg_macro = "deny"
todo = "deny"
```

`unsafe_code` is `deny` rather than `forbid` on purpose. `forbid` cannot be
overridden by an inner attribute, and the three FFI crates need
`#![allow(unsafe_code)]` at their crate root. `cargo xtask unsafe-audit`
checks that the allow attribute appears nowhere else.

## The main loop

There is one main thread. It owns the window, the GL context (or Metal
layer), the ECS world and rendering.

```rust
pub fn run(mut app: App) -> Result<(), Error> {
    let mut platform = Platform::new(&app.config.window)?;
    let mut gfx = gfx::Device::new(platform.gpu_surface())?;  // keeps no borrow (ADR-0016)
    let mut renderer = Renderer::new(&mut gfx, &app.config.render)?;
    let mut clock = FixedClock::new(TICK_HZ /* 60 */, MAX_FRAME /* 0.25 s */);
    let mut stopwatch = Stopwatch::start();
    let mut events = Vec::with_capacity(256);

    loop {
        platform.pump_events(&mut events);                   // non-blocking
        if app.handle_window_events(&events) == Flow::Quit { break; }
        for event in &events { app.input.handle(event); }    // edges accumulate here
        events.clear();

        let frame = clock.begin_frame(stopwatch.lap());      // clamps hitches
        while let Some(tick) = clock.consume_tick() {
            app.input.latch(&app.action_map, &mut app.actions); // edges -> this tick
            app.schedule.run_fixed(&mut app.world, &tick);   // snapshot first; then its commands (ecs.md)
        }
        draw.clear();                                        // the RenderFrame resource
        app.schedule.run_frame(&mut app.world, &frame);      // frame.dt, frame.alpha; Extract fills `draw`

        gfx.begin_frame(platform.gpu_surface())?;
        renderer.render(&mut gfx, &app.assets, &draw,
                        Output { target: PassTarget::Surface, size: platform.window_size() })?;
        gfx.end_frame(platform.gpu_surface())?;              // present: GL swap or Metal drawable

        if app.assets.has_pending() {                        // GPU uploads on main thread
            app.assets.finish_completed_loads(&mut FinalizeCtx::new().with(&mut gfx));
        }
        app.dev_hooks();                                     // hot reload, profiler (dev)
        if platform.is_minimized() { std::thread::sleep(MINIMIZED_SLEEP); }
    }
    Ok(())
}
```

As built (ADR-0022), `App::update(elapsed, events)` is everything from
feeding the events to the frame stages, and `App::run` calls it once a
frame before drawing. The snapshot and the extract are the engine's own
systems, `nadir::snapshot` and `nadir::extract_sprites` (see "Stages").

Rules:

- **Fixed tick: 60 Hz.**
  - Gameplay, AI, movement and collision run only in fixed stages, with
    `dt = 1/60`.
  - Animation playback, the camera, UI and cosmetic effects run in frame
    stages.
- **Hitch clamp:** a frame longer than 250 ms is clamped. The game slows down
  briefly instead of running dozens of catch-up ticks (the "spiral of death").
- **The clock is exact and does not read the time.** `FixedClock` is told how
  long each frame took, so the same frame lengths give the same ticks on
  every machine. It accumulates in nanoseconds times the tick rate, which
  makes a tick a whole number of units: the rate does not drift.
- **Interpolation:** `PrevTransform` is copied before each tick. Extract lerps
  `prev → current` by `alpha`. The engine gives a `PrevTransform` to every
  entity with a `Sprite` and to the root of every drawn child; to teleport,
  write both (ADR-0022).
- **Pacing:**
  - Use vsync (swap interval 1) when available.
  - Keep a limiter as well, which sleeps to about 1 ms before the deadline
    and then spins. A driver can accept an interval of 1 and not wait for
    the display: llvmpipe, and XWayland (see `GlContext::set_swap_interval`).
  - The limiter keeps a schedule: each frame is due one interval after the
    last was due. A frame that overruns starts the schedule again from where
    it ended, so no frames hurry to make up for it.
  - On Windows the sleep uses a high-resolution waitable timer; see
    `docs/03-platforms.md`.
- **Minimized windows** sleep instead of spinning, because some drivers stop
  blocking on swap when the window is hidden.
- **Window modal loops** (Windows title-bar drag/resize, macOS live resize)
  are handled in `platform` so the game keeps rendering. See
  `docs/03-platforms.md`.

## Stages

```rust
pub enum FixedStage { Input, Gameplay, Movement, PostMovement, Cleanup }
pub enum FrameStage { Animate, Camera, Ui, Extract }
```

- Systems run in stage order, then insertion order within a stage. The order
  is printed by the debug console (`schedule`).
- `App::new` adds the engine's systems first in their stages (ADR-0022), so a
  game's systems in the same stage run after them:

  | Stage | Engine system | Does |
  | :- | :- | :- |
  | tick `Input` | `nadir::snapshot` | copies `Transform` into `PrevTransform` |
  | tick `Movement` | `nadir::movement` | rebuilds `Collisions`, moves each `Velocity`, sliding `Collider`s |
  | tick `PostMovement` | `nadir::parent_offsets` | puts each `Parent`'s child at its root plus the offsets |
  | tick `PostMovement` | `nadir::triggers` | sends `TriggerEnter` and `TriggerExit` |
  | frame `Extract` | `nadir::extract_sprites` | draws each `Sprite` between the last two ticks |
- Structural changes (spawn, despawn, add or remove component) are deferred
  through `Commands`. They are applied after each fixed tick and after the
  frame stages, never mid-iteration. The one exception is the engine's
  `nadir::snapshot`, first in the tick: it adds a missing `PrevTransform` at
  once, so that a system can write it in an entity's first tick (ADR-0022).
- Events are double-buffered per tick (see `docs/subsystems/ecs.md`).

## Public API sketch (game-facing)

```rust
use nadir::prelude::*;

fn main() -> Result<(), nadir::Error> {
    App::new(Config {
        title: "Sandbox".into(),
        virtual_resolution: Some(UVec2::new(480, 270)),
        ..Config::default()
    })
    .add_plugin(DefaultPlugins)
    .add_fixed_system(FixedStage::Gameplay, "player_move", player_move)
    .add_fixed_system(FixedStage::Gameplay, "enemy_ai", enemy_ai)
    .add_frame_system(FrameStage::Camera, "camera_follow", camera_follow)
    .run()
}

fn player_move(world: &World, tick: &Tick) {
    let input = world.resource::<ActionState<Action>>();
    let players = world.read::<Player>();
    let mut vel = world.write::<Velocity>();
    for (e, _) in players.iter() {
        if let Some(v) = vel.get_mut(e) {
            v.0 = input.axis2(Action::Move) * PLAYER_SPEED;
        }
    }
}
```

Systems are plain functions `fn(&World, &Tick)` (fixed) or
`fn(&World, &Frame)` (frame). Storages use interior mutability with runtime
borrow checks (see the ECS spec). Borrow conflicts panic with the system name
and component type, so they are caught on the first run in tests.

As built in M4.4 (ADR-0022), there are no plugins yet: `App::new` adds the
engine's systems and resources itself. A game's actions come from
`App::add_actions`, its reflected components from
`app.register_component::<T>()`, and its entities from `app.world_mut()`
during setup. `Config::virtual_resolution` is a plain `UVec2`. Closures that
capture nothing coerce to the system type; systems keep their state in
resources.

## Data flow per frame

```
OS events ─► platform::Event ─► input (device state, edge accumulation)
                                   │ latch per tick
                                   ▼
                     ┌─────── fixed tick ×N ────────┐
                     │ gameplay → movement/collision│◄── resources: time, RNG, events
                     │ → triggers → cleanup         │
                     └──────────────┬───────────────┘
                                    ▼
                     frame stages: animate → camera → UI
                                    ▼
             extract (ECS → RenderFrame: sprite instances, tile chunks, text, debug)
                                    ▼
             render (sort, batch, upload, passes) ─► gfx ─► GL / Metal ─► present
                                    │
             audio commands ──► SPSC queue ──► audio thread (mixer) ─► OS device
```

## Threading model

| Thread | Owns | Talks to others via |
| :- | :- | :- |
| Main | Window, GPU context, ECS world, renderer, input | Channels to workers; a command queue to audio |
| Audio (one) | Mixer state, voices, streaming decoders, OS audio device | Command queue in; atomic counters/status out |
| Workers (N = clamp(cores − 1, 1, 4)) | File reads, decode (QOI, QOA, data), dev re-cook | `std::sync::mpsc` jobs in, results out; GPU uploads happen on main |
| Hot-reload watcher (dev only) | Polling of source/cooked file mtimes | Channel of changed paths to main |

- No thread touches the GPU except main.
- No thread touches the ECS except main.
- The simulation is single-threaded by design until profiling proves
  otherwise (ADR-0006).

## Feature flags

| Feature | Crates | Enables |
| :- | :- | :- |
| `dev` | `nadir`, `debug`, `assets`, `platform`, `sandbox` | Debug UI, profiler, console, hot reload, loose-file VFS, allocation counter, extra assertions |
| (none) | — | Release behavior: pack-file VFS, no debug code compiled in |

Run dev builds with `cargo dev` (the alias above). Release builds never
include `dev`. `cargo xtask release-no-dev` checks this: `nadir-debug` must
not appear in the sandbox's non-dev dependency tree.

A member turns a `dev` feature on only from its `[features]` table or a
dev-dependency, never from a normal dependency (M3.5). A build of the whole
workspace unifies features, and `dev` in `nadir-platform` replaces the
global allocator. A dev-dependency reaches only builds of test, bench or
example targets, which a release build does not make. Nothing checks this
rule yet: `release-no-dev` looks only for `nadir-debug`.

## Configuration and user data

- Settings live in `settings.jsonc` in the user config directory (paths per
  OS are in `docs/03-platforms.md`):
  - display mode, vsync, volume per bus, input bindings, language
- The file is versioned. Unknown keys are preserved on save; invalid values
  fall back to defaults with a logged warning, never a crash.
- Saves live in the user data directory: binary format, versioned, written
  atomically (see `docs/subsystems/scene.md`).
- Logs: `nadir.log` plus 3 rotated copies, in the user data directory
  (on macOS, in `~/Library/Logs`). Each run starts a new file. If none can
  be opened, the game runs without one, after a warning.
- The folders are named after `Config::studio` and `Config::game`, which
  must be valid folder names on every OS.
- Crash reports: the `crashes/` directory in user data (see
  `docs/05-release.md`).
