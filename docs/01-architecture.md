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
| 5 — facade | `nadir` (App, main loop, components, ECS glue systems) |
| 4 — tooling inside the engine | `debug` |
| 3 — composed subsystems | `render`, `audio`, `scene` |
| 2 — base subsystems | `gfx`, `ecs`, `assets`, `input`, `collision` |
| 1 — OS behavior and formats | `platform`, `codec` |
| 0 — foundations | `core`, `sys` |

`diagrams/c4-architecture.md` draws these layers and each crate's
dependencies, `diagrams/game-loop-sequence.md` follows the main loop
through the code, and `diagrams/class-diagram.md` shows the main types.

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
| `nadir_engine` (facade, `nadir` in code; ADR-0036, ADR-0039) | all crates above |
| `nadir-cook` | `core`, `codec`, `assets`, `scene` |
| `xtask` | `core`, `codec` |
| `nadir-bench` | any workspace crate |
| `sandbox` | `nadir_engine`, `nadir-debug` (behind `dev` feature) |

## Workspace layout

```
.
├── Cargo.toml                # [workspace], [workspace.package], [workspace.dependencies], [workspace.lints], profiles
├── rust-toolchain.toml       # pinned stable toolchain + rustfmt, clippy
├── clippy.toml               # allow-unwrap-in-tests = true, allow-expect-in-tests = true
├── .cargo/config.toml        # aliases: xtask, dev; static C runtime on Windows MSVC (ADR-0036)
├── crates/{core,sys,platform,gfx,codec,ecs,input,render,audio,assets,collision,scene,debug,engine}/
├── tools/cook/
├── xtask/
├── bench/
├── games/sandbox/
├── assets/                   # source assets for the sandbox
├── tests/golden/             # golden-image references (.qoi)
├── fuzz/corpus/              # seed inputs per parser
├── diagrams/                 # C4, game-loop sequence and class diagrams (Mermaid)
└── docs/
```

`.cargo/config.toml`:

```toml
[alias]
xtask = "run --profile xtask --package xtask --"
dev   = "run --package sandbox --features dev --"

# Windows: every executable links the C runtime in (ADR-0036).
[target.'cfg(all(target_os = "windows", target_env = "msvc"))']
rustflags = ["-C", "target-feature=+crt-static"]
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
// App::run (crates/engine/src/app.rs), with App::update inlined and
// condensed: error handling, the modal frames and the `dev` allocation
// counter are left out.
pub fn run(mut app: App) -> Result<(), Error> {
    log_file::start(&app.config.studio, &app.config.game);  // nadir.log, the last three kept
    let mut platform = Platform::new(&app.config.window())?;
    let mut gfx = Device::new(platform.gpu_surface())?;     // keeps no borrow (ADR-0016)
    let render = RenderConfig { virtual_resolution: app.config.virtual_resolution };
    let mut renderer = Renderer::new(&mut gfx, &render)?;
    let mixer = MixerOutput(app.mixer.take());              // the mixer App::new made
    let stream = open_audio(&AudioOutputConfig::default(),  // the audio thread; on Err a warning,
                            Box::new(mixer));               // and the game runs silent (ADR-0051)
    let mut limiter = FrameLimiter::new(app.config.frame_rate_limit, Instant::now());
    let mut stopwatch = Stopwatch::start();
    let mut events = Vec::with_capacity(256);

    loop {
        platform.pump_events(&mut events);                   // non-blocking
        if events.contains(&Event::CloseRequested) { break; }

        // App::update(elapsed, events): everything a frame does but draw.
        for event in &events { input.handle(event); }        // the Input resource: edges accumulate here
        let frame = app.clock.begin_frame(stopwatch.lap());  // clamps hitches
        while let Some(tick) = app.clock.consume_tick() {
            latch(&mut input, &app.world);                   // edges -> this tick's actions; or a replay's (ADR-0025)
            app.schedule.run_fixed(&mut app.world, &tick);   // snapshot first; then its commands (ecs.md)
        }
        draw.clear();                                        // the RenderFrame resource
        draw.time = simulated_time(&app.clock);              // ticks + alpha: animated tiles (ADR-0023)
        app.schedule.run_frame(&mut app.world, &frame);      // frame.dt, frame.alpha; Extract sends the sound, fills `draw`

        gfx.begin_frame(platform.gpu_surface())?;
        renderer.render(&mut gfx, &assets, &tiles, &draw,    // the AssetServer and TileLayers resources
                        Output { target: PassTarget::Surface, size: platform.window_size() })?;
        gfx.end_frame(platform.gpu_surface())?;              // present: the GL swap
        if assets.has_pending() {                            // GPU uploads on the main thread, within a budget
            assets.finish_completed_loads(&mut FinalizeCtx::new().with(&mut gfx));
        }
        events.clear();

        if let Some(due) = limiter.end_of_frame(Instant::now()) {
            precise_sleep(due);                              // the frame limiter: "Pacing", below
        }
    }
    drop(stream);                                           // the sound stops, 2 s at most, before an error shows
    Ok(())
}
```

