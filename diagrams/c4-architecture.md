# C4 component and architecture diagrams

These diagrams show Nadir in the C4 model's views: the system and the
people and systems around it, its containers (the programs and the files
between them), and the components inside the game and the cook, which are
the workspace's crates. They reflect the code at commit 0cba65f (version
0.2). What the roadmap plans but the code does not have yet is drawn
dashed and labelled "planned".

## Legend

| Style | Means |
| :- | :- |
| Dark blue, rounded | A person |
| Blue box | Part of Nadir: a system, a container or a component |
| Grey box | A system outside Nadir |
| Cylinder | Files on disk |
| White box with a dashed border | Planned: not built yet |
| Solid arrow | Uses, reads, writes or depends on, as labelled |
| Dotted arrow | Planned: not built yet |

## 1. System context

The game built on Nadir and Nadir's own tools, with the people, editors,
OS interfaces and services they deal with. The Windows and macOS windows
are written but still wait on checks on those systems (roadmap M1.5, M1.6),
and there the game runs without drawing until their OpenGL contexts land,
and without sound until their audio output does (M6.4, M6.5).

```mermaid
flowchart TB
    developer(["Game developer<br/>writes the game, draws its sprites, lays out its levels"])
    player(["Player<br/>plays the game"])

    subgraph scope["Nadir"]
        game["Game built on Nadir<br/>the sandbox, linked with the nadir_engine facade"]
        tools["Nadir's tools<br/>nadir-cook, the asset cook, and xtask, the build automation"]
    end

    aseprite["Aseprite<br/>sprite sheets: PNG and JSON exports"]
    ldtk["LDtk<br/>level projects: .ldtk and .ldtkl files"]
    editors["Image editors and font generators<br/>PNG images, BMFont .fnt fonts"]
    fs[("Filesystem<br/>assets/ sources, target/cooked/ files and game.kpak, the game's log files")]
    os["OS window and input APIs<br/>X11 on Linux, Win32 on Windows, AppKit on macOS"]
    gpu["GPU driver<br/>OpenGL 3.3 core through EGL, on Linux"]
    gpu_planned["GPU on Windows and macOS<br/>WGL and NSOpenGL contexts, then Metal: planned"]
    audio["OS audio API<br/>ALSA, through libasound, on Linux"]
    audio_planned["Audio on Windows and macOS<br/>WASAPI and CoreAudio: planned"]
    pad_api["Gamepad APIs<br/>evdev, XInput, GameController: planned"]
    gha["GitHub Actions<br/>workflows CI, CodeQL, Pages and Release"]
    pages["GitHub Pages<br/>the demo page at jabbott.dev/Nadir"]
    channels["Release channels<br/>GitHub release and apt repository, the cook's image on ghcr.io, the crates on crates.io"]

    developer -->|"draws sprites"| aseprite
    developer -->|"lays out levels"| ldtk
    developer -->|"draws images, makes fonts"| editors
    aseprite -->|"exports into assets/"| fs
    ldtk -->|"saves projects into assets/"| fs
    editors -->|"saves into assets/"| fs
    developer -->|"writes and runs"| game
    developer -->|"runs cargo xtask"| tools
    tools -->|"reads assets/, writes target/cooked/ and game.kpak"| fs
    game -->|"reads the cooked files or game.kpak, writes nadir.log"| fs
    player -->|"plays with keyboard and mouse"| game
    game -->|"opens a window, reads its events"| os
    game -->|"draws through OpenGL 3.3 core"| gpu
    game -.->|"planned M2.6, M2.7, M10"| gpu_planned
    game -->|"plays sound through ALSA"| audio
    game -.->|"planned M6.4, M6.5"| audio_planned
    game -.->|"planned M8"| pad_api
    gha -->|"runs cargo xtask"| tools
    gha -->|"deploys the demo page"| pages
    gha -->|"publishes a tagged release"| channels
    player -->|"watches the demo, downloads the Linux build"| pages
    player -->|"downloads a release"| channels

    classDef person fill:#08427b,stroke:#052e56,color:#ffffff
    classDef system fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class developer,player person
    class game,tools system
    class aseprite,ldtk,editors,fs,os,gpu,audio,gha,pages,channels external
    class gpu_planned,audio_planned,pad_api planned
```

