# 00 — Overview

## Purpose

Nadir is a 2D engine specialized for top-down games: action-adventure,
farming/life sims, twin-stick shooters, small-scale tactics. It is built in
Rust for Windows, macOS and Linux, almost entirely from first principles.

It is developed alongside one real game (`games/sandbox`, later the shipping
title). The rule is: **the game pulls features in, the engine never pushes
them.** A subsystem is not "done" until the sandbox uses it.

## Goals

1. Ship a commercial-quality top-down game on Windows, macOS and Linux.
2. Own the entire stack above the OS and GPU driver, so that any bug can be
   understood and fixed in-house.
3. Keep the engine small enough for one or two engineers to hold in their
   heads. As an estimate, about 35–45k lines of Rust at M11.
4. Fast iteration: hot reload of textures, data, levels, shaders and audio in
   dev builds.

## Non-goals

- Web/WASM, Android, iOS, consoles (ADR-0001).
- 3D rendering of any kind; skeletal animation; rigid-body physics (ADR-0011).
- A general-purpose editor. Levels are authored in LDtk; sprites in Aseprite
  or any PNG tool (ADR-0012).
- Networking/multiplayer. Not in scope; it would require a new ADR because it
  changes simulation determinism requirements.
- Scripting languages. Gameplay is Rust plus data files.
- Native Wayland before M12 (ADR-0009).

## Performance budgets

These are the numbers we test against. "Min-spec" is the slowest machine we
promise to support. Pick a concrete machine at M0 and record it here.

| Budget | Target |
| :- | :- |
| Frame rate | 60 fps sustained at 1920×1080 on min-spec |
| Min-spec GPU class | Integrated GPU, about 2018 (e.g. Intel UHD 620); must support OpenGL 3.3 core |
| Sprites on screen | 10,000 at 60 fps on min-spec, ≤ 30 draw calls |
| Simulated entities | 5,000 active entities with collision at 60 Hz, ≤ 4 ms per tick on min-spec |
| Frame-time spikes | 99th percentile frame ≤ 20 ms during gameplay |
| Cold start to menu | ≤ 2 s from SSD |
| Level load | ≤ 500 ms for a 256×256-tile level |
| Audio latency | ≤ 50 ms trigger-to-speaker; zero underruns in a 10-minute soak |
| Steady-state allocations | 0 heap allocations per frame in render, ECS iteration, audio callback, collision |
| Shipped binary | ≤ 15 MB per platform (excluding assets) |

## Assumptions

Revisit these if any turns out to be wrong.

- One or two engineers, with an AI coding agent doing much of the
  implementation under review.
- Single-player only.
- Pixel-art-friendly rendering is the default. Smooth (non-pixel) scaling is
  supported but secondary.
- OpenGL 3.3 core remains available on macOS for the near term. The Metal
  backend (M10) removes this risk.
- Content creators use LDtk for levels and Aseprite, or any tool that exports
  PNG plus JSON, for sprites.
- Desktop machines have at least 4 GB RAM, a dual-core CPU and an SSD.

## Scope estimate

Sizes are rough line-count estimates for planning, not commitments.
S ≈ <1k lines, M ≈ 1–3k, L ≈ 3–6k, XL ≈ 6k+.

| Area | Size | Why |
| :- | :- | :- |
| `sys` bindings (all OSes) | L | Hundreds of declarations, mostly mechanical, each needs layout tests |
| `platform` Windows | L | Win32 window, input, WGL, WASAPI, XInput, DPI, modal-loop handling |
| `platform` Linux | L | X11, EGL, ALSA, evdev gamepads, inotify hotplug |
| `platform` macOS | L | Objective-C runtime glue, Cocoa, NSOpenGL/CAMetalLayer, AudioUnit, GameController |
| `gfx` GL 3.3 backend | M | Small RHI, function loader, state cache |
| `gfx` Metal backend | M | Same RHI over objc messaging |
| `codec` | M | inflate, PNG, QOI, QOA, WAV, LZ, JSON, CRC |
| `ecs` | M | Sparse sets, queries/joins, resources, events, commands, schedule |
| `render` | M | Batcher, sort keys, tilemaps, camera, text, debug draw |
| `audio` | M | Mixer, voices, buses, resampler, streaming |
| `assets` | M | Handles, storage, VFS, pack format, hot reload, worker pool |
| `collision` | M | Shapes, spatial hash, move-and-slide, queries, pathfinding |
| `scene` | M | Reflection macro, prefabs, level import, saves, states |
| `debug` | L | Immediate-mode UI, inspector, console, profiler |
| `cook` | M | Atlas packer, importers, pack writer, incremental cache |
| `xtask` + harnesses | M | CI checks, golden tests, fuzz smoke, packaging |

The biggest risk is not any one subsystem. It is **breadth of OS surface**:
three windowing systems, three audio APIs, three gamepad APIs and two GPU
APIs, all through hand-written bindings. The roadmap reflects this. Linux
lands first (it is where CI and agents run), then Windows, then macOS. Every
binding gets layout tests.

## Top risks and mitigations

| Risk | Mitigation |
| :- | :- |
| Hand-written FFI has subtle ABI or layout bugs | Bindings only in `sys`; `size_of`/`align_of` asserts against values taken from official headers; safe wrappers in `platform`; `/add-ffi-binding` skill enforces the procedure |
| Apple removes OpenGL | Metal backend at M10; RHI designed for both from day one; pull M10 earlier if Apple announces removal |
| Wayland-only systems without XWayland | Rare today on desktop distros; native Wayland in backlog (M12) with a written trigger condition |
| Scope creep (editor, render graph, scripting) | Explicit non-goals; anything new needs an ADR |
| Untrusted file parsing (saves, mods, downloaded content) | Runtime loads only simple formats; every parser fuzzed; size and allocation caps |
| Gamepad diversity | XInput, evdev and GameController cover mainstream pads; HID-level support is backlog |
