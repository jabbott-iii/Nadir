# Nadir — 2D top-down game engine (Rust, desktop only)

**Status:** design complete, no code yet. Work starts at milestone M0 in
`docs/roadmap.md`. Any command, crate, or file this document mentions does not
exist until the roadmap task that creates it is done. Check before assuming.

## What we are building

- An engine for top-down 2D games: tilemaps, y-sorted sprites, kinematic
  movement, many entities, bitmap text, mixed audio, gamepad support.
- **Targets (only these):**
  - Windows 10+ (x86_64, MSVC)
  - macOS 12+ (Apple Silicon and Intel, shipped as a universal binary)
  - Linux (x86_64, X11; Wayland sessions run through XWayland until native
    Wayland lands, see the roadmap backlog)
- **Out of scope:** web/WASM, Android, iOS, consoles, 3D, networking.
- **Built from the ground up:** zero third-party runtime crates. We write our
  own OS bindings, graphics backends, math, ECS, codecs, mixer, UI and tools.

## Read before working

| If you are touching… | Read first |
| :- | :- |
| Anything | `docs/00-overview.md`, then `docs` |
| `crates/core` | `docs/subsystems/core.md` |
| `Cargo.toml`, `Cargo.lock`, a new crate | `docs/02-dependency-policy.md` |
| `crates/sys`, `crates/platform` | `docs/subsystems/sys-and-platform.md`, `docs` |
| `crates/gfx` | `docs/subsystems/gfx.md` |
| `crates/render` | `docs/subsystems/render.md` |
| `crates/ecs`, `crates/scene` | `docs/subsystems/ecs.md`, `docs` |
| `crates/input` | `docs/subsystems/input.md` |
| `crates/audio` | `docs/subsystems/audio.md` |
| `crates/codec`, `crates/assets`, `tools/cook` | `docs/subsystems/assets-and-codecs.md` |
| `crates/collision` | `docs/subsystems/collision.md` |
| `crates/debug` | `docs/subsystems/debug-tools.md` |
| Tests, CI, `xtask` | `docs/04-quality.md` |
| Packaging, signing, crash reports | `docs/05-release.md` |
| Why something is the way it is | `docs/decisions.md` |

## Non-negotiable rules

1. **No third-party dependencies.** Do not add any crate outside this
   workspace to any `[dependencies]`, `[dev-dependencies]` or
   `[build-dependencies]` table, unless `docs/02-dependency-policy.md` lists
   it as `Approved`. If you believe one is needed, stop. Write the evaluation
   the policy describes and ask the user. `cargo xtask deps` enforces this.
2. **Layering is one-way.** A crate may depend only on crates listed for it in
   `docs/01-architecture.md` → "Dependency matrix". `cargo xtask layers`
   enforces this.
3. **`unsafe` lives in three crates only:** `nadir-sys`, `nadir-platform`,
   `nadir-gfx`. Every other crate inherits `unsafe_code = "deny"`.
   - Every `unsafe` block carries a `// SAFETY:` comment explaining why it is
     sound.
   - Every `pub unsafe fn` has a `# Safety` doc section.
   - Raw OS declarations live only in `nadir-sys`.
4. **Platform `cfg` lives in a few places only.** `#[cfg(target_os = ...)]`
   appears only in `nadir-sys`, `nadir-platform`, the `nadir-gfx`
   backend selection, and `xtask` packaging code. Game-facing APIs are
   identical on all three OSes.
5. **Honest verification.**
   - Never state that a build, test, benchmark or check passed unless you ran
     it in this session and saw it pass.
   - If a command does not exist yet, say so instead of guessing.
   - Report what you did not run.
6. **Smallest complete change.**
   - No unrelated refactors, renames, reformatting or "while I'm here" fixes.
   - Preserve public APIs unless the task authorizes a break; record breaks
     in `docs/decisions.md`.
7. **Git is read-only for agents.** Do not commit, amend, rebase, merge, tag,
   push, force-push or rewrite history. Leave all changes uncommitted for
   human review.
8. **No secrets.** Never write signing identities, certificates, tokens or
   store credentials into the repo. Release secrets exist only in CI secret
   storage (see `docs/05-release.md`).

## Workspace map

