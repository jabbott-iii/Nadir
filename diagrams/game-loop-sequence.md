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
  schedule lists them. `move_player`, `count_ticks`, `footsteps`,
  `follow_camera` and `report` are the sandbox's, in its default scene,
  the field.
- The other names are types and functions of the code.

## 1. Setup, before the window opens

`App::new` builds the world, the schedule and the `AssetServer`, which reads
loose files with the `dev` feature, and without it the pack `game.kpak` when
there is one, and the mixer, which it keeps until `App::run`. The sandbox
then loads its atlas, its sounds and the level `field` before `App::run`
opens anything, so the workers decode their textures with no GPU device to
make them yet. Then it calls `App::run`, or, with the capture or headless
option, runs frames with no window (section 10).

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
    App->>App: nadir_audio::new: Audio, AudioState, each bus's volume queued, the Mixer kept
    App->>App: Schedule: the engine's 8 systems
    App->>App: FixedClock::new(60 Hz, 250 ms)
    App-->>Main: app
    Main->>Srv: Sprites::load: load_now an Atlas
    Srv->>Srv: decode and make it now, no GPU needed
    Main->>Srv: load_pages: load_in each page
    Srv-)Wk: a decode job for each page
    Main->>App: Clips add_atlas
    Main->>Srv: load_now the step's sound
    Main->>App: add_actions, add move_player, count_ticks, footsteps, report
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
    Main->>Srv: load_now the pond's sound and the two tunes
    Main->>App: spawn_player, the two MusicAreas, the pond's AudioEmitter
    Main->>App: add follow_camera
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
device and the renderer, then the audio output with the mixer, and enters
`run_loop`. Windows and macOS have no OpenGL context yet (roadmap M2.6 and
M2.7), so there the game runs undrawn, and no audio output (M6.4, M6.5),
so it runs silent.

```mermaid
sequenceDiagram
    box Main thread
        participant Main as sandbox main
        participant Run as App::run
        participant Plat as Platform, X11 Backend
        participant Egl as WindowGl, EGL
        participant Dev as gfx Device
        participant Rend as Renderer
        participant Out as open_audio
        participant RL as App::run_loop
    end
    box Audio thread
        participant Thr as nadir-audio
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
    alt the App still holds the mixer
        Run->>Out: open_audio(48 kHz stereo, MixerOutput(mixer))
        alt Linux, with libasound
            Out-)Thr: spawn it: the mixer moves there
            Thr->>Thr: snd_pcm_open default, set_params, or none yet
            Thr-->>Out: opened, or not, within 2 s
            Out-->>Run: the AudioStream
        else Windows, macOS, or no libasound
            Out-->>Run: Err: a warning, the game runs silent
        end
    else taken, or an update ran first
        Run->>Run: log it: the game runs silent
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
            opt dev feature, and the frame drew
                Core->>Core: HotReload::frame (section 11)
            end
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
    Sch->>Game: footsteps
    Game->>Wld: every 12 ticks the player moves, Audio play_at its feet
    Sch->>Wld: apply_commands
```

`nadir::snapshot` also gives a `PrevTransform` to each drawn entity that
has none. `move_colliders` moves the entities with a `Collider` and a
`Velocity` in collider order, each seeing where the ones before it went.

## 6. The frame stages: Schedule::run_frame

After the ticks, the frame stages run once. They animate, put the camera
between the last two ticks, send the frame's sound to the mixer (section
12), and fill the `RenderFrame` that section 7 draws.

```mermaid
sequenceDiagram
    box Main thread
        participant Sch as Schedule::run_frame
        participant Eng as engine glue and level
        participant Game as sandbox systems
        participant Wld as World
        participant RF as RenderFrame
        participant Aud as Audio
    end

    Note over Sch,Aud: FrameStage::Animate
    Sch->>Eng: nadir::animate
    Eng->>Wld: advance each Animator by the time
    Eng->>Wld: write its frame into the Sprite
    Eng->>Wld: send AnimationFinished, flip by Facing
    Note over Sch,Aud: FrameStage::Camera
    Sch->>Game: follow_camera
    Game->>RF: camera on the player, lerp by alpha
    Note over Sch,Aud: FrameStage::Ui
    Sch->>Game: report
    Game->>Wld: count the frame, once a second log the stats
    Note over Sch,Aud: FrameStage::Extract
    Sch->>Eng: nadir::sound
    Eng->>Aud: set_listener: the AudioListener, or the camera
    Eng->>Aud: start, place, change or stop each AudioEmitter
    Eng->>Aud: play_track of the MusicArea the listener is in
    Eng->>Aud: update: drop what the mixer is done with
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
`nadir::sound` places the listener and the emitters where they are drawn,
the same way. An emitter starts once its sound has loaded and it is within
800 px of the listener, and stops a little past that, or when its entity
or the emitter goes.

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
    opt the frame has UI
        Rend->>Dev: sprite or text pipeline, a scissor a view, draw in push order
    end
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
GPU device, then the audio output, before the error is shown, and then the
`Platform` closes the window.

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
    box Audio thread
        participant Thr as nadir-audio
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
    opt there is an AudioStream
        Run->>Thr: drop it: the stop flag, then join, 2 s at most
        Thr->>Thr: snd_pcm_close, drop the mixer
    end
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
that a recording is the same each time. Neither plays sound: the first
`App::update` drops the mixer that `App::new` made, so no audio output
opens, and `Audio` does nothing.

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

    Note over Sb,Srv: The first App::update drops the mixer: silent from then on.
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

## 11. Hot reload, under `dev`

With the `dev` feature, `App::new` starts the asset server's watcher, and
`App::run_loop`, where there is a device, starts the hot reload and its
shader watcher (M5.8, ADR-0043). After each drawn frame, `HotReload::frame`
finds again what the frame's finish swapped in, makes the level again once
no reload is pending, rebuilds the programs whose files changed, and asks
for the reads of what the watcher saw change, which later frames' finishes
make (section 7). `App::offscreen` and `App::update` never reload.

```mermaid
sequenceDiagram
    box Main thread
        participant HR as HotReload
        participant Srv as AssetServer
        participant Clp as Clips
        participant Lib as PrefabLibrary
        participant Lvl as engine level
        participant Rend as Renderer
    end
    box Watcher threads
        participant AW as nadir-assets-watch
        participant SW as nadir-shader-watch
    end

    par every 250 ms
        AW->>AW: Source::modified of each loaded path
        AW-)Srv: each path that changed, on a channel
    and every 250 ms
        SW->>SW: the times of the shader files
        SW-)HR: the programs whose files changed, in an atomic mask
    end
    HR->>Srv: drain_swapped: atlases, prefabs, levels
    opt an atlas was swapped
        HR->>Srv: load_in its pages not loaded yet
        HR->>Clp: reload_atlas: its clips replaced by name, under their handles
    end
    opt a prefab was swapped
        HR->>Lib: insert the new file, load and check its bases
    end
    opt the level is due, and no load or reload is pending
        HR->>Lvl: reload_level: made again behind its handles
    end
    opt a program's files changed
        HR->>HR: read its two files
        HR->>Rend: reload_program, or the last one kept
    end
    HR->>Srv: reload_changed: a read for each path received
