# Architecture decision records

New ADRs are appended at the end using the template below; `/record-decision`
does this for you. Never edit an accepted ADR's decision. Supersede it with a
new ADR and set the old one's status to `Superseded by ADR-NNNN`.

```markdown
## ADR-NNNN — <title>

- **Status:** Proposed | Accepted | Superseded by ADR-NNNN
- **Date:** YYYY-MM-DD
- **Context:** <forces at play, constraints, what prompted this>
- **Decision:** <what we will do>
- **Consequences:** <good and bad results, follow-up work>
- **Alternatives considered:** <option — why not>
- **Revisit when:** <a concrete trigger>
```

---

## ADR-0001 — Desktop-only targets: Windows, macOS, Linux

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:** Web/WASM, Android, iOS and consoles each impose architecture
  constraints:
  - an inverted event loop
  - no blocking
  - no threads, or restricted threads
  - mobile surface loss
  - NDA SDKs
  
  These dominate engine design while serving no current product goal.
- **Decision:**
  - Support Windows 10+ x64, macOS 12+ (universal) and Linux x64 (X11,
    with XWayland on Wayland sessions).
  - Nothing else.
- **Consequences:**
  - The engine owns a simple polling main loop.
  - Plain `std` threads and channels; no async runtime.
  - `std::time::Instant` works everywhere.
  - No surface-loss handling.
  - Borderless fullscreen only; exclusive fullscreen is not supported, which
    avoids mode-switch bugs on all three OSes.
  - Porting to a new platform later would be a major project.
- **Alternatives considered:** keep web as Tier 2. Rejected: it forces async
  asset I/O and loop inversion, and the user removed it from scope.
- **Revisit when:** a business case for another platform appears.

## ADR-0002 — Build it ourselves; zero third-party runtime crates

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:** The project goal is to own the whole stack and minimize
  external code. The user's rule: a dependency is acceptable only if the
  solution cannot reasonably be built in-house.
- **Decision:**
  - The approved third-party crate set is empty.
  - Exceptions follow the test and process in
    `docs/02-dependency-policy.md`, and require user approval.
  - Enforced by `xtask deps`.
- **Consequences:**
  - Much more code to write and test: OS bindings, GPU backends, codecs,
    ECS, mixer, UI.
  - Testing harnesses (property tests, golden images, fuzz smoke,
    benchmarks) are part of the engine.
  - Full understanding of and control over the stack.
  - No transitive supply-chain risk.
  - Small binaries and fast clean builds.
- **Alternatives considered:**
  - The previous design: `winit`, `wgpu`, `bevy_ecs`/`hecs`, `kira`,
    `egui`, `serde`. Faster to start, but a large dependency tree.
  - A hybrid: adopt only `winit` + `wgpu`. Rejected: per the user's rule,
    both can reasonably be built for a 2D, desktop-only scope.
- **Revisit when:** a switch trigger in the register fires.

## ADR-0003 — Hand-written OS bindings, isolated in `nadir-sys`

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:** Zero dependencies rules out `windows-sys`, `objc2` and
  `x11-dl`. We call a bounded set of OS functions, likely a few hundred in
  total.
- **Decision:**
  - Declare exactly what we call in `nadir-sys`, with header citations
    and layout assertions.
  - Load optional libraries with `dlopen`/`LoadLibraryW`.
  - Wrap everything safely in `nadir-platform`.
- **Consequences:**
  - ABI and layout errors are the main risk, mitigated by layout tests, the
    FFI review checklist and the `/add-ffi-binding` procedure.
  - The objc `msg_send!` must handle `_stret` on x86_64.
- **Alternatives considered:** the generated binding crates. Kept as
  register fallbacks, each with a trigger.
- **Revisit when:** declarations exceed about 1,500, or ABI bugs recur.

## ADR-0004 — Own RHI: OpenGL 3.3 core everywhere first, Metal on macOS later

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:**
  - A 2D renderer needs textured instanced quads, a few render targets and
    blending.
  - GL 3.3 core is available on all three desktop OSes and min-spec
    hardware.
  - macOS GL is deprecated (frozen at 4.1) but still present.
  - Vulkan is far more code than 2D needs, and isn't native on macOS.
- **Decision:**
  - A small RHI (`nadir-gfx`) with a GL 3.3 core backend from M2.
  - A Metal backend from M10 becomes the macOS default.
  - No D3D or Vulkan backends unless triggered.
- **Consequences:**
  - One backend to debug for most of development.
  - Windows relies on vendor GL drivers. Generally fine on current hardware,
    but a known risk for some older Intel drivers; the min-spec check
    covers it.
  - The RHI is designed around Metal's model (baked pipeline state) from
    day one, so adding Metal doesn't reshape the API.
