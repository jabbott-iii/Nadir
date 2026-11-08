# Game loop sequence diagrams

These diagrams follow the game loop through the code: how the sandbox sets
up and opens its window, what one pass of the main loop does, from the
window's events through the 60 Hz ticks and the frame stages to the draw
and the swap, and how the game shuts down. They reflect the code at commit
`0cba65f` (version 0.2). The simulation and the drawing share the main
thread: a frame runs its ticks, then its frame stages, then draws. Only the
asset workers run beside it.

How to read them:

- A box is a thread. The main thread is the one that calls `App::run`.
- A solid arrow is a call, and a dashed arrow its answer. A solid arrow
  with an open head sends a job or a result through a channel to another
  thread, without waiting.
- `nadir::snapshot` and the like are the engine's systems, named as the
  schedule lists them. `move_player`, `count_ticks`, `follow_camera` and
  `report` are the sandbox's, in its default scene, the field.
- The other names are types and functions of the code.

## 1. Setup, before the window opens

`App::new` builds the world, the schedule and the `AssetServer`, which reads
loose files with the `dev` feature, and without it the pack `game.kpak` when
there is one. The sandbox then loads its atlas and the level `field` before
`App::run` opens anything, so the workers decode their textures with no GPU
device to make them yet. Then it calls `App::run`, or, with the capture or
headless option, runs frames with no window (section 10).

```mermaid
sequenceDiagram
    box Main thread
        participant Main as sandbox main
        participant App as App
        participant Pack as engine pack
        participant Srv as AssetServer
        participant Lvl as engine level
    end
    box Asset worker threads
        participant Wk as Workers
    end

    Main->>Main: Options::parse, a Config
    Main->>App: App::new(config)
    App->>App: World::new, insert Input
    App->>Pack: asset_server(assets, on_failure, read_pack)
    alt dev feature
        Pack->>Srv: AssetServer::new, a LooseSource
    else no dev, and game.kpak opens
        Pack->>Srv: with_source(PackSource)
    else no dev, and there is no pack
        Pack->>Srv: AssetServer::new, a LooseSource
    else no dev, and the pack does not open
        Pack->>Srv: with_source(Unopened)
    end
    Srv->>Wk: Workers::new: 1 to 4 threads
    App->>App: the other resources, the events, the TypeRegistry
    App->>App: Schedule: the engine's 7 systems
    App->>App: FixedClock::new(60 Hz, 250 ms)
    App-->>Main: app
    Main->>Srv: Sprites::load: load_now an Atlas
    Srv->>Srv: decode and make it now, no GPU needed
    Main->>Srv: load_pages: load_in each page
    Srv-)Wk: a decode job for each page
    Main->>App: Clips add_atlas, add_actions
    Main->>App: add move_player, count_ticks, report
    Main->>App: collision_layers(World, People)
    Main->>Lvl: App::load_level("field")
    Lvl->>Lvl: unload_level: the last level goes
    Lvl->>Srv: load_now_in levels/field.klvl
    Lvl->>Srv: load_now_in each prefab
    Lvl->>Srv: load_now_in each tile set's Atlas
    Srv-)Wk: jobs for the level's textures
    Lvl->>App: Clips add_atlas for each tile set
    Lvl->>App: spawn the tall tiles and the prefabs
    Lvl->>App: TileLayers add, Collisions set_grid
    Lvl->>App: CurrentLevel keeps what it made
    Main->>App: spawn_player, add follow_camera
    opt the record or replay option
        Main->>App: add the recording, or play_replay
    end
    alt the capture or headless option
        Main->>App: frames with no window (section 10)
    else a window
        Main->>App: run (section 2)
    end
```

## 2. Startup: the window, the GPU and the loop

`App::run` opens the log file, the window and its OpenGL context, then the
device and the renderer, and enters `run_loop`. Windows and macOS have no
OpenGL context yet (roadmap M2.6 and M2.7), so there the game runs undrawn.