```

An atlas, a prefab or the current level's file swapped makes the level
due. A texture needs nothing: the renderer looks its handle up each frame.
With nothing changed, the frame allocates nothing.

## 12. Sound: the main thread and the audio thread

`nadir::sound` (section 6) and the game's own plays queue commands
through `Audio`. The audio thread, which `App::run` started (section 2),
takes them at the start of each period, mixes, and writes to the device.
What the mixer is done with comes back to the main thread, which drops it
in `Audio::update`. Neither waits for the other: the audio thread only
tries the queue's lock, and the queue never grows (M6.2, M6.3, M6.6).

```mermaid
sequenceDiagram
    box Main thread
        participant Snd as nadir::sound
        participant Aud as Audio
    end
    box Audio thread
        participant Thr as nadir-audio
        participant Mix as MixerOutput, Mixer
    end

    Snd->>Aud: set_listener, play_sound_at, set_voice, stop, play_track
    Aud->>Aud: push each on the command queue, or count it dropped
    opt the dropped count rose around a send
        Snd->>Snd: not recorded: the next frame sends it again
    end
    loop each period of the device, 7.5 ms by default
        Thr->>Mix: render(out, 2, 48 kHz)
        Mix->>Mix: try_lock: run the commands queued
        Mix->>Mix: mix the voices and the music, the buses, the limiter
        Mix->>Mix: what ended, to the return queue
        Thr->>Thr: snd_pcm_writei, or wait for room
        opt an underrun
            Thr->>Mix: underrun: Mixer::count_underrun
        end
    end
    Snd->>Aud: update
    Aud->>Aud: swap the return queue, drop what came back
    Aud->>Aud: at most once a second, warn of underruns and drops
```

The listener and the emitters that play are placed when they move, and a
move that the half-full queue dropped is sent again the next frame. With no device, lost or not
there yet, the thread renders on in time and tries the device every
second.

## Not shown, because it is not built yet

- Gamepads (M8), and the debug tools in the game (M7.10, M7.3 to M7.6):
  the debug UI's core is built (M7.2), but nothing draws it yet.
  Text in the world, and the UI (M7.8), are the renderer's, from the
  frame's `TextList` and `UiList`, within its draw: the UI planned in the
  order pushed, its instances written after the world's.
- Drawing on Windows and macOS (M2.6, M2.7), and the Metal backend (M10).
  There the loop runs as section 3 shows, with no `Graphics`.
- Sound on Windows and macOS (M6.4, M6.5): there `open_audio` fails, and
  the game runs silent.

## Sources

- `crates/engine/src/app.rs`, `glue.rs`, `level.rs`, `pack.rs`,
  `limiter.rs`, `replay.rs`, `tick_stats.rs`, `frame_allocs.rs`,
  `collisions.rs`, `animation.rs`, `log_file.rs`, `hot_reload.rs`,
  `sound.rs`
- `crates/audio/src/audio.rs`, `mixer.rs`, `shared.rs`
- `crates/core/src/time.rs`
- `crates/ecs/src/schedule.rs`, `world.rs`, `events.rs`
- `crates/input/src/input.rs`
- `crates/assets/src/server.rs`, `assets.rs`, `workers.rs`, `source.rs`,
  `watch.rs`
- `crates/render/src/renderer.rs`, `batcher.rs`, `plan.rs`, `sort.rs`,
  `frame.rs`, `chunks.rs`, `texture.rs`, `atlas.rs`
- `crates/gfx/src/device.rs`, `gl/mod.rs`
- `crates/platform/src/lib.rs`, `sleep.rs`, `audio.rs`, `linux/x11.rs`,
  `linux/x11/input.rs`, `linux/x11/connection.rs`, `linux/egl.rs`,
  `linux/alsa.rs`
- `games/sandbox/src/main.rs`, `replay.rs`, `capture.rs`