As built (ADR-0022), `App::update(elapsed, events)` is everything from
feeding the events to the frame stages, and `App::run` calls it once a
frame before drawing. If `Device::new` fails, `App::run` logs it and runs
the loop without drawing; any other error ends the loop, and is shown to
the player (`nadir_platform::show_error_dialog`) before `run` returns it.
The snapshot and the extract are the engine's own systems,
`nadir::snapshot` and `nadir::extract_sprites` (see "Stages").
`App::offscreen(size)` runs the same frames with no window: each
`Offscreen::frame` updates, draws into a render target on an offscreen
context, makes the loaded assets, and returns the pixels. The sandbox's
`--capture` records the demo page's scenes with it (ADR-0029).

Sound (M6.6, ADR-0051): `App::new` makes the mixer and the `Audio`
resource, and `App::run` hands the mixer to the audio output once the
window and the device are open. Each frame, the engine's `nadir::sound`,
in `Extract`, sends the listener, the emitters and the music areas to the
mixer and calls `Audio::update`. Only `App::run` opens the output:
`App::update`'s first call drops a mixer the app still holds, so tests,
headless runs and `App::offscreen` are silent.

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
- **Minimized windows** are paced like any other window. Some drivers stop
  blocking on swap when the window is hidden; with `Config::frame_rate_limit`
  set (60 by default), the limiter still holds the rate, and with no limit a
  hidden window runs unpaced. The loop does not ask whether the window is
  minimized, because `Platform` cannot tell yet.
- **Window modal loops** (Windows title-bar drag/resize, macOS live resize)
  are handled in `platform`, which calls back into the loop
  (`Platform::set_modal_frame_callback`), so the ticks and frame stages keep
  running. Those frames are unpaced, and are not drawn until `ModalFrame`
  can present (M2.6, M2.7). See `docs/03-platforms.md`.

## Stages

```rust
pub enum FixedStage { Input, Gameplay, Movement, PostMovement, Cleanup }
pub enum FrameStage { Animate, Camera, Ui, Extract }
```

- Systems run in stage order, then insertion order within a stage. The main
  loop logs the order at debug level when it starts (`Schedule::describe`),
  and the debug console's `schedule` command will print it (M7.5).