- **Alternatives considered:**
  - `wgpu`: excluded by ADR-0002.
  - Vulkan + MoltenVK: a third-party layer on macOS and a heavy API.
  - D3D11 + Metal + GL: three backends from the start, too much early
    surface.
- **Revisit when:**
  - Apple announces removal of OpenGL (pull M10 forward).
  - Windows GL driver defects affect players (add a D3D11 backend).
  - A need arises for compute.

## ADR-0005 — Hand-written shaders per backend (GLSL 330 + MSL)

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:** Shader cross-compilation (`naga`, SPIRV-Cross) is a large
  dependency. A 2D engine needs about 7 small shader programs.
- **Decision:**
  - Write each shader twice: GLSL 330 and MSL.
  - A Rust interface constant per shader, plus a parity test, keeps
    attribute locations and uniform layouts in sync.
  - Golden tests per backend check visual parity.
- **Consequences:** duplicated shader logic (small); the parity test is
  essential.
- **Alternatives considered:** our own tiny shader language translating to
  both. Rejected for now: more engineering than maintaining 7 pairs.
- **Revisit when:** there are more than about 20 programs, or divergence
  bugs recur.

## ADR-0006 — Own sparse-set ECS with a single-threaded, explicitly ordered schedule

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:**
  - Rust ownership punishes object graphs.
  - Our budget is ≤ 5,000 active entities.
  - Determinism (replays, tests) matters more than parallel throughput.
- **Decision:**
  - Sparse-set storage behind `RefCell`, with no `unsafe`.
  - Mutations drive iteration.
  - Deferred commands.
  - Stage plus insertion order.
  - No parallel systems.
- **Consequences:**
  - Simple, debuggable and deterministic.
  - Multi-component queries are lookups rather than archetype-dense scans,
    which is slower in theory and acceptable at this scale.
  - Borrow conflicts are runtime panics, caught by running every system in
    tests.
- **Alternatives considered:**
  - An archetype ECS: needs `unsafe` and is much more code.
  - No ECS (struct-of-arrays per entity type): less flexible for prefabs
    and the inspector.
- **Revisit when:** ECS overhead exceeds 25% of tick time at budget load.

## ADR-0007 — Reflection via `macro_rules!`; own JSONC and KBJ formats

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:**
  - Saves, prefabs, the inspector and settings need field-level reflection.
  - Proc macros realistically need `syn`/`quote`, and `serde` is a
    dependency.
- **Decision:**
  - `reflect_struct!` / `reflect_enum!` / `reflect_newtype!` define the type
    and its `Reflect` impl together.
  - `Value` (our JSON tree) is the interchange.
  - JSONC is for human-edited files; KBJ is for cooked data and saves.
- **Consequences:**
  - Component definitions use macro syntax.
  - No generics, and no data-carrying enums without hand-written impls.
  - One reflection path feeds saves, prefabs, the inspector and determinism
    hashing.
- **Alternatives considered:** hand-written impls for every type (tedious,
  error-prone); a hand-written proc-macro parser over `TokenStream` (large
  effort).
- **Revisit when:** macro limitations block more than 3 needed component
  shapes.

## ADR-0008 — Simple runtime formats; complex authoring formats parsed only by the cook tool

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:**
  - PNG, Vorbis and JSON level files are complex to parse safely.
  - Vorbis decoding in particular is a large build.
  - The shipped game should contain only small, fuzzable parsers.
- **Decision:**
  - The runtime loads QOI textures, QOA or PCM16 audio, KBJ data, KLZ-packed
    `.kpak` archives, and our level and font formats.
  - The cook tool parses PNG (own inflate), WAV, Aseprite JSON, LDtk JSON
    and BMFont.
- **Consequences:**
  - QOA music is larger than Vorbis: about 307 kbps at 48 kHz stereo,
    versus roughly 128–192 kbps. That's acceptable for a 2D game's
    soundtrack, so monitor download size.
  - The runtime attack surface is small.
  - The cook tool is required before running content.
- **Alternatives considered:** our own Vorbis decoder (large, intricate
  spec); shipping PNG decoding in the runtime (larger attack surface, slower
  loads).
- **Revisit when:** the download size budget is exceeded because of audio.

## ADR-0009 — Linux: X11 + EGL first; native Wayland deferred

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:**
  - Most Linux desktops default to Wayland, but ship XWayland.
  - Native Wayland needs `libwayland-client`, xdg-shell protocol handling,
    `libxkbcommon` keymaps and client-side decorations: a large OS surface.