## 2. Containers

The programs of the workspace and the files that pass between them. The
cook turns `assets/` into loose files and one pack, `game.kpak`; a game
built with the `dev` feature reads the loose files, and one built without
it reads the pack, or the loose files where there is no pack.

```mermaid
flowchart LR
    developer(["Game developer"])
    gha["GitHub Actions<br/>workflows CI, Pages and Release"]
    os["OS and GPU driver<br/>window, events, OpenGL, sound"]
    logs[("The user's logs folder<br/>nadir.log and the logs of the three runs before")]

    subgraph ws["Nadir workspace"]
        xtask["xtask<br/>Rust binary, run as cargo xtask: the checks, cook, golden, fuzz-smoke, bench, soak, site, dist"]
        cook["nadir-cook<br/>Rust library and CLI: nadir-cook [--force] SOURCE OUT"]
        game["Game executable: sandbox<br/>Rust binary on nadir_engine. cargo run -p sandbox, or cargo dev for the dev feature"]
        bench["nadir-bench<br/>Rust binaries: the benchmark harness and the audio soak"]
        src[("assets/<br/>PNG images and sprite.jsonc sidecars, WAVs and sound.jsonc sidecars, Aseprite JSON, LDtk projects, data JSONC and prefabs")]
        loose[("target/cooked/ loose files<br/>atlas QOI pages, KBJ and font files, sounds, data KBJ, levels .klvl, the cache .cache.kbj")]
        pack[("target/cooked/game.kpak<br/>the pack: every cooked file, KLZ where that is smaller")]
        dist[("target/dist/<br/>nadir-VERSION-TARGET: sandbox, nadir-cook, assets/game.kpak, as .zip or .tar.gz, and .debs on Linux")]
        site[("target/site/<br/>the demo page, its videos and posters, the Linux download")]
    end

    developer -->|"runs"| xtask
    developer -->|"runs"| game
    gha -->|"runs cargo xtask ci, golden, fuzz-smoke, site and dist"| xtask
    xtask -->|"cook: cargo run --release --package nadir-cook. With --watch: builds it once, runs it on each change"| cook
    cook -->|"reads"| src
    cook -->|"writes, skipping jobs and files that did not change"| loose
    cook -->|"packs every output into"| pack
    game -->|"with dev: reads the loose files, and reloads those that change"| loose
    game -->|"without dev: reads the pack when it is there, else the loose files"| pack
    game -->|"writes"| logs
    game -->|"window, events, OpenGL, sound"| os
    xtask -->|"bench: cargo run --release --package nadir-bench; soak: the same, --bin soak"| bench
    xtask -->|"site and dist: build it in release and run it"| game
    xtask -->|"dist: lays out, checks and packs"| dist
    xtask -->|"site: captures, encodes with GStreamer, packs the download"| site

    classDef person fill:#08427b,stroke:#052e56,color:#ffffff
    classDef container fill:#438dd5,stroke:#2e6295,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    class developer person
    class xtask,cook,game,bench,src,loose,pack,dist,site container
    class gha,os,logs external
```

- The sandbox reads the folder `assets` beside its executable when there
  is one, as in the release files and the demo page's download, and
  `target/cooked/` otherwise.
- A pack that is there but does not open is logged, and every load then
  fails with its error: the game does not fall back to loose files that
  may not be its cook's.
- The game's asset server reads and decodes on worker threads, two fewer
  than the cores, from 1 to 4, and makes the assets on the main thread.

## 3. Components of the game executable