```mermaid
sequenceDiagram
    box Main thread
        participant Main as sandbox main
        participant Run as App::run
        participant Plat as Platform, X11 Backend
        participant Egl as WindowGl, EGL
        participant Dev as gfx Device
        participant Rend as Renderer
        participant RL as App::run_loop
    end

    Main->>Run: run()
    Run->>Run: log_file::start: nadir.log
    Run->>Plat: Platform::new(config.window())
    Plat->>Plat: XOpenDisplay, scale from Xft.dpi
    Plat->>Egl: WindowGl::open
    Egl->>Egl: load libEGL, eglInitialize
    Egl->>Egl: eglChooseConfig
    Plat->>Plat: create_window, WM_DELETE_WINDOW
    Plat->>Egl: make_current(window, vsync)
    Egl->>Egl: eglCreateWindowSurface, sRGB
    Egl->>Egl: eglCreateContext, eglMakeCurrent
    Egl->>Egl: check 3.3 core, eglSwapInterval
    Plat->>Plat: XMapWindow shows the window
    Plat-->>Run: the Platform, or an error for fatal
    Run->>Dev: Device::new(gpu_surface)
    alt Linux: an OpenGL 3.3 core context
        Dev->>Dev: GlFns::load, DeviceClaim::take
        Run->>Rend: Renderer::new(device, 480 x 270)
        Rend->>Dev: create_pipeline x 3, create_buffer x 3
        Rend->>Dev: create_render_target: virtual image
        Note over Run,Rend: Graphics holds the Renderer and the Device
    else Windows, macOS: a stand-in context
        Dev-->>Run: Err MissingFunction
        Run->>Run: no Graphics: nothing is drawn
    end
    Run->>RL: run_loop(platform, graphics)
    RL->>RL: debug log the systems in order
    RL->>RL: FrameLimiter::new, Stopwatch::start
    RL->>RL: EngineCore in an Rc RefCell
    RL->>Plat: set_modal_frame_callback
    Note over Plat: X11 drops it. Windows, macOS keep it.
    RL->>RL: the first pass (section 3)
```

The first frame's lap is only the time since `Stopwatch::start` in
`run_loop`, well under a tick, so it runs no tick. The textures queued in
setup are made at the end of a drawn frame, once a worker has decoded them
and within the 2 ms budget, so they show from the second frame at the
earliest.

## 3. One pass of the main loop

`run_loop` pumps the window's events, runs one frame through
`EngineCore::frame`, which calls `App::update` (sections 4 to 6) and
`Graphics::draw` (section 7), and then waits for the frame limiter. A close
request ends the loop before its frame runs.

```mermaid
sequenceDiagram
    box Main thread
        participant RL as App::run_loop
        participant Plat as Platform
        participant Core as EngineCore
        participant Upd as App::update
        participant Gfx as Graphics
        participant Lim as FrameLimiter
    end

    loop until the window asks to close
        RL->>Plat: pump_events(events), no wait
        Plat->>Plat: XPending, XNextEvent, Translator
        opt Windows, macOS: a drag or a resize
            Plat->>Core: the modal frame callback
            Core->>Upd: a frame: no events, not drawn
        end
        Plat-->>RL: events
        break the events hold CloseRequested
            RL->>RL: leave the loop (section 9)
        end
        RL->>Plat: gpu_surface, window_size
        RL->>Core: frame(events, surface, size)
        Core->>Core: elapsed: the Stopwatch lap
        Core->>Upd: update(elapsed, events)
        opt there is Graphics
            Core->>Gfx: draw(world, surface, size)
            Gfx-->>Core: Ok, or an error kept in EngineCore
        end
        opt dev feature
            Core->>Core: FrameAllocs of this frame
        end
        break EngineCore holds an error
            RL->>RL: return Err (section 9)
        end
        RL->>RL: events clear
        RL->>Lim: end_of_frame(now)
        alt a limit, and the frame ended early
            Lim-->>RL: due: one interval after the last
            RL->>RL: precise_sleep(due)
        else no limit, or the frame overran
            Lim-->>RL: None, and an overrun restarts the schedule now
        end
    end
```

On Windows the modal callback runs every 10 to 16 ms while the window is
dragged or resized, and on macOS once for each step of a resize. `precise_sleep`
sleeps until 1 ms before the due time, then spins.

## 4. The ticks of a frame: App::update

`App::update` feeds the events to `Input`, then runs every tick that is due
at 60 Hz, each with its own latch of the actions, and then the frame stages
once. `FixedClock` counts a frame for at most 250 ms, so a stall runs at
most 15 ticks, and the rest of it is lost.