- **Decision:**
  - Implement X11 (via `dlopen`) with EGL. It runs natively on X11 and via
    XWayland on Wayland.
  - Native Wayland is a backlog item with a trigger.
- **Consequences:**
  - On Wayland, XWayland may scale the game less crisply on fractional-scale
    setups, so an in-game UI scale setting is provided.
  - The game requires XWayland on Wayland sessions, with a clear error
    message if it's missing.
- **Alternatives considered:** Wayland first (more code, while X11 would
  still be needed for X11 sessions).
- **Revisit when:** a target distro ships without XWayland by default, or
  player reports call for it.

## ADR-0010 — Bitmap fonts only (TrueType deferred)

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:** TrueType parsing and rasterization is a meaningful build.
  Pixel-art games usually use bitmap fonts, and BMFont-format generators are
  common external tools.
- **Decision:**
  - The runtime supports bitmap fonts (BMFont text format, cooked).
  - A TrueType-to-bitmap rasterizer inside the cook tool is backlog.
- **Consequences:**
  - Limited glyph coverage; localization into large scripts needs the
    backlog item.
  - No runtime font rasterization cost.
- **Alternatives considered:** a runtime TrueType rasterizer (more code at
  runtime, little benefit for pixel art).
- **Revisit when:** localization or design needs scalable or large-glyph-set
  text.

## ADR-0011 — Kinematic collision; no rigid-body physics

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:** Top-down games rarely need dynamics. Kinematic movement is
  more controllable and deterministic.
- **Decision:**
  - AABB and circle colliders, a tile grid, a spatial hash, move-and-slide,
    sweeps, triggers and queries.
  - A* and flow fields for navigation.
- **Consequences:** pushable objects, knockback and similar effects are
  scripted kinematically.
- **Alternatives considered:** our own impulse-based physics (unneeded);
  `rapier2d` (a dependency).
- **Revisit when:** a game design needs stacking or physical dynamics.

## ADR-0012 — Use external content tools; no custom level editor

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:** Building editors is the classic engine scope trap.
- **Decision:**
  - Levels are authored in LDtk; sprites and animations in Aseprite (or
    any PNG tool plus a sidecar); fonts with BMFont-compatible generators.
  - We parse their output files in the cook tool. These tools are not code
    dependencies.
  - In-engine tooling is limited to the debug inspector and console.
- **Consequences:**
  - The importers must track tool format versions (pin and record them).
  - Artists use familiar tools.
- **Alternatives considered:** a custom editor (huge); Tiled instead of LDtk
  (also viable; LDtk chosen for its entity-field model and auto-layers).
- **Revisit when:** level-iteration speed becomes the bottleneck (see the
  play-in-editor backlog item).

## ADR-0013 — Color pipeline: sRGB textures, linear blending

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:** Correct blending happens in linear space, but artists judge
  colors in sRGB. Pixel art with hard edges is mostly unaffected; soft
  alpha edges and lights are affected.
- **Decision:**
  - Upload color textures as sRGB formats.
  - Render to sRGB targets with sRGB-aware blending (`GL_FRAMEBUFFER_SRGB`;
    `*_sRGB` pixel formats on Metal).
  - Vertex colors are given in sRGB and converted to linear on the CPU.
- **Consequences:**
  - Alpha-blended edges may look slightly different from the paint program
    preview.
  - Golden references are generated under this pipeline.
- **Alternatives considered:** blend in sRGB space (matches paint programs,
  but is physically wrong for lights and fades).
- **Revisit when:** art direction reports mismatches that can't be fixed in
  content. A `Caps` flag would switch modes.

## ADR-0014 — Subsystems are ECS-agnostic; the facade owns components and glue

- **Status:** Accepted
- **Date:** 2026-10-12
- **Context:** Coupling render, audio, collision and input to the ECS makes
  them harder to test, and ties every subsystem to ECS API churn.
- **Decision:**
  - `render`, `audio`, `collision`, `input` and `assets` expose plain Rust
    APIs.
  - `nadir` (the facade) defines the components and the systems that
    translate between the ECS and the subsystems (extract, emitters,
    collider sync).
- **Consequences:**
  - The facade is the largest integration crate.
  - Subsystems have focused unit tests and benchmarks.
  - Swapping the ECS later would touch only the facade and `scene`.
- **Alternatives considered:** each subsystem ships its own ECS plugin
  (tighter coupling across the dependency graph).
- **Revisit when:** the facade becomes unwieldy. Then split it into
  per-subsystem glue crates, still in layer 5.