The crates of the game executable by layer, with an arrow for each entry
in a crate's `[dependencies]` table. The debug crate has no code and no
dependencies yet. The dotted arrow is an edge still to come: the
sandbox's to debug, behind `dev` (the matrix's row for the sandbox). The
edges the matrix allows out of debug (core, ecs, scene, render, input) are
not drawn.

```mermaid
flowchart TB
    subgraph L6["Layer 6: applications"]
        n_sandbox["sandbox<br/>games/sandbox: the demo game. Field and stress scenes, replays, offscreen capture"]
    end
    subgraph L5["Layer 5: facade"]
        n_engine["nadir_engine, nadir in code<br/>App and the main loop, Config, the engine's components and systems, load_level, re-exports"]
    end
    subgraph L4["Layer 4: tooling inside the engine"]
        n_debug["nadir-debug<br/>planned M7: debug UI, inspector, console, profiler"]
    end
    subgraph L3["Layer 3: composed subsystems"]
        n_render["nadir-render<br/>sprite batching, y-sort, tilemap chunks, camera, texture and atlas assets"]
        n_scene["nadir-scene<br/>reflection macros, type registry, world hash, prefabs, a level's entities"]
        n_audio["nadir-audio<br/>the mixer: voices, buses, resampling, positional sound, QOA music streamed a frame at a time, the command and return queues; the Sound and Music assets, played by handle"]
    end
    subgraph L2["Layer 2: base subsystems"]
        n_gfx["nadir-gfx<br/>RHI over OpenGL 3.3 core: device, textures, buffers, render targets, pipelines, passes"]
        n_ecs["nadir-ecs<br/>generational entities, sparse sets, resources, events, commands, schedule"]
        n_assets["nadir-assets<br/>handles, typed storage, loose and pack sources, worker threads, scopes"]
        n_input["nadir-input<br/>physical keys, action maps, bindings, per-tick latching, replays"]
        n_collision["nadir-collision<br/>shapes, tile grids, spatial hash, move-and-slide, sweeps, queries, triggers"]
    end
    subgraph L1["Layer 1: OS behavior and formats"]
        n_platform["nadir-platform<br/>window and events, GL context through EGL on Linux, audio output through ALSA on Linux, user folders, error dialog, precise sleep, dev allocation counter"]
        n_codec["nadir-codec<br/>inflate, PNG behind the png feature, QOI, QOA, WAV, the sound file and resampling, KLZ, JSON and JSONC, KBJ, BMFont, Aseprite, LDtk, atlas, level, prefab and pack files"]
    end
    subgraph L0["Layer 0: foundations"]
        n_core["nadir-core<br/>math, color, rects, time, logging, RNG, hashing, handles, keys, profile scopes, CLI parser"]
        n_sys["nadir-sys<br/>raw FFI: Win32, Cocoa and objc, X11, EGL, ALSA, evdev key codes, OpenGL types, dynamic loading"]
    end

    n_sandbox --> n_engine
    n_engine --> n_assets
    n_engine --> n_audio
    n_engine --> n_collision
    n_engine --> n_core
    n_engine --> n_ecs
    n_engine --> n_gfx
    n_engine --> n_input
    n_engine --> n_platform
    n_engine --> n_render
    n_engine --> n_scene
    n_render --> n_assets
    n_render --> n_codec
    n_render --> n_core
    n_render --> n_gfx
    n_scene --> n_assets
    n_scene --> n_codec
    n_scene --> n_core
    n_scene --> n_ecs
    n_audio --> n_assets
    n_audio --> n_codec
    n_audio --> n_core
    n_gfx --> n_core
    n_gfx --> n_platform
    n_gfx --> n_sys
    n_ecs --> n_core
    n_assets --> n_codec
    n_assets --> n_core
    n_input --> n_core
    n_input --> n_platform
    n_collision --> n_core
    n_platform --> n_core
    n_platform --> n_sys
    n_codec --> n_core
    n_sandbox -.->|"planned, dev only"| n_debug

    classDef container fill:#438dd5,stroke:#2e6295,color:#ffffff
    classDef component fill:#85bbf0,stroke:#5d82a8,color:#000000
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class n_sandbox container
    class n_engine,n_render,n_scene,n_audio,n_gfx,n_ecs,n_assets,n_input,n_collision,n_platform,n_codec,n_core,n_sys component
    class n_debug planned
```

