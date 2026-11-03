# Nadir — 2D top-down game engine (Rust, desktop only)

**Status:** in development, at version 0.2. `docs/roadmap.md` ticks each task
once it is done and verified. A task whose code is written but still needs a
check on another OS stays unticked, with a `⏳ needs <OS>` note. The next task
is the first unticked one whose dependencies are all ticked, skipping one
whose remaining work needs an OS you cannot run. This document also describes
what is not built yet, such as the Metal backend (and
`cargo xtask golden --backend`), audio and the debug tools, whose crates are
still empty. Some of what it mentions may not exist yet, and some exists
before its roadmap task is ticked: check before assuming.

## What we are building

- An engine for top-down 2D games: tilemaps, y-sorted sprites, kinematic
  movement, many entities, bitmap text, mixed audio, gamepad support.
- **Targets (only these):**
  - Windows 10+ (x86_64 and aarch64, MSVC)
  - macOS 12+ (Apple Silicon and Intel; the game's packages are a universal
    binary)
  - Linux (x86_64 and aarch64, X11; Wayland sessions run through XWayland
    until native Wayland lands, see the roadmap backlog)
  - aarch64 Windows and Linux joined with the release pipeline (ADR-0036).
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
| The demo page: `site/`, `docs/features.jsonc`, `.github/workflows/pages.yml` | `docs/04-quality.md` ("Demo page"), ADR-0029 |
| Packaging, signing, crash reports | `docs/05-release.md` |
| Releases: `.github/workflows/release.yml`, `.github/release/`, `tools/cook/Dockerfile`, `xtask` `dist` and `crates` | `docs/05-release.md` ("Engine releases"), ADR-0036 |
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
| `crates/codec` | `nadir-codec` | Adler-32, inflate, PNG (feature `png`, cook only), QOI, QOA, WAV, KLZ block compression, JSON(C), binary JSON (KBJ), BMFont, Aseprite exports and LDtk projects (cook only), the cook's atlas and level files, prefab files, pack files | deny |
| `crates/ecs` | `nadir-ecs` | Generational entities, sparse-set storage, resources, events, commands, schedule | deny |
| `crates/input` | `nadir-input` | Physical keys, device state, action maps, per-tick latching, bindings | deny |
| `crates/render` | `nadir-render` | Sprite batching, y-sort, tilemap chunks, camera, bitmap text, debug draw, passes | deny |
| `crates/audio` | `nadir-audio` | Mixer, voices, buses, resampling, streaming, sound assets | deny |
| `crates/assets` | `nadir-assets` | Handles, typed storage, VFS (loose/pack), loaders, hot reload, worker pool | deny |
| `crates/collision` | `nadir-collision` | Shapes, spatial hash, move-and-slide, queries, A*, flow fields | deny |
| `crates/scene` | `nadir-scene` | Reflection macros, prefabs, levels, saves, game states | deny |
| `crates/debug` | `nadir-debug` | Immediate-mode debug UI, inspector, console, profiler, trace export (feature `dev`) | deny |
| `crates/engine` | `nadir_engine` (`nadir` in code; ADR-0036, ADR-0039) | Facade: `App`, main loop, components, re-exports | deny |
| `tools/cook` | `nadir-cook` | Offline asset pipeline (library + CLI) | deny |
| `xtask` | `xtask` | Build automation: CI checks, goldens, packaging | deny |
| `games/sandbox` | `sandbox` | The game that drives requirements | deny |
| `bench` | `nadir-bench` | Own micro-benchmark harness and benches | deny |

## Commands (verify one exists before relying on it)