```mermaid
sequenceDiagram
    box Main thread
        participant Upd as App::update
        participant Inp as Input
        participant Clk as FixedClock
        participant Lt as Actions latch
        participant Rp as ReplayInput
        participant Sch as Schedule
        participant Wld as World
    end

    Upd->>Inp: handle each Event
    Upd->>Clk: begin_frame(elapsed)
    Clk->>Clk: cut to 250 ms, add to the accumulator
    Clk-->>Upd: Frame: dt, alpha
    loop while consume_tick gives a Tick
        alt a replay plays
            Upd->>Rp: latch(input, world)
            Rp->>Inp: latch into a set of its own
            Rp->>Wld: swap in the replay's ActionState
            Rp->>Rp: next_tick: read one tick ahead
            Upd->>Wld: Replay Playing
        else live input
            Upd->>Lt: latch(input, world)
            Lt->>Inp: latch(ActionMap, ActionState)
        end
        Upd->>Sch: run_fixed(world, tick)
        opt the tick was replayed
            Upd->>Upd: end_replay_tick
        end
        Upd->>Upd: count it in TickStats
    end
    Upd->>Wld: set TickStats
    Upd->>Wld: RenderFrame clear, set its time
    Upd->>Sch: run_frame(world, frame)
    Upd->>Upd: profile::take_frame
    Note over Upd,Wld: 16.67 ms runs 1 tick. 40 ms runs 2, at alpha 0.4. 1 s counts as 250 ms: 15 ticks.
```

`Input` keeps the key and button changes in order, so a latch gives each
tick its own presses and releases, and a tap between two ticks still
counts. Before `add_actions`, and while a replay plays, the live input is
latched into a set of its own that nothing reads. `end_replay_tick` sets
`Replay` to `Ended` or `Failed` when no tick follows, and the input is live
from then on. The `RenderFrame`'s time is the ticks so far plus `alpha`.

## 5. One tick: Schedule::run_fixed

The tick stages run in order, and in each stage the engine's systems come
before the game's, one at a time. The commands they queued apply once,
after the last stage.

```mermaid
sequenceDiagram
    box Main thread
        participant Sch as Schedule::run_fixed
        participant Eng as engine glue
        participant Game as sandbox systems
        participant Col as Collisions
        participant Wld as World
    end

    Sch->>Wld: clear_tick_events
    Note over Sch,Wld: FixedStage::Input
    Sch->>Eng: nadir::snapshot
    Eng->>Wld: each PrevTransform is its Transform
    Note over Sch,Wld: FixedStage::Gameplay
    Sch->>Game: move_player
    Game->>Wld: Velocity, Facing, Animator from Move
    Note over Sch,Wld: FixedStage::Movement
    Sch->>Eng: nadir::movement
    Eng->>Col: rebuild from Collider and Transform
    Eng->>Col: move_colliders: move_and_slide
    Col->>Wld: write the slid Transforms
    Eng->>Wld: move the rest freely
    Note over Sch,Wld: FixedStage::PostMovement
    Sch->>Eng: nadir::parent_offsets
    Eng->>Wld: each child at its root plus offset
    Sch->>Eng: nadir::triggers
    Eng->>Col: sync, update_triggers
    Col->>Wld: send TriggerEnter, TriggerExit
    Note over Sch,Wld: FixedStage::Cleanup
    Sch->>Game: count_ticks
    Game->>Wld: Report ticks plus 1
    Sch->>Wld: apply_commands
```

`nadir::snapshot` also gives a `PrevTransform` to each drawn entity that
has none. `move_colliders` moves the entities with a `Collider` and a
`Velocity` in collider order, each seeing where the ones before it went.

## 6. The frame stages: Schedule::run_frame

After the ticks, the frame stages run once. They animate, put the camera
between the last two ticks, and fill the `RenderFrame` that section 7
draws.

```mermaid
sequenceDiagram
    box Main thread
        participant Sch as Schedule::run_frame
        participant Eng as engine glue and level
        participant Game as sandbox systems
        participant Wld as World
        participant RF as RenderFrame
    end

    Note over Sch,RF: FrameStage::Animate
    Sch->>Eng: nadir::animate
    Eng->>Wld: advance each Animator by the time
    Eng->>Wld: write its frame into the Sprite
    Eng->>Wld: send AnimationFinished, flip by Facing
    Note over Sch,RF: FrameStage::Camera
    Sch->>Game: follow_camera
    Game->>RF: camera on the player, lerp by alpha
    Note over Sch,RF: FrameStage::Ui
    Sch->>Game: report
    Game->>Wld: count the frame, once a second log the stats
    Note over Sch,RF: FrameStage::Extract
    Sch->>Eng: nadir::extract_sprites
    Eng->>RF: a SpriteDraw for each Sprite, lerped
    Sch->>Eng: nadir::extract_level
    Eng->>RF: the level's background and tile layers
    Sch->>Wld: apply_commands
    Sch->>Wld: clear_frame_events
```