- The game reaches the subsystems through the facade, which re-exports
  assets, audio, collision, ecs, input, render and scene as modules of
  `nadir`.
- `nadir_engine` uses `nadir-codec` only in its tests, as a
  dev-dependency, which the diagram leaves out; `nadir-audio`'s tests use
  `nadir-platform`'s allocation counter the same way.
- Features along these edges: the sandbox's `dev` turns on `nadir/dev`,
  which turns on `nadir-platform/dev`, the allocation counter. No
  dependency is optional.

## 4. Components of the cook tool

The `nadir-cook` package is one library, whose `cook` function does the
work, and the CLI over it. Its modules are drawn with an arrow to each
module whose items they use, and the package's three workspace
dependencies below. The error type's module, `error.rs`, which most of
them use, is left out.

```mermaid
flowchart TB
    subgraph cook_pkg["nadir-cook: tools/cook"]
        cli["CLI, main.rs<br/>reads nadir-cook [--force] SOURCE OUT, runs cook, prints the report"]
        lib["cook, lib.rs<br/>plans the jobs, cooks data JSONC to KBJ, checks prefabs, writes each output through a temporary file"]
        scan["scan<br/>walks SOURCE and sorts what it finds into jobs"]
        config["config<br/>sprite.jsonc and sound.jsonc sidecars, and atlas.jsonc"]
        cache["cache<br/>.cache.kbj: the hashes each job read and wrote"]
        atlas["atlas<br/>images, Aseprite exports and fonts into QOI pages, a KBJ, and a glyph file for each font"]
        image["image<br/>trims frames, extrudes their edges"]
        packer["pack<br/>MaxRects rectangle packing"]
        level["level<br/>each LDtk level into levels/NAME.klvl"]
        sound["sound<br/>each WAV below audio/ into audio/PATH.kpcm or .qoa, resampled"]
        kpak["kpak<br/>writes game.kpak, reads it back with the game's reader"]
    end
    n_assets["nadir-assets<br/>AssetPath, the pack's file name"]
    n_codec["nadir-codec, with the png feature<br/>the formats the cook reads and writes"]
    n_core["nadir-core<br/>hashing, CLI parser"]

    cli --> lib
    lib --> scan
    lib --> config
    lib --> cache
    lib --> atlas
    lib --> level
    lib --> sound
    lib --> kpak
    sound --> config
    atlas --> config
    atlas --> image
    atlas --> packer
    image --> packer
    cook_pkg -->|"feature png"| n_codec
    cook_pkg --> n_assets
    cook_pkg --> n_core
    n_assets --> n_codec
    n_assets --> n_core
    n_codec --> n_core

    classDef component fill:#85bbf0,stroke:#5d82a8,color:#000000
    class cli,lib,scan,config,cache,atlas,image,packer,level,sound,kpak,n_assets,n_codec,n_core component
```

## 5. The workspace's other programs

`xtask` and `nadir-bench` are not part of the game or the cook. `xtask`
links only `nadir-core` and `nadir-codec`, and runs the other packages
through cargo.