| Path | Package | Purpose | `unsafe` |
| :- | :- | :- | :- |
| `crates/core` | `nadir-core` | Math, color, rects, time, logging, RNG, hashing/CRC32, generational handles, key enums, profiling scopes, errors, test helpers | deny |
| `crates/sys` | `nadir-sys` | Hand-written raw FFI declarations (Win32, Cocoa/objc, X11, EGL, GL, ALSA, CoreAudio, WASAPI, XInput, evdev) | allowed |
| `crates/platform` | `nadir-platform` | Windows, event loop, input devices, GL/Metal surfaces, audio device output, paths, dynamic loading | allowed |
| `crates/gfx` | `nadir-gfx` | Small render hardware interface (RHI) with an OpenGL 3.3 core backend and, from M10, a Metal backend | allowed |
| `crates/codec` | `nadir-codec` | Adler-32, inflate, PNG (feature `png`, cook only), QOI, QOA, WAV, KLZ block compression, JSON(C), binary JSON (KBJ), BMFont | deny |
| `crates/ecs` | `nadir-ecs` | Generational entities, sparse-set storage, resources, events, commands, schedule | deny |
| `crates/input` | `nadir-input` | Physical keys, device state, action maps, per-tick latching, bindings | deny |
| `crates/render` | `nadir-render` | Sprite batching, y-sort, tilemap chunks, camera, bitmap text, debug draw, passes | deny |
| `crates/audio` | `nadir-audio` | Mixer, voices, buses, resampling, streaming, sound assets | deny |
| `crates/assets` | `nadir-assets` | Handles, typed storage, VFS (loose/pack), loaders, hot reload, worker pool | deny |
| `crates/collision` | `nadir-collision` | Shapes, spatial hash, move-and-slide, queries, A*, flow fields | deny |
| `crates/scene` | `nadir-scene` | Reflection macros, prefabs, levels, saves, game states | deny |
| `crates/debug` | `nadir-debug` | Immediate-mode debug UI, inspector, console, profiler, trace export (feature `dev`) | deny |
| `crates/engine` | `nadir` | Facade: `App`, plugins, main loop, re-exports | deny |
| `tools/cook` | `nadir-cook` | Offline asset pipeline (library + CLI) | deny |
| `xtask` | `xtask` | Build automation: CI checks, goldens, packaging | deny |
| `games/sandbox` | `sandbox` | The game that drives requirements | deny |
| `bench` | `nadir-bench` | Own micro-benchmark harness and benches | deny |

## Commands (created in M0; verify they exist before relying on them)

| Command | What it does |
| :- | :- |
| `cargo xtask ci` | Runs fmt check, clippy (`-D warnings`), tests, `deps`, `layers`, `unsafe-audit`: the full PR gate |
| `cargo xtask deps` | Fails if `Cargo.lock` contains any non-workspace package not approved in the policy |
| `cargo xtask layers` | Fails if a crate depends on a crate the dependency matrix forbids |
| `cargo xtask unsafe-audit` | Fails if `unsafe` appears outside the allowed crates or lacks a `SAFETY:` comment |
| `cargo xtask golden [--bless] [--backend gl\|metal]` | Renders golden scenes offscreen and compares them to references |
| `cargo xtask fuzz-smoke [--secs N]` | Runs the dependency-free mutation fuzzer over every parser |
| `cargo xtask cook` | Cooks `assets/` into `target/cooked/` |
| `cargo xtask bench` | Runs the benchmark harness in release mode |
| `cargo run -p sandbox` | Runs the sandbox game (opens a window) |

`xtask` never uploads, publishes or signs with real credentials. Publishing
happens only in CI release jobs.

## Conventions

- **Toolchain and edition:** Rust edition 2024, stable toolchain pinned in
  `rust-toolchain.toml`. In edition 2024, `extern` blocks must be written as
  `unsafe extern "C" { ... }`.
- **Coordinates:** world space is +X right, **+Y down**, which matches tile
  editors and screen space. Units are pixels at zoom 1. Angles are radians;
  positive rotation is clockwise on screen.
- **Time:** simulation runs at a fixed 60 Hz tick. Rendering interpolates
  between the last two ticks.
- **Errors:**
  - Each crate defines its own error enum implementing `Display` and
    `std::error::Error`.
  - No `unwrap()` in library code (`clippy::unwrap_used` is denied outside
    tests). Use `expect("why this cannot fail")` only for real invariants.
- **Logging:** use the `nadir_core::log` macros (`error!`, `warn!`,
  `info!`, `debug!`, `trace!`). `print!`/`eprint!` are denied in library
  crates.
- **Allocation:** no per-frame heap allocation in steady-state render, ECS
  iteration, audio callbacks or collision. Reuse buffers. The dev allocation
  counter (see `docs/04-quality.md`) checks this.
- **Docs:** every `pub` item has a doc comment (`missing_docs` = warn, `-D` in
  CI).
- **Tests:**
  - Unit tests live in `#[cfg(test)] mod tests` beside the code.
  - Integration tests live in each crate's `tests/`.
  - Golden images live in `tests/golden/`.

## Definition of done for a roadmap task

1. Every acceptance criterion for the task in `docs/roadmap.md` is met.
2. The task's verification commands were run. Report each command and its
   real result.
3. The checkbox is ticked only after steps 1 and 2.
4. Docs are updated if behavior or design changed. If a decision changed,
   add an ADR in `docs/decisions.md`.
5. The final message summarizes:
   - changes and files affected
   - validation performed
   - checks not run, and why
   - assumptions
   - unresolved issues and residual risks