| Command | What it does |
| :- | :- |
| `cargo xtask ci` | Runs fmt check, clippy (`-D warnings`), `cook`, tests, `deps`, `layers`, `crates`, `unsafe-audit`, `headers`, `workflows`, `release-no-dev`, `features`: the full PR gate |
| `cargo xtask deps` | Fails if `Cargo.lock` contains any non-workspace package not approved in the policy |
| `cargo xtask layers` | Fails if a crate depends on a crate the dependency matrix forbids |
| `cargo xtask crates` | Fails if a crate that crates.io gets lacks a description, license or repository, or copies of the root's `LICENSE` and `NOTICE` in its folder; is at another version than `nadir_engine`; or depends on a workspace crate crates.io does not get or by another version (ADR-0036, ADR-0037) |
| `cargo xtask unsafe-audit` | Fails if `unsafe` appears outside the allowed crates or lacks a `SAFETY:` comment |
| `cargo xtask headers` | Fails if a source file lacks the Apache-2.0 header of `LICENSE`'s appendix, or has it altered or out of place (ADR-0037) |
| `cargo xtask workflows` | Fails if a GitHub workflow uses an action it is not allowed, pins one by anything but a commit SHA, or gives its token more than it may (ADR-0032) |
| `cargo xtask features` | Fails if a ticked roadmap task belongs to no feature in `docs/features.jsonc`, or a feature's `ticked` is not what the roadmap ticks |
| `cargo xtask golden [--bless] [--backend gl\|metal]` | Renders golden scenes offscreen and compares them to references |
| `cargo xtask fuzz-smoke [--secs N] [--target T]` | Runs the dependency-free mutation fuzzer over every parser, or over one |
| `cargo xtask cook [--force]` | Cooks `assets/` into `target/cooked/`, skipping the jobs (atlas groups, data files, LDtk projects) whose files did not change, unless `--force`, and packs what it wrote into `target/cooked/game.kpak` |
| `cargo xtask bench [filter]` | Runs the benchmark harness in release mode |
| `cargo xtask dist [--out DIR]` | Builds this machine's release files into `target/dist`: runs the laid-out programs, then packs the archive and, on Linux, the `.deb`s (needs `dpkg-dev`; ADR-0036) |
| `cargo xtask site [--out DIR]` | Builds the demo page into `target/site`, or a new, empty or earlier `DIR`: captures, videos, the Linux download (needs EGL and GStreamer) |
| `cargo run -p sandbox` | Runs the sandbox game (opens a window). It draws the cooked sprites: run `cargo xtask cook` first, which `cargo xtask ci` also does, as the sandbox's tests need them too |
| `make release VERSION=v<version> [DRY_RUN=1]` | For maintainers, never agents (rule 7): checks that HEAD is on `main` at that version, then tags it and pushes the tag, which starts the release workflow (`docs/05-release.md`) |

`xtask` never uploads, publishes or signs with real credentials. Publishing
happens only in CI: the release workflow (`.github/workflows/release.yml`,
ADR-0036), and the demo page's workflow (`.github/workflows/pages.yml`,
ADR-0029).

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
- **License headers:** every source file (Rust, GLSL, scripts, the site's
  HTML, CSS and JS, the `Makefile`, the Dockerfile, the workflows) starts
  with the Apache-2.0 header from `LICENSE`'s appendix, in its comment
  syntax. It goes after a shebang, an HTML doctype or a GLSL
  `#version 330 core`, which nadir-gfx requires on line 1. A new file gets
  it too (`cargo xtask headers` checks). A new crate for crates.io gets
  copies of `LICENSE` and `NOTICE` in its folder (`cargo xtask crates`), and a
  `README.md` of its own, in the shape the others have, whose examples
  `src/lib.rs` tests (ADR-0039).
  `NOTICE` holds only what a license requires there; anything else not
  ours keeps its own license and notice beside it, as the RFC 8259
  examples do in `crates/codec/tests/json.rs` (ADR-0037).
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
   add an ADR in `docs/decisions.md`. A ticked task belongs to a feature in
   `docs/features.jsonc`, which the demo page lists: add it to one, or
   describe a new one, and rewrite the feature's summary for what is built
   now and update its `ticked` (`cargo xtask features` checks).
5. The final message summarizes:
   - changes and files affected
   - validation performed
   - checks not run, and why
   - assumptions
   - unresolved issues and residual risks