```mermaid
flowchart TB
    xtask["xtask<br/>the checks, cook, golden, fuzz-smoke, bench, soak, site, dist"]
    bench["nadir-bench<br/>the benchmark harness and its benches, and the audio soak"]
    n_engine["nadir_engine"]
    n_render["nadir-render"]
    n_audio["nadir-audio"]
    n_ecs["nadir-ecs"]
    n_collision["nadir-collision"]
    n_platform["nadir-platform"]
    n_codec["nadir-codec"]
    n_core["nadir-core"]

    xtask -->|"feature png"| n_codec
    xtask --> n_core
    bench --> n_engine
    bench --> n_render
    bench --> n_audio
    bench --> n_ecs
    bench --> n_collision
    bench -->|"soak: open_audio"| n_platform
    bench --> n_codec
    bench --> n_core

    classDef container fill:#438dd5,stroke:#2e6295,color:#ffffff
    classDef component fill:#85bbf0,stroke:#5d82a8,color:#000000
    class xtask,bench container
    class n_engine,n_render,n_audio,n_ecs,n_collision,n_platform,n_codec,n_core component
```

## The crates

Every workspace member, its layer, its `unsafe` policy and its direct
workspace dependencies. "deny" is the workspace's `unsafe_code = "deny"`,
which every member inherits through `[lints] workspace = true`. "allowed"
is a `#![allow(unsafe_code)]` in the crate's `src/lib.rs`; there clippy's
`undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block` and rustc's
`unsafe_op_in_unsafe_fn` still deny, and `cargo xtask unsafe-audit` checks
that no other crate allows it.

| Crate | Path | Layer | `unsafe` | Direct workspace dependencies |
| :- | :- | :- | :- | :- |
| `nadir-core` | `crates/core` | 0, foundations | deny | none |
| `nadir-sys` | `crates/sys` | 0, foundations | allowed | none |
| `nadir-platform` | `crates/platform` | 1, OS behavior and formats | allowed | core, sys |
| `nadir-codec` | `crates/codec` | 1, OS behavior and formats | deny | core |
| `nadir-gfx` | `crates/gfx` | 2, base subsystems | allowed | core, platform, sys |
| `nadir-ecs` | `crates/ecs` | 2, base subsystems | deny | core |
| `nadir-assets` | `crates/assets` | 2, base subsystems | deny | core, codec |
| `nadir-input` | `crates/input` | 2, base subsystems | deny | core, platform |
| `nadir-collision` | `crates/collision` | 2, base subsystems | deny | core |
| `nadir-render` | `crates/render` | 3, composed subsystems | deny | assets, codec, core, gfx |
| `nadir-scene` | `crates/scene` | 3, composed subsystems | deny | assets, codec, core, ecs |
| `nadir-audio` | `crates/audio` | 3, composed subsystems | deny | assets, codec, core |
| `nadir-debug` | `crates/debug` | 4, tooling inside the engine | deny | none: planned, no code yet |
| `nadir_engine` (`nadir` in code) | `crates/engine` | 5, facade | deny | assets, audio, collision, core, ecs, gfx, input, platform, render, scene |
| `nadir-cook` | `tools/cook` | 6, applications | deny | assets, codec with `png`, core |
| `xtask` | `xtask` | 6, applications | deny | codec with `png`, core |
| `sandbox` | `games/sandbox` | 6, applications | deny | `nadir_engine` |
| `nadir-bench` | `bench` | 6, applications | deny | `nadir_engine`, audio, codec, collision, core, ecs, platform, render |

## Sources

- `Cargo.toml`, `.cargo/config.toml` and each member's `Cargo.toml`
- each crate's `src/lib.rs`, `tools/cook/src/main.rs`
- `crates/engine/src/app.rs`, `crates/engine/src/pack.rs`,
  `crates/engine/src/log_file.rs`, `crates/assets/src/workers.rs`
- `games/sandbox/src/main.rs`
- `xtask/src/main.rs`, `xtask/src/watch.rs`, `xtask/src/dist.rs`,
  `xtask/src/site.rs`, `xtask/src/layers.rs`
- `.github/workflows/ci.yml`, `codeql.yml`, `pages.yml`, `release.yml`
- `docs/01-architecture.md` for the layers, and `docs/roadmap.md` for
  what is planned