- `App::new` adds the engine's systems first in their stages (ADR-0022), so a
  game's systems in the same stage run after them:

  | Stage | Engine system | Does |
  | :- | :- | :- |
  | tick `Input` | `nadir::snapshot` | copies `Transform` into `PrevTransform` |
  | tick `Movement` | `nadir::movement` | rebuilds `Collisions`, moves each `Velocity`, sliding `Collider`s |
  | tick `PostMovement` | `nadir::parent_offsets` | puts each `Parent`'s child at its root plus the offsets |
  | tick `PostMovement` | `nadir::triggers` | sends `TriggerEnter` and `TriggerExit` |
  | frame `Animate` | `nadir::animate` | advances each `Animator` by the clock's step, writes its frame into its `Sprite`, sends `AnimationFinished`, mirrors each Sprite that faces left and unmirrors the rest with a `Facing` (ADR-0024) |
  | frame `Extract` | `nadir::sound` | puts the listener at the first `AudioListener` or the camera, plays each `AudioEmitter` at its entity between the last two ticks, sends the track of the `MusicArea` the listener is in, then calls `Audio::update` (ADR-0051) |
  | frame `Extract` | `nadir::extract_sprites` | draws each `Sprite` between the last two ticks |
  | frame `Extract` | `nadir::extract_level` | draws the `CurrentLevel`'s tile layers, its background the clear colour |
- Structural changes (spawn, despawn, add or remove component) are deferred
  through `Commands`. They are applied after each fixed tick and after the
  frame stages, never mid-iteration. The one exception is the engine's
  `nadir::snapshot`, first in the tick: it adds a missing `PrevTransform` at
  once, so that a system can write it in an entity's first tick (ADR-0022).
- Each event goes into two buffers (see `docs/subsystems/ecs.md`). The
  tick's is cleared as each tick starts, so a tick system sees what its own
  tick sent before it ran; the frame's holds everything sent since the last
  frame, across its ticks, and is cleared after the frame stages.

## Public API sketch (game-facing)

```rust
use nadir::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Move,
}

impl ActionSet for Action {
    const ALL: &'static [Self] = &[Action::Move];

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        "move"
    }

    fn kind(self) -> ActionKind {
        ActionKind::Axis2
    }
}

const PLAYER_SPEED: f32 = 120.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut actions = ActionMap::<Action>::new()?;
    actions.bind(
        Action::Move,
        0,
        Binding::KeyComposite2 { up: Key::W, down: Key::S, left: Key::A, right: Key::D },
    )?;

    let mut app = App::new(Config {
        title: "Sandbox".into(),
        virtual_resolution: UVec2::new(480, 270),
        ..Config::default()
    })
    .add_actions(actions)
    .add_fixed_system(FixedStage::Gameplay, "player_move", player_move)
    .add_frame_system(FrameStage::Camera, "camera_follow", camera_follow);

    let world = app.world_mut();
    let hero = world.resource_mut::<AssetServer>().load::<Texture>("hero.qoi");
    world
        .spawn()
        .with(Player)
        .with(Transform::at(Vec2::new(240.0, 135.0)))
        .with(Velocity::default())
        // Drawn, so the engine keeps a PrevTransform for it too.
        .with(Sprite::new(hero, Vec2::new(16.0, 16.0)));

    app.run()?;
    Ok(())
}

fn player_move(world: &World, _tick: &Tick) {
    let input = world.resource::<ActionState<Action>>();
    let players = world.read::<Player>();
    let mut velocities = world.write::<Velocity>();
    for (player, _) in players.iter() {
        if let Some(velocity) = velocities.get_mut(player) {
            velocity.0 = input.axis2(Action::Move) * PLAYER_SPEED;
        }
    }
}

fn camera_follow(world: &World, frame: &Frame) {
    let Some(player) = world.read::<Player>().entities().first().copied() else {
        return;
    };
    let Some(now) = world.read::<Transform>().get(player).map(|it| it.position) else {
        return;
    };
    // Where the last tick started: the camera moves between ticks, as the
    // sprite is drawn.
    let then = world.read::<PrevTransform>().get(player).map_or(now, |it| it.0.position);
    world.resource_mut::<RenderFrame>().camera.position = then.lerp(now, frame.alpha);
}
```

Systems are plain functions `fn(&World, &Tick)` (fixed) or
`fn(&World, &Frame)` (frame). Storages use interior mutability with runtime
borrow checks (see the ECS spec). Borrow conflicts panic with the system name
and component type, so they are caught on the first run in tests.