`nadir::animate` advances by how far `RenderFrame` time moved since the
last frame, and writes the clip frame's texture and uv, and its size and
pivot when the frame has a placement. A
sprite is drawn `alpha` of the way from its `PrevTransform` to its
`Transform`, and a child where its root is drawn, plus its offset.

## 7. Drawing, presenting, and the finished loads

`Graphics::draw` renders the `RenderFrame` into the virtual image, scales
it into the window by the largest whole factor, and swaps. Then, on the
main thread, it makes the assets that the workers have decoded, within a
2 ms budget.

```mermaid
sequenceDiagram
    box Main thread
        participant Gfx as Graphics
        participant Rend as Renderer
        participant Bat as SpriteBatcher, Plan
        participant Tc as TileCache
        participant Dev as gfx Device
        participant Ctx as GlContext
        participant Srv as AssetServer
    end

    Gfx->>Dev: begin_frame(surface)
    Gfx->>Rend: render(device, assets, tiles, frame)
    Rend->>Bat: build(sprites, camera, textures)
    Bat->>Bat: skip a sprite whose Texture waits
    Bat->>Bat: sort_key: layer, depth, texture
    Bat->>Bat: sort, batch runs of one texture
    Bat->>Bat: push_instance for each one drawn
    Rend->>Tc: evict the layers gone
    Rend->>Tc: prepare the visible chunks
    Tc->>Dev: write_buffer the changed chunks
    Rend->>Rend: steps: tile layers among batches
    Rend->>Dev: write_buffer the sprite instances
    Rend->>Dev: begin_pass(the virtual image)
    loop each step
        alt a Sprites step
            Rend->>Dev: sprite pipeline, texture, draw
        else a Tiles step
            Rend->>Dev: tile_chunk pipeline, each chunk
        end
    end
    Rend->>Dev: begin_pass(Surface), blit, upscaled
    Rend-->>Gfx: FrameStats
    Gfx->>Gfx: set the FrameStats resource
    Gfx->>Dev: end_frame(surface)
    Dev->>Ctx: swap_buffers: eglSwapBuffers
    Gfx->>Srv: has_pending
    opt loads or unloads wait
        Gfx->>Srv: finish_completed_loads
        Srv->>Srv: try_recv, free the unloaded
        Srv->>Dev: Texture finalize: create_texture
    end
```

The depth in a sort key is the pivot's y for a y-sorted sprite, or a fixed
depth. Ties keep their submission order. A chunk goes to the GPU the first
time the view shows it, and again only after one of its tiles changes. Each
draw is one `glDrawArraysInstanced`.

## 8. The asset worker threads

A load returns a handle at once. A worker reads and decodes the file, and
the main thread makes the asset at the end of a later drawn frame (section
7).

```mermaid
sequenceDiagram
    box Main thread
        participant Srv as AssetServer
        participant Sto as Assets of T
    end
    box Asset worker threads
        participant Wk as nadir-assets-N
        participant Src as Source
    end

    Srv->>Sto: load_in: a Decoding slot, job: its read's number
    Srv-)Wk: Workers::run: the job on the queue
    par the main thread goes on
        Note over Srv,Sto: the main thread runs ticks, frame stages, draws
    and a worker takes the job
        Wk->>Wk: lock the queue, recv
        Wk->>Src: read(path)
        Src-->>Wk: the bytes
        Wk->>Wk: T::decode
        Wk-)Sto: the handle, the read's number, the result
    end
    Srv->>Sto: a drawn frame: finish, try_recv
    Sto->>Sto: finalize: upload a Texture
    Note over Srv,Src: No worker touches the GPU or the World.
```

A `LooseSource` reads a file. A `PackSource` reads an entry of the pack, in
turns with the other workers, and checks and decompresses it. A panic in a
read or a decode fails the asset, and with no worker thread a job runs on
the thread that asked.

## 9. Shutdown

The loop ends on a close request or on a frame's error, which is logged and
shown before the window goes. `EngineCore` drops first, the app before the
GPU device, and then the `Platform` closes the window.