There are no plugins (ADR-0022): `App::new` adds the engine's systems and
resources itself, `nadir::movement` among them, which moves the player by
the `Velocity` that `player_move` sets. A game adds its actions with
`App::add_actions`, its reflected components with
`App::register_component::<T>()`, and its entities through
`App::world_mut()` during setup. Closures that capture nothing coerce to the
system type; systems keep their state in resources. The facade's README
(`crates/engine/README.md`) has a fuller game, with sprites, tiles and
collision, and the sandbox (`games/sandbox/src/main.rs`) is the engine's
own.

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

             extract's nadir::sound ──► command queue ──► audio thread (mixer) ─► OS device
```

## Threading model

| Thread | Owns | Talks to others via |
| :- | :- | :- |
| Main | Window, GPU context, ECS world, renderer, input | Channels to workers; a command queue to audio |
| Audio (one), `nadir-audio`: started by `nadir_platform::open_audio` (on Linux from M6.3, ADR-0047), which calls the mixer through `AudioRender`; the facade's `App::run` starts it with the game's mixer (M6.6, ADR-0051) | Mixer state, voices, streaming decoders, OS audio device | Command queue in; atomic counters/status out; logs only when the device is lost and when it opens again |
| Workers (N = clamp(cores − 2, 1, 4), the asset server's, ADR-0030) | File reads, decode (QOI, QOA, data) | `std::sync::mpsc` jobs in, results out; GPU uploads happen on main |
| Hot-reload watcher, `nadir-assets-watch` (M5.16; the facade starts it under `dev` in `App::new`, M5.8, ADR-0042) | Polling of the mtimes of the loaded cooked files every 250 ms; `cargo xtask cook --watch` cooks, in its own process | Channel of changed paths to main |
| Shader watcher, `nadir-shader-watch` (M5.8; the facade's, under `dev`, from `App::run`, ADR-0043) | Polling of the mtimes of render's shader files every 250 ms | An atomic mask of the changed programs, read by main |

- No thread touches the GPU except main.
- No thread touches the ECS except main.
- The simulation is single-threaded by design until profiling proves
  otherwise (ADR-0006).

## Feature flags

| Feature | Crates | Enables |
| :- | :- | :- |
| `dev` | `nadir`, `platform`, `sandbox` | A failed asset shows its placeholder; the allocation counter is the global allocator, read through `FrameAllocs` (and `thread_alloc_counts`, `process_alloc_counts`); hot reload: `App::run` reloads the loose files and shaders that change (M5.8, ADR-0043); the debug UI: the windows a game adds with `App::add_debug_window`, each on a key of its own, drawn over the game's UI in the engine's font, holding the game's input back while they have the pointer or a field, their places kept in `debug_ui.jsonc` (M7.10, ADR-0058). Still to join it: the profiler, the console and the other tools (M7.3 to M7.6) |
| (none) | — | Release behavior: a failed asset shows nothing, there is no allocation counter, and no debug code is compiled in. Assets load from the cook's pack, `game.kpak`, in `Config::assets`, or from its loose files where there is no pack (M5.7, ADR-0041); with `dev`, from the loose files |

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
- Development builds (`dev`) keep the debug UI's windows' places in
  `debug_ui.jsonc` beside it, which only `App::run` reads and writes
  (M7.10, ADR-0058).
- Saves live in the user data directory: binary format, versioned, written
  atomically (see `docs/subsystems/scene.md`).
- Logs: `nadir.log` plus 3 rotated copies, in the user data directory
  (on macOS, in `~/Library/Logs`). Each run starts a new file. If none can
  be opened, the game runs without one, after a warning.
- The folders are named after `Config::game`; on Windows they sit inside a
  folder named after `Config::studio` (`nadir_platform::user_dirs`). Both
  must be valid folder names on every OS.
- Crash reports: the `crashes/` directory in user data (see
  `docs/05-release.md`).