```mermaid
sequenceDiagram
    box Main thread
        participant Main as sandbox main
        participant Run as App::run
        participant RL as App::run_loop
        participant Core as EngineCore
        participant Srv as AssetServer
        participant Plat as Platform
    end
    box Asset worker threads
        participant Wk as Workers
    end

    alt the player closes the window
        RL->>Plat: pump_events
        Plat->>Plat: a WM_DELETE_WINDOW message: CloseRequested
        Plat-->>RL: CloseRequested
        RL->>RL: break, before the frame
    else a frame failed to draw
        Core-->>RL: the error, after the frame
    end
    RL->>Core: drop EngineCore
    Core->>Srv: drop the App: the World, its AssetServer
    Srv->>Wk: Workers drop: stop, close, join
    Core->>Core: drop Graphics: Renderer, then Device
    RL-->>Run: Ok, or the Err
    opt an Err
        Run->>Run: fatal: log, show_error_dialog
    end
    Run->>Plat: drop the Platform
    Plat->>Plat: release EGL, destroy the window
    Plat->>Plat: eglTerminate, XCloseDisplay
    Run-->>Main: Ok, or the Err
    opt the record option
        Main->>Main: report the ticks and the hash
    end
```

A worker finishes the job it is running, and the queued ones are dropped
unrun. The `Device` deletes its GL objects while the context is still
current. This is the Linux order: on Windows and macOS the modal callback
holds a second `Rc` of the `EngineCore`, which then drops with the
`Platform`.

## 10. Frames without a window

Tests, benchmarks and two of the sandbox's options run frames with no
window. `App::update` alone draws nothing, so no load finishes, while
`App::offscreen` draws into a render target and waits for every load, so
that a recording is the same each time.

```mermaid
sequenceDiagram
    box Main thread
        participant Sb as sandbox capture, replay
        participant Upd as App::update
        participant Ofs as Offscreen
        participant Gfx as Graphics
        participant Dev as gfx Device
        participant Srv as AssetServer
    end

    alt the headless option
        loop until Replay ends or fails
            Sb->>Upd: update(16.67 ms, no events)
        end
        Sb->>Sb: log the ticks and the hash
    else the capture option
        Sb->>Ofs: App::offscreen(size)
        Ofs->>Dev: Device::new_headless: a surfaceless EGL context
        Ofs->>Dev: Renderer::new, create_render_target(size)
        Sb->>Ofs: frame(0 s): the setup's assets
        loop each captured frame
            Sb->>Ofs: frame(16.67 ms, events)
            Ofs->>Upd: update(elapsed, events)
            Ofs->>Gfx: draw_offscreen
            Gfx->>Dev: the frame, into the target
            opt loads or unloads wait
                Gfx->>Srv: finish_all_loads: wait, make all
            end
            Gfx->>Dev: read_pixels(target)
            Ofs-->>Sb: RGBA8 pixels, for a file
        end
    end
```

## Not shown, because it is not built yet

- The audio thread and its mixer (roadmap M6).
- The game reloading what changed (M5.8). nadir-assets' reload behind a
  handle and its watcher thread (M5.16), and nadir-render's program
  rebuilt from new sources (M5.17), are built, but the facade does not
  start or call them yet, so the loop does not reach them.
- Gamepads (M8), bitmap text (M7.1), and the debug UI, profiler and console
  (M7.2 to M7.6).
- Drawing on Windows and macOS (M2.6, M2.7), and the Metal backend (M10).
  There the loop runs as section 3 shows, with no `Graphics`.

## Sources

- `crates/engine/src/app.rs`, `glue.rs`, `level.rs`, `pack.rs`,
  `limiter.rs`, `replay.rs`, `tick_stats.rs`, `frame_allocs.rs`,
  `collisions.rs`, `animation.rs`, `log_file.rs`
- `crates/core/src/time.rs`
- `crates/ecs/src/schedule.rs`, `world.rs`, `events.rs`
- `crates/input/src/input.rs`
- `crates/assets/src/server.rs`, `assets.rs`, `workers.rs`, `source.rs`
- `crates/render/src/renderer.rs`, `batcher.rs`, `plan.rs`, `sort.rs`,
  `frame.rs`, `chunks.rs`, `texture.rs`, `atlas.rs`
- `crates/gfx/src/device.rs`, `gl/mod.rs`
- `crates/platform/src/lib.rs`, `sleep.rs`, `linux/x11.rs`,
  `linux/x11/input.rs`, `linux/x11/connection.rs`, `linux/egl.rs`
- `games/sandbox/src/main.rs`, `replay.rs`, `capture.rs`
