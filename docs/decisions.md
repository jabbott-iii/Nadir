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

- **Status:** Accepted; ADR-0036 adds aarch64 Windows and Linux to its targets
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

## ADR-0015 — GlContext is an unsafe trait, with one OpenGL context per thread

- **Status:** Accepted
- **Date:** 2026-10-18
- **Context:**
  - `nadir-gfx` calls the GL functions that
    `GlContext::get_proc_address` returns, in `unsafe` blocks. Those calls
    are sound only if each pointer is the named function, and a 3.3 core
    context is current on the thread.
  - The API sketch made `GlContext` a safe trait. Safe code in any crate
    could then implement it to return any address, and gfx would call it.
  - Under libglvnd the GL functions are dispatch stubs. Every context gets
    the same addresses, and each call goes to whichever context is current
    on the thread. libglvnd also gives a stub for any name that starts with
    `gl` (seen on 2026-10-18 with Mesa and NVIDIA).
  - A context that is current on one thread cannot be made current on
    another until the first releases it: `EGL_BAD_ACCESS` on Mesa and
    NVIDIA.
- **Decision:**
  - `pub unsafe trait GlContext`. An implementor promises that whenever
    `get_proc_address` returns a pointer that is not null:
    - the pointer is that function, valid until the process exits, and
      callable through the type that `nadir_sys::gl` declares for it
    - the context is OpenGL 3.3 or later with the core profile, current on
      the thread that made the value, with no other context made current
      there while the value exists
    - the value is neither `Send` nor `Sync`

    Every method is sound to call at any time on that thread.
  - A thread has at most one context of `nadir-platform`'s: the window's
    (Linux, for now) or a `HeadlessGl`. A second fails with
    `PlatformError::GlContextExists`.
  - `HeadlessGl` is neither `Send` nor `Sync` on every system. A marker
    field ensures this, and `compile_fail` doctests check it.
- **Consequences:**
  - gfx's `SAFETY` comments can rest on these promises, and gfx can still
    write a mock context for its tests, with an `unsafe impl` of its own.
  - Only `nadir-sys`, `nadir-platform` and `nadir-gfx` can implement the
    trait, because every other crate denies `unsafe_code`.
  - A thread that holds the window cannot also make a headless context.
    libtest runs each test on its own thread.
  - A pointer that is not null does not prove that the driver has the
    function. The version check is what guards against a missing one.
  - WGL (M2.6) and NSOpenGL (M2.7) must keep the same promises.
- **Alternatives considered:**
  - A safe trait, as sketched: unsound, for the reason above.
  - A safe trait sealed by a private supertrait: sound, but gfx could not
    mock it. Its "a missing function is an error" path could then not be
    tested on Linux, where libglvnd gives a stub for every `gl` name.
  - `make_current` in the trait, or making the context current in every
    method: neither covers the GL calls that gfx makes directly through
    the stubs.
- **Revisit when:** something needs two OpenGL contexts on one thread,
  such as a second window or contexts that share objects for a loader
  thread.

## ADR-0016 — A gfx Device holds no borrow of its context

- **Status:** Accepted
- **Date:** 2026-10-18
- **Context:**
  - The main loop keeps `gfx::Device::new(platform.gpu_surface())` alive
    while it calls `platform.pump_events(&mut events)`. A `Device<'a>` that
    borrowed the surface would not compile there. The `'static` modal-frame
    callback (Windows, macOS) could not hold it either.
  - A device that borrows nothing must still prove, before every GL call,
    that its context exists and is current. Under libglvnd a call with no
    context does nothing, silently; on macOS it crashes.
  - The sketch in `gfx.md` had `Pass` methods return `()`. The acceptance
    of M2.3 needs a destroyed id to give `StaleHandle`, and a draw that
    would read outside a buffer must be refused, because GL does not
    always catch it.
  - `gfx.md` asked for `glGetError` after every call "in dev". The feature
    table gives `dev` to no such crate as `gfx`.
- **Decision:**
  - `nadir-platform` gives each of its contexts a `GlContextId` that is
    never reused. `GlContextId::is_current()` reads a thread-local, and is
    `true` while that context exists on this thread.
    `GlContext::context_id()` is a provided method, `None` by default. It
    extends ADR-0015's promise: with `Some(id)`, the function pointers stay
    callable while `id.is_current()`.
  - The per-thread claim of ADR-0015 holds the id, and moves to the shared
    `gpu.rs`, so that WGL (M2.6) and NSOpenGL (M2.7) take it too.
  - `Device` keeps the id and no borrow. Each call that would reach GL
    checks the id first, and fails with `GfxError::ContextLost` once the
    context is gone. A thread has at most one device
    (`GfxError::DeviceExists`).
  - `begin_frame` and `end_frame` take the surface: one for its drawable
    size, the other to swap.
  - `begin_pass`, the five `Pass` setters that take an id or data, and both
    draws return `Result`. `set_viewport` and `set_scissor` stay `()`.
  - `glGetError` is read after every call under `cfg(debug_assertions)`. In
    every build it is also read once a frame, before every draw, and after
    every call that allocates.
- **Consequences:**
  - The facade can keep the device beside the platform, and in the
    modal-frame callback's shared state.
  - Each call that reaches GL pays a thread-local read: about 1.5 ns,
    measured.
  - The device fails closed on a context that has no id: the stand-in of
    Windows and macOS until M2.6/M2.7, and any mock.
  - Render code adds a `?` to each pass call.
  - A future context that forgets to take the claim, or to override
    `context_id`, gets `NoContext` from gfx, not undefined behavior.
  - A context has one device. A device whose context is gone does not
    count, so a thread can make one for its next context.
  - The draw checks cover what the device binds. Shader code is trusted:
    an array index out of bounds in GLSL is undefined behavior that no check
    covers. GLSL sources may not use `#extension`.
- **Alternatives considered:**
  - `Device<'a>` borrowing the context. It is sound by the borrow checker,
    but the main loop does not compile, and `Platform` would need `&self`
    pumping on three systems.
  - A required liveness token on `GlContext`, built on an `Rc`/`Weak`
    pair. It is as sound, but it breaks the ADR-0015 trait, and every
    implementation changes.
  - Setters that return `()`, with errors at the draw or kept for a
    `finish` call. Errors would show up later than the call that caused
    them.
  - A `dev` feature on gfx, or a runtime switch, for per-call checks. The
    first changes the feature table, and the second costs a branch per
    call.
- **Revisit when:** a second context per thread is needed (see
  ADR-0015), or the per-call liveness check shows up in a profile.

## ADR-0017 — Golden scenes draw in GL test binaries; xtask compares their pixels with QOI references

- **Status:** Accepted
- **Date:** 2026-10-18
- **Context:**
  - The golden harness (`docs/04-quality.md`) needs three things: scenes
    drawn on an offscreen context, references stored as QOI, and a
    per-channel comparison that writes diff images.
  - The dependency matrix allows no crate to do all three. `nadir-gfx` and
    its tests may use `core`, `sys` and `platform`, so they can draw but
    cannot use `nadir-codec`. `xtask` may use only `core` and `codec`, so it
    can read and write QOI but cannot draw. `cargo xtask layers` checks
    dev-dependencies too.
  - `core.md` placed "golden comparison helpers (pure pixel diff)" in
    `nadir_core::testing`. With the comparison there, M2.4 would touch four
    crates.
- **Decision:**
  - Each golden scene is a test in the GL test binary of the crate that
    draws it: `crates/gfx/tests/headless/golden.rs` from M2.4, `render`'s
    from M3. A scene draws into a render target and reads it back. When
    `NADIR_GOLDEN_DIR` names a directory, it writes the raw RGBA8 pixels
    there as `<scene>.pam` (Netpbm PAM), with the renderer's name in a
    comment.
  - `cargo xtask golden` runs those tests with `cargo test`, setting the
    variable, then compares each PAM with `tests/golden/<scene>.qoi`. The
    comparison, the diff images and the QOI files belong to xtask; the
    comparison does not go into `nadir_core::testing`.
  - The user chose the comparison's place on 2026-10-18.
- **Consequences:**
  - No new edge in the dependency matrix. M2.4 touches codec, gfx (tests
    only) and xtask.
  - The scenes also run, writing nothing, in `cargo xtask ci`, so a scene
    that hits a GL error or fails its own pixel checks fails CI on every
    system that has a context.
  - The PAM writer (in the test binary) and reader (in xtask) are two
    copies of one small format. A unit test in xtask pins what the reader
    takes.
  - A scene that skips for want of a context writes nothing, and xtask
    reports its reference as not drawn: the run fails, it does not pass
    empty.
  - `render`'s scenes join by adding their test binary to xtask's list of
    scene commands. Their tests need a device without `platform`
    (`Device::new_headless`, noted for M3.2).
- **Alternatives considered:**
  - The comparison in `nadir_core::testing`, as `core.md` had it. It is
    usable by every crate, but only xtask compares, and it makes a
    four-crate task.
  - A QOI encoder copied into the gfx tests, so they could compare
    in-process. It is a second encoder to keep byte-identical.
  - A new crate for the scenes, depending on gfx, platform and codec. It
    needs a new row in the matrix.
  - Raw pixels with the size in the file name. That is about 10 lines
    shorter than PAM, but has no self-check, and image viewers cannot open
    it.
- **Revisit when:** a crate other than xtask needs to compare images
  in-process, for example a test that checks its own pixels against a
  reference.

## ADR-0018 — Pixel-perfect mode snaps sprite corners and rounds the zoom

- **Status:** Accepted
- **Date:** 2026-10-20
- **Context:**
  - `render.md` defined pixel-perfect mode as rendering the world into a
    render target of the virtual resolution, scaled up by a whole factor,
    with the camera snapped to whole virtual pixels. It said nothing of
    the sprites or the zoom.
  - A sprite's corner is its position less pivot × size. With the default
    pivot (0.5, 1), an odd-width sprite at a whole position has its corner
    on a half pixel. So does any sprite at a `.5` position. Its edges and
    its texel boundaries then fall on pixel centres. GL leaves both the
    edge coverage and the nearest-texel choice to the vendor there, so a
    13-pixel-wide sprite can draw a texel column twice and drop another,
    differently on each GPU, and moving sprites shimmer.
  - A non-integer zoom puts texel boundaries on pixel centres whatever is
    snapped: at zoom 1.5, a third of the pixel columns.
  - The design critique of M3.3 found this. The user chose both rules on
    2026-10-19.
- **Decision:**
  - In pixel-perfect mode, an unrotated sprite's top-left corner is moved
    to the nearest whole virtual pixel (halves up) when its instance is
    packed. Sorting still uses its exact y. Rotated sprites are not moved.
  - The zoom is rounded to a whole number, at least 1 (halves up; NaN is
    1).
  - The camera's origin, the view's top-left corner in virtual pixels,
    snaps the same way: `floor(x + 0.5)`, so each one-pixel move of the
    camera moves the view by one pixel, across zero too.
- **Consequences:**
  - Every texel of an unrotated sprite at whole-number zoom lands on whole
    pixels, the same on every GPU, which also makes goldens portable.
  - Motion is in whole virtual pixels. Sub-pixel smoothness belongs to the
    smooth-camera and native-resolution modes, which no task has built yet.
  - A fractional zoom is not available in pixel-perfect mode.
- **Alternatives considered:**
  - Snapping the camera only, as `render.md` had it. Pixels at texel
    boundaries then depend on the vendor, and odd sprites shimmer.
  - Allowing a fractional zoom and documenting that it is not exact. That
    leaves the shimmer in, in the mode named for having none.
  - Snapping in the vertex shader. It costs the same, but does it on the
    GPU, where the CPU-side tests cannot check it.
- **Revisit when:** the smooth-camera or native-resolution mode is built,
  or a game needs rotated sprites to stay on the pixel grid.

## ADR-0019 — Until the ECS, systems share a typed resource map; with no GPU device the game runs undrawn

- **Status:** Superseded by ADR-0022
- **Date:** 2026-10-21
- **Context:**
  - M3.4 makes the sandbox move a character. Its systems need shared state:
    the latched actions, the player's position, the textures and the
    frame to draw. Until then systems were closures `FnMut(&Tick)` and
    `FnMut(&Frame)` with no shared state but what they captured (M1.3).
  - The ECS `World` and its stages arrive in M4.4. Its resource calls are
    `resource`, `resource_mut` and `insert_resource` (`ecs.md`).
  - Windows and macOS have no OpenGL device until M2.6 and M2.7, so
    `Device::new` fails there.
  - The user chose both points below on 2026-10-21. The design critique
    of M3.4 added the rules for action sets and for render errors.
- **Decision:**
  - The facade has `Resources`, a map of one value per type behind
    `RefCell`, with the `World`'s resource calls. Systems become
    `FnMut(&Resources, &Tick)` and `FnMut(&Resources, &Frame)`: a break
    of the M1.3 signatures. A missing resource or a conflicting borrow
    panics, naming the type and the running system (or the engine step,
    such as `nadir::draw`).
  - The engine's own resources are `Input`, `AssetServer`, `RenderFrame`
    and `FrameStats`, plus `ActionMap<A>` and `ActionState<A>` from
    `App::add_actions`. The loop feeds every event to `Input`, latches
    once a tick before the fixed systems, empties the `RenderFrame` before
    the frame systems, then draws it and finalizes completed loads.
  - An app has one action set. `Input::latch` starts the next tick, so a
    second set's latch would see no edges. Adding another type panics;
    adding the same type again replaces its bindings.
  - When `Device::new` fails, `App::run` logs the error and runs the game
    without drawing. A failure of `Renderer::new` on a working device, or
    of a frame's drawing, ends the game with the error and the error
    dialog, as a failure to open the window does.
  - `Interpolated<T>` (`previous`, `current`, `snapshot`, `lerp`) is the
    interim interpolation. The game snapshots in its fixed system and
    lerps by `frame.alpha` in its frame systems.
  - `Config` gains `vsync`, `virtual_resolution` (a `UVec2`, as
    `RenderConfig` has it) and `assets` (the asset root).
- **Consequences:**
  - The sandbox moves, draws and is tested now, with M4.4's resource calls.
    M4.4 swaps `Resources` for `World`, systems from closures to `fn`
    items with stages, and `Interpolated` for `PrevTransform` and the
    snapshot system.
  - `ecs.md` names the borrows `Res` and `ResMut`; code that names
    `Ref` or `RefMut` changes then.
  - Every game's system closures changed signature. Only the sandbox and
    the facade's tests had any.
  - On Windows and macOS the sandbox shows an empty window until M2.6 and
    M2.7. Modal frames (a window drag or live resize) simulate but do not
    draw until `ModalFrame` carries a surface.
  - A frame system reads `ActionState` as the last tick left it: edges
    belong in fixed systems, as `App::add_actions` says.
  - `virtual_resolution` becomes an `Option` when the native-resolution
    mode is built (the API sketch in `01-architecture.md`).
- **Alternatives considered:**
  - Building M4.4's `World` now: the ECS crate (M4.1) does not exist.
  - Plain structs captured by `Rc<RefCell<_>>` in each closure: the
    roadmap's "plain structs" with no shared access path, and none of it
    survives M4.4.
  - Several action sets, each with its own `Input`: no game needs a second
    set yet, and the latch would have to be designed again for it.
  - Running undrawn on any render failure: a lost context or an
    out-of-memory error would then leave a frozen window with no message.
- **Revisit when:** M4.4 replaces `Resources` with the ECS `World`, or a
  game needs a second action set.

## ADR-0020 — Reflection takes a context argument, writes compact encodings, and leaves Handle and Entity to prefabs and saves

- **Status:** Accepted; ADR-0038 builds `Handle<T>` as its asset's path, through the context's asset server (M5.11)
- **Date:** 2026-10-25
- **Context:**
  - `docs/subsystems/scene.md` sketched `Reflect::to_value(&self)` and
    `apply_value(&mut self, v)`. With those signatures:
    - an unknown-field warning cannot name the file it comes from, which
      the spec asks for;
    - a `Handle<T>` cannot be written as its asset path, nor read back,
      without the asset server;
    - an `Entity` cannot be written as its `SaveId` without the world's
      map of them.
  - The spec named the built-in types, but not how `Vec2`, `IVec2`,
    `Color` and `Rect` are written. That is the on-disk format of prefabs,
    levels and saves.
  - The user decided all three points on 2026-10-25, in M4.2.
- **Decision:**
  - `to_value` and `apply_value` take a `&mut ReflectContext`. It holds the
    source's name, the path to the field at hand, and the warnings so far.
    Slots for the asset server and the `SaveId` map are added when prefabs
    and saves need them.
  - The encodings:
    - `Vec2` and `IVec2` are `[x, y]`.
    - `Color` is an sRGB hex string, `"#rrggbb"`, or `"#rrggbbaa"` when it
      is not opaque. Reading needs the `#`, and takes either case.
    - `Rect` is `{"min": [x, y], "max": [x, y]}`.
  - `Handle<T>` is reflected with prefabs and levels (M5.6), and `Entity`
    with saves (M9.4), where the asset server and the `SaveId` map exist.
    M4.2 builds every other built-in.
- **Consequences:**
  - Every `Reflect` impl takes the context, hand-written ones included. It
    also owns the path: `cx.field(name, …)` and `cx.index(i, …)` take the
    step off again whatever happens inside.
  - A warning names the file, the type, the field and the path. For
    example: prefabs/slime.jsonc: `Health` has no field `hp`, so it was
    left out (at `components.Health`). An error names the source and the
    path the same way.
  - `FromValue` is implemented per type, not for every `Default` type:
    `Color` has no `Default`, and `Entity` will have none either. The
    macros make a struct from its `Default` with the data applied, so a
    struct with a `Color` field needs a hand-written `Default`.
  - Two more encoding rules follow from `Value`:
    - A `u64` or `usize` too large for an `i64` is written as a string of
      digits.
    - Non-finite floats become `null` in JSON text, so they do not survive
      it.
  - ADR-0007 stands: the macros and `Value` are unchanged as the
    interchange.
- **Alternatives considered:**
  - The spec's signatures, with a thread-local holding the file name and,
    later, the asset server. Nothing would show when a call runs outside
    the scope.
  - `apply_value` returning the unknown fields for its caller to log. That
    names the file, but does not help handles and entities.
  - Named fields everywhere (`{"x": 1, "y": 2}`, `{"r", "g", "b", "a"}`):
    self-describing, but verbose in every prefab. Arrays everywhere
    (`[r, g, b, a]`, `[x0, y0, x1, y1]`): the most compact, and the least
    readable.
  - Building the `Handle` and `Entity` impls now, against test fakes. That
    designs the context before anything uses it.
- **Revisit when:** M5.6 or M9.4 adds the asset server or the `SaveId` map
  to the context. Also when a format needs a value the encodings cannot
  write, such as a non-finite float that must survive a save.

## ADR-0021 — Movement sweeps each axis and never goes deeper; colliders can move between rebuilds; tiles have a layer

- **Status:** Accepted
- **Date:** 2026-10-25
- **Context:**
  - `docs/subsystems/collision.md` says to move along X, then "resolve
    against the tile grid and solid colliders. Push out along X only",
    then the same along Y. It does not say what happens to a collider that
    starts inside a solid, such as one spawned in a wall or caught by a
    closing door.
  - The spec rebuilds the spatial hash once a tick. The facade moves
    entities one after another, so each later mover would be checked
    against where the earlier ones were at the start of the tick. Two
    movers walking into each other would end the tick overlapping by both
    their speeds.
  - The spec does not say how tiles relate to collision layers. The
    prefab example in `docs/subsystems/scene.md` gives masks such as
    `["World", "Player"]`.
  - The user decided these three points on 2026-10-25, in M4.3, and asked
    for a movement benchmark besides the required one.
- **Decision:**
  - Each axis of a move is a clamped sweep, not a move followed by a push
    out. A solid blocks the axis only if the motion goes toward its
    center. The mover stops where it would start to overlap the solid, or
    stays put if it already does.
    - So a mover that starts inside a solid can leave it toward the nearer
      side along the motion, and never goes deeper. An exact touch counts
      as ahead.
    - A solid that the mover overlaps across the axis by `EPSILON`
      (0.01 px) or less does not block it. This is what keeps seams
      between tiles from snagging.
    - A move longer than half the collider's smallest dimension is split
      into substeps, at most 16. Each axis is swept, so tunnelling does not
      depend on the substeps.
  - `CollisionWorld::set_position(index, position)` moves a collider
    between rebuilds:
    - The hash keeps the bounds it was built with. Queries widen their
      region by the largest move since the rebuild, up to a quarter of a
      hash cell.
    - A collider moved further than that goes on a list that every query
      scans until the next rebuild. One fast projectile does not widen
      every query.
  - `TileGrid` has a `pub layer: LayerMask`, every layer by default. A
    mover or a cast collides with tiles when its mask has the grid's
    layer.
  - The `collision_slide_5k` benchmark times 5,000 `move_and_slide` calls
    a tick, beside the required `collision_hash_5k`.
  - Choices made in M4.3 where the spec was open:
    - Casts and `nearest` take an `ignore` collider, so that a cast from
      an entity's feet does not hit its own collider.
    - A trigger mover is stopped by tiles only: it detects colliders, and
      moves into them to do so.
    - Casts share `EPSILON`: a cast along an axis grazes a solid it
      overlaps across the motion by `EPSILON` or less, and one that passes
      within `EPSILON` of a circle's surface grazes it. So a cast along the
      wall a mover rests against passes, as the move would.
    - A cast along the seam between two solid tiles, or within `EPSILON`
      of it, is stopped, while one along the outside of a wall passes.
    - A mover no more than `EPSILON` either side of its center, across its
      motion, treats a tile and its solid neighbours as one wall, so it
      cannot slip along a seam.
    - A query whose region spans more cells than hold colliders walks the
      cells that do, so a query never costs more than the colliders'
      cells.
- **Consequences:**
  - The spec's invariant holds as "a mover that starts clear ends
    overlapping no solid by more than `EPSILON`". A mover that starts
    inside a solid stays inside it until it moves out; it is not pushed.
  - The bound is `EPSILON` give or take one f32 step at the mover's
    position: under 0.002 px within ±16,384 px of the origin, under
    0.008 px within ±65,536 px. A mover may end up to `EPSILON` into a
    solid it passes, so rounding can only add to that.
  - The facade calls `set_position` after each move (M4.4). A teleport
    should rebuild the world instead.
  - A level loader sets `grid.layer` to the layer its masks call "World"
    (M4.5, M5.6). A grid built in code collides with everything until then.
  - The query order is: colliders too big to hash, then colliders moved
    far, then cell by cell. The spec said "by cell, then by insertion".
  - On the i7-11800H, `collision_hash_5k` (rebuild and 5,000 queries) takes
    1.5 ms. `collision_slide_5k` takes 2.2 ms, for a crowd among walls in
    which one move in four is stopped. The M4 exit budget is 4 ms a tick
    for 5,000 entities, everything included, on the min-spec machine.
- **Alternatives considered:**
  - Push out along the shallower axis before moving. It ends clear, but it
    can pop several pixels in one tick, or into another solid in tight
    spaces.
  - Ignore any solid the mover starts inside. It can then walk straight
    through the wall it spawned in.
  - A snapshot for the whole tick, as the spec has it. It is a smaller
    API, but movers overlap by up to a tick's movement.
  - One drift for every moved collider. A single fast mover would widen
    every later query in the tick.
  - Tiles that always block, or tiles on layer 0 by default. The first
    leaves no way for a ghost to pass walls, and the second is easy to
    forget in scenes built in code.
- **Revisit when:**
  - A game needs colliders pushed out of solids, for example by moving
    platforms or crushers.
  - Profiling in M4.4 shows that the 5,000-entity tick misses its 4 ms
    budget on min-spec.
  - A level needs half-cell tiles or diagonal walls (backlog).

## ADR-0022 — The facade runs the game in a World of staged systems; the engine owns interpolation, movement, triggers, parents and sprite drawing

- **Status:** Accepted; that `Sprite` and `Collider` are registered with the world only is superseded by ADR-0038, which reflects both and registers them in the `TypeRegistry` (M5.11)
- **Date:** 2026-10-26
- **Context:**
  - ADR-0019 gave the facade an interim `Resources` map, closure systems
    with no stages, and `Interpolated<T>`. It said M4.4 swaps them for the
    ECS `World`, `fn` systems in stages, and `PrevTransform` with a
    snapshot system. Its revisit trigger was M4.4.
  - M4.4 asks for the components `Transform`, `PrevTransform`,
    `Velocity`, `Sprite`, `Collider`, `Player`, `Parent` and `SaveId`,
    and for glue systems: snapshot, movement and collision, triggers to
    events, parent offsets, and extract (ADR-0014).
  - The ECS schedule takes `fn(&World, &Tick)` and `fn(&World, &Frame)`
    pointers, and names only the systems it runs in its borrow panics.
  - `Component` is a nadir-ecs trait, so the facade cannot implement it
    for `nadir_collision::Collider`. Reflection has no encoding yet for
    `Handle` or `Entity` (ADR-0020), nor names for collision layers.
  - The M4 exit asks for 5,000 entities with collision at 4 ms a tick or
    less on min-spec. The M4.3 note and ADR-0021 give the profiling of
    that tick to M4.4.
  - The user decided three points on 2026-10-26: `Collider` stays
    unreflected until prefabs (M5.6); M4.4 adds a benchmark of the whole
    tick and a 5,000-entity stress scene; and if that tick misses its
    budget, a roadmap task follows to make collision faster. The design
    critique of M4.4 led to the rest.
- **Decision:**
  - Replaced from ADR-0019:
    - The `App` holds a `World`, a `Schedule` and the clock. Systems are
      `fn` pointers added to a stage with a name:
      `add_fixed_system(stage, name, system)` and
      `add_frame_system(stage, name, system)`. Closures that capture
      nothing coerce. `Resources`, `Interpolated` and `Lerp` are gone.
    - `App::update(elapsed, events)` is public. It runs one frame without
      drawing it: input, then each tick (latch, tick stages), then
      `TickStats`, then the frame stages. `App::run` calls it each frame.
      Tests, benchmarks and headless runs call it directly. The
      `profile` feature's scope records are drained at the end of each
      update, into a buffer the app keeps.
    - `App::new` adds the engine's systems, each first in its stage:
      - `nadir::snapshot` (tick `Input`): copies `Transform` into
        `PrevTransform`. It gives one at once to every entity with a
        `Sprite` that has none, and to the root of every drawn child, so
        a system can write it from an entity's first tick.
      - `nadir::movement` (tick `Movement`): rebuilds the `Collisions`
        world from the entities with a `Collider` and a `Transform`. It
        then moves each one that has a `Velocity` with `move_and_slide`
        and `set_position` (ADR-0021), in the collider storage's order,
        then the rest freely by `velocity × dt`.
      - `nadir::parent_offsets` (tick `PostMovement`): puts each child at
        the first entity up its chain of `Parent`s that has none, its
        root, plus the offsets on the way. The chain is limited to 8
        steps; a longer one, or a loop, leaves the child where it is,
        after one warning, and so does a root without a `Transform`.
      - `nadir::triggers` (tick `PostMovement`): copies every collider's
        position from its `Transform`, then sends `TriggerEnter` and
        `TriggerExit` events, which name entities.
      - `nadir::extract_sprites` (frame `Extract`): draws each `Sprite`
        at its interpolated `Transform`, and a child at its root's
        interpolated position plus its offsets.
    - Engine steps outside the schedule (input, latch, emptying the
      `RenderFrame`, drawing) show `(none)` as the system in a borrow
      panic. The schedule's system name is private to nadir-ecs, and the
      game cannot make those steps conflict.
  - The components:
    - `Transform { position }`, `Velocity(Vec2)` in pixels a second,
      `Player` and `SaveId(u64)` are reflected and in the `TypeRegistry`,
      which is a world resource.
    - `PrevTransform(Transform)` is the engine's own, and `Sprite` (a
      `Handle`), `Collider` (a newtype of the collision crate's) and
      `Parent { entity, offset }` (an `Entity`) are registered with the
      world only. `app.register_component::<T>()` adds a game's reflected
      component to both.
  - `Collisions` is the collision resource:
    - It holds the `CollisionWorld` (read-only outside the engine, plus
      the tile grid) and maps collider indices to entities and back.
    - A collider's id is its entity's generation and index. The last two
      rebuilds' maps are kept, so an exit can name an entity despawned
      since.
    - Its trigger pairs are not public: the events are the interface.
  - `TickStats` holds the frame's ticks and how long they took.
  - Kept from ADR-0019, unchanged:
    - An app has one action set: a second type panics, and the same type
      replaces its bindings. Until the game adds one, the loop latches
      into a private set, so that each tick ends and the wheel moves on.
    - When `Device::new` fails, the game runs undrawn after logging why.
      A failure of `Renderer::new` on a working device, or of a frame's
      drawing, ends the game with the error and the error dialog.
    - `Config` keeps `vsync`, `virtual_resolution` (a `UVec2`) and
      `assets`.
- **Consequences:**
  - A game's systems set velocities in `Gameplay`, react to moves after
    the engine's in `Movement` or `PostMovement`, and read trigger events
    after `nadir::triggers`.
    - A `Transform` written up to `nadir::triggers` reaches the triggers
      that tick.
    - One written after reaches the collision world at the next tick's
      rebuild.
    - Queries before `nadir::movement`, and in frames, see the last
      tick's colliders. An entity despawned since still answers: check
      `World::is_alive`.
  - To teleport an entity, a fixed system writes both its `Transform`
    and its `PrevTransform`.
  - A child's solid collider is in its parent's way when the child's
    layer is in the parent's mask. Give it another layer, or make it a
    trigger.
  - `move_and_slide`'s contacts are dropped. No contact event exists yet.
  - The whole 5,000-entity tick is measured:
    - `facade_tick_5k` in nadir-bench takes 6.3 ms on the i7-11800H.
    - The sandbox's `--scene stress-entities`, in release, reports 5.5 ms
      a tick.
    - The movement system is 6.5 of the 6.6 ms of a settled tick:
      0.3 ms of rebuild, 5.5 ms of `move_and_slide`, with 4,200 of the
      5,000 moves stopped in a jammed crowd, and about 0.6 ms from
      `set_position`.
    - This meets ADR-0021's revisit trigger. Roadmap task M4.8 makes
      collision faster before the M4 exit.
  - The facade gains nadir-ecs, nadir-scene and nadir-collision as
    dependencies, and nadir-bench gains the facade (both allowed by the
    matrix).
- **Alternatives considered:**
  - Engine steps outside the schedule for the snapshot and the extract,
    as the loop sketch in `01-architecture.md` had them: the schedule's
    printout would not show them, and game systems could not be placed
    around them.
  - `PrevTransform` added by the game: entities spawned from prefabs,
    levels and saves would not be interpolated, and a forgotten one
    jumps.
  - A public `Collisions::set_position(entity, ...)` for games that move
    colliders after the engine: every game would have to remember it,
    where copying from `Transform` before the triggers costs one pass
    over the colliders.
  - Hash maps from collider ids to entities, refilled each tick: every
    collider hashed each tick, where only the few pairs that begin or end
    need looking up.
  - Making nadir-ecs's system-name guard public, so that engine steps are
    named in panics: a change to a fourth crate, for steps the game cannot
    make conflict.
  - Changing nadir-collision in this task to meet the budget: the change
    would spread over four crates. The user chose a separate task.
- **Revisit when:** a game needs systems that capture state, a second
  action set, contact events, or plugins (the API sketch's
  `add_plugin`); or M4.8 changes what `nadir::movement` asks of the
  collision crate.

## ADR-0023 — Tile layers live in a store the game owns, which the renderer reads beside the frame; chunks rebuild by revisions unique to the process; the frame's time is a Duration from the simulation clock

- **Status:** Accepted
- **Date:** 2026-10-27
- **Context:**
  - M4.5 asks for tile chunks: the `tile_chunk` shader, a chunk cache,
    culling and animated tiles; tall tiles as y-sorted sprites; and a
    hand-built map with a collision grid.
  - `render.md` gives the layer data to the facade or scene, and the
    renderer `TileChunkSet` GPU caches keyed by layer ID. `RenderFrame`'s
    `tile_layers` are "references to cached chunk sets + layer/depth
    info". It does not say how the renderer reaches the data.
  - `Renderer::render(device, assets, frame, output)` sees only the frame
    and the asset server. The asset server (M3.2) cannot hold an asset
    made in code, nor hand one out to change. Systems see only the
    `World`, and the renderer lives in the facade's loop, which skips
    drawing on modal frames and when there is no GPU.
  - `render.md` gives `RenderFrame.time` as `f32` seconds. An `f32` holds
    milliseconds exactly only up to 2^24, about 4.6 hours.
  - `FrameStats` is a public struct with no `#[non_exhaustive]`, built as
    a literal in nadir-bench and the facade's tests.
  - The user decided four points on 2026-10-27: the store beside the
    frame; four crates, with the chunk benchmark that `04-quality.md`
    requires; the time as a `Duration` from the simulation clock; and the
    sandbox's field as a hand-built map, with `stress-entities`' walls as
    a tile layer. The design critique (46 of 51 findings survived their
    refuters) led to the rest.
- **Decision:**
  - nadir-render holds the CPU side as plain data:
    - `TileSet { texture, columns, rows }`: a grid of equal tiles,
      numbered row by row. Its `block_uv(index, size)` gives a tall tile's
      sprite region.
    - `Tile { index, frames, frame_ms, flip }`: an animation is
      consecutive tiles, picked in the shader from the frame's time.
    - `TileLayer`: square cells of `tile_size` world units from an
      origin, as `TileGrid`'s, cut into chunks of `TileLayer::CHUNK` = 32
      cells a side.
    - `TileLayers`, a generational store handing out `Handle<TileLayer>`.
    - `TileLayerDraw { tiles, layer, depth }` in the frame.
  - **Breaking:** `Renderer::render` takes `tiles: &TileLayers` beside the
    asset server. The facade inserts a `TileLayers` resource and passes it.
    A game adds its layers to the resource, and pushes a `TileLayerDraw`
    for each to draw in an `Extract` system.
  - **Revisions:** each chunk of a layer has a revision, a `u64` from one
    counter for the whole process. A new layer takes a fresh one, and each
    `set` that changes a cell gives its chunk a fresh one.
    - The renderer packs a chunk when the view first shows it, and again
      only when its revision differs from the one it built. Frames that are
      not drawn need nothing.
    - A layer replaced in place, or a store replaced whose handles come
      again, can never pass for the chunks already built.
    - The renderer frees a layer's chunks at its first frame after the
      layer leaves the store.
  - **GPU:**
    - Each non-empty chunk has a `Static` buffer of 1,024 tile instances,
      made when it first has tiles.
    - An instance is six `f32`s: the cell, then the tile's index, frames,
      frame length and flip bits.
    - The `TileLayer` uniform block packs the snapped origin, the tile
      size, the tile set's grid, a placeholder flag, and the time in
      milliseconds as a `uint` (bit-exact). It is set once per layer, and
      there is one draw per visible non-empty chunk.
    - The shader does the frame and tile arithmetic in `uint`.
  - **Culling:** the view's world rect, a tile wider all round, to the
    chunks whose cells overlap it. Touching its far edge does not count.
    For 16-pixel tiles at 480 × 270 that is at most 2 × 2 chunks per
    layer.
  - **Order:** a tile layer sorts as a sprite at `SortMode::Fixed(depth)`
    in its layer, before the sprites of the same layer and depth. It goes
    into the sorted sprites at its position, splitting a batch it falls
    in. The sprite program, buffers and texture are bound again after it.
    `FrameStats` gains `tiles` and `chunks`. Its `batches` counts the
    sprite draws made; `SpriteBatcher::stats` still counts before any
    split.
  - **Textures:** a layer whose tile set's texture is loading is not
    drawn. A failed one shows its placeholder, all of it in every tile,
    when the server shows placeholders, as sprites do.
  - **Time:** `RenderFrame.time` is a `Duration`, which `clear()` leaves
    alone. `App::update` sets it before the frame stages from the
    `FixedClock`: its ticks and `alpha`. So whatever slows or pauses the
    clock (M7.6) slows or pauses tile animation. The shader gets the
    milliseconds modulo 2^32, so they wrap after 49.7 days.
  - **Tall tiles:** these are sprites of a tile set's block, y-sorted by
    their feet. The sandbox spawns them as entities; the level importer
    (M5.6) will emit them.
  - **Crates:** nadir-render, the facade, the sandbox and nadir-bench,
    four crates with the user's agreement. nadir-bench adds
    `tile_chunk_rebuild`, which `TileLayer::pack_chunk` makes public.
- **Consequences:**
  - Steady frames allocate nothing. The facade's `steady_frames` test now
    draws a tile layer of animated tiles across a chunk corner.
  - A tile set must be a whole texture cut into equal tiles, with no
    margin or spacing. Pixel-perfect tiles need a whole tile size, and a
    texture whose size is a whole multiple of the grid, as for sprites.
  - `tile_chunk` has GLSL only; its MSL comes with M10.3, as for the other
    programs.
  - A layer's tile set and origin reach the shader only as uniforms, so a
    setter for either would need no repacking. There is none yet: they
    are fixed when a layer is made, and a new layer packs anew.
- **Alternatives considered:**
  - The store as a field of `RenderFrame` that `clear()` keeps: no
    signature change, but the frame would mix per-frame lists with state
    that lasts, and `clear()` would mean two things.
  - `Rc<RefCell<TileLayer>>` in each draw: shared mutable state in the
    frame.
  - Tile layers as assets: needs the asset server to take assets made in
    code and to hand them out to change, a fifth crate, before M5.3
    rewrites that server.
  - Revisions counted per layer from 0: a layer replaced in place, or a
    store replaced, filled in the same order, keeps drawing the old
    chunks.
  - One buffer per layer for every chunk: memory for chunks never shown,
    and a zero-size buffer for an empty layer.
  - `f32` seconds for the time: milliseconds drift after hours.
  - Wall-clock time: tile animation would not follow the simulation's
    pause and time scale.
- **Revisit when:** a tile set is cooked into a shared atlas page, or
  carries LDtk padding or spacing (M5.4, M5.6); a level's layers are too
  many to draw a layer at a time; or the asset server can hold layers.

## ADR-0024 — Clips live in a store the facade owns; an Animator plays one in a frame stage on the simulation clock; Facing mirrors a sprite that faces left

- **Status:** Accepted; what `nadir::animate` writes (a frame's size and pivot too), the flip about the quad's centre, and clips only by handle, are superseded by ADR-0035; ADR-0038 reflects `Animator` as its clip's name (M5.11)
- **Date:** 2026-10-28
- **Context:**
  - M4.6 asks for the `Animator` component and clips, hand-defined until
    M5, a frame-stage system, and a flip by `Facing`.
  - `01-architecture.md` puts animation playback in frame stages. Events
    sent there reach only frame readers: a tick clears its buffer before
    fixed systems run (`ecs.md`).
  - The asset server cannot hold an asset made in code (ADR-0023). M5.5
    will import clips from Aseprite, and M5.6 names one in a prefab
    (`"Animator": { "clip": "anim/slime_idle" }`). M5.4 trims frames in
    the atlas.
  - `RenderFrame.time` follows the `FixedClock` (ADR-0023). Animated tiles
    pick their frame from it.
  - The user decided four points on 2026-10-28: clips in a store in the
    facade; clip time from the simulation clock; `Facing::Left` mirrors
    and the other ways unmirror; and the sandbox's people walking, with
    `Facing` in place of its hand-written flips. The design critique (37
    of 43 findings survived their refuters) led to the rest.
- **Decision:**
  - **Clips:** the facade has `Clip`, `ClipFrame`, `Playback` and
    `Clips`.
    - A `ClipFrame` is a region of a texture shown for a `Duration`. It is
      `#[non_exhaustive]` and made with `ClipFrame::new`, so that a trimmed
      frame's size and pivot can be added later.
    - A `Clip` is frames in order, and `Playback::Loop` or
      `Playback::Once`. `Clip::new` panics on no frames, a frame of zero,
      or frames longer than a `Duration` holds in all.
    - `Clip::frame_at(elapsed)` gives the frame shown at a time:
      half-open frames, wrapped by the duration for `Loop`, and the last
      from the end on for `Once`.
    - `Clips` is a generational store handing out `Handle<Clip>`, a
      resource that `App::new` adds. It is the only source of
      `Handle<Clip>`: a clip is never an asset of the asset server too,
      whose handles index another arena.
  - **Animator:** a component of a clip handle, a time into the clip, and
    whether a `Once` clip finished. `new`, `starting_at(clip, elapsed)`,
    `play(clip)` (which goes on if the clip is already playing) and
    `restart()`.
  - **`nadir::animate`**, first in the frame stage `Animate`:
    - Each frame it advances every animator by the change in
      `RenderFrame.time` since the last frame. So clips slow, stop and step
      with the clock, as tiles do. An animator that starts is advanced on
      its first frame by that frame's whole step.
    - It writes the shown frame's texture and uv into the entity's
      `Sprite`. An animator whose clip left `Clips` stays as it is, after
      one warning.
    - It sends `AnimationFinished { entity, clip }` once, on the step at
      which a `Once` clip reaches its end. It is sent again only after
      `restart`, or after `play` of another clip.
    - Then it sets `flip.x` of the Sprite of every entity with a `Facing`:
      true for `Left`, false otherwise. It leaves `flip.y` alone.
  - **Facing:** a reflected unit enum, `Down` (the default), `Up`, `Left`
    or `Right`. It is registered by `App::new`: simulation state, set in
    fixed systems. An entity with no `Facing` keeps the flip its game
    gives it.
  - **Frame state:** an animator's time and finished flag, and the
    texture, uv and flip that `nadir::animate` writes, move once a frame.
    Fixed systems may call `play` and `restart`, but must not decide
    gameplay by them.
  - **Crates:** the facade, the sandbox and nadir-bench.
    - The sandbox's people stand or walk four frames from a new sheet
      (`person.qoi`, 60 × 20) and the atlas, now 128 × 64.
    - nadir-bench adds `facade_frame_10k`.
- **Consequences:**
  - Steady frames allocate nothing: the real-loop `steady_frames` test
    animates its 10,000 walkers, and `steady_animation` sends and replays
    2,500 finishes a cycle in frames of fixed lengths.
  - A frame-stage send also fills the event's tick buffer until the next
    tick. Its peak is the finishes of the step of the last frame that ran
    a tick, and of the frames without a tick after it: less than a tick
    plus that step. That is up to two ticks' worth at 60 frames a second
    (a frame that straddles a tick runs none or two), and up to 250 ms and
    a tick of clips after a hitch.
  - After a hitch (a frame counts for at most 250 ms), early frames of a
    clip can be skipped, and a `Once` clip no longer than the step ends on
    its first frame.
  - An animator's phase counts from the frame that first sees it.
    `Animator::starting_at(clip, RenderFrame.time)` keeps a looping clip
    in step with tiles of the same frame lengths, to the millisecond
    their time is rounded to.
  - Readers of `AnimationFinished` run in frames. A spawn or despawn they
    make lands at a tick that depends on frame timing, so the world hash
    (M4.7) must leave such cosmetic entities out, or games time them in
    ticks.
  - `Animator` is not reflected: its clip is a handle (M5.6). Its time is
    transient: saves rebuild it (`scene.md`).
  - `Facing` mirrors only its own entity's Sprite, about the quad's centre
    (render's flip): a `Parent`'s child needs its own `Facing`, and its
    offset is not mirrored.
- **Alternatives considered:**
  - Clips as assets, through a new `AssetServer::add`: a second crate now,
    and `Clip` would need an `Asset` impl that cannot decode until M5.5.
  - `&'static [ClipFrame]` clips: no store, but M5.5 could not load clips
    without breaking the API.
  - Advancing by `Frame::dt`: clips would keep playing while the
    simulation is paused, out of step with the tiles.
  - Advancing a new animator by zero on its first frame: frame 0 always
    shows, but every clip then lags the clock by a frame, and
    `starting_at` could not match the tiles.
  - Up and Down keeping the last flip: side-only art works with no game
    code, but the flip would depend on history, and asymmetric up and down
    art could come out mirrored.
  - Four phase-shifted copies of a walk clip for the crowds: three clips
    with nothing new in them, which an Aseprite export (M5.5) would not
    have.
- **Revisit when:** a clip's frames are trimmed in the atlas (per-frame
  size and pivot, and mirroring about the pivot, M5.4 and M5.5); a game
  needs held items that follow their root's `Facing`; or the asset server
  can hold assets made in code.

## ADR-0025 — The facade plays replays a tick ahead in place of the live input; the world hash is FNV-1a of the reflected components; the determinism test runs a game of its own against one hash file for every system

- **Status:** Accepted; ADR-0038 has the hash leave out what `nadir::animate` writes of every `Sprite`, through a context for a hash (M5.11)
- **Date:** 2026-10-28
- **Context:**
  - M4.7 asks for a headless run mode, input recording in the sandbox, a
    world hash through reflection, and `nadir/tests/determinism.rs` with a
    stored recording and hash sequence. The test must fail when a system
    is made nondeterministic.
  - `04-quality.md` scoped determinism to the same build and the same
    system.
  - The facts:
    - The sandbox is a binary crate, so the facade's tests cannot run its
      field.
    - nadir-input's `InputPlayer` borrows its bytes, and its end shows only
      on the read after its last tick.
    - `World` cannot list its entities.
    - KBJ is not built (M5.2).
    - The engine's tick systems use only exactly rounded float operations,
      but `min`, `max` and `clamp` may give either zero, and NaN bits vary,
      from one processor to another.
  - The user decided four points on 2026-10-28:
    - the test runs a game of its own, from the facade's public API;
    - three crates (nadir-scene, the facade and the sandbox), the App
      taking a `'static` player;
    - one stored hash file for all three systems;
    - the sandbox records, replays, and replays headless.
  - The design critique made 61 points, and 60 survived their refuters.
    They led to the rest.
- **Decision:**
  - **The world hash** is `TypeRegistry::hash_world(world, cx)`, with
    `Registration::each` (a type's components in storage order) under it.
    - It is a 64-bit FNV-1a of a byte form: for each type in registration
      order, its name; then for each component, the entity's index and
      generation and its `to_value` with `cx`.
    - Values are written as tag bytes and contents, with lengths and counts
      first. Floats count by their bits, but zeros of either sign count as
      one, and so do all NaNs.
    - It takes the `ReflectContext`, so that the asset server (M5.6) and
      the SaveId map (M9.4) can reach it without a break.
    - Components of types not registered, and resources, are not in it.
    - `nadir::world_hash(world)` takes it with the engine's context.
      `App::world_hash()`, the sandbox and the test's whole-world column
      call it, so later context slots reach them. The test's per-type
      columns build a context of their own, which must get the same
      slots.
  - **Replays:** `App::play_replay(InputPlayer<'static, A>)` comes after
    `add_actions::<A>`.
    - From the next tick on, each tick's `ActionState<A>` is the replay's
      next tick, swapped in from a state read one tick ahead. The live
      input is latched into a set of its own, so `Input` keeps its ticks
      and nothing of it reaches the actions.
    - The resource `Replay` says how far it got: `Playing { ticks }`,
      then `Ended { ticks }` as soon as the last tick has run, or
      `Failed { ticks, error }` when the next cannot be read (logged). The
      ticks after it are latched live.
    - `add_actions` while a replay plays panics. So does `play_replay`
      before the game's actions are added, or with another set.
    - The pointer and the wheel stay live: replays do not hold them.
  - **The headless run** is `App::update` in frames of one tick, with no
    window. It runs every frame stage, logs only to the standard error, and
    finishes no asset load: tick systems must not depend on load state.
  - **The test** (`crates/engine/tests/determinism.rs`):
    - The game: a player steered and dashing, 200 walkers on a walled grid
      of tiles, a tenth of them triggers, and pickups dropped near the
      player, all from one seeded `Pcg32` (now re-exported by the facade).
    - The fixtures: `tests/determinism/replay.nrpl` and `hashes.txt`, the
      hash before the first tick and after each, whole and by type. They
      come from a live run of a scripted input in frames of mixed lengths.
      `NADIR_BLESS=1` writes them, after it checks two live runs alike and
      their replay at three other pacings.
    - The checks:
      - two live runs in one process agree, so nondeterminism shows
        without the fixtures;
      - the live run matches the fixtures;
      - the replay matches them in frames of one tick, of seven (whose last
        runs live ticks past the end) and of mixed lengths, so the world
        cannot depend on how ticks fall into frames;
      - the run covers dashes, pickups, reused entity slots and trigger
        entries;
      - the walkers moving in another order give other hashes.
    - On a mismatch it names the first tick and the types that differ, and
      writes the hashes found under the build's test output, a file for
      each check.
  - **The sandbox:**
    - `--record FILE` writes each tick as it comes.
    - `--replay FILE` plays one in the window, then the keys take over.
    - `--headless` plays one with no window.
    - Recording and replaying each log the ticks and the world's hash after
      them.
  - **Crates:** nadir-scene, the facade, the sandbox.
- **Consequences:**
  - One `hashes.txt` serves Linux, Windows and macOS only while the tick
    path keeps these rules:
    - no `sin`, `cos`, `atan2`, `powf`, `exp` or the like (so not
      `Vec2::from_angle` or `angle`);
    - no iteration of a `FastHashMap` or `FastHashSet`, whose order is
      fixed per target only;
    - no state whose bits a zero's sign or a NaN could set.

    The sandbox's stress scenes use `sin` and `cos`, so their hashes hold
    on one system only. If a system's CI run disagrees, the fallback is
    files per target (`hashes.<os>-<arch>.txt`, chosen by
    `std::env::consts::{OS, ARCH}`), made from the hashes the failure
    prints: the differences named above come from the processor. CI runs
    x86_64 Linux and Windows and arm64 macOS; x86_64 macOS is not run.
  - A game replays alike at any frame rate only if its frame systems write
    no reflected state and spawn or despawn nothing, and its tick systems
    read nothing that frames set (an `Animator`'s time, the `Sprite` fields
    that `nadir::animate` writes, `TickStats`). A frame spawn shifts
    the entity slots, and through collider ids the trigger order, of later
    tick spawns. So leaving cosmetic entities out of the hash, which
    ADR-0024 offered, is not enough: games spawn them in ticks.
  - The handover from a replay to live keys loses edges: an action held
    in the replay's last tick is not just released, and a key held through
    it is held but not just pressed. nadir-input would need a way to latch
    from a given state.
  - Each new reflected type, or change to the simulation's results,
    changes `hashes.txt`: bless it again, and say why. An unchanged file
    shows that results stayed bit for bit.
  - The sandbox's `--record` hashes the world after each frame that ran a
    tick, which allocates and takes time (about 16 ms a frame for
    `stress-sprites` in a debug build), so a recording run's frame rate
    and allocations are not the scene's own. It logs its ticks and hash
    when the window is closed; after a crash or a kill its file keeps
    every tick, but nothing is logged. The replay plays in the scene it is
    given, as nothing in a replay names one.
- **Alternatives considered:**
  - The sandbox as a library, with the test in its crate: the test would
    replay the real field, but the Verify line and `04-quality.md` name the
    facade's test.
  - An `InputPlayer` that owns its bytes, in nadir-input: no leak, but a
    fourth crate.
  - Finding the end on the tick after the last: that tick runs live, so a
    headless hash would be one tick late.
  - Hashing only stored per-system hashes, or comparing on Linux alone:
    safer for CI, but nothing would catch a difference elsewhere.
  - Leaving cosmetic entities out of the hash by a tag: frame spawns still
    shift later ids.
  - A canonical entity numbering in the hash: independent of slots, but it
    would hide an id divergence until it changed a value. Saves (M9.4) may
    need it.
- **Revisit when:** CI disagrees with `hashes.txt` on another system;
  M5.6 reflects `Sprite`, whose frame-written fields must stay out of the
  hash; M8.4 changes the replay format or needs edges across a handover;
  or M9.4 compares a world across a save and a load.

## ADR-0026 — `move_and_slide` gathers a move's solids once and takes contacts from the first pass; hash entries hold their bounds; moves end where per-sweep queries put them

- **Status:** Accepted
- **Date:** 2026-10-29
- **Context:**
  - ADR-0022 measured the facade's 5,000-entity tick at 6.3 ms on the
    i7-11800H, against the M4 exit budget of 4 ms on min-spec. That met
    ADR-0021's revisit trigger. Roadmap task M4.8 makes collision cheaper
    and keeps ADR-0021's rules.
  - Counted in one jammed tick of `facade_tick_5k`:
    - 10,000 axis sweeps, of which 7,511 were stopped. Each stopped sweep
      asked for its solids a second time, for the contacts.
    - 16,438 hash queries returned 193,550 candidates, of which 49,218
      overlapped the mover across its motion.
  - M4.8 named three ways:
    - gather the hash's candidates once per move;
    - find the contacts in the first pass, or skip that pass;
    - re-bucket moved colliders, which needs an ADR because it changes
      ADR-0021's `set_position`.
  - A design critique, with each point checked by a refuter, found two
    gaps in the first design:
    - A margin scaled to the mover alone drops giants kept aside, whose
      edges round by pixels.
    - Negative sizes break the overlap argument.

    The critique also found that the existing tests and the determinism
    hashes caught none of nine planted bugs in the gather.
  - The user decided on 2026-10-29:
    - all three steps below;
    - an old defect in `CollisionWorld::candidates`, found by the
      critique, goes to the backlog.
- **Decision:**
  - **One query per move.** `move_and_slide` gathers, once, the solid
    colliders on its mask, other than its own, whose bounds overlap the
    box round the collider at both ends of the move.
    - The box is grown by a margin of 1 px plus 2^-16 (128 f32 steps) of
      the largest coordinate rounding could involve: the box's own, plus
      the widest collider in the hash. The hash records that width at each
      rebuild. It is usually under 1,024 cells, but cells past the `i32`
      range saturate, so a collider there can be any width.
    - Colliders kept aside, which can be wider, are all gathered.
    - A negative size counts as its absolute value.
    - Each sweep then scans its tiles, as before, and offers those
      colliders.
    - The gather holds up to 32, in a fixed array on the stack. A move
      with more near it asks the hash on each sweep, as before. So does a
      mover of negative or not-a-number size, or whose box is not finite.
  - **Contacts from the first pass.** A sweep remembers up to 16 solids
    that would stop it, and where. Those within `EPSILON` of where it stops
    are its contacts, in the order the second pass would have found them.
    A sweep that more than 16 would stop, anywhere along it, takes the
    second pass.
  - **Bounds in the hash.**
    - Each hash entry holds the bounds its collider was hashed with, so
      entries grow from 12 to 28 bytes.
    - `CollisionWorld::near_candidates` serves the gather. It passes over
      a hashed collider whose hull misses the region grown by the drift
      plus 2^-16 of it. A collider not moved far is within the drift of
      where it was hashed, up to rounding.
    - Other queries are unchanged.
  - **The contacts' order.** Within a sweep the tiles come row by row,
    then the colliders, in an order that is fixed but not specified. It
    may differ between a move with up to 32 solids near and one with more.
  - **No change to the public API**, nor to ADR-0021's rules or
    `set_position`.
  - **The moves end where per-sweep queries put them,** bit for bit, with
    two exceptions:
    - The sign of a zero coordinate can differ. The order of the solids
      can choose it when zero-width colliders meet at ±0, and the world
      hash takes zeros of either sign alike (ADR-0025).
    - Where rounding exceeds `EPSILON`, a sweep's own query, widened only
      by `EPSILON` and the drift, can miss a solid that its arithmetic
      stops at. The gather finds it and stops where the sweep's arithmetic
      says. Rounding exceeds `EPSILON` beyond about 2^17 px from the
      origin, or at a hashed solid whose half extent is 2^18 px or more
      (about 2^19 px wide), which cells over 512 px allow. This is the
      backlog's defect in `candidates`.
  - **Tests.** A crate-private `Shortcuts` turns each shortcut off, for:
    - a differential test of 15,000 random moves (`src/tests.rs`, about
      2 s in debug). Test-only counters show that moves fell back and that
      sweeps took a second pass.
    - giants kept aside, hashed in cells of 1e8 px, and hashed in one
      saturated cell, against the sweeps' own queries;
    - a solid whose edge rounds, to even, onto the gather's region at
      100,000 px. A margin of 0 would drop it; the margin's size rests on
      the argument above.
    - scenarios at the limits: a solid kept aside, 33 solids near with the
      stopper last, 17 stoppers at once, and a contact exactly `EPSILON`
      behind.
- **Consequences:**
  - On the i7-11800H, 2026-10-29: old and new runs interleaved, pinned to
    one core, medians of four rounds:

    | Benchmark | Old | New |
    | :- | :- | :- |
    | `facade_tick_5k` | 6.32–6.38 ms | 2.40–2.43 ms |
    | `collision_slide_5k` | 2.17–2.19 ms | 1.04–1.07 ms |
    | `collision_hash_5k` | 1.43–1.45 ms | 1.50–1.54 ms (about +4%) |

    - The hash bench is slower, by 3.7 to 7.5% round by round, because
      every query, and the rebuild, walk the larger entries.
    - In three full unpinned runs earlier that day, of the code before the
      review, every benchmark moved by up to ±12% from one run to the
      next. Those runs were not saved.
    - `bench/baseline.json` holds `facade_tick_5k` 2.39 ms and
      `collision_slide_5k` 1.05 ms, from a full run pinned to one core. In
      that run the other entries were 0.6% slower to 10.9% faster than
      their baselines, which were left as they were. `collision_hash_5k`
      read 0.4% slower.
  - Per step, as the critique measured `facade_tick_5k` on its prototype:
    the first-pass contacts alone gave about 4.0 ms; with the gather,
    3.2 ms; with bounds in the entries, 2.7 to 2.9 ms. For
    `collision_slide_5k`, the steps gave about 2.0, 1.7 and 1.4 ms. The
    code review's two changes took it the rest of the way:
    - all-zero fillers, so that the gather's array is filled by a
      memset;
    - a margin from the widest collider in the hash, rather than 1,024
      cells.
  - The sizes of the fixed arrays:
    - In a jammed tick, of the bench or of the stress scene, a move
      gathers at most 6 solids, and a sweep has at most 4 stoppers.
    - 1,000 dashes of 150 px in the bench's crowd gather up to 29, and
      take 5.8 ms against 13.6 ms before.
    - At 300 px, 621 of the 1,000 moves fall back, and 489 sweeps take a
      second pass. They take 11.5 ms against 19.5 ms.
    - Both arrays are zeroed each move and each sweep: the gather's by a
      memset, a sweep's by sixteen 16-byte stores. Nothing allocates:
      `steady_ticks` still counts 0.
  - `--scene stress-entities`, 12 s in release with `dev`:
    - Headless, timing 120 ticks after 120, 600 and 1,200 ticks of
      settling, `App::update` takes 2.50 to 2.63 ms a tick, against 5.81
      to 6.57 ms before.
    - In a window with `--no-vsync --fps 1000`, a tick takes 2.1 to
      2.3 ms. Before, it took 4.8 to 6.4 ms, settling near 5.7 ms.
    - With vsync at 60 frames a second, the old build reports 5.0 to
      5.5 ms after 8.0 ms in its first second. The new one swings from
      2.0 to 6.1 ms from one second to the next. The likely cause is
      Linux's `schedutil` governor lowering the clock under that light
      load. The clock was not recorded.
    - So a vsynced tick time measures the clock as much as the code.
      M11.5's min-spec measurement should say which governor ran.
  - The determinism test's `hashes.txt` is unchanged. Its game never
    reaches the cases the gather changes, so the differential test is
    what shows the moves unchanged.
  - The facade's `move_colliders` is untouched. ADR-0022's revisit trigger
    on what `nadir::movement` asks of the crate is not met.
- **Alternatives considered:**
  - Re-bucketing moved colliders: the drift, about 4.3 px in a jammed
    tick, now widens one query per move rather than up to four.
  - Smaller hash cells: with the gather, cells of 48 to 16 px were no
    faster than 64 px.
  - Skipping the contacts' pass when the caller wants none: an API change,
    for less than remembering the stoppers costs.
  - A scratch `Vec` behind a `RefCell`, which would make `CollisionWorld`
    not `Sync`, or a scratch argument, an API break, instead of fixed
    arrays with fallbacks.
  - Hashed bounds in a parallel array, or read from the hash's
    per-collider bounds: the critique measured both slower in the facade
    tick.
  - Turning a zero stop of -0.0 into +0.0: a single stopper at -0.0 would
    then differ from before instead.
  - A margin of the mover's scale alone: giants kept aside were dropped,
    as above.
- **Revisit when:** M11.5's min-spec machine measures `facade_tick_5k` or
  the stress scene over 4 ms a tick; a game's moves often fall back to
  per-sweep queries (more than 32 solids near, or more than 16 stopping a
  sweep); or the backlog fix to `candidates` lands, which ends the second
  exception.

## ADR-0027 — The PNG decoder reads every color type and depth, rejects damage libpng loads, rounds 16-bit samples, and reports the color chunks it ignores

- **Status:** Accepted; its rule that the cook warns for any chunk in `Image::ignored` is superseded by ADR-0031; its 120 s of CI fuzzing a target is superseded by ADR-0028 (30 s)
- **Date:** 2026-10-30
- **Context:**
  - M5.1 adds the cook tool's PNG decoder. The spec's subset left out
    grayscale at bit depths 1, 2 and 4. That is valid PNG, which every
    decoder must read, and optimizers (optipng, oxipng, pngcrush) write it
    for masks and two-tone art.
  - libpng 1.6, behind most tools artists use, loads several kinds of
    damage with a warning at most. Measured through GdkPixbuf 2.42.10 with
    libpng 1.6.43:
    - a bad CRC in an ancillary chunk
    - bytes after `IEND`, or after the image data's zlib stream
    - image data that inflates to more than the image needs, or an `IDAT`
      after another chunk once it has ended
    - a palette index past the palette, drawn black
    - `PLTE` in a grayscale image, after `IDAT` or after `tRNS`, or of a
      bad length as an RGB image's suggestion
    - `tRNS` in an image with alpha, after `IDAT`, twice, before the
      palette, of a bad length, or with more alphas than the palette can
      use
  - The spec said "ignored, with a single warning" for the color and text
    chunks, but the codec logs nothing and never knows a file's path.
  - The spec did not say how 16-bit samples become 8-bit.
  - `CodecError` gains a `Png` variant that exists only with the `png`
    feature. Cargo turns a feature on for every crate in a build that wants
    it, so an exhaustive `match` elsewhere could compile in one build and
    not another.
- **Decision** (the user chose the first three on 2026-10-30):
  - Read every PNG color type and bit depth, grayscale at 1, 2 and 4 bits
    among them.
  - Reject each kind of damage above with an error that names its byte.
    The module doc of `png.rs` and the PNG subset in
    `docs/subsystems/assets-and-codecs.md` list each difference from libpng
    that was measured. Accept what libpng accepts and is harmless: a
    palette longer than the bit depth can index, a palette `tRNS` with no
    alphas, and a `tRNS` value past the bit depth, read in the depth's bits
    (libpng warns about the last two). One case goes the other way: image data
    that reaches further back than its zlib header's window decodes as
    written, as zlib does given its largest window, where libpng rejects
    it.
  - Round a 16-bit sample to the nearest 8-bit value, `round(v / 257)`.
    libpng's `png_set_strip_16` truncates; Go's image/png gives the exact
    16-bit values, which round the same way.
  - `png::decode` names the ignored color and text chunks in
    `Image::ignored`, and the cook tool (M5.4) warns once for a file that
    has any. `cICP`, `mDCV` and `cLLI`, from the PNG Third Edition, join
    the list: they say the colors are not sRGB.
  - Mark `CodecError` `#[non_exhaustive]`. Nothing outside the codec
    matched it exhaustively.
  - Commit the generator of the test vectors,
    `crates/codec/tests/png/make_vectors.py`. It is the repo's first
    Python file: it uses only the standard library, and nothing runs it
    but a person adding a vector.
  - Keep CI's fuzzing at 120 s a target. M5.2 sets a total budget, or adds
    the nightly job that `docs/04-quality.md` describes.
- **Consequences:**
  - Optimized art cooks. A damaged export fails with the byte at fault, and
    is exported again rather than cooked as libpng would show it.
  - A file that every viewer opens can still be refused, for one of the
    reasons listed.
  - 16-bit art differs from libpng's reading by at most 1 in a channel.
  - Of the 4,033 PNGs on the development machine and the 48 of Go's
    PngSuite copy and test data, the decoder and Go's image/png decode
    every file both accept to the same pixels. Each file one of them rejects is broken on
    purpose; the only one the decoder alone rejects is Go's
    `invalid-palette.png`, for its palette index.
  - A `match` on `CodecError` outside the crate needs a wildcard arm.
  - Decoding holds the inflated image data beside the pixels, up to 2.25
    times their size (16-bit RGBA one pixel wide), so up to 3.25 times
    `max_decoded_bytes` in all; and a copy of the compressed image data
    when it is split over several chunks, as libpng's exporters split it.
    The field's doc says so.
- **Alternatives considered:**
  - Rejecting grayscale below 8 bits, as the spec had it: the cook would
    refuse optimized art.
  - libpng's leniency: damaged art would be cooked as libpng happens to
    show it, without a word.
  - Truncating 16-bit samples as libpng does: rounding is nearer, and
    matches Go's exact values.
  - Logging the warning in the codec: it could not name the file, and
    could not be tested by value.
  - Defining `Png` and `png::ErrorKind` without the feature: the `png`
    module would exist half-way without it.
  - A README of recipes in place of the generator: nobody could add a
    vector, or check one, without writing a generator again.
  - Converting 16-bit RGBA in place to keep the peak near the image data's
    size: more code, for a cook tool that runs on development machines.
- **Revisit when:** an export from the team's own tools (Aseprite first) is
  refused by one of these rules; the cook needs color management, so that
  a non-sRGB `iCCP` or `cICP` must be honored; or decoding a source image
  runs a development machine out of memory.

## ADR-0028 — The fuzzer builds optimized and runs 30 s a target in CI; KBJ has one encoding per value and keeps every float; the WAV and BMFont readers are strict

- **Status:** Accepted
- **Date:** 2026-10-31
- **Context:**
  - M5.2 adds four parsers, KLZ, KBJ, WAV and BMFont, each with a fuzz
    target. CI fuzzed each target for 120 s in the Linux job, whose limit
    is 30 minutes: 8 targets would take 16 minutes. `cargo xtask` built the
    fuzzer in the dev profile, where `png` ran 6,453 inputs a second; at
    `opt-level = 3`, with the dev profile's overflow checks and debug
    assertions, it runs 94,798. A nightly job of 30 minutes a parser is
    M11.6's.
  - The spec gave KBJ's grammar only. A grammar like it allows several
    encodings of one value: LEB128 numbers padded with empty groups, and
    NaNs with any payload. Reflection writes non-finite floats as
    `Value::Float`, which JSON text writes as `null` (ADR-0020), and saves
    (M9.4) and cooked data are KBJ.
  - libsndfile, behind most tools that read WAV, also reads 8-bit and
    32-bit integer PCM, takes a frame's size from the bit depth whatever the
    block alignment says, and reads a file whose RIFF size is wrong, as a
    file written to a pipe or cut short has. Of 294 WAV files on the
    development machine, 5 are 8-bit or 32-bit integer and 1 has a wrong
    block alignment; the other 288 decode to libsndfile's samples, bit for
    bit. GStreamer writes odd 24-bit data without its pad byte.
  - BMFont's text format has dialects. AngelCode's BMFont writes ids as
    code points, or as bytes of a code page (`charset="ANSI"`), can pack
    four glyph sets into a page's channels (`packed=1`), and writes its
    fallback glyph as id −1. Hiero writes `unicode=0` with code points.
    Snowb writes `unicode=1` beside a charset that lists the ids, and can
    write fixed-point advances (`xFpBits`). Distance-field generators write
    fractional numbers and a `distanceField` line. The text pipeline draws
    glyphs as an alpha mask times a tint (`render.md`).
- **Decision** (the user chose the fuzzing budget, KBJ's floats and the
  WAV subset on 2026-10-31):
  - `cargo xtask` builds with a new `[profile.xtask]`: the dev profile at
    `opt-level = 3`. CI fuzzes each target for 30 s. A hang is an input
    that takes more than 2 s, not 10, and each target reports its slowest
    input. The nightly job stays with M11.6.
  - Following from that, the `png` target decodes to 16 MiB of pixels at
    most, so that no valid input comes near the hang limit.
  - KBJ has one encoding per value, and `decode` accepts only that: no
    LEB128 number with more bytes than it needs, and no NaN but the quiet
    NaN `0x7ff8_0000_0000_0000`, which `encode` writes for every NaN.
    Infinities, −0.0 and subnormals are stored exactly. `encode` takes the
    `Limits` and refuses, with `CodecError::KbjEncode`, a value `decode`
    would reject: the same key twice, or nesting, entries or a string past
    a limit. Decoding reserves at most 1,024 entries ahead for each array
    or object, since the containers open at once all claim the same bytes.
  - The WAV reader is strict: 16- and 24-bit PCM and 32-bit float only,
    each plain or extensible, mono or stereo; the block alignment must be
    the format's; the RIFF size must be the file's, with or without the
    pad byte of an odd form. It stops reading at `data`, so a missing pad
    byte after odd data does not matter. Errors for a format it refuses say
    to export again.
  - The BMFont reader is strict: the text form only; ids are code points
    when `unicode=1` or `charset` is empty, Windows-1252 bytes for
    `charset="ANSI"`, and any other charset is refused; `packed=1`, a
    non-zero `xFpBits`, fractional numbers and `distanceField` are refused;
    every page needs a `page` line. Unknown keys are ignored, unknown tags
    refused. Id −1 is kept as `Font::fallback`.
  - A KLZ stream ends with a block of literals only, whose token's match
    half is 0; a stream that ends after a match is refused. What the
    encoder writes is not part of the format.
  - Commit the generators of the KLZ and WAV vectors beside them, as
    ADR-0027 did for PNG: standard-library Python, which nothing runs but
    a person adding a vector.
- **Consequences:**
  - CI's fuzz step takes about 4 minutes for 8 targets and tries more
    inputs than 16 minutes in the dev build would, but for `klz`, which
    compresses each input and runs only about 3.4 times as fast optimized.
    A first `cargo xtask` builds xtask, nadir-core and nadir-codec again,
    optimized, in `target/xtask/`; the checks it runs (`cargo test`,
    `cargo clippy`) keep their own profiles.
  - An input that is slow only without optimization is less likely to
    reach the hang limit. The lower limit and the slowest-input report
    make up for some of that; the nightly runs (M11.6) for more.
  - With the game's limits, a valid PNG at the 64 KiB input cap that
    inflates to an 8192 × 8192 image took the `png` target 1.3 to 2.1 s, as
    long as the hang limit. The target therefore decodes to 16 MiB of
    pixels at most (2048 × 2048 RGBA), where its slowest valid input,
    16-bit RGBA interlaced, takes 325 ms: a sixth of the limit.
  - A KBJ file decoded and encoded again gives the same bytes, so two
    files that hold one value are one file, as the pack's dedup by content
    hash needs. A save keeps its floats, which JSON text does not: that
    answers ADR-0020's trigger for non-finite floats in saves, and ADR-0020
    stands for text. `kbj::encode` can fail, and callers handle it.
  - Some WAV files that libsndfile reads are refused, among them jsfxr's
    exports, which are 8-bit unless asked otherwise, and files written to a
    pipe. They are exported again rather than cooked.
  - Some fonts are refused: other code pages, packed and distance-field
    fonts, and fonts without `page` lines (5 of 5 BMFont files on the
    development machine, which are DDS-paged game fonts).
  - The repo has three Python files, all vector generators.
- **Alternatives considered:**
  - A nightly job now: it is M11.6's, 8 targets of 30 minutes run one
    after another take 4 hours, and with a fixed seed every night repeats
    the last.
  - Keeping 120 s in the dev build: 16 minutes of the job's 30.
  - KBJ writing non-finite floats as `null`, as JSON text does: a save
    would lose them. Accepting any NaN payload and padded numbers: two
    files for one value, and decoding then encoding would not give the
    bytes back.
  - Reading 8-bit and 32-bit integer WAV, and taking the block alignment
    from the bit depth, as libsndfile does: more paths for formats every
    tool exports differently in seconds. Reading a wrong RIFF size to the
    end of the file: a file cut short would cook without a word.
  - Packed and distance-field fonts: the text pipeline would need a
    channel select and an SDF shader, which no task asks for.
- **Revisit when:** a fuzz run longer than CI's finds a bug that 30 s
  missed, or the hang limit fires on an input that is correct; a save
  needs a NaN payload; a tool the team uses can export only a WAV format
  or font this refuses; or the game wants distance-field text.

## ADR-0029 — CI records the sandbox and publishes a demo page at jabbott.dev/Nadir/; a features file covers the roadmap

- **Status:** Accepted; what its download packs, and the sandbox's fallback to the workspace's `assets`, are superseded by ADR-0033
- **Date:** 2026-10-31
- **Context:**
  - The user asked for a CI/CD workflow that deploys a working demo of
    the engine showing its features. The page should match jabbott.dev's
    style and list the features, each with a description, below the demo,
    and it should update by itself with each build and its features.
  - The engine is desktop-only, with no web or WASM build (ADR-0001), and
    so far runs only on Linux: M1.5 and M1.6 are open.
  - GitHub Pages already served `https://jabbott.dev/Nadir/`: Jekyll
    rendered `main`'s README there, and jabbott.dev's Deployments menu
    links to it.
  - `docs/04-quality.md` allowed only `actions/checkout`, `actions/cache`
    and the artifact actions, with `contents: read` outside release jobs.
    A Pages deployment needs GitHub's Pages actions, with `pages: write`
    and `id-token: write`.
  - CI already draws offscreen on Mesa's llvmpipe, for the goldens.
- **Decision** (the user chose on 2026-10-31: the demo recorded in CI,
  replacing the page at `/Nadir/`, a features file with a check, and
  jabbott.dev's live stylesheet):
  - **A recording, not a web build:** ADR-0001 stands.
    - `App::offscreen(size)` runs a game with no window. Each
      `Offscreen::frame(elapsed, events)` updates, draws into a render
      target on an offscreen context, and returns the pixels.
    - The sandbox's `--capture DIR` draws 600 frames, one tick each (10 s),
      at the virtual resolution, 480 × 270. It writes them raw, with a
      `capture.json` of the run's numbers.
    - In the field, a fixed list of key presses in the code walks the
      person. A committed replay would need recording again whenever the
      action map changed; `--replay` still plays one in its place.
  - **`cargo xtask site [--out DIR]`** builds the page into
    `target/site`, running external tools but making no network calls.
    `DIR` must be new, empty, or one an earlier run made and marked, and a
    run removes only what an earlier one wrote. It:
    - checks the features, and builds the sandbox in release;
    - captures `field`, `stress-sprites` and `stress-entities`;
    - encodes each with GStreamer (`gst-launch-1.0`) into a VP9 WebM, at
      twice the size with nearest-neighbour scaling, and a PNG poster;
    - packs the sandbox with `assets/` into a Linux x86_64 `.tar.gz` with
      `tar`, with its SHA-256 from `sha256sum` and the oldest glibc it runs
      on, read from the version names in the executable;
    - and fills `site/index.html` from `git`, `cargo metadata`, the
      roadmap, the features and the captures, beside `site/nadir.css` and
      `site/nadir.js`.
  - **`.github/workflows/pages.yml`:**
    - It runs after CI succeeds on a push to `jabbott-iii/Nadir`'s `main`
      (`workflow_run`), or by hand from `main`.
    - A gate job, which waits in no queue, skips a commit `main` has moved
      past. The build and the deploy then each wait in a queue of their
      own, so a run that will not publish never displaces one that will.
    - A run by hand publishes only a commit whose push to `main` CI passed,
      as the page says; its gate has `actions: read` to find that run.
    - It builds the commit CI passed on `ubuntu-latest` with `contents:
      read`, checking `main` again when the build starts. The build and the
      deploy run only in the attempt whose gate let them through: "Re-run
      failed jobs" keeps the gate's old answer and skips them, and "Re-run
      all jobs", which asks the gate again, or a run by hand is how to
      retry. Each attempt uploads its own artifact, `github-pages-<attempt>`,
      since a run's artifacts include every attempt's and
      `actions/deploy-pages` refuses two of one name.
    - Only its deploy job has `pages: write` and `id-token: write`.
    - It may use `actions/upload-pages-artifact` and `actions/deploy-pages`,
      pinned by SHA, besides the actions `ci.yml` may use.
    - The repository's Pages source becomes "GitHub Actions".
  - **`docs/features.jsonc`** lists the features, each with an area, a
    description and the roadmap tasks it needs. The page marks each one
    shipped, in progress or planned from the roadmap's ticks.
    Each feature also lists, as `ticked`, the tasks that were ticked when
    its summary was written. `cargo xtask features`, part of `ci`, fails
    when a ticked task belongs to no feature, a feature names a task the
    roadmap lacks, or a feature's `ticked` is not what the roadmap ticks:
    ticking a task means bringing its feature's summary up to date.
  - **The page loads jabbott.dev's own stylesheet and script**, and its
    header and footer repeat that site's. `site/nadir.css` adds the page's
    own components with that site's tokens.
  - **The download:** the sandbox looks for `assets` beside its executable
    first, as the download has it, then the workspace's.
- **Consequences:**
  - Good:
    - The page shows what the commit draws, with numbers from that run:
      the field's 600 frames were the same bytes on llvmpipe and on an
      RTX 3070.
    - The features move with the roadmap's ticks, and the check makes each
      task describe what it ships.
    - No crate is added, and nothing in the engine is web-specific.
  - Bad:
    - A publishing job holds write tokens, through two more actions.
    - Publishing depends on Ubuntu's Mesa and GStreamer packages.
    - The page depends on jabbott.dev's markup and class names.
    - The videos are VP9 WebM only:
      - Ubuntu's `openh264enc` ignored its bitrate and wrote the crowd at
        14 Mbps, so there is no H.264 copy. A browser without VP9 WebM
        shows the poster.
      - Chromium decoded `colorimetry=bt709` 10 levels too green, so the
        frames are converted with GStreamer's default, BT.601, which comes
        back within 2 levels: about 43 dB for the field.
      - The 10,000-sprite crowd does not compress well: about 6.5 MB, at
        about 20 dB.
    - Building the page took 54 to 63 s on the development machine, on
      llvmpipe. It is a workflow of its own after CI for each push to
      `main`.
    - When the newest commit fails CI, the page waits for the next one
      that passes.
    - The download needs Linux x86_64 with X11, EGL and the glibc its
      executable names (2.34 on 2026-10-31). It is not the release package
      (`docs/05-release.md`).
- **Alternatives considered:**
  - A WASM build that plays in the browser: it reverses ADR-0001, and
    needs a web platform layer, WebGL and the browser's event loop.
  - Run the windowed sandbox under Xvfb and record the screen with ffmpeg:
    the frames would follow the wall clock on llvmpipe, and differ from
    run to run.
  - QOI frames played on a canvas by the page: pixel-exact, but 10 to
    100 KB a frame, tens of MB a scene.
  - ffmpeg in place of GStreamer: it is not installed on the development
    machine, so the pipeline could not be run before CI. GStreamer's ran
    here exactly as CI runs it.
  - Keep the README at `/Nadir/` and the demo at `/Nadir/demo/`: the
    workflow would have to render Markdown too.
  - Features generated from the roadmap's task titles: they read as a
    task list.
  - A copy of jabbott.dev's styles in the repository: it would not follow
    that site's changes.
  - Push the page to a `gh-pages` branch with `contents: write`: that token
    could write any branch, where the Pages deployment's token can write
    only Pages.
- **Revisit when:** a build that plays in the browser becomes a goal
  (ADR-0001); Windows or macOS draws (M1.5 and M2.6, M1.6 and M2.7) and
  the page can offer their downloads, built on those runners, or M11.1's
  packages; jabbott.dev changes its markup or class
  names; an H.264 encoder on the runner honors its bitrate, or visitors
  without VP9 matter; or the page's job takes more than 10 minutes.

## ADR-0030 — The asset server decodes on its own workers and finalizes within a frame budget; an asset stays while any scope it was loaded in holds it; offscreen frames wait for their loads

- **Status:** Proposed; its rule that one storm run of three must keep to the limit is superseded by ADR-0034; `LooseSource` stays in every build, beside the `PackSource` that release builds read (ADR-0041)
- **Date:** 2026-10-31
- **Context:**
  - M5.3 moves reading and decoding off the main thread. The architecture
    gives the workers as `clamp(cores − 1, 1, 4)` threads with `mpsc` jobs
    in and results out, and GPU uploads on the main thread. Until now
    `load` read and decoded at once (M3.2), and the facade made the assets
    at the end of each drawn frame. A headless run makes none, so tick
    systems must not depend on load state (ADR-0025).
  - The task accepts a storm of 1,000 loads with no main-thread stall over
    4 ms a frame, and unloading a scope that frees its assets. The spec
    names three scopes, but `load` gives one handle for one path, and it
    does not say what an asset loaded in two scopes does. Unloading must
    destroy GPU textures, and `Asset` had no hook for it.
  - On the i7-11800H (16 threads), with 1,000 QOI images of 64 × 64: the
    1,000 calls to `load` take 0.55 ms in an optimized build and 2.5 to
    2.9 ms in the dev profile, where the hash map, arena and channel code,
    generic over the asset type, is compiled unoptimized; one run took
    7 ms. The storm then finishes in 8 ms over 5 frames optimized, and
    in about 160 ms over 50 frames unoptimized.
  - The demo page's recordings (ADR-0029) and the engine's offscreen tests
    must show the same frames on every run.
  - A review ran the storm on 4 CPUs (`taskset`) beside one busy process.
    With `cores − 1` workers, the main thread and the workers took every
    core, and the scheduler held the main thread for a time slice in the
    middle of a frame: 4.2 to 5.6 ms frames, in 6 runs of 6. With
    `cores − 2` workers it passed 10 runs of 10, and on 2 to 4 CPUs beside
    up to two busy processes.
- **Decision:**
  - **Workers:**
    - Each `AssetServer` starts `clamp(cores − 2, 1, 4)` threads named
      `nadir-assets-N`, not the architecture's `cores − 1`: that leaves the
      main thread a core and one to spare, for the audio thread, the GPU
      driver's and the rest of the system. They take jobs from one `mpsc`
      queue behind a mutex, and each asset type has a channel for its
      results.
    - A job reads the file through the server's `Source` and decodes it.
      A panic in either fails that asset alone, as
      `AssetError::Invalid("reading or decoding it panicked")`, and the
      worker goes on.
    - If no thread starts, jobs run on the thread that asks for the load.
    - Dropping the server drops the jobs still queued and waits for those
      running.
  - **Loading and finishing:**
    - `load` lowercases and checks the path, finds the asset or adds it,
      and queues its job. It never reads, so it never waits. A path that
      fails the check, or has the wrong extension, fails with no job.
    - `finish_completed_loads` takes the workers' results and frees the
      assets of unloaded scopes, all of them, outside the budget. It then
      makes decoded assets until a budget runs out: 2 ms unless
      `set_finish_budget` changes it. It makes one at least each call, and
      it never waits for a worker.
    - Each type's assets are made in the order they were loaded, passing
      over those still decoding. The types share the budget and take turns
      at going first, one call after the next, so a backlog of one type
      cannot hold another's back for long.
    - `finish_all_loads` waits for every job, then makes every asset, with
      no budget: for tools, tests, and frames that must show every asset
      loaded before them. It makes them type by type, in the order of each
      type's first load, and each type's in load order.
  - **The facade:** a window's frame calls `finish_completed_loads`.
    `Offscreen::frame` calls `finish_all_loads`, so a recording is the
    same on every run. A headless run still makes nothing. A load finishes
    in a frame nobody can know in advance, so ADR-0025's rule stands: tick
    systems stay off load state.
  - **Scopes:**
    - There are three: `Global`, `Level` and `Transient`. `load` loads in
      `Global`, and `load_in(path, scope)` in any of them.
    - An asset belongs to every scope it was loaded in. `unload_scope`
      removes the assets it leaves in none, and returns how many it
      removed. Their handles stop resolving at once, and loading the path
      again loads it anew.
    - The next finish frees each removed asset, and each failed asset's
      placeholder, with `Asset::release(self, ctx)` and that frame's
      context. By default `release` drops the asset; `Texture::release`
      destroys its GPU texture. An asset unloaded while it decodes is
      dropped when its result comes.
  - **Paths and sources:**
    - `AssetPath` is a normalized path: its ASCII letters are lowercased,
      then it is checked by M3.2's byte grammar. A path that fails is
      refused, not repaired, so a `\` is an error, not a separator.
    - `Source` is `read` and `modified`, which defaults to `None`.
    - `LooseSource` is in every build until M5.7 gives release builds a
      `PackSource`: the architecture's loose files in development only
      wait for that. `AssetServer::new(root, ..)` reads a `LooseSource`,
      and `with_source` reads any other.
  - **The 4 ms criterion:** the storm test (`crates/assets/tests/storm.rs`)
    holds every frame's finish under 4 ms in every build. It holds the
    whole first frame, with the game's 1,000 calls to `load`, under 4 ms
    only in an optimized build:
    `cargo test --release -p nadir-assets --test storm`. One run of three
    must keep to the limit, so that another process taking a core for a
    time slice does not fail it.
  - `Assets` stays `Sync`: it holds its result `Receiver` in a mutex that
    it reaches only through `&mut self`, so never locks.
- **Consequences:**
  - No file is read and nothing is decoded on the main thread, and making
    assets keeps to a budget each frame. Freeing an unloaded scope's
    assets does not: the next finish frees them all. Unloading a level's
    scope gives back its GPU memory.
  - In a window, an asset shows a frame or more after its load. M9.1's
    loading state hides a level's.
  - In a window, which frame makes which asset varies from run to run, and
    so do the order between types and the GPU ids; offscreen, none of them
    does.
  - Each server starts its own threads, so a test that makes a server
    starts up to four. A machine of 4 cores loads with 2 workers, not 3.
  - `has_pending` stays true while a job is out, even an unloaded asset's,
    so the facade makes a `FinalizeCtx` in those frames, which allocates
    nothing.
  - A dev build spends about 3 µs of main-thread time on each `load`.
  - `Clips` and `TileLayers` are not assets (ADR-0023, ADR-0024), so no
    scope removes them: M5.6's `load_level` must remove a level's own.
  - The API change: `finish_completed_loads` right after a `load` no longer
    makes the asset, and code that needs it made calls `finish_all_loads`.
    `LoadState::Loading` now also covers an asset being decoded. `Asset`
    gains `release`, with a default.
- **Alternatives considered:**
  - One scope per asset, the first it was loaded in or the longest-lived:
    unloading a level would free an asset the global scope still uses, or
    keep a level's asset for good.
  - Counting handles: the spec rejects it for hidden `Drop` costs.
  - One pool for the whole process: hidden global state, and a game has
    one server.
  - Making assets in load order across types too: a merge of every type's
    list at each step, for an order nothing needs; taking turns is enough
    to keep one type from holding the others back.
  - Retrying the storm alone, with `cores − 1` workers: beside a busy
    process it still failed 3 runs of 8, and a game's frames would hitch
    the same way while it loads.
  - A budget that also counts the time the game's `load` calls take: a
    shorter first frame, but it budgets time the server cannot stop.
  - Waiting for every load in a window's frames too: that is the stall
    again.
  - Holding the first frame to 4 ms in the dev profile too: its 1,000
    `load` calls alone take 2.5 to 7 ms there.
- **Revisit when:** M5.6's levels show assets appearing after their loading
  state ends; a texture's upload alone takes more than the 2 ms budget; a
  level takes more than a second to load because of the budget; audio
  (M6.6) or the re-cook (M5.8) needs workers of its own; the platform layer
  can lower a thread's priority, which could give back the spare core; or
  a game needs the dev build's `load` to cost less.

## ADR-0031 — The cook packs each folder's PNGs into an atlas of QOI pages and a KBJ of frames, trimmed, with tile sets as pages of their own; it caches by group, owns `atlas/`, and lists what it does not cook

- **Status:** Accepted; the cook's outputs below `atlas/` alone, a group of PNGs alone, and its report's counts, are superseded by ADR-0035
- **Date:** 2026-10-31
- **Context:**
  - M5.4 builds `nadir-cook` and `cargo xtask cook`: a library and a CLI,
    an incremental cache, and PNG → trim → MaxRects (padding and
    extrusion) → QOI. It must cook twice to the same bytes, cook again
    only the atlas group of a changed PNG, and reject paths that differ
    only in case.
  - `assets/` held only the sandbox's hand-made QOIs: `atlas.qoi`, whose
    regions are constants in `stress.rs`, `tiles.qoi`, a `TileSet`
    (ADR-0023: a whole texture cut into equal tiles, with no padding),
    and `person.qoi`, whose walk frames feed `ClipFrame`s, which have no
    size or pivot yet (ADR-0024). Nothing at run time reads a cooked
    atlas.
  - ADR-0027 had the cook warn for any chunk the PNG decoder ignores. Of
    the 4,033 PNGs on the development machine, 739 have only text chunks
    and 25 `sRGB` alone, and the cook takes every PNG as sRGB anyway.
  - `xtask` may depend only on `core` and `codec`, and `nadir-cook` on
    `core`, `codec`, `assets` and `scene`. CI runs the tests on Linux,
    Windows and macOS, and the last two do not tell names apart by case.
  - A code review (4 reviewers and a skeptic for each) found sprites in
    the order and case the files were found in, Greek's final sigma
    getting past the case rule, folders of outputs that differ in
    non-ASCII case, which on Windows and macOS made the cook remove a
    group's own files, and a prune quadratic in the free rectangles.
  - A critique of the design (3 critics and a skeptic for each) found,
    among others: an atlas with no page sizes, from which no `uv` can be
    computed; a tile set's page named by its position, which moves when
    other pages are added; cache paths trusted for deletes; group names
    not lowercased; and tests of case and of names not UTF-8 that cannot
    make their files on Windows and macOS.
- **Decision** (the user chose the scope, the trimming, the PNG warning,
  the files left alone and M5.9's place on 2026-10-31):
  - **Scope:** the cook alone. PNG sources of the sandbox's sprites,
    converted pixel for pixel from its QOIs, go into `assets/sprites/`,
    with sidecars. The sandbox keeps drawing its QOIs. A runtime atlas
    loader and the sandbox's switch to cooked sprites are a task of their
    own, M5.9, placed right after M5.4, and M5.5 depends on it.
  - **Groups:** an image's folder names its group. A folder's
    `atlas.jsonc`, `{ "group": "name" }`, puts its images (not its
    subfolders') in another; the name must be lowercase and make asset
    paths. Several folders may share a group. An image directly in the
    source folder is an error.
  - **Sidecars:** `<stem>.sprite.jsonc` beside `<stem>.png` gives
    `frames` (a sheet's cell size), `pivot`, `trim`, or `tiles` (a tile
    set). Sizes are whole numbers of 1 or more that divide the image's.
    Unknown keys and wrong types are errors.
  - **Trimming and packing:** every frame is trimmed to the box of its
    pixels whose alpha is above 0, unless its sidecar says `"trim":
    false`, and its offset and untrimmed size are recorded, for M5.5's
    `ClipFrame` size and pivot. Frames are sorted by height, width, name
    and index, and packed by MaxRects with the best short-side fit, never
    rotated, 2 px apart, with their edges extruded 1 px. Pages are squares
    of 64 to 4096: the smallest that holds all, or 4096 pages filled in
    turn, each shrunk to fit its frames. A frame whose width or height
    with its extrusion passes 4096 is an error. A tile set is a page of
    its own, as it is, at most 4096 × 4096, named after it.
  - **Outputs:** for group G, `atlas/G_<n>.qoi` (RGBA, sRGB) and
    `atlas/G.kbj`, version 1, which lists each page's path and size, and
    each sprite's frames (page, place, trimmed size, offset, untrimmed
    size) and pivot, or tile size for a tile set. A tile set's page is
    `atlas/<sprite>.tiles.qoi`. The pivot is in the untrimmed frame's
    pixels, from (0, 0) at its top-left corner to (sw, sh) at its
    bottom-right, and is the bottom centre when the sidecar gives none.
  - **PNG warning:** once per file that has a color chunk other than
    `sRGB`: `gAMA`, `cHRM`, `iCCP`, `cICP`, `mDCV` or `cLLI`. This
    changes ADR-0027's rule.
  - **Files no pipeline cooks:** listed once a run, with why, and left
    alone. So are, rather than failing the cook, the names among them
    that are not asset paths, differ from another only in case, are not
    UTF-8, or are symbolic links. Names starting with `.` are left out.
  - **Names:** a file the cook reads must have an asset path and a name
    that is UTF-8, and not be a symbolic link. No other name in a folder
    above or beside it may equal its own once folded: each character
    uppercased, then lowercased, so that `Σ`, `σ` and `ς` are one, as
    Windows and macOS see them. Two groups' atlas files or tile sets'
    pages, or the folders they are in, that fold alike are an error. The
    rules run on the list the walk makes, so that they are tested on
    lists made by hand on every system.
  - **Order:** a group's sprites, its tile sets' pages, and the packing's
    ties go by sprite name, never by the order or the case the files were
    found in, so that a cook after a rename of case alone, which the cache
    does not see, writes what a clean one does.
  - **Cache:** `.cache.kbj`, version 1: the cooker (the cook's version and
    a hash of its executable), an options string (empty: no option
    changes output yet), and per job (`atlas/G`) the FNV-1a hashes of the
    files it read and wrote, in path order. A job is skipped when its
    files, the cooker and the options match and its outputs are there as
    written; `--force` skips none. Each file is read once, so a hash and
    what is cooked come from the same bytes. A cache that names a path
    the cook would not write is not used.
  - **Output:** each file is written through a `.tmp` and a rename, and
    the cache last. The cook owns `OUT/atlas/`: after a cook, every file
    there that no job wrote is removed. The output and source folders may
    not hold each other.
  - **CLI:** `nadir-cook [--force] SOURCE OUT`, exit 1 for a failed cook
    and 2 for a command line not understood. `cargo xtask cook [--force]`
    runs it, built in release mode, on `assets/` and `target/cooked/`.
- **Consequences:**
  - A cooked atlas holds what a loader needs: page sizes for `uv`s,
    offsets and untrimmed sizes for trimmed frames, and tile sizes. M5.9
    reads it.
  - `assets/sprites/` holds each sprite twice, as the QOI the sandbox
    draws and the PNG the cook packs, until M5.9 retires the QOIs.
  - Until M5.5 gives `ClipFrame` a size and pivot, a trimmed walk frame
    drawn as it is would be stretched: the person's frames trim to 10 and
    12 pixels wide.
  - A rebuilt cook cooks everything again once.
  - Names that differ only in non-ASCII case, or in Unicode
    normalization alone (é composed or not), cannot be tested on disk on
    every system; the first is checked, the second is not. The fold can
    join names that Windows and macOS keep apart (`ß` and `ss`), which
    errs on the safe side.
  - No test makes a name that is not UTF-8: that needs platform `cfg`,
    which `tools/cook` may not have.
- **Alternatives considered:**
  - Moving the sandbox to cooked sprites now: a runtime atlas asset, and
    the sandbox, its tests, the demo page and the download moving to
    `target/cooked`, over about five crates.
  - Keeping sheet frames whole: a clip's frames would share one size, but
    sheets and loose images would differ.
  - Warning for every ignored chunk, or for `sRGB` too: 924 or 185 of the
    4,033 PNGs would warn.
  - Failing for files no pipeline cooks: the sandbox's QOIs would have had
    to leave `assets/`, breaking it until M5.9.
  - A cache entry per source file, as the spec first had it: a changed
    image re-packs its whole group anyway, so the group is the unit.
  - Removing only what the old cache listed: a damaged or forced cache
    would leave files behind.
  - Tile sets as numbered pages after the packed ones: a level naming one
    would point at another page once the group grew.
  - Pages of any size, not only squares: less memory for some groups, a
    search the critique did not ask for.
- **Revisit when:** a group needs more than a few 4096 pages, or squares
  waste enough memory to matter; a sheet's frames must stay one size,
  which M5.5's clip import may show; an artist's tool writes PNGs whose
  colors are not sRGB without a chunk saying so; a project needs links in
  `assets/`; or M5.7's pack needs an option that changes output, which the
  cache's options string must then hold.

## ADR-0032 — CodeQL scans the Rust code and the workflows; Dependabot keeps the actions' pins current; `cargo xtask workflows` enforces the CI supply-chain rules

- **Status:** Accepted; ADR-0036 adds `release.yml`'s row, whose `release` job may write `contents` and `image` job `packages`
- **Date:** 2026-10-31
- **Context:**
  - `docs/04-quality.md` says every action is pinned by its full commit
    SHA with its tag in a comment, that workflows use only
    `actions/checkout`, `actions/cache` and the artifact actions (plus the
    Pages actions in the demo page's workflow, ADR-0029), and that tokens
    get `contents: read` unless a job needs more. Nothing checked this.
  - The user added `.github/workflows/codeql.yml` from GitHub's template.
    It pinned `actions/checkout@v7` and `github/codeql-action/*@v4` by tag,
    had no top-level `permissions`, left the checkout's token in
    `.git/config`, and used an action the rules did not allow.
  - The user also added `.github/dependabot.yml` from GitHub's starter
    file, with an empty `package-ecosystem`, so it did nothing.
  - On 2026-10-31 the user asked to fix Dependabot's file, to pin
    CodeQL's actions by SHA and allow CodeQL, and to update the workflows
    for proper security vetting and CI.
- **Decision:**
  - **CodeQL is allowed,** in `codeql.yml` only: `github/codeql-action/init`
    and `analyze`, pinned by SHA (v4.39.1). It scans `rust` and `actions`
    (the workflows themselves) with no build, with the `security-extended`
    queries, on pushes to `main`, on pull requests, weekly and by hand. Its
    token has no default permissions; the job has `contents: read` and
    `security-events: write`, to upload the results.
  - **Dependabot watches `github-actions` only,** weekly, for releases at
    least 7 days old (`cooldown`), with the week's updates in one pull
    request whose commits start `ci:`. It updates the SHA and the tag
    comment on the same line. There is no `cargo` entry: there are no
    crates to update, and `cargo xtask deps` fails on one.
  - **`cargo xtask workflows`,** part of `cargo xtask ci`, reads each file
    of `.github/workflows` line by line and fails:
    - a workflow with no row in its table of the actions each workflow may
      use and the jobs that may grant `write`, with the permissions each
      may (`codeql.yml`'s `analyze`: `security-events`; `pages.yml`'s
      `deploy`: `pages` and `id-token`; no other job);
    - an action not in its row, or not pinned by 40 hex digits, or with no
      `# v…` comment on its line;
    - top-level permissions other than `{}` or `contents: read`, or none;
    - a job's `write` on any permission its row does not name, including
      one GitHub adds later, or a job's permissions written on one line
      (`write-all`, `read-all`, `{ … }`), which it cannot read;
    - `pull_request_target`, which runs with the base repository's token;
    - an `actions/checkout` without `persist-credentials: false`.
  - **A newer push to a pull request cancels its older run,** in CI and
    CodeQL. Any other run is a concurrency group of its own: a group lets
    one run wait, so a third push would cancel the second, and every
    commit of `main` needs its CI verdict for the demo page's workflow.
- **Consequences:**
  - Good:
    - The rules in `docs/04-quality.md` are checked on every push, on all
      three systems, as `deps` and `layers` check the dependency rules.
      Against the template's `codeql.yml`, the check reports all five of
      its problems.
    - Code scanning covers the Rust code and the workflows, including
      their injection and unpinned-action queries.
    - Pins move forward by reviewed pull requests, a week after a release.
  - Bad:
    - A line-based reading misses a `uses:` or permission written in flow
      style (`{ uses: … }`) or in a block scalar. The workflows are written
      one key a line.
    - `security-extended` raises more alerts to triage than the default
      suite.
    - CodeQL's action is one more action with write access, though only
      to code scanning.
    - A new workflow, such as the release one (`docs/05-release.md`), must
      add its row to the table first.
- **Alternatives considered:**
  - zizmor or actionlint in CI: tools from outside the workspace,
    downloaded on every run; CodeQL's `actions` queries cover workflow
    injection and unpinned actions, and the xtask check covers the
    project's own rules.
  - OpenSSF Scorecard or `actions/dependency-review-action`: more actions
    with tokens, for a workspace with no third-party crates, whose
    dependency rule `cargo xtask deps` already enforces.
  - `cargo audit` or `cargo deny`: there are no crates to audit.
  - Leave CodeQL's actions pinned by tag: a moved tag would run new code
    with `security-events: write`, unreviewed.
  - Parse the workflows as YAML: needs a YAML parser of our own for a
    check of a few line shapes.
- **Revisit when:** a workflow needs a flow-style `uses:` or a reusable
  workflow from outside the repository; the release workflow lands; CodeQL's
  alerts outgrow triage; or GitHub makes immutable actions the default,
  which would make a tag as safe as a SHA.

## ADR-0033 — The game reads the cooked atlas through a strict reader in codec and an `Atlas` asset in render; `load_now` makes it before the first tick; `cargo xtask ci` cooks before its tests

- **Status:** Accepted; atlas version 1, the person's untrimmed sheet and the sandbox's refusal of trimmed person frames, and `Sprite::from_atlas`'s limits on flipping and animating, are superseded by ADR-0035; ADR-0037 adds `LICENSE` and `NOTICE` to its download; that its download holds the loose `assets/atlas` is superseded by ADR-0041: it holds the pack, `assets/game.kpak`, alone
- **Date:** 2026-10-31
- **Context:**
  - Roadmap M5.9: the game draws cooked atlases. The cook writes
    `atlas/<group>.kbj`, version 1 (ADR-0031), and the sandbox drew
    hand-made QOIs, with an atlas's regions written as constants in
    `stress.rs`.
  - The sandbox builds its world, sprites, clips and tile sets, before its
    first tick, and needs the regions to do it. The asset server loads
    only on its workers, and finalizes in later frames (ADR-0030).
  - The rules for parsers (`.claude/rules/parsers.md`) give every reader of
    files a fuzz target, and the fuzzer, in `xtask`, may reach only `core`
    and `codec`.
  - The sandbox may depend on `nadir` alone, tests included, so it cannot
    cook for its own tests.
  - ADR-0029's download packed `assets/`, and the sandbox fell back to the
    workspace's `assets`: both change here.
  - On 2026-10-31 the user chose: `AssetServer::load_now`; a `cook` check
    in `cargo xtask ci` before `test`; the grass on the same page as the
    rest, so that the stress scene is one batch; and the task in one
    change across seven crates.
- **Decision:**
  - **`nadir_codec::atlas::parse`** reads the file into `AtlasFile`,
    `Page`, `SpriteEntry` (`SpriteKind::Pivot` or `Tiles`) and `Frame`,
    `#[non_exhaustive]`. It reads the version before anything else (any
    other than 1 is not supported), refuses unknown members, and checks
    every frame on a page and inside its cell, the names in byte order,
    and a tile set one whole page. Its errors are
    `CodecError::Atlas { at, kind }`, `at` a member path such as
    `sprites[2].frames[0].w`: a KBJ value has no byte offset. The fuzz
    target `atlas` asserts what the reader promises of what it accepts.
  - **The cook reads what it writes** with that reader, and fails the
    group if the reader refuses it, and its version is the reader's. The
    reader does not check that page paths are asset paths, which the game
    also asks: an output path past an asset path's 1,024 bytes, from a
    source path near that length, would still cook.
  - **`nadir_render::Atlas`** is the asset: pages (paths that must be
    lowercase asset paths ending `.qoi`, and sizes), and sprites by name
    with each frame's uv, trimmed size, offset and untrimmed size, and a
    sprite's pivot or a tile set's tile size. It needs nothing to
    finalize, and has no placeholder. `Atlas::load_pages(server, scope)`
    loads its pages as `Texture`s in a scope, by page index.
  - **`AssetServer::load_now(_in)`** makes one asset before it returns:
    read and decoded on the calling thread if it is new, waited for if a
    worker has it, made now if it waits to be. It makes and frees nothing
    else, and spends no budget. A failure, a context without what
    `finalize` needs included, is final, as any failure is, and returns
    the error.
  - **`Sprite::from_atlas(texture, sprite, frame)`** makes a sprite of a
    frame at its trimmed size, with the sprite's pivot normalized in the
    trimmed quad, so that its pixels land where the untrimmed cell's
    would. Flipped, its frame must be untrimmed or trimmed evenly left
    and right, since render's flip mirrors about the quad's middle;
    animated, every frame must have its trimmed size and offset, as an
    untrimmed sheet's do, since `nadir::animate` writes only the uv
    (M5.5).
  - **The sandbox** reads `atlas/sprites.kbj` with `load_now` from
    `assets` beside its executable, else the workspace's `target/cooked`,
    and refuses, saying to run `cargo xtask cook`, an atlas that is
    missing or is not `assets/` as it is now cooked: a sprite it draws
    missing, the person's frames trimmed, a tile set other than 8 × 4.
    The person's sheet is cooked untrimmed (`"trim": false`) until M5.5
    gives clip frames a size; the trees and rocks are trimmed and placed
    by their offsets. The hand-made QOIs and `stress.rs`'s regions are
    gone.
  - **`cargo xtask ci` cooks before `test`** (the check `cook`, as
    `cargo xtask cook` runs it), on all three systems. `cargo xtask site`
    cooks first, and its download holds `target/cooked/atlas` and nothing
    else of that folder, as `assets/atlas` beside the sandbox.
- **Consequences:**
  - Good:
    - The three captures are the same, frame for frame: 1,800 frames
      byte for byte as before, on llvmpipe, with the same world hashes.
      The field's mean batches go from 17.7 to 9.4 (its trees, rocks and
      person share a page) and the stress scene's from 2 to 1 (the grass
      joins it).
    - The cook cannot write an atlas file the codec reader refuses.
    - A stale cook says so, rather than drawing stretched frames.
  - Bad:
    - `cargo test -p sandbox`, `cargo run -p sandbox` and `cargo dev` need
      `cargo xtask cook` first. `cargo xtask ci` does it, and adds a release
      build of the cook to each CI job, not yet timed on Windows and macOS.
    - Sprites on one page share a texture, so at the same feet y they are
      drawn in the order they are extracted, the props before the person,
      where the person's texture handle had put them first. No captured
      frame has such a tie; in play, a person stopped beside a trunk at
      exactly its feet's y is drawn in front of it rather than behind.
      Spawning the person first would keep the old order, and change every
      world hash.
    - `Sprite::from_atlas` places a frame up to the `f32` rounding of its
      normalized pivot, exact for the sandbox's frames, not for every size.
    - The sprite bench keeps the grass a texture of its own, 2 batches, so
      that it is the frame its recorded times were measured on.
- **Alternatives considered:**
  - Sprites and clips naming atlas entries, resolved by a facade system once
    the atlas loads: no blocking, but every sprite, clip and tile set waits
    on a lookup, and a tick before the atlas loads has nothing to draw.
  - The game calling `finish_all_loads` with an empty context before any
    texture loads: no new API, but any asset loaded earlier that needs the
    GPU fails.
  - The reader in render, with no fuzz target: one crate fewer, against
    the rule for parsers.
  - A separate CI step that cooks before `cargo xtask ci`: the local gate
    would need it run first.
  - The grass in a folder of its own, keeping the stress scene's 2 batches.
- **Revisit when:** M5.5 gives clip frames a size and pivot, and flips
  about the pivot (then turn the person's trim back on); a game needs an
  asset with a GPU resource before its window opens; M5.7's pack replaces
  `target/cooked` for release builds; or a second atlas version is
  written.

## ADR-0034 — The storm test runs the storm up to five times, 2 s apart; one run must keep to the limit

- **Status:** Accepted
- **Date:** 2026-11-01
- **Context:**
  - ADR-0030 holds every frame's finish in the storm test
    (`crates/assets/tests/storm.rs`) under 4 ms. One run of three must keep
    to the limit, so that another process taking a core for a time slice
    does not fail it. The three runs came back to back.
  - CI run 41286083390, for `ad30aee` (M5.9), failed on `macos-latest`
    alone: all three runs went over, their slowest finishes 4.5, 8.5 and
    24.6 ms, and the test took 6.3 s. The same commit passed on Linux and
    Windows. In the five runs before, the test passed on all three systems
    in 1.7 to 2.2 s, about one run's time.
  - M5.9 changed nothing a finish does: not `finish`, `Budget`, the 2 ms
    budget or the worker count. About 6 to 8 images a frame reached the
    main thread on average (1,000 over 127 to 165 frames), far under the
    budget, so a 24.6 ms finish is time the main thread did not run.
  - A shared CI machine can stay busy for seconds, and three runs back to
    back, about 3 s, fit inside one such spell.
- **Decision:**
  - The storm test runs the storm up to 5 times (`ATTEMPTS`), and waits
    2 s (`PAUSE`) before each run after the first. One run must keep to the
    limit, as before.
  - What a run must keep to is unchanged: every finish under 4 ms, and in
    an optimized build the whole first frame too.
  - A run that keeps to it ends the test, so a first run that does adds no
    time.
- **Consequences:**
  - Good:
    - To fail the test, a busy spell must now last across five runs over
      about 13 s, not three runs over about 3 s.
    - A server whose own work goes over the limit still fails every run:
      its work does not change from one run to the next.
  - Bad:
    - The test still times the wall clock: a longer spell fails it, more
      rarely.
    - A failing test takes about 8 s longer.
    - An overrun that happens only some of the time is a little more likely
      to pass, one run of five rather than one of three.
- **Alternatives considered:**
  - Re-running the CI job by hand: no change, and the next busy spell fails
    CI again.
  - More runs back to back, with no pause: they fit inside the same spell.
  - Moving the 4 ms check to the benchmarks, nightly or by hand on fixed
    hardware, and keeping a storm in `cargo test` that checks only that the
    1,000 load: no more such failures, but M5.3's criterion would no longer
    be checked by `cargo test`.
  - Timing the main thread's CPU time rather than the wall clock: a time
    slice taken by another process would not count, but it needs new
    bindings in `nadir-sys` and `nadir-platform`, and assets may not depend
    on platform, even for tests (M2.3).
  - Letting a few frames go over the limit: changes what M5.3's criterion
    says.
- **Revisit when:** the storm test fails in CI with all five runs over the
  limit (then move the check to the benchmarks, or time CPU), or CI moves
  to machines with cores of their own.

## ADR-0035 — Aseprite exports join their folder's atlas, their tags become clips in the atlas file (version 2), and clip frames carry their own size and pivot; flips mirror about the pivot; the cook also writes fonts' glyph files and data as KBJ

- **Status:** Accepted
- **Date:** 2026-11-01
- **Context:**
  - Roadmap M5.5: cook Aseprite animations, JSONC data and BMFont fonts,
    and draw the sandbox's animations from Aseprite exports. Its notes:
    a `ClipFrame` has no size or pivot, so a trimmed frame is stretched
    to its sprite's quad (ADR-0024), and the person's sheet was cooked
    whole (`"trim": false`, ADR-0033); render mirrors a sprite about its
    quad's centre, wrong for a frame trimmed unevenly; a clip never
    becomes an asset (ADR-0024); and `bmfont::parse` leaves the pages,
    their size, their alpha and the fallback to the cook (M5.2).
  - The spec planned clips in files of their own, `anim/<name>.kbj`,
    with each frame's page path and region. A clip's frame needs the
    atlas frame's size, offset and pivot as well, which the atlas file
    holds.
  - Aseprite's exporter (`src/app/doc_exporter.cpp`) writes `repeat` as a
    string, only when it is more than 0, and `meta.image` as a file name;
    its player (`src/doc/playback.cpp`) counts each one-way pass of a
    ping-pong tag as a repeat, shows the turning frames once, and shows a
    one-frame tag again on each repeat. Aseprite is not installed here.
  - The user decided four points on 2026-11-01: one task across six crates
    (codec, render, the facade, the cook, the sandbox and xtask); clips in
    the atlas file, version 2; render's flip mirrors about the pivot; and
    the sandbox's export written by hand in Aseprite's format. A design
    critique (33 points, one refuted) led to the rest.
- **Decision:**
  - **Aseprite exports:** every `.json` is read as one, by
    `nadir_codec::aseprite::parse`, a cook-only reader in codec with its
    fuzz target. It takes `--format json-array` data (frames, `meta.image`,
    `meta.size`, `meta.frameTags`), ignores the members it does not use,
    and refuses what would cook wrongly: json-hash, a rotated frame,
    `--inner-padding`, frames of several canvases, durations and repeats
    past Aseprite's 16 bits. Its errors are member paths
    (`CodecError::Aseprite`).
    - An export joins its folder's atlas group as the sprite named by its
      path without `.json`. Its sheet is `meta.image`, matched as an asset
      path beside it, which is then not a sprite of its own. Each frame is
      placed in its canvas by `spriteSourceSize`, then trimmed as any
      frame. A sidecar of its name gives `pivot` and `trim` only.
    - Each tag is a clip of the frames Aseprite plays: with no `repeat` a
      loop (forward, reverse, a ping-pong cycle without either end
      twice), with `repeat` N that many passes, then stopped. Each step
      lasts its frame's duration. The clips of an export have at most
      `max_entries` frames together: each tag's clip is counted, with
      those before it, before it is made.
  - **Atlas file version 2:** a sprite with a pivot may have `clips`, by
    name: a name with no `/`, `loop`, and `[frame, ms]` steps of its own
    frames. The reader refuses version 1: the cook writes it again.
    Render's `AtlasSprite::clips` and `clip(name)` give them.
  - **Clip frames:** `ClipFrame` gains `placement: Option<Placement>`, a
    size and a normalized pivot, which `nadir::animate` writes into the
    Sprite with the texture and uv; `None` leaves the Sprite's as they
    are. `ClipFrame::from_atlas` places a frame as `Sprite::from_atlas`
    does. `Clip::try_new` returns a `ClipError` where `Clip::new` panics.
    `Clips` has names: `add_named`, `find`, `name`, and
    `add_atlas(&Atlas, pages)`, which adds every sprite's clips as
    `<sprite>/<clip>` (`sprites/person/walk`), all or none, and returns
    their handles.
  - **Flips:** render's `SpriteDraw::flip` mirrors a sprite about its
    pivot: `1 - pivot.x` for `flip.x`, `1 - pivot.y` for `flip.y`. A
    sprite with its pivot at the middle of its width mirrors in place left
    to right, as before; one with its pivot at its feet hangs below them
    mirrored top to bottom. The snapped corner's offset is rounded to
    1/256 of a world unit, so that a normalized pivot's `f32` rounding
    (7/13 × 13 is 7.0000005) never moves a corner on a half-pixel tie to
    the pixel below.
  - **Fonts:** every `.fnt` (BMFont text) joins its folder's atlas group.
    Its pages, matched as asset paths beside it, must be PNGs of the
    font's page size, with the glyphs in their alpha (`alphaChnl` 0 or 2,
    or none given) and some pixel clear. Each glyph with a pixel above
    alpha 0 is a trimmed frame of the sprite named by the font's path
    without `.fnt`, by id, then BMFont's fallback glyph; the sprite's
    pivot is (0, 0). The group's job writes `atlas/<font>.font.kbj`
    (version 1): the face, size, line height and base, the atlas file and
    sprite, the glyphs by id (a frame if it has pixels, offsets and
    advance), the kerning pairs sorted, and the glyph a missing character
    draws: BMFont's fallback, else `?`, which must have pixels. Its reader
    comes with M7.1.
  - **Data:** each `.jsonc` below the source's `data/` is a job of its
    own, written as KBJ to `data/<path>.kbj`. The cook owns `data/` as it
    owns `atlas/`.
  - **The cook:** an atlas job's inputs are every file it read: images,
    sheets, pages, exports, fonts, sidecars and folders' settings.
    `JobReport::cooked_counts` became `counts: Option<Counts>`
    (`Atlas { sprites, pages, clips, fonts }` or `Data`), and `CookError`
    gained `Claimed` and `SpriteNameTaken`. A file name used twice among
    the outputs is refused, as one that folds alike was.
  - **The sandbox:** `assets/sprites/person.json` is an export of
    `person.png` in Aseprite 1.3's json-array format, written by hand, as
    `aseprite -b person.aseprite --sheet person.png --data person.json
    --format json-array --list-tags` would write it (its `meta.version` a
    placeholder), with tags `stand` (frame 0) and `walk` (1 to 4), 100 ms
    a frame. The person is trimmed again. The sandbox takes its gait from
    `Clips::add_atlas`, and the crowds' stagger from the walk's length.
- **Consequences:**
  - Good:
    - The game loads one file for a group's frames and clips, and one
      strict reader checks that every step is a frame of its sprite.
      M5.8's reload of `atlas/<group>.kbj` reaches both.
    - Trimmed frames animate and flip as their untrimmed cells would. The
      person's frames are 10 and 12 wide again, as M5.4 cooked them.
    - Fonts and data cook now, deterministically, cached like atlases.
  - Bad:
    - Version 1 atlas files are refused: an older cook must run again
      (`cargo xtask ci` cooks before its tests).
    - A flip with a pivot off the middle draws elsewhere than before, and
      `flip.y` with a feet pivot hangs a sprite below its feet. No sprite
      the engine or the sandbox draws did either.
    - No file Aseprite itself wrote is cooked: the sandbox's export and
      the codec's vectors are written by hand from its source. Exports
      with `--split-layers`, `--split-tags`, `--ignore-empty`, `--tag` or
      `--frame-range` number their frames apart from their tags, which
      the reader cannot always see.
    - Every `.json` is read as an Aseprite export: another tool's JSON in
      the source folder fails the cook. Data must be JSONC below `data/`.
    - The font file has no reader until M7.1, so the cook cannot check it
      as it checks the atlas file.
    - The acceptance covers sprite clips; the field's animated tiles stay
      in `map.rs` until a level supplies them (M5.6).
- **Alternatives considered:**
  - Clips in `anim/<name>.kbj`, as the spec had it: a second format, a
    reader and an asset type, with the atlas frames' data repeated, and
    two files for M5.8 to reload together.
  - Mirroring about the pivot in the facade's `Sprite` only: render's
    `SpriteDraw::flip` would mean another thing.
  - Rounding the snapped offset to whole pixels: an odd cell's default
    pivot is a half pixel, which would move.
  - Every `.jsonc` as data: a misspelt sidecar would cook silently.
  - Fonts' files in `fonts/`: a second owned folder, and
    `fonts/fonts/pixel.kbj` for `assets/fonts/pixel.fnt`.
- **Revisit when:** a real Aseprite export of the person (or another
  sprite) is added, which then replaces the hand-written one; a game
  needs another tool's JSON among its sources; M7.1 reads the font file
  (then the cook checks its fonts with that reader); or a clip needs
  frames from two sprites.

## ADR-0036 — Releases from a tag: six targets on GitHub Releases, a signed apt repository beside them, the cook's image on ghcr.io, and the crates on crates.io with the facade as NadirGE

- **Status:** Accepted; ADR-0037 adds `NOTICE` beside `LICENSE` in its archives and image, beside the `copyright` file in its `.deb`s, and both to its crates; the facade's package name NadirGE is superseded by ADR-0039 (`nadir_engine`); that its archives and the sandbox's `.deb` hold the cooked atlases, and that dist checks them byte for byte, is superseded by ADR-0041: they hold the pack, `assets/game.kpak`, alone, which dist checks against the laid-out cook's; nadir-audio goes to crates.io from the release after 0.3.0, before the facade uses it (ADR-0048, ADR-0050), so 14 crates, and the token stays until it exists there
- **Date:** 2026-11-01
- **Context:**
  - The user asked for a release pipeline that a pushed version tag
    starts:
    - Linux, Windows and macOS programs for both x86_64 and arm64;
    - a container image tagged with the version;
    - a Debian apt release;
    - the crates on crates.io, under the name NadirGE if `nadir` is taken;
    - and crates.io metadata in the root manifest's `[workspace.package]`.
  - The engine is a library. Its only programs are `nadir-cook` and the
    sandbox, and only the cook runs without a window.
  - On 2026-11-01:
    - `nadir` on crates.io was a static site builder (0.1.6);
    - `nadirge` and every `nadir-*` name the workspace uses were free.
  - ADR-0001 supports Windows and Linux on x64 only, and the roadmap's
    backlog left aarch64 Windows and Linux to "store or platform demand".
    Every crate passed `cargo clippy --all-targets --all-features -D
    warnings` for `aarch64-unknown-linux-gnu`, `aarch64-pc-windows-msvc`,
    `x86_64-pc-windows-msvc` and both macOS targets.
  - The user's choices (asked 2026-11-01):
    - a release carries the cook and the sandbox, and the image only the
      cook. The sandbox draws only on Linux so far: with no OpenGL context
      on Windows until M2.6, nor on macOS until M2.7, it runs there in an
      empty window (ADR-0019). The user chose to ship it on all six
      targets anyway;
    - the image is tagged with the version and `latest`;
    - the apt repository is signed and lives on GitHub Releases;
    - crates.io takes a token from a CI secret.
  - M11.2 (signing, notarization and the game's packages) is still ahead.
- **Decision:**
  - **Targets:** aarch64 Linux (gnu) and Windows (MSVC) join ADR-0001's
    targets. A release builds all six:
    `{x86_64,aarch64}-{unknown-linux-gnu,pc-windows-msvc,apple-darwin}`.
    Each builds on a hosted runner of its own system and processor, so
    its programs run there before release: `ubuntu-24.04`,
    `ubuntu-24.04-arm`, `windows-2025`, `windows-11-arm`,
    `macos-15-intel` and `macos-15`. macOS ships one archive per
    processor; the universal binary stays with the game's packages
    (M11.1).
  - **`cargo xtask dist [--out DIR]`** builds the release files for the
    machine it runs on:
    - it cooks `assets/`, and builds `nadir-cook` and the sandbox in
      release, `--locked`;
    - it lays out `nadir-<version>-<target>/`: the two executables, the
      sandbox's cooked atlases in `assets/` beside it, `README.txt` and
      `LICENSE`. On Windows and macOS the README says the sandbox shows
      an empty window until M2.6 and M2.7;
    - it runs them:
      - `nadir-cook --version` must print the version;
      - the laid-out cook must cook `assets/` into the atlases
        `target/cooked` has, byte for byte;
      - the sandbox must refuse the scene `none`, which it does before
        it opens a window;
    - it packs a `.zip` on Windows, with the system's `tar.exe`, and a
      `.tar.gz` elsewhere.
    - On Linux it also makes two Debian packages with `dpkg-deb`:
      - `nadir-cook`, as `/usr/bin/nadir-cook`;
      - `nadir-sandbox`, in `/usr/lib/games/nadir-sandbox` with its
        atlases, and linked from `/usr/games/nadir-sandbox`.

      A package's file is named for the release's version
      (`nadir-cook_0.2.0-rc.1_arm64.deb`), while its control says Debian's
      (`0.2.0~rc.1`): GitHub turns `~` into `.` in an asset's name.
      `dpkg-shlibdeps` gives each package's dependencies on the
      libraries it links (`libc6 (>= 2.34), libgcc-s1 (>= 4.2)` from an
      `ubuntu-24.04` build). The sandbox also depends on the libraries it
      loads at run time: `libx11-6`, `libegl1` and `libopengl0`.

    It signs nothing and uploads nothing.
  - **Windows** executables link the C runtime statically
    (`target-feature=+crt-static` in `.cargo/config.toml`, for every
    MSVC build). They then need no Visual C++ redistributable, and CI
    tests what ships.
  - **`nadir-cook --version`** prints `nadir-cook <version>`.
  - **`.github/workflows/release.yml`**, on a pushed tag `v*`. Its jobs:
    - `gate` stops unless:
      - the tag is `v` plus `[workspace.package]`'s version;
      - its commit is on `main`;
      - and CI has a successful push run of that commit.

      A version with a `-` is a pre-release.
    - `build` runs one runner per target:
      - `cargo xtask dist`;
      - on Linux, the archived sandbox's `--capture` with llvmpipe;
      - and, on the two Linux runners, a cook linked statically against
        musl for the image.
    - `crates` (environment `crates-io`), once every build passed, runs
      `cargo publish --workspace --locked`, which packages and builds
      every crate from its package before it uploads any, then publishes
      them in the order of their dependencies, with the
      `CARGO_REGISTRY_TOKEN` secret. It runs before anything else is
      published: crates.io refuses what no local check sees, and a
      refusal then leaves nothing out. cargo refuses to start if
      crates.io has any of them at the version already, so each attempt
      leaves out those it has. crates.io takes 5 new crates at once and
      then one each 10 minutes, so an attempt it stops with `status 429`
      waits 10 minutes and tries again, up to 15 times; the first
      release, of 13 new crates, takes about 80 minutes.
    - `release` (environment `release`, `contents: write`), once the
      crates are out:
      - makes the flat apt repository from the two Linux targets' `.deb`s with
        `.github/release/apt-repo.sh`: `Packages`, `Packages.gz`,
        `Release`, and `InRelease` and `Release.gpg` signed with the
        `APT_SIGNING_KEY` secret, plus the public key as
        `nadir-archive-keyring.gpg`;
      - sums every file into `SHA256SUMS`;
      - creates the GitHub release as a draft with all of them, then
        publishes it.

      A pre-release is not marked latest. A draft from a failed attempt
      is filled again; a published release is never changed.
    - `image` (`packages: write`), once the crates are out, logs in to
      ghcr.io with the job's token
      and builds `ghcr.io/jabbott-iii/nadir-cook` for `linux/amd64` and
      `linux/arm64` with `docker buildx`, from `tools/cook/Dockerfile`:
      `FROM scratch`, the static cook, `LICENSE`, nothing run. The
      daemon's own builder makes one platform at a time on these runners,
      so the job makes a `docker-container` builder (BuildKit's
      `moby/buildkit` image). It pushes with no provenance attestation, so
      the index lists the two platforms alone, and tags the version and,
      unless it is a pre-release, `latest`. It then runs the pushed amd64
      image's `--version`.

    Each build checks that its runner's host is its target, and runs
    `cargo --locked xtask dist`: a Cargo.lock that a version's raise
    left behind fails there. A required reviewer on `crates-io` holds the
    whole release. The gate is a check of the tagged commit's own
    workflow, not an access control: a `v*` tag ruleset limits who can
    tag. The workflow may use `actions/checkout` and `upload-artifact` /
    `download-artifact` only. Only `release` may write contents, and only
    `image` packages; `cargo xtask workflows` checks both.
  - **apt** reads the newest release:

    ```
    deb [signed-by=/usr/share/keyrings/nadir-archive-keyring.gpg] https://github.com/jabbott-iii/Nadir/releases/latest/download ./
    ```

    apt asks for `…/latest/download/./InRelease`. On 2026-11-01 GitHub
    redirected such a path to the newest release's asset, as it does
    without the `./`. Locally, apt 2.8.3 verified an ed25519-signed
    `InRelease` made by the script, fetched both packages, and refused a
    tampered `InRelease` and `Packages.gz`.
  - **crates.io** gets 13 crates:
    - the facade, as the package **NadirGE**, whose library keeps the
      name `nadir` (`[lib] name`), so code still writes `nadir::`;
    - `nadir-cook`;
    - and the eleven `nadir-*` crates the facade needs.

    `publish = false` keeps the sandbox, `xtask`, `nadir-bench`,
    `nadir-audio` and `nadir-debug` off it; the last two have no code yet.
    `[workspace.package]` holds the shared metadata, which every crate
    for crates.io inherits, with a description of its own. The user's
    `name = "NadirGE"` there, which cargo ignores in that table, moved to
    `crates/engine/Cargo.toml`. `[workspace.dependencies]` gives each
    workspace crate its path and the release's version.
  - **`cargo xtask crates`,** in `cargo xtask ci` after `layers`, fails
    when a crate that crates.io gets:
    - lacks a description, license or repository;
    - is at another version than NadirGE;
    - or depends on a workspace crate that crates.io does not get, or by a
      requirement other than `^<version>`.
  - **Docs:** the dependency matrix and `cargo test -p NadirGE` name the
    package. `docs/05-release.md` gains "Engine releases": the secrets,
    the one-time settings and the steps of a release.
- **Consequences:**
  - Good:
    - A tag publishes everything at one version, from a commit CI
      passed, with each program run on its own system and processor.
    - The apt repository needs no server: the release's assets are the
      repository, signed by a key only the `release` environment holds.
    - The image is the static cook alone: no base image to keep up to
      date, and nothing to emulate when it builds.
    - A crate of one release never builds against an older release's
      crates. A manifest without what crates.io requires fails CI
      (`cargo xtask crates`); one that will not build from its package
      fails the `crates` job before any crate is uploaded; and anything
      else crates.io refuses stops the release before the GitHub release
      and the image go out.
  - Bad:
    - Nothing is signed or notarized until M11.2. macOS quarantines a
      downloaded archive (its README says how to clear that), and
      SmartScreen warns on Windows.
    - The Windows and macOS archives carry a sandbox that shows an empty
      window until M2.6 and M2.7.
    - The first release spends about 80 minutes on crates.io's limit for
      new crates, unless crates.io raises it for the account.
    - The Linux programs need glibc 2.34 or newer (the `ubuntu-24.04`
      build's symbols), not `docs/03-platforms.md`'s 2.31 floor, which
      stays the game packages' (M11.1).
    - apt sees only the newest release, and an `apt update` while a
      release publishes can fetch an `InRelease` and a `Packages.gz` of
      two releases, failing until it runs again.
    - `cargo xtask ci` runs on x86_64 Linux and Windows and arm64 macOS
      only. The arm64 Linux and Windows targets, and Intel macOS, are
      built and run only by a release (and passed clippy here, once).
    - The packages fail `lintian` on a missing `changelog.gz` and on
      unstripped executables (kept, for symbolicated backtraces), and
      warn on a copyright notice and missing manual pages. apt does not
      check these.
    - The first publish of 13 new crates needs a token: trusted
      publishing works only on crates that exist.
    - crates.io shows each crate the repository's README, which is the
      design bundle's.
    - `windows-11-arm` and `macos-15-intel` are images GitHub may retire
      before the others.
- **Alternatives considered:**
  - `nadirge-*` for every crate: free too, but a rename through every
    manifest, doc and check, when only `nadir` is taken.
  - The apt repository on the demo page's Pages site: every version
    kept, but `pages.yml` would rebuild it from release assets at each
    deploy, and the two workflows would depend on each other.
  - `.deb` files with no repository: no key to keep, but no
    `apt upgrade`.
  - An image `FROM debian` with the glibc cook: a base image to update,
    and the sandbox could not run in it anyway.
  - Cross-compiling the arm64 targets on x86_64 runners: nothing would
    run on arm64 before release.
  - A container action (`docker/build-push-action`), or
    `rust-lang/crates-io-auth-action` for trusted publishing: third-party
    actions, where a few `docker` and `cargo` commands do.
- **Revisit when:**
  - M11.2 signs and notarizes the game's packages (then the engine's
    archives too);
  - GitHub announces the retirement of `macos-15-intel` or
    `windows-11-arm`;
  - all 13 crates exist on crates.io (then trusted publishing can replace
    the token);
  - apt users need an older version than the newest;
  - `nadir-audio` or `nadir-debug` gains code that the facade uses (then
    crates.io gets it).

## ADR-0037 — The Apache-2.0 header on every source file; NOTICE at the root and in every release file, the demo download and the crates; third-party notices kept beside their material

- **Status:** Accepted
- **Date:** 2026-11-01
- **Context:**
  - The user asked for an Apache-2.0 `NOTICE` at the root and the
    license's headers on all source files, then approved the rest below:
    `NOTICE` in every release file, the RFC 8259 notice beside the
    examples, both files in each crate for crates.io, and a check of the
    headers. `LICENSE`'s appendix already
    names the holder: "Copyright 2026 Joseph Anthony Abbott III".
  - The license's section 4(d) binds a redistributor to a NOTICE only "if
    the Work includes a "NOTICE" text file as part of its distribution". A
    `NOTICE` in the repository alone does not reach ADR-0036's archives,
    packages and image, or the demo page's download (ADR-0029, ADR-0033).
  - Some files cannot start with a comment: nadir-gfx refuses a GLSL
    source whose first line is not `#version 330 core`
    (`crates/gfx/src/validate.rs`), a script needs its `#!` first, and an
    HTML page its doctype.
  - A review of the repository found one piece of third-party material:
    `crates/codec/tests/json/rfc8259_image.json` and
    `rfc8259_locations.json` are the examples of IETF RFC 8259, section
    13, with copies in `fuzz/corpus/json` and KBJ encodings in
    `fuzz/corpus/kbj`. The IETF Trust's Legal Provisions (TLP 5.0) count
    JSON as a Code Component, under the Revised BSD License, whose notice
    must go with it: the license's text, or the section 6.d legend "in the
    program or other file that includes the Code Component". `cargo
    package` ships the examples in nadir-codec.
  - Everything else is the project's own: the sprites, the other fixtures
    (`make_vectors.py` regenerates its vectors byte for byte), the
    corpora, the shaders and the site. The inflate decoder, `Pcg32` and the
    site's GitHub mark were checked and need no notice.
- **Decision:**
  - **The header:** the appendix's boilerplate, word for word, in each
    file's comment syntax and followed by a blank line, on every source
    file: Rust, GLSL, the Python and shell scripts, the site's HTML, CSS
    and JS, the `Makefile`, the Dockerfile and the workflows (279 files on
    2026-11-01). It goes first, or after a `#!` line, an HTML doctype or a
    GLSL `#version 330 core`. Configuration and data carry none: TOML,
    Markdown, JSON(C), test vectors, corpora, assets, goldens, the issue
    templates and `dependabot.yml`.
  - **`NOTICE`** holds the project's name and copyright line, and nothing
    else a license does not require there, as the ASF's guidance has it.
  - **It ships beside `LICENSE`** in each release archive and in the demo
    page's download, whose READMEs say "see LICENSE and NOTICE", and at
    `/NOTICE` in the image. In each `.deb` it goes in
    `/usr/share/doc/<package>/`, beside the `copyright` file, which points
    to the license's text in `/usr/share/common-licenses/Apache-2.0`;
    Debian keeps no NOTICE.
  - **Each crate for crates.io keeps copies of both** in its folder, as
    cargo leaves them out of its package otherwise: from outside the folder
    it takes only `Cargo.lock` and the files that `readme` and
    `license-file` name. They are `crates/*/LICENSE` and `NOTICE`, and
    `tools/cook`'s, for the 13 crates without `publish = false`.
    `cargo xtask crates` fails a crate without them, or with copies that
    differ from the root's.
  - **`cargo xtask headers`**, in `cargo xtask ci` after `unsafe-audit`,
    fails a source file whose header is missing, altered or out of place,
    and names the line and what it should be. It knows the kinds above by
    extension or name, and `.metal` too, for M10's shaders. Its text is
    the appendix's, which a test compares with `LICENSE`.
  - **Third-party notices stay with their material:** the RFC 8259
    examples keep the IETF Trust's section 6.d legend, with the credit to
    the IETF and the RFC that its section 4.d asks for, in
    `crates/codec/tests/json.rs`, which reads them and ships beside them.
    A file beside the copies in `fuzz/corpus` would be read as a seed.
- **Consequences:**
  - Good:
    - Whoever passes on a release archive, the image, the download or a
      crate's package has the license and the notice to pass on with it
      (sections 4(a) and 4(d)). A `.deb` carries the notice, and its
      `copyright` file points to the license's text, which every Debian
      system keeps.
    - The header is the same text in every file; `cargo xtask headers`
      keeps it so, and `AGENTS.md` and `CONTRIBUTING.md` say where it goes
      in a new one.
  - Bad:
    - Each source file is 14 lines longer (16 in CSS and HTML, whose
      comment opens and closes on lines of its own), and the lines below
      its header moved down as many.
    - 26 copies to keep: a change to `LICENSE` or `NOTICE` is copied to
      each crate (`cargo xtask crates` names the ones that differ). Git
      keeps one copy of identical files, so they cost no space.
    - A new kind of source file (another extension) is unchecked until
      `kind` in `xtask/src/headers.rs` knows it.
    - `cargo xtask dist`, `cargo xtask site` and the image job read
      `NOTICE`, so a checkout without it fails them.
  - Follow-up: whether the image's static cook and the archives should
    carry the notices of what they link statically (musl in the image,
    Rust's standard library everywhere) was not reviewed.
- **Alternatives considered:**
  - An `SPDX-License-Identifier: Apache-2.0` line alone: shorter, but not
    the appendix's boilerplate.
  - The RFC 8259 notice in `NOTICE`: everyone who passes on a release file
    would carry it, though none of them holds the examples.
  - JSON written here in place of the RFC 8259 examples: no third-party
    material at all, but the tests, corpora and docs that use them
    rewritten, beyond the request.
  - `license-file = "LICENSE"`, inherited: cargo copies the file into
    each package from outside its folder, and crates.io still shows
    `license`, but with both fields set cargo 1.99 warns "only one of
    `license` or `license-file` is necessary" once for each crate, 13
    times, on commands that build, such as `cargo clippy` (not on
    `cargo metadata`). It would carry no `NOTICE` either.
  - Symbolic links to the root's files: no copies to keep, but Git for
    Windows checks links out as text files naming their target, and CI
    runs on Windows.
  - No copies at all: section 4 asks this of redistributors, not of the
    licensor, but a redistributor of a crate would have no license text
    to pass on.
- **Revisit when:**
  - a file that is not the project's own enters the repository;
  - cargo stops warning when `license` and `license-file` are both set
    (then `license-file` can replace the `LICENSE` copies);
  - an outside contributor's file needs its own copyright line.

## ADR-0038 — M5.6 split into four tasks; LDtk 1.5.3 and its conventions; the level file `.klvl` (version 1) and the prefab file; how prefabs, reflection and `load_level` behave

- **Status:** Accepted; as built in M5.11, a handle the asset server does not know, and an `Animator`'s clip with no name, are written as `null` with a warning, since `to_value` cannot fail (ADR-0020), and reading them back is an error; for a hash, a nameless clip is written as its handle; as built in M5.12, the cook's `Counts` is `#[non_exhaustive]`, with `Levels { levels, entities }` (a break), a project's job is `ldtk/<its path>`, without `.ldtk`, only the tile sets of the layers it draws are read, a layer whose tiles are all tall is no tile layer, an entity's field with no value is left out and its overrides are in name order, a level's own fields are not cooked, with a warning, a level's tile layers and grid hold at most 2^23 cells together, and the data job reads each prefab of `data/prefabs/` with `prefab::read` and checks its name; as built in M5.6, `load_level` is `App::load_level`, and `nadir::load_level` on a world, with `unload_level`, the resource `CurrentLevel` and an error of its own, `LevelError`, so the facade's `Error` is unchanged; `Clips` knows an atlas by its pages and holds one added twice once more, until each lets go (`Clips::release_atlas`); a level puts back the clear colour it replaced when it goes, and its prefabs stay in the `PrefabLibrary`; the engine's `nadir::extract_level` draws the level's tile layers; the download, the release archives and the sandbox's `.deb` carry `data/` and `levels/` beside `atlas/` (ADR-0033, ADR-0036); and the sandbox names its collision layers, so the field's world hash changed once more, colliders now written with their layers' names (`8587378f99a52c1a` → `298521738b5650ef` over the capture, its frames the same); that the download, the release archives and the sandbox's `.deb` carry `data/` and `levels/` beside `atlas/` is superseded by ADR-0041: they carry the pack, `assets/game.kpak`, alone
- **Date:** 2026-11-01
- **Context:**
  - Roadmap task M5.6 ("cook: LDtk importer; scene: prefabs and level
    loading") changes `nadir-cook`, `nadir-scene`, the facade, the
    sandbox, and `xtask` to ship the levels: more than the three crates
    `CLAUDE.md` allows a task. A scoping pass mapped its work and its
    notes, and the user accepted a split and a recommended answer to
    each open question on 2026-11-01. LDtk's editor cannot run where the
    work is done.
  - LDtk has no notion of animated tiles, tall tiles, collision, prefabs
    or component fields, so the cook needs conventions for them. Its JSON
    schema, version 1.5.3 (`https://ldtk.io/files/JSON_SCHEMA.json`), is
    the latest release found; its member names are used below.
  - A new parser must have a fuzz target (`.claude/rules/parsers.md`), and
    `xtask`, which runs the fuzzer, may link only `nadir-core` and
    `nadir-codec` (the dependency matrix): every reader the cook or the
    game takes from a file goes in `nadir-codec`.
- **Decision:**
  - **The split**, run in this order, each at most three packages:
    1. **M5.10** (nadir-codec, xtask): the LDtk reader, the level file
       reader, the prefab file reader, their fuzz targets, this ADR and
       the roadmap's split.
    2. **M5.11** (nadir-scene, the facade): `Handle` reflection, the
       context's slots, `PrefabLibrary` with `extends`, a level's entity
       spawn, and the facade's `Sprite`, `Collider` and `Animator`
       reflected.
    3. **M5.12** (nadir-cook, the sandbox): LDtk to `levels/<name>.klvl`,
       and the sandbox's field and props as an LDtk project and prefabs.
    4. **M5.6** (the facade, the sandbox, xtask): `load_level`, the
       sandbox's field drawn from its level, and `levels/` and `data/`
       shipped. It keeps M5.6's number, so M5.8 and M9.1 still depend on
       levels loading. New numbers rather than `M5.6a`, which
       `cargo xtask features` refuses. All four are the feature "Levels
       and prefabs", which each ticks itself into.
  - **LDtk version:** 1.5.3, recorded here and in
    `docs/subsystems/assets-and-codecs.md` ("LDtk version"), not in
    `assets/README.md`, which the cook would list as a file it skips. The
    reader takes a `jsonVersion` of 1.5.x only; any other is refused with
    a note to save the project with LDtk 1.5.3. A project with
    Multi-Worlds (a non-empty `worlds`) is refused.
  - **The LDtk reader** (`nadir_codec::ldtk`, cook only): reads the
    members it needs and ignores the rest. It checks: an `intGridCsv` of
    `__cWid` × `__cHei` entries in an IntGrid layer; a tile's `f` of 0 to
    3 and its `t` a tile of its layer's tile set; every tile set, layer
    and entity definition a uid refers to; a level external (an
    `externalRelPath`) exactly when its `layerInstances` is `null`; and an
    external level's `.ldtkl` naming a level of its project. Errors name
    the member path.
  - **Conventions** (M5.12 applies them):
    - **Animated tiles:** a tile set tile whose `customData` is
      `{"frames": N, "ms": M}` starts an animation of the N tiles from it,
      each shown M ms, as `render::Tile::animated`.
    - **Tall tiles:** a tile whose `customData` is `{"tall": [C, R]}` is
      the top-left tile of a block of C × R tiles. Placed in a cell, it is
      not a cell's tile but a sprite of the block, whose bottom-left cell
      is that cell and whose feet are the bottom middle of the block, as
      the sandbox's tall walls are (`TileSet::block_uv`).
    - **Collision:** the IntGrid layer named `Collision`; every non-zero
      value is solid, in the collision layer "World".
    - **Prefabs:** an entity spawns the prefab of its identifier in
      snake_case (`EnemyBase` → `enemy_base`).
    - **Overrides:** an entity field named `Component__field` (more `__`
      for nested fields) overrides that field of that component. Int,
      Float, Bool, String, Multilines, FilePath and enum fields map to
      their JSON value; Point, Color, Tile, EntityRef and array fields are
      refused in version 1.
    - **Placement:** a level's top left is at (0, 0); `worldX` and
      `worldY` are not used, as one level loads at a time. A layer's
      `__pxTotalOffsetX` and `Y` apply to its tiles and entities, and an
      offset on the `Collision` layer is refused. Each level of a project
      is a `.klvl` of its own, `levels/<its identifier in
      snake_case>.klvl`, and two of one name in a cook are an error.
    - **Drawing:** tile layers (Tiles, AutoLayer, and an IntGrid's auto
      tiles) are drawn from the bottom of LDtk's list up, each in sort
      layer 0 at a depth of its place from the bottom (0, 1, …), behind
      every y-sorted sprite; a layer that is not `visible` is left out.
      Tall tiles are y-sorted sprites in sort layer 1, the sandbox's. A
      layer's opacity below 1, a tile's alpha below 1, two tiles in one
      cell, and a tile set with `spacing` or `padding` are refused.
    - **Side outputs:** a project that turns on backups, the simplified
      export, an image export or the Tiled export is refused, with the
      setting to turn off. An `.ldtkl` no project claims is an error.
  - **The level file** `levels/<name>.klvl`, version 1, is KBJ, read with
    `nadir_codec::level::parse`, which checks the version first (any
    other is not supported) and refuses an unknown member. Its members:
    `version` (1); `size`, the level's `[w, h]` in pixels;
    `background`, `[r, g, b]`; `assets`, the paths it uses in byte order,
    each tile set's atlas among them; `tilesets`, each
    `{atlas, sprite, columns, rows, animations: [{tile, frames, ms}]}`;
    `layers`, drawn first to last, each
    `{tileset, size: [columns, rows], cell, origin: [x, y], layer, depth,
    cells}`, with one integer a cell, row by row: 0 for none, otherwise
    1 + the tile + 65536 × its flip (1 for x, 2 for y); an optional
    `grid`, `{size, cell, origin, layer, solid}`, with `solid` one 0 or 1
    a cell; `tall`, each `{tileset, tile, size: [c, r], feet: [x, y],
    layer}`; `entities`, each `{prefab, position: [x, y], overrides}`,
    with `overrides` an object of component names; and the optional
    `music`, `ambience` (asset paths) and `camera` (`[x, y, w, h]`).
    Positions are whole pixels. The cook writes it, as it writes the
    atlas file; the game reads it.
  - **The prefab file** `data/prefabs/<name>.kbj`, cooked from
    `assets/data/prefabs/<name>.jsonc`, is read with
    `nadir_codec::prefab::parse` (or `prefab::read` from a JSON value, for
    the cook to check its source): `{name, extends?, components}`, no
    other member. A prefab's name is lowercase letters, digits and `_`,
    and equals its file's stem (nadir-scene checks that). `extends` names
    another prefab. `components` maps component type names to values,
    none `null`: a derived prefab cannot remove a base's component in
    version 1.
  - **Fuzz targets:** `ldtk` (each input as a project, and as an external
    level of a fixed project), `level` and `prefab`, with seeds in
    `fuzz/corpus/<target>/`.
  - **Prefabs** (M5.11): base, then derived, then a level's per-instance
    overrides apply onto one value per component; an override may add a
    component the prefab lacks. An `extends` chain stops at 16, and a
    cycle is an error naming it (`slime -> enemy_base -> slime`), as is a
    missing base. An unknown component at spawn is an error naming the
    prefab's cooked path, the prefab and the component, and the failed
    spawn leaves no entity; the cook's errors name the `.jsonc`. Each
    prefab is checked against the registry when it loads, and the cook
    checks that each LDtk entity's prefab exists; the debug-build check of
    every prefab file at startup is deferred, as listing a `Source` would
    add nadir-assets to M5.11. `spawn` takes the library, the registry and
    the server as arguments, so nadir-ecs needs no change, and returns
    each entity with its position, for the facade to give it its
    `Transform`.
  - **Reflection** (M5.11): `ReflectContext` holds the asset server
    shared while hashing, so a hash never loads, and exclusive with a
    `Scope` while loading; a handle the server does not know is an error,
    not a panic. While hashing (`is_hashing()`), every `Sprite` leaves out
    its texture, uv, size, pivot and `flip.x`, which `nadir::animate`
    writes, as a flag cannot tell which sprites have an `Animator` or a
    `Facing` (a departure from M4.6's note: the fields are the game's on
    other sprites); a per-field marker waits for M9.4's `saved` flag. A
    prefab writes a sprite as `{"atlas": <path>, "sprite": <name>,
    "frame": <index>}`; written back it gives the texture's path and uv;
    the texture is required. The game declares its collision-layer names
    (`App::collision_layers(&["World", "People"])`); `LayerMask::ALL` is
    `"all"`, an unnamed bit its number, and an empty `{}` collider an
    error. An `Animator` whose clip has no name cannot be written.
    `ReflectErrorKind`, `FieldInfo`, the cook's `Counts` and `CookError`,
    and the facade's `Error` become `#[non_exhaustive]` in the task that
    first adds to them, a break recorded then.
  - **`load_level`** (M5.6): reads the `.klvl` and its prefabs before it
    returns, while textures stream in as they do now; despawns only the
    entities the previous level spawned; takes its clear colour from the
    level; gives no grid when the level has none; and skips the clips of
    an atlas the game already added (`Clips` tracks them), so the
    sandbox's global atlas is no `NameTaken`. A missing `.klvl` or prefab
    is an error from `load_level`; the release dialog for a missing
    texture moves to M9.1's loading state. M5.6 measures the 500 ms
    level-load budget, which M11.5 enforces. The sandbox's person is
    still spawned in code; its walls, trees and rocks come from the
    level in map.rs's order, so that the captures' frames stay those of
    M5.9. The world hashes change once, in M5.11 (three more registered
    types), which records them; M5.6 compares with M5.11's.
  - **The sandbox's level** (M5.12): `assets/levels/field.ldtk`, written
    once in 1.5.3's format by a scratch script from map.rs's `MAP` (not
    committed), never opened in LDtk; `MAP` stays as the reference that a
    test compares the cooked level with. A tile set's `relPath` may use
    `..` only to stay inside `assets/`.
- **Consequences:**
  - Good:
    - Each task can be finished and checked on Linux alone, and each new
      reader is fuzzed where `xtask` can reach it.
    - The level and prefab formats are checked as strictly as the atlas
      file, and an LDtk project from another version is refused before
      it is misread.
  - Bad:
    - Four tasks where there was one, and the roadmap's M5.6 notes are
      spread over them.
    - The conventions are ours: a project made in LDtk needs the tile
      sets' custom data, the `Collision` layer and the field names written
      as above, and nothing in the editor checks them; the cook does.
    - `field.ldtk` was not saved by LDtk, so the reader is tested on files
      written from the schema, not on the editor's own output, until
      someone opens and saves it in LDtk 1.5.3.
    - Leaving the animated sprite fields out of every sprite's hash
      leaves out the fields of sprites the game moves by hand too.
  - Follow-up: M5.11 sets the status lines of ADR-0020 (`Handle`
    reflection), ADR-0024 (`Animator` reflected) and ADR-0025 (what the
    hash leaves out) when it builds them.
- **Alternatives considered:**
  - M5.6 in one task: past the three-crate limit.
  - `M5.6a` to `M5.6d`: `cargo xtask features` reads only `M<n>.<n>`,
    and teaching it suffixes changes xtask for a name.
  - Any 1.x `jsonVersion`: LDtk changes members between minor versions.
  - LDtk's enum tags for animated and tall tiles: they carry no numbers.
  - Chunked tile indices in the level file: chunking is the renderer's
    job, and a 256 × 256 level is 65,536 entries, far under KBJ's cap.
  - The level writer in nadir-codec: the atlas file's precedent puts the
    writer in the cook, and the reader's tests encode JSON text.
  - The prefab reader in nadir-scene: it would have no fuzz target.
  - A committed generator of `field.ldtk`: a second source of the level
    to keep beside `MAP`.
- **Revisit when:**
  - LDtk releases a version whose `jsonVersion` is not 1.5.x and the
    project is saved with it;
  - a level needs a convention above that it cannot meet (a flipped tall
    tile, an offset collision layer, a Point or array field);
  - the debug-build prefab check at startup is wanted (a `Source` listing
    or an index the cook writes);
  - M9.4's `saved` flag gives the reflect macros a per-field marker.

## ADR-0039 — The facade's package is `nadir_engine`; each crate for crates.io has a README of its own, whose examples are tested; the crates go to 0.1.1

- **Status:** Accepted; its version, 0.1.1, is superseded by ADR-0040 (0.2.0); nadir-audio, added for crates.io in M6.9, has a README in its shape (ADR-0050)
- **Date:** 2026-11-01
- **Context:**
  - Release v0.1.0 (ADR-0036) put twelve of the thirteen crates on
    crates.io at 0.1.0, from `nadir-core` to `nadir-cook`; on 2026-11-01,
    `NadirGE`, the facade, was not there. A version once published cannot
    be published again.
  - The user asked for the facade's package to be named `nadir_engine`, for
    every crate's documentation to serve the developers who would use it,
    and for the crates to go to 0.1.1, ready for crates.io. Neither
    `nadir_engine` nor `nadir-engine`, which crates.io takes for the same
    name, was taken.
  - Every crate's crates.io page showed the repository's README
    (`readme.workspace = true`), which is about the whole engine; most
    crates' own docs were a few lines with no example.
  - The release workflow's token is limited to the crates `nadir-*` and
    `NadirGE` (`docs/05-release.md`), a pattern that may not match the
    underscore of `nadir_engine`.
- **Decision:**
  - **The facade's package is `nadir_engine`**, its library still `nadir`,
    so a game writes `nadir_engine = "0.1"` and `use nadir::prelude::*`.
    The workspace's dependency key stays `nadir` (`package =
    "nadir_engine"`), so no crate's code changes. The name is the user's,
    with an underscore, though the other crates use hyphens.
  - **Every crate goes to 0.1.1**, `[workspace.package]`'s version and each
    of `[workspace.dependencies]`, as `cargo xtask crates` checks.
  - **Each of the 13 crates has a `README.md` of its own** (`readme =
    "README.md"`), its crates.io page, in one shape: what it is, how it
    fits in Nadir and how a game reaches it, its status, how to use it with
    an example, its features and platforms where they apply, links to its
    API reference, its design and the engine, and its license.
  - **Each crate's docs** (`//!` in `src/lib.rs`), its docs.rs page, say
    what it is, how it fits, its main items by purpose, and what matters
    to use it well, with examples that are doc tests.
  - **A README's examples are doc tests too:** each `src/lib.rs` has a
    `#[cfg(doctest)]` item whose doc is the README, so that the examples on
    crates.io compile as the API changes.
  - **docs.rs** documents the crates' optional features (`profile`, `png`,
    `dev`), and `nadir-sys`, `nadir-platform` and `nadir-gfx` for Linux,
    Windows and macOS (`[package.metadata.docs.rs]`).
  - The repository's README stays the repository's.
- **Consequences:**
  - Good:
    - Each crate's page says what that crate is for and how to start; the
      facade's is the engine's guide.
    - An example that stops compiling fails `cargo test`, as the crate docs'
      do.
  - Bad:
    - Thirteen READMEs to keep true as the crates change, beside the crate
      docs they partly repeat.
    - `NadirGE`, never published, leaves nothing to point users from; the
      twelve crates at 0.1.0 stay there, with no facade of that version.
    - The facade's name breaks the workspace's hyphenated naming.
  - Follow-up, for a maintainer: give the crates.io token `nadir_engine`
    in its crate scopes, or a pattern that matches it, before the next
    release, as `publish-new` must reach the new name.
- **Alternatives considered:**
  - `nadir-engine`: matches the other crates and the token's `nadir-*`,
    but not the name the user chose.
  - Keeping `NadirGE`: the user asked for the change.
  - One README for every crate, the repository's: it says nothing of the
    crate a reader is looking at.
  - `#![doc = include_str!("../README.md")]` as the crate docs: one text to
    keep, but the README's shape is a landing page's, and the crate docs
    that already reference the design would be lost.
- **Revisit when:**
  - a crate is added for crates.io (it gets a README in this shape);
  - crates.io's trusted publishing replaces the token (then its scopes no
    longer matter).

## ADR-0040 — The crates go to 0.2.0, as M5.11 and M5.12 broke public enums

- **Status:** Accepted; its version, 0.2.0, is superseded by ADR-0044 (0.3.0)
- **Date:** 2026-11-02
- **Context:**
  - ADR-0039 set every crate at 0.1.1, ready for crates.io, where twelve
    are at 0.1.0 (ADR-0036). 0.1.1 was never published.
  - Since then, M5.11 made nadir-scene's `ReflectErrorKind`
    `#[non_exhaustive]`, with the variants `MissingField`, `NoContext`,
    `BadAssetPath` and `UnknownName`, and M5.12 made nadir-cook's `Counts`
    `#[non_exhaustive]`, with `Levels` (ADR-0038). Code that matches either
    exhaustively, as 0.1.0 allowed, does not compile against them.
  - Cargo takes a 0.x version's second number as its major one: a crate
    that asks for `0.1` takes 0.1.1 as compatible, so the breaks published
    as 0.1.1 would break builds that `cargo update` reaches. The user asked
    on 2026-11-02 for the crates to go to 0.2.0 for publishing.
- **Decision:**
  - **Every crate goes to 0.2.0**, `[workspace.package]`'s version and each
    of `[workspace.dependencies]`, as `cargo xtask crates` checks. 0.1.1 is
    not published.
  - The crates' READMEs, the repository's README, `AGENTS.md`'s status,
    the facade's crate docs and `docs/05-release.md` name 0.2
    (`nadir_engine = "0.2"`). The roadmap's notes and the earlier ADRs keep
    0.1, as history.
  - This supersedes only ADR-0039's version.
- **Consequences:**
  - Good: a game on 0.1 is not broken by `cargo update`, and the breaks
    are a new version as Cargo reads one.
  - Bad: crates with no break, `nadir-sys` say, go to 0.2.0 too, as the
    workspace keeps one version (ADR-0036); and the 0.1 line ends at 0.1.0,
    with no facade on crates.io (ADR-0039).
- **Alternatives considered:**
  - 0.1.1 with the breaks: Cargo would offer it to code it breaks.
  - 0.1.1 from before M5.11 (`21751b4`), then 0.2.0: two releases where
    the user asked for one.
  - Only nadir-scene and nadir-cook at 0.2.0: the workspace's one version
    (ADR-0036) and `cargo xtask crates` require all of them to agree.
- **Revisit when:**
  - the crates reach 1.0, when a break needs a major version;
  - or the crates stop sharing one version.

## ADR-0041 — M5.7 split into three tasks; the pack's reader in codec; every cook writes `game.kpak`, which release builds read

- **Status:** Accepted: the split, and release builds reading `<assets>/game.kpak`, which the user chose on 2026-11-02; proposed with them, for review: the format's details, and every cook writing the pack; as built in M5.13, the reader's checks run in the order its docs list, two entries may share bytes only as one blob (the same offset, sizes, codec and CRC), and every read of an entry checks its CRC, not only the first; as built in M5.14, every cook writes the pack after the stale files are removed and before the cache, only when its bytes change, having read it back with the game's reader, and the pack is not in the cook's `Report` and its errors are `CookError`'s existing variants, so the cook's API is unchanged; `PackSource` keeps the file open behind a mutex and reads only an entry's bytes under it; as built in M5.7, proposed for review: a pack that is there but does not open is logged by `App::new`, and every load then fails with the error that opening it gave, rather than reading the loose files beside it; the facade picks the source in a private function whose choice the tests make, since `cargo xtask ci` tests with `dev`; and `cargo xtask dist` checks the pack it ships against the laid-out cook's, in place of the loose folders
- **Date:** 2026-11-02
- **Context:**
  - Roadmap task M5.7 ("pack files") asks for the `.kpak` writer in the
    cook, `PackSource` in nadir-assets with its TOC fuzzed, and release
    builds that read the pack. That changes nadir-codec, xtask,
    nadir-cook, nadir-assets and the facade: more than the three packages
    `CLAUDE.md` allows a task. The user chose on 2026-11-02 to split it
    in three, as ADR-0038 split M5.6.
  - A parser must have a fuzz target (`.claude/rules/parsers.md`), and
    `xtask`, which runs the fuzzer, may link only `nadir-core` and
    `nadir-codec` (the dependency matrix), so the TOC reader goes in
    `nadir-codec`, as ADR-0038 put the level and prefab readers there.
  - The format of `docs/subsystems/assets-and-codecs.md` ("Pack file
    `.kpak` (version 1)") leaves open what the TOC's CRC covers, what an
    entry's CRC covers, how the string table is laid out, and where the
    TOC ends.
  - The cook's outputs are loose files below `atlas/`, `data/` and
    `levels/`, listed in its cache (ADR-0031); dev builds will reload them
    as they change (M5.8); release builds, without `dev`, read loose files
    too, until now (ADR-0030).
- **Decision:**
  - **The split**, run in this order, each at most three packages:
    1. **M5.13** (nadir-codec, xtask): `nadir_codec::kpak`, the pack's
       reader: the header, the TOC and its string table checked, an
       entry found by a binary search of the path hashes, and an entry's
       bytes checked against its CRC and decompressed; hand-made vectors;
       the `kpak` fuzz target and its seeds; this ADR and the roadmap's
       split.
    2. **M5.14** (nadir-cook, nadir-assets): the cook writes the pack, and
       `PackSource` reads it, with the round trip and a damaged entry
       tested.
    3. **M5.7** (the facade, xtask, the sandbox): release builds read the
       pack, and the release files and the demo page's download ship it.
       It keeps M5.7's number. New numbers rather than `M5.7a`, which
       `cargo xtask features` refuses.
  - **The format** fills in what the design left open, at version 1, all
    numbers little-endian:
    - The header is the design's 32 bytes. Bit 0 of `flags` says a string
      table follows the TOC entries; the other bits, and `reserved`, are
      0.
    - The TOC is at `toc_offset`, a multiple of 16, and runs to the end of
      the file: `toc_size` bytes, the `entry_count` entries of 32 bytes,
      then the string table if there is one. `toc_crc32` is the CRC-32
      (`nadir_core::hash::crc32`) of those `toc_size` bytes.
    - The string table is, for each entry in TOC order, its path's length
      as a `u16` and the path's bytes. The reader checks that each path
      hashes to its entry's `path_hash`.
    - An entry's `crc32` is the CRC-32 of its stored bytes, checked before
      they are decompressed, so that a damaged entry is a checksum error
      that names its path, from the string table, not a KLZ error.
    - The entries are in ascending `path_hash` order, no hash twice; each
      starts at a multiple of 16 between the header and the TOC, and two
      may share bytes (dedup); codec 0 stores `raw_size` bytes as they
      are, codec 1 as KLZ. `entry_count` is at most `Limits::max_entries`,
      the design's 1,000,000 pack entries, and each size at most
      `Limits::max_decoded_bytes`.
  - **Where the pack is:** every cook writes `OUT/game.kpak`, of the
    outputs its cache lists, with the string table, so the pack and the
    loose files are always one cook; the cook owns that file as it owns
    `atlas/`, `data/` and `levels/` (M5.14).
  - **Who reads it:** the facade, without `dev`, reads
    `<Config::assets>/game.kpak` through a `PackSource` when that file is
    there, and the loose files otherwise; with `dev` it reads the loose
    files, which hot reload watches (M5.8). `LooseSource` stays in every
    build. The release files and the download ship `game.kpak` in
    `assets/` in place of the loose folders (M5.7).
- **Consequences:**
  - Good: each task is reviewable alone, and the reader is fuzzed before
    anything writes a pack; a damaged entry names its path; the game's
    tests, which run without `dev`, read the pack wherever `cargo xtask
    cook` wrote one, and the loose files in folders that have none.
  - Bad: every cook packs every output again, even when no job changed,
    which costs time in proportion to the game's assets (M5.14 measures
    it; M5.8's 1 s may need a cook that skips it, with the option in the
    cache's `options`, ADR-0031); and `target/cooked` holds each output
    twice, loose and packed.
- **Alternatives considered:**
  - M5.7 whole, across five packages: against `CLAUDE.md`'s limit.
  - Two tasks, the reader with the cook's writer, then `PackSource` with
    the facade: `PackSource`'s tests would need the cook's packs from a
    task already done, and the reader's fuzzing would wait on the writer.
  - The TOC reader in nadir-assets: `xtask` cannot link it, so it could
    not be fuzzed.
  - The entry's CRC over its raw bytes: a damaged KLZ stream would be a
    KLZ error, or a wrong decode, before the CRC could name it.
  - A pack only when asked (`--pack`): a pack left from an earlier cook
    would be read by a release build after a cook without it, older than
    the loose files beside it.
  - `LooseSource` compiled out of release builds: the facade's tests, and
    a game that ships loose files, read them without `dev`.
- **Revisit when:** packing every output takes more than a few hundred
  milliseconds of a cook, which M5.8's 1 s reload cannot afford; a game
  needs files from outside its pack in a release build, such as mods; or a
  second pack version is written.

## ADR-0042 — M5.8 split into four tasks; `cook --watch` in xtask runs a cook built once; the cook rewrites only outputs that changed; reload in nadir-assets and nadir-render, started by the facade under `dev`

- **Status:** Accepted: the split, and M5.8's demonstration run by the session, which the user chose on 2026-11-03; proposed with them, for review: the rest
- **Date:** 2026-11-03
- **Context:**
  - Roadmap task M5.8 ("hot reload") asks for `cargo xtask cook --watch`,
    the runtime watcher, and reloading textures, data, levels and shaders
    in dev. A scoping pass found it changes xtask, nadir-cook,
    nadir-assets, nadir-render, nadir-scene, the facade and the sandbox:
    more than the three packages `CLAUDE.md` allows a task. The user chose
    on 2026-11-03 to split it in four, as ADR-0038 split M5.6 and ADR-0041
    split M5.7.
  - `xtask` may not link `nadir-cook` (the dependency matrix), so it can
    only run the cook's binary. `cargo xtask cook` runs it through
    `cargo run --release`, which checks the build on every run and builds
    the cook again whenever an engine crate it uses changed.
  - The `dev` feature belongs to the facade, nadir-platform and the
    sandbox (`docs/01-architecture.md`); nadir-assets has no features,
    and nadir-render's one, `headless-test`, builds its own offscreen
    tests.
  - The cook writes every output of a job it cooks, through a `.tmp` and a
    rename, even when the bytes are those already there; only the pack is
    written when its bytes change (ADR-0041). A watcher of times of change
    would reload every page of a group for one changed sprite, and the
    level for a prefab whose edit leaves it the same.
  - An atlas job writes its files in path order, so `atlas/<group>.kbj`
    comes before the pages it names (`.` sorts before `_` and `/`).
  - ADR-0041 warned that packing every output on every cook may not fit
    M5.8's 1 s; for the sandbox, a cook with nothing to cook takes 0.00 to
    0.01 s, the pack included (M5.14).
  - `docs/01-architecture.md`'s threading table gave the asset workers a
    "dev re-cook", and the watcher the times of "source/cooked" files,
    where the design has the engine never link the cook and the watcher
    poll the cooked files.
- **Decision:**
  - **The split**, run in this order, each at most three packages:
    1. **M5.15** (xtask, nadir-cook): `cargo xtask cook --watch`; the cook
       writes an output only when its bytes change, and an atlas group's
       pages before the files that name them; this ADR and the roadmap's
       split.
    2. **M5.16** (nadir-assets): a reload behind the same handle, and the
       watcher thread that polls the loaded paths' times of change.
    3. **M5.17** (nadir-render): a program rebuilt from new shader
       sources, and a texture reloaded behind its handle, drawn.
    4. **M5.8** (the facade, nadir-scene, the sandbox): under `dev`, the
       game reloads what changed (textures, atlases with their clips,
       prefabs, the current level, shaders), and the demonstration. It
       keeps M5.8's number. New numbers rather than `M5.8a`, which
       `cargo xtask features` refuses.
  - **`cargo xtask cook --watch [--force]`** lives in xtask, not in the
    cook's command line: it builds `nadir-cook` in release once, cooks
    (`--force` for that first cook alone), then polls `assets/` every
    100 ms: what the cook's walk sees, files, folders and links (not
    followed), names starting with `.` left out, each file and link by its
    time of change and its length.
    After a change and one poll with no further change, it cooks again,
    running the built binary; a cook that fails is printed and the watch
    goes on. The binary is the one built at the start: a change to the
    cook's own code needs the watch started again.
  - **The cook rewrites an output only when its bytes change**, the cache
    and the pack too, so that a file keeps its time of change unless its
    bytes do; and within an atlas job it writes the pages before the atlas
    file and the fonts' files that name them, so that a game polling the
    output never reads an atlas file naming pages not written yet. The
    outputs' bytes, the cache's format and the cook's API are unchanged.
  - **No option to skip the pack**: the watch's cooks pack as every cook
    does; ADR-0041's trigger to revisit that stands, measured by M5.15.
  - **The reload and its watcher are nadir-assets'**, compiled in every
    build with no feature, and started by the facade under `dev`, as it
    picks `OnFailure::Placeholder` there (M5.16). The watcher polls the
    loaded paths through `Source::modified`, not a walk of a folder. A
    reload keeps the handle; one that fails keeps the value it had. The
    published API grows but does not break: `LoadState` and `Source` keep
    their shapes.
  - **Shaders:** nadir-render rebuilds a program from sources it is given,
    with no feature and no file reading of its own, and keeps the program
    it had when the new sources do not compile or do not match the
    program's interface (M5.17); the facade, under `dev`, reads the files
    and hands them over (M5.8).
  - **M5.8's Verify:** the session runs it, with a test under `dev` that
    rewrites a cooked file and waits for the change, and with the window:
    timestamped edits made while `cargo xtask cook --watch` and `cargo dev`
    run, the window captured before and after, the reloads' times read
    from the log, and every edit put back.
  - `docs/01-architecture.md`'s threading table: the workers re-cook
    nothing; the watcher polls the loaded cooked files' times.
- **Consequences:**
  - Good: each task is reviewable alone, and the first three are tested
    without a window; an edit reloads only what it changed; the watch pays
    cargo's start once, and keeps watching through a broken file.
  - Bad: the watch runs a cook built when it started; a cook reads each
    output it writes back first, to compare; a game that copies an
    atlas's regions outside what the facade reloads (the sandbox's
    `Sprites`) sees no change until it looks them up again; texture and
    shader reloads can be seen only on Linux until M2.6 and M2.7.
- **Alternatives considered:**
  - M5.8 whole, across seven packages: against `CLAUDE.md`'s limit.
  - Three tasks, asset and shader reload together: the largest task of the
    set, reviewed as one.
  - Shader reload left to the backlog: it changes the roadmap's Do line.
  - `nadir-cook --watch`: a public command line change, and the released
    cook would watch too.
  - `cargo run` on each change: cargo's start on every cook, and a rebuild
    of the cook in release whenever an engine crate changed.
  - A skip-the-pack option in the cache's `options`: a changed options
    string makes every job stale, and a skipped pack goes stale, which
    release builds would read (ADR-0041).
  - A `dev` feature in nadir-assets or nadir-render: a feature table entry
    and a facade feature list for what a runtime setting does.
  - A watcher that walks the output folder: tied to loose files, and it
    would see `.tmp` files and the cache.
  - The game waiting for the cook's cache, written last, before reloading:
    ties the engine to the cook's private file, and misses hand edits.
- **Revisit when:** a cook of one changed file takes more than 0.5 s in
  the watch, half of M5.8's 1 s; a renderer needs to keep a replaced
  texture until the GPU is done with it (Metal, M10); or a game needs to
  reload in a release build, such as for mods.

## ADR-0043 — M5.8: the game's hot reload, under `dev` only; the level made again behind its handles; an atlas's clips found again by name; prefabs replaced in place

- **Status:** Proposed
- **Date:** 2026-11-07
- **Context:**
  - ADR-0042 left the game's side of hot reload to M5.8: under `dev`, the
    facade starts nadir-assets' watcher and watches render's shader
    files, and each frame reloads textures, atlases (their sprites'
    regions and clips found again by name), prefabs, the current level
    and shaders. The roadmap's Do line names `PrefabLibrary::remove`, in
    nadir-scene, for the prefabs.
  - A `Sprite` keeps no atlas or sprite name (ADR-0038): a prefab's sprite
    of an atlas frame, and a tall tile's, copy their regions at spawn.
    `Sprite`'s fields are public and it is not `#[non_exhaustive]`, so a
    field more breaks 0.2; `Config` is the same.
  - `load_level` unloads `Scope::Level`, which ends the handles in it, and
    makes the tile layers' handles anew, and the clips' when only the
    level held its atlas. `Clips` knows an atlas by its pages' textures.
  - The `PrefabLibrary` keeps every prefab a level read, and a later load
    uses the kept one without reading it again (ADR-0038), while the
    level's next load unloads its file, which the watcher then stops
    watching.
  - `drain_swapped` gives each swapped handle once. The asset watcher's
    first look is 250 ms after it starts, and a path loaded before it
    started is watched from that look (M5.16). Render's shader folder,
    `SHADER_DIR`, is outside `Config::assets`, so outside the asset
    server's source.
  - Captures and recordings must not reload: a reload changes the world.
    M4.6 left open whether an animator that finished a clip that plays
    once holds the new last frame of a clip replaced under its handle.
- **Decision:**
  - **The reload is `dev`'s** (`#[cfg(feature = "dev")]`), so builds
    without `dev` compile none of it, nor render's `SHADER_DIR` path of the
    machine that built them. What every build compiles changes nothing
    there: `PrefabLibrary::remove`, the finished animator's rule below,
    and a level's hold taken only of an atlas with clips, which is all
    `Clips` holds. No public API is added but `PrefabLibrary::remove`, and
    no `Config` field.
    `App::new` starts the asset watcher, so that the files the game's
    setup reads are watched from before they are read. `App::run`'s loop,
    where there is a device, starts the hot reload and a thread,
    `nadir-shader-watch`, which looks at the times of change of each
    program's two files every 250 ms and marks the programs that changed
    in an atomic mask. `App::offscreen` and `App::update` never reload.
  - **Each drawn frame, after its finish:**
    1. the atlases, prefabs and level files swapped in are drained; a
       texture needs nothing, so its swaps are left for the game;
    2. an atlas's clips are replaced in place by name, under their
       handles (`Clips::reload_atlas`, the crate's own), in the atlas of
       its pages, or of one of them when it gains or loses a page: a clip
       new to it is added, one it no longer has stays as it was, with a
       warning, so that its handle stays good. A page not loaded yet is
       loaded in `Scope::Global`, since the game and a level may share the
       atlas;
    3. a prefab's new file replaces it in the `PrefabLibrary` under its
       name (`insert`), and `load` then loads and checks the bases it now
       extends; a file that names another prefab now is refused, and one
       the library has not read, as a prefab a level could not load
       before it was cooked, is left for the level to read;
    4. once an atlas, a prefab or the current level's file was swapped,
       and no load or reload is pending, so that the files of one cook
       seen in one look land together, the current level is made again
       (`reload_level`, the crate's own);
    5. the programs whose files changed are rebuilt from them
       (`Renderer::reload_program`, with no Metal source until M10);
    6. the paths the watcher saw change are handed to the asset server
       (`reload_changed`).
  - **The level made again** keeps what a game may hold of it: it unloads
    no scope, so its assets keep their handles; its tile layers keep
    theirs in `TileLayers`, in their order, a layer more added and one
    fewer removed; its atlases are held again before the last hold goes,
    so their clips keep theirs. Its entities are spawned anew from its
    files, which finds their sprites' regions again by name: they get new
    ids, and what happened to them since the load is undone. A level that
    cannot be made again stays as it was.
  - **A level's prefabs whose files were unloaded are read again:** under
    `dev`, before a level's prefabs are read, by a load or a reload, each
    of its entities' prefabs, and each base, whose file the asset server
    no longer holds, as when the last load of a level unloaded
    `Scope::Level`, is removed from the library (`PrefabLibrary::remove`,
    new in nadir-scene), so that the level reads it from its file, and the
    watcher watches that file, again. Without `dev` the library keeps them,
    as ADR-0038 has it.
  - **A finished animator holds the new last frame** of a clip that plays
    once and was replaced, and does not end again (`Animator::advance`).
  - **Logging:** each step is logged with its time, and once every reload
    the watcher handed over is made, how long that took.
- **Consequences:**
  - Good: the handles a game holds stay good through a reload; a frame
    with nothing changed allocates nothing (the facade's count); the files
    of a cook seen in one look make the level once; release builds hold
    none of it.
  - Bad: a level made again spawns its entities anew, so their ids and
    state are lost; what a game copied out of an asset itself (the
    sandbox's `Sprites`, for its stress scenes) and the entities it
    spawned of prefabs itself do not change; the facade drains the
    atlases', prefabs' and levels' swaps, so a game cannot see them; a page
    an atlas gains is loaded in `Scope::Global`, and stays until the game
    ends, even for an atlas only a level uses; a cook whose files straddle
    a look of the watcher makes the level twice, the first time maybe
    refused, and logged, with half the cook's files; a shader edited
    before `App::run`'s loop starts is seen at its next save, the renderer
    having been built from the sources compiled in;
    `dev` runs one more thread; on Windows and macOS nothing reloads until
    M2.6 and M2.7, as loads finish only in drawn frames.
- **Alternatives considered:**
  - A reloaded prefab removed with `PrefabLibrary::remove`, as the
    roadmap's Do line has it, and the level made again reading it anew:
    the library would lack the prefab until something read it again, so
    the game's own spawns of it would fail; `insert` and `load` keep it
    whole and check it. `remove` serves the prefabs whose files were
    unloaded instead.
  - A page an atlas gains loaded in the level's scope when its other
    pages are the level's: a game that shares the atlas would hold clips
    of a page the level frees.
  - A component, or a `Sprite` field, naming each sprite's atlas frame, to
    find regions again without making the level again: a field breaks
    0.2, and a component costs every sprite storage that only `dev` uses.
  - The level made again at each swap: a cook writes an atlas's pages,
    then its file, then the level, and the level would be made two or
    three times.
  - The asset watcher started by `App::run`: an edit in the first 250 ms
    would be missed, the setup's files then watched from its first look.
  - The shaders through the asset server: their folder is outside its
    source, and a second server starts workers of its own.
  - The shader files polled on the main thread: system calls in the
    frame, and a path converted, and so allocated, on Windows.
  - A public event of what reloaded: no task needs it yet.
  - Reloading in `App::offscreen` too: recordings would differ from run
    to run.
- **Revisit when:** a game needs to learn of a reload, to look up again
  what it copied (the sandbox's stress scenes, a UI); a level's entities
  must keep their state through a reload; the Metal sources need
  reloading (M10.3); a page gained by an atlas only a level uses must be
  freed with the level; or the level made twice by one cook gets in a
  game's way.

## ADR-0044 — The crates go to 0.3.0

- **Status:** Accepted
- **Date:** 2026-11-07
- **Context:**
  - Release v0.2.0 (`c3e45ba`) is out: its tag is on `origin`, and on
    2026-11-02 the release workflow published the crates on crates.io and
    the GitHub release (ADR-0040). crates.io takes a version only once, and
    the tag ruleset keeps a pushed tag from being moved or deleted
    (ADR-0036).
  - Ten commits have landed on `main` since, among them the facade's
    level loading (M5.6), the pack (M5.13, M5.14, M5.7), `cook --watch`
    (M5.15) and hot reload (M5.16, M5.17, M5.8).
  - `make release` refuses a `VERSION` other than `v` and HEAD's
    `[workspace.package]` version, and one `origin` already has; the
    workflow's gate refuses a tag other than that version. With the
    workspace at 0.2.0, no tag gets through either.
  - The user asked on 2026-11-07 for every crate at 0.3.0.
- **Decision:**
  - **Every crate goes to 0.3.0**, `[workspace.package]`'s version and each
    of `[workspace.dependencies]`, with `Cargo.lock`, as `cargo xtask
    crates` checks. The next release is `make release VERSION=v0.3.0`.
  - The crates' READMEs, the repository's README, `AGENTS.md`'s status,
    the facade's crate docs, `docs/05-release.md` and the `Makefile`'s
    examples name 0.3 (`nadir_engine = "0.3"`, `v0.3.0`), and the cook's
    cache examples name nadir-cook 0.3.0. The roadmap's notes, the earlier
    ADRs and the diagrams' "as of commit" lines keep 0.2, as history.
  - This supersedes only ADR-0040's version.
- **Consequences:**
  - Good: `v0.3.0` is a tag neither `origin` nor crates.io has, so
    `make release` and the gate take it once CI passes on its commit; and
    code that asks for `0.2` takes none of what changed since 0.2.0 until
    it asks for `0.3`.
  - Bad: a game on 0.2 edits its `Cargo.toml` to move, even where nothing
    it uses changed; crates with no change since 0.2.0 go to 0.3.0 too, as
    the workspace keeps one version (ADR-0036); and the 0.2 line ends at
    0.2.0.
- **Alternatives considered:**
  - 0.2.1: Cargo takes it as compatible with 0.2 and gives it to code that
    asks for `0.2` at its next `cargo update`, which is right only if
    nothing since 0.2.0 breaks such code; the user asked for 0.3.0.
  - Leaving the version at 0.2.0: `make release` and the gate refuse every
    tag until it changes.
  - Only the crates that changed at 0.3.0: the workspace's one version
    (ADR-0036) and `cargo xtask crates` require all of them to agree.
- **Revisit when:**
  - the crates reach 1.0, when a break needs a major version;
  - or the crates stop sharing one version.

## ADR-0045 — M6.1: QOA held to its specification; a frame decoded at a time into the caller's buffer; the encoder goes on from the 16 bits a frame holds

- **Status:** Proposed
- **Date:** 2026-11-07
- **Context:**
  - M6.1 asks for QOA's encoder and decoder "per the spec", with a fuzz
    target, the round trip's signal-to-noise ratio measured and asserted,
    and a decoder that never panics. The reference, `qoa.h` (MIT, Dominic
    Szablewski), describes the format in its comments; qoaformat.org
    publishes it.
  - The reference's decoder reads more than the format allows: a frame
    larger than its slices; a last slice's unused residuals, which the
    format says must be 0, as anything. At a frame it cannot read, a short
    frame before the last, or one whose channels or sample rate differ from
    the first's, it stops and gives the samples before it, as it does for a
    file cut short. It refuses a file of 0 samples (a stream) and more than
    8 channels (`QOA_MAX_CHANNELS`), though its comments speak of up to
    31.
  - The audio design keeps a track's QOA in memory and decodes it on the
    audio thread, a frame at a time, allocating nothing (M6.2). Each frame
    carries its predictors' state, so frames decode alone.
  - The reference's encoder keeps its LMS weights in a C `int` from frame
    to frame, but a frame holds 16 bits of each. Where they outgrow 16
    bits, the decoder starts the frame from other weights than the encoder
    measured with. Its penalty for large weights keeps them small for most
    signals, but not all: a pattern of period 4 at full scale carries
    weights of 57,865 into its second frame.
- **Decision:**
  - **The decoder holds a file to the format.** Besides what the reference
    refuses, it refuses, where the reference stops or reads on: a frame
    whose channels or sample rate differ from the first's; a frame whose
    samples, size or place is not the one the file's sample count gives it;
    a file shorter or longer than its frames; and a last slice whose unused
    residuals are not 0. The header's
    sample count gives the file's length, which is checked before anything
    is allocated.
  - **The API:** `qoa::header` reads and checks the header and the file's
    length; `qoa::decode_frame(bytes, &header, offset, &mut out)` decodes
    the frame at an offset into the caller's buffer, allocating nothing,
    and says how many samples it wrote and where the next frame is; it
    takes only the offsets where the file's frames start, so it checks
    each frame against its place; `qoa::decode` is `header` and a
    `decode_frame` for each frame; `qoa::encode(&Header, &[i16])` writes a
    file. `Header` (channels, sample rate, samples a channel), `Sound`,
    `Frame`, `FRAME_LEN`, `MAX_CHANNELS` and `MAX_SAMPLE_RATE` are public;
    `CodecError::Qoa` and `CodecError::QoaEncode` carry their errors.
  - **1 to 8 channels** on both sides, as the reference's encoder and
    decoder take; sample rates of 1 to 16,777,215 Hz.
  - **The encoder** is the reference's search, in the reference's 32-bit
    arithmetic, which wraps where C's `int` does on the machines it runs
    on. At each frame's start it goes on from the 16 bits of each weight
    the frame holds, as the decoder will, where the reference goes on from
    its own.
  - The tables are computed from the specification's formulas at compile
    time, but for the 16 scale factors, and a test checks all of them
    against the formulas in floating point.
  - The vectors are made by a port of the reference's encoder in Python,
    which shares no code with the crate: the Rust decoder must give the
    samples it reconstructed, and the Rust encoder its bytes. Each of seven
    signals keeps the SNR measured for it, to the dB.
- **Consequences:**
  - Good: a damaged or cut file is an error, not a shorter or noisier
    sound. A music voice keeps an offset into its `Arc<[u8]>` and needs no
    decoder state, and can seek to any frame by arithmetic. What the
    encoder measured is what decodes.
  - Bad: a file another encoder wrote with nonzero unused residuals, or
    frames larger than their slices, is refused, though the reference plays
    it; the game's QOA comes only from the cook (M6.6), so this matters only
    for files from elsewhere. Streams of unknown length are not read. The
    bytes were compared with a port written from `qoa.h`'s description,
    not with `qoa.h` itself, which was read but not compiled.
- **Alternatives considered:**
  - Decoding as leniently as the reference: a file cut short would play as
    a shorter sound, and the length check before allocating would go.
  - A streaming decoder that holds the file: it would borrow the bytes, or
    own an `Arc`, for state each frame already carries.
  - The reference's carry-over exactly: the same bytes where its weights
    outgrow 16 bits, but a file that decodes to other samples than it
    measured. Loud, regular signals reach the case (the vector
    `wide_weights`).
  - 255 channels, as the frame header's byte allows, or 31, as `qoa.h`'s
    comments say: neither the reference's encoder nor its decoder takes
    more than 8.
- **Revisit when:**
  - a game must read QOA that another encoder wrote, and a file the
    reference plays is refused;
  - `qoa.h` changes its format, its limits or its encoder's search;
  - or the mixer (M6.2) needs state that `decode_frame` does not give.

## ADR-0046 — M6.2: the mixer takes its sounds as `Arc`s until M6.6's assets; queues that never grow; a stolen voice fades out beside the new one; a track still sounding is brought back; gains move over 16 ms

- **Status:** Proposed; its `Arc`s in `play`, `play_at` and `play_music` give way to handles in M6.9 (ADR-0048), as ADR-0050 says, and the same track is the same file's bytes there, no longer the same `Arc`
- **Date:** 2026-11-08
- **Context:**
  - M6.2 asks for the mixer, offline: voices, buses, constant-power pan,
    positional attenuation, fades, the resampler, stealing, the limiter,
    music streamed from in-memory QOA, and the command and return queues,
    with every offline test of `docs/subsystems/audio.md` and no
    allocation in `render`.
  - The spec's API plays a `Handle<Sound>` and a `Handle<Music>`, but the
    assets that make them are M6.6's, with the cook's WAV to PCM16 and
    QOA, and the facade's components.
  - The spec's command queue is a `Mutex<VecDeque>` with capacity
    reserved, which the audio thread only `try_lock`s; the audio thread
    never frees, so what it is done with goes back to the main thread. It
    sizes the scratch buffers for the largest callback the device reports,
    ×2; smooths positions per callback; steals with a 5 ms fade; ramps
    every start and stop over 2 ms at least; and pans by `cos θ, sin θ`.
  - Three critics of the design, before it was built, found: a queue
    the main thread may grow grows without bound while nothing renders (no
    device, an unplugged one, offscreen runs), then floods the first
    callback; a return queue swapped for a new `Vec` either stops taking
    returns (capacity 0) or allocates every frame; volumes, positions and
    samples with no bounds carry NaN and infinities to the device, past
    what an `f32` limiter bounds; smoothing over one callback is a step on
    a callback of 64 frames; a crossfade back to a track still fading out
    restarted it; `√2 · (cos θ, sin θ)` lifts a stereo sound's near side
    by 3 dB, into the limiter.
- **Decision:**
  - **The API:** `nadir_audio::new(AudioConfig) -> (Audio, Mixer)`.
    `Audio`, for the main thread, has the spec's calls, taking
    `&Arc<SoundData>` and `&Arc<MusicData>` where the spec takes handles,
    and `stop_music`; `update`, once a frame, drops what the mixer
    returned and logs underruns, dropped commands and music that would not
    decode, once a second at most; `stats` gives the voices sounding,
    underruns, the last callback's time, the device's rate and the dropped
    commands. `Mixer::render(out, channels, rate)` and
    `Mixer::count_underrun` are the audio thread's. M6.6 adds the handles.
  - **The command queue never grows:** past `AudioConfig::commands`
    (1,024) a command is dropped on the main thread and counted; changes to
    a voice and the listener's moves are dropped once it is half full, to
    keep room for plays and stops. With the mixer dropped, nothing is
    queued.
  - **The audio thread neither allocates, frees, blocks nor logs.** It
    `try_lock`s both queues, a poisoned lock taken as it is, and leaves
    them for its next callback if it cannot. It locks the command queue
    only to take each command, so the main thread never waits for one to
    be carried out, and takes at most a queue's worth a callback. A
    command still queued when the mixer is dropped is dropped by the next
    `update`. What it is done with waits in
    a `Vec` of fixed capacity, `2 · (voices + 2) + commands`, the sounds
    the slots can hold and a full queue's worth turned away: a command
    that brings a sound waits, with those after it, while the sounds held
    and waiting would pass it. `update` swaps the return queue with a
    spare of the same capacity and drops outside the lock.
  - **Voices and stealing:** 48 slots. A new voice steals a voice
    fading out already, whatever its priority, or else the one of the
    lowest priority no higher than its own, the oldest of those; for a
    sound at its `max_instances` (0 counts as 1), only among that sound's
    voices. If there is none, it does not play. The stolen voice fades out
    over 5 ms in its slot while the new one plays from its start, so for
    5 ms a slot sounds two, and is not stolen from.
  - **Gains and fades:** a voice's gains move to a change of volume, pan
    or position over 16 ms, whatever the callback, not over one callback.
    Every start, stop and change of a bus's volume ramps linearly over
    2 ms at least, its length in samples rounded up; a stop of a voice
    already fading out keeps the sooner end. A sound that ends on its own plays to its last sample, as
    authored, with no fade: a ramp there would silence a sound shorter
    than two ramps, and the cook (M6.6) can end sounds at 0.
  - **Pan:** a mono sound by the constant-power law of the spec; a stereo
    sound by a balance law, `√2 · (cos θ, sin θ)` capped at 1, which keeps
    the centre and the near side at unity. Music is not panned.
  - **Music:** two slots, each with a buffer of a QOA frame and a sample
    carried from the frame before, decoded on the audio thread; tracks
    loop. A track sounding in either slot, fading or not, fades back in
    from where it is when played again; a new track takes a free slot, or
    the quieter one after a 2 ms fade. `stop_music` fades every track out
    within its fade. `MusicData::new` decodes every frame once, so a
    track cannot fail as it plays.
  - **Bounds:** samples within −1 … 1, rates of 1 to 384,000 Hz, at most
    2³¹ frames, so that a 32.32 position a step past the end fits;
    volumes clamped to 0 … 16, pitch to 1/16 … 16; a position that is not
    finite is silent, and a listener that is not finite ignored; the
    limiter is the signal to −1 dBFS and `u / (1 + u)` above, capped below
    1, and gives 0 for NaN.
  - **Mixing in parts of 512 frames,** whatever the callback, in place
    of scratch sized from the device's largest callback: a callback of
    any size allocates nothing.
- **Consequences:**
  - Good: the queues' memory is fixed at `new`; a device that stops for
    minutes costs neither memory nor a flood at its return. A moving
    sound's gains change smoothly from one frame's position to the next on
    any device. The device never gets NaN, an infinity or full scale.
    Crossing back and forth over an area's border does not restart its
    music.
  - Bad: the API changes again in M6.6, from `Arc`s to handles (the crate
    is not published, ADR-0036). A burst of more commands than the queue
    holds between two callbacks loses the rest, counted and logged. For
    5 ms after a steal, a slot sounds two voices, so a callback can mix up
    to twice `voices`. A fade's length is counted in samples at the rate
    when it starts, so a change of rate mid-fade stretches or shortens it.
    A sound whose data ends on a nonzero sample clicks at its end.
- **Alternatives considered:**
  - Handles in M6.2, as the spec writes: `nadir-assets`' `Sound` and
    `Music` types and their loaders are M6.6's.
  - A queue the main thread grows: unbounded while nothing renders.
  - A lock-free SPSC ring: the spec keeps it for when profiling shows the
    mutex contended, and it would need `unsafe` in `nadir-platform`.
  - Stealing into a slot only once the stolen voice is silent: 5 ms of
    delay, and a voice's slot held by one that never sounded.
  - Smoothing over one callback, as the spec says: a step on short
    callbacks, a 170 ms glide on an 8,192-frame one.
  - Constant power for stereo too: +3 dB on the near side.
  - A 2 ms fade at every natural end: a 1 ms sound would never be heard.
- **Revisit when:**
  - M6.6 makes the sound and music assets (then the API takes handles);
  - profiling shows the command mutex contended (then the SPSC ring);
  - a game drops commands in normal play (`AudioStats::dropped`);
  - or the soak (M6.7) shows a callback near its budget.

## ADR-0047 — M6.3: audio output is `open_audio`, a free function, which waits on ALSA 2 s at most; a device lost or absent is played on in time and tried every second

- **Status:** Proposed; its move of M6.3's acceptance to M6.6, and of
  M6.4's and M6.5's verification to the example, accepted by the user on
  2026-11-08
- **Date:** 2026-11-08
- **Context:**
  - `docs/subsystems/sys-and-platform.md` had `Platform::open_audio(&mut
    self, AudioConfig, Box<dyn AudioRender>)`, with only
    `render(out, channels, sample_rate)`, and `docs/03-platforms.md` a
    blocking `snd_pcm_open` (mode 0) and a loop of blocking
    `snd_pcm_writei`, with `snd_pcm_recover` on `-EPIPE` and `-ESTRPIPE`.
    M6.2's note asks for the underruns to reach the callback, since only
    the closure that owns nadir-audio's `Mixer` can call
    `Mixer::count_underrun` (ADR-0046).
  - alsa-lib 1.2.11's source, which the design critique read:
    - A blocking `snd_pcm_open` of a device that another program holds
      waits until it is free (`pcm.c`, `snd_pcm_open`'s mode).
    - A blocking `snd_pcm_writei` on a plugin, which `default` always is
      (PipeWire's, PulseAudio's, or `plug` over `dmix`), polls again after
      each timeout, for ever, when the sound server stops taking samples
      (`snd_pcm_write_areas`, `pcm.c:7691-7702`, and
      `snd_pcm_wait_nocheck`, `pcm.c:2928-3022`). A stop flag is then never
      read, and a drop that joins hangs the game's exit.
    - `snd_pcm_set_params` fails unless the device takes exactly the rate
      and channels asked for; `snd_pcm_recover` handles `-EINTR`, `-EPIPE`
      and `-ESTRPIPE`, and waits, a second at a time, for a suspended
      system to resume.
    - alsa-lib prints its own errors to stderr; `snd_lib_error_set_local`
      sets a handler for one thread, with a `va_list` that both Linux
      targets pass as a pointer, but is not in every libasound.
    - `SND_PCM_NONBLOCK` keeps the kernel's device and the writes from
      waiting, but not a plugin's own open: PipeWire's plugin waits for
      its server's answer with no time limit (`pw_thread_loop_wait`, the
      review found), and a file device on a FIFO with no reader waits too.
    - ALSA's `null` device, the usual way to silence ALSA, takes samples as
      fast as they come and never returns `-EAGAIN`: a loop paced only by
      the device renders thousands of times faster than real time.
  - Under PipeWire, unplugging a device moves the stream to another one,
    and ALSA reports nothing.
  - `nadir_audio::AudioConfig` already exists (ADR-0046), and the facade
    will name both. `PlatformError` is not `#[non_exhaustive]`; v0.3.0,
    the next release (ADR-0044), is not out yet.
  - M6.3's Accept, "sandbox audio plays", and its Verify, `cargo dev`,
    need the facade to open audio with the mixer, which is M6.6's Do, and
    M6.6 depends on M6.3. nadir-audio is not published (ADR-0036), so the
    published facade cannot depend on it before M6.6 makes it publishable.
- **Decision:**
  - **The API** (`nadir-platform`, every system):
    - `open_audio(&AudioOutputConfig, Box<dyn AudioRender>) ->
      Result<AudioStream, PlatformError>`, a free function: no backend
      needs the window, and a tool or a test opens audio without one, as
      `create_headless_gl` does GL.
    - `AudioOutputConfig { sample_rate, channels, latency_ms }`: 8,000 to
      384,000 (48,000), 1 to 8 (2), 5 to 500 ms (30), checked before any
      backend sees it, on every system (`InvalidAudioConfig`).
    - `AudioRender::render(out, channels, sample_rate)` fills a buffer of
      any length, which can change from call to call;
      `AudioRender::underrun()` hears of each underrun the system reports,
      before the next render. A render that panics ends the stream.
    - `open_audio` waits for the device to open 2 s at most, then starts
      the stream without it, as for a device that is not there.
    - `AudioStream` is `Send` and `Sync` (asserted at compile time), and
      has `latency() -> Option<Duration>`, the device's buffer, `None`
      while there is no device, and once the thread has ended, however.
      Its drop stops the thread and waits for it, and for the render's own
      drop, up to 2 s, then leaves it to end with the process.
    - `PlatformError` becomes `#[non_exhaustive]`, with
      `InvalidAudioConfig { field, expected }` and `AudioCallFailed {
      call, error }`: a break for a `match` on it with no `_` arm, which
      lands in 0.3.0. Windows and macOS return `Unsupported` until M6.4
      and M6.5.
  - **The Linux backend** (`crates/platform/src/linux/alsa.rs`): the audio
    thread opens the device with `SND_PCM_NONBLOCK`, and no ALSA call
    waits. It renders a period, writes what fits, and on `-EAGAIN` waits
    with `snd_pcm_wait` 20 ms at a time, reading the stop flag after each.
    `-EPIPE` is an underrun, then `snd_pcm_recover`, as are `-ESTRPIPE`
    and `-EINTR`. The device is lost on any error that `snd_pcm_recover`
    does not handle, on four errors in a row, or when it takes no frames
    for 2 s: the thread closes it, warns once, with ALSA's reason, and
    renders on in time with no device while it tries to open it again
    every second, with alsa-lib's messages silenced on that thread where
    `snd_lib_error_set_local` exists (and around `snd_pcm_recover`, which
    prints a failure to prepare whatever its `silent`); it logs once more
    when the device opens, and once when a try opens it but it refuses
    the format, until that changes. A device that does not open at the
    start, or not within the 2 s, is treated the same way; only a device
    that opens and refuses the format within them fails `open_audio`. A
    device that never makes the thread wait is held to the clock: the
    thread sleeps while it is more than a buffer ahead.
    `NADIR_ALSA_DEVICE`, set and not empty, names the ALSA device in place
    of `default`.
  - **One contract for M6.4 and M6.5** (`docs/03-platforms.md`): a render
    of any length, underruns as each system reports them, rendering in
    time with no device and a try every second, a panic ending the
    stream, an open that waits at most 2 s, a `Send` and `Sync` stream
    whose drop waits at most 2 s.
  - **M6.3's acceptance** (the user's decision, 2026-11-08): the
    platform's example, `cargo run -p nadir-platform --example audio`, is
    M6.3's check, by ear, with `NADIR_ALSA_DEVICE` naming a USB device for
    the unplug; "sandbox audio plays" and `cargo dev` moved to M6.6, whose
    Accept already asks for the sandbox's footsteps and music. The Accept
    of M6.4 and M6.5, "as M6.3", now means the example too, and their
    Verify is the example on Windows and on macOS, with a USB device
    unplugged and plugged in again, in place of `cargo dev`, which plays
    sound only once M6.6 has landed.
- **Consequences:**
  - Good: neither the start nor the exit of a game waits on ALSA more
    than 2 s; a device unplugged, busy, slow or not there yet is played on
    and found again, and the mixer's time goes on meanwhile, on a device
    with no clock too; one contract for the three systems; the tests run
    through libasound's `null` and `file` devices in CI, which installs
    `libasound2t64` on its Linux runner.
  - Bad: a device that has stopped is noticed after 2 s, while the mixer's
    time stands still; a stalled device wakes the thread every 20 ms; a
    suspend that never ends, or a plugin's open that hangs, leaves the
    thread running after its drop;
    the first open's failure still prints alsa-lib's lines, as do retries
    on a libasound without `snd_lib_error_set_local`; under PipeWire, the
    recovery runs only on a device opened past the server
    (`NADIR_ALSA_DEVICE`) or when the server itself stops; the thread
    asks for no real-time priority, as PipeWire's and PulseAudio's own
    threads have it and the 30 ms buffer covers ours.
- **Alternatives considered:**
  - `Platform::open_audio(&mut self, ...)`, as the spec had it: nothing
    about audio needs the window, and a soak tool or a test would need
    one.
  - Blocking writes, with a watchdog thread to unblock them: nothing
    unblocks a `snd_pcm_writei` that polls for ever but closing the PCM
    under it, from another thread.
  - Failing `open_audio` when the device does not open: the render, with
    the mixer, is gone with the thread, and the game has no sound for the
    session even once the device is plugged in.
  - Giving the render back in the error: every caller would have to
    retry, and each backend the same way.
  - The format's numbers as accessors (rate, channels, period, buffer):
    the rate and channels only echo the config, and the period and buffer
    go stale when the device opens again with others.
  - `AudioConfig` as the name: two types of that name in the facade.
  - `snd_lib_error_set_handler`: its handler is variadic, which stable
    Rust cannot define, and process-wide.
  - Falling back to `S16_LE` when `FLOAT_LE` is refused: `default`'s
    `plug` converts already, and a device that opens and refuses the
    format is an error the player can fix by choosing another.
- **Revisit when:**
  - a native PipeWire backend lands (the roadmap's backlog);
  - a game needs to choose its output device in its settings;
  - the soak (M6.7) shows underruns that the thread's scheduling causes
    (then real-time priority);
  - or M6.4 or M6.5 finds the contract cannot be kept.

## ADR-0048 — M6.6 split into three tasks: the cook's sounds (M6.8), the audio assets and handles (M6.9), the game's sound (M6.6); bus volumes in `Config` until the settings file

- **Status:** Accepted: the user chose the split on 2026-11-08; its
  revisit taken in M6.6, whose fourth package is nadir-cook's workspace
  test, test only, as the user chose on 2026-11-08 (ADR-0051)
- **Date:** 2026-11-08
- **Context:**
  - Roadmap task M6.6 asks for the cook's WAV to PCM16 or QOA, the
    `Sound` and `Music` assets, the `AudioEmitter` and `AudioListener`
    components, music crossfades and bus volumes from settings. A scoping
    pass found that it changes nadir-codec (`.kpcm`, which does not exist
    yet, and its fuzz target, which xtask registers), nadir-cook,
    nadir-audio (the assets, the handles in its API, and what crates.io
    needs of it), the facade, the sandbox (which has no sound yet) and
    xtask (the `.deb`'s `libasound2`): more than the three packages
    `CLAUDE.md` allows a task.
  - "Bus volumes from settings" needs the settings file, which is M9.3,
    two milestones later.
  - nadir-audio is off crates.io "until the facade uses it, from M6.6"
    (ADR-0036), and the published facade cannot depend on a crate that is
    not published (`cargo xtask crates`).
  - ADR-0038, ADR-0041 and ADR-0042 split M5.6, M5.7 and M5.8 the same way.
- **Decision:** run in this order, each at most three packages:
  1. **M6.8** (nadir-codec, nadir-cook, xtask): the `.kpcm` format and its
     reader and writer, with a fuzz target; the cook's WAV job, PCM16 or
     QOA by a `<stem>.sound.jsonc` sidecar; the cache and the pack.
  2. **M6.9** (nadir-audio): `Sound` and `Music` as nadir-assets assets,
     converted to the device's rate; `Audio::play`, `play_at` and
     `play_music` taking handles, as ADR-0046 foresaw; nadir-audio ready
     for crates.io, published from the next release, before the facade
     uses it, so that M6.6 stays within three packages.
  3. **M6.6** (the facade, the sandbox, xtask): the game opens audio and
     plays it, with the components, the music between areas, the
     sandbox's own sounds and the `.deb`'s `libasound2`. It keeps M6.6's
     number, its Accept, and a Verify of `cargo test -p nadir_engine -p
     sandbox` and `cargo dev`.
  - **Bus volumes** are a field of the facade's `Config` in M6.6; M9.3's
    settings file sets them.
  - New numbers rather than `M6.6a`, which `cargo xtask features` refuses;
    M6.8 and M6.9 go before M6.6 in the roadmap, and join the "Audio mixer"
    feature.
- **Consequences:**
  - Good: each task stays within three packages and has a check of its
    own; the cook's and the assets' parts land and are tested before the
    game uses them.
  - Bad: three tasks where there was one, and nadir-audio on crates.io a
    task before the facade uses it; the bus volumes are set in code until
    M9.3.
- **Alternatives considered:**
  - M6.6 whole: six or seven packages in one change.
  - Two tasks, the cook with the assets: four packages.
  - Publishing nadir-audio in M6.6: a fourth package there.
  - Bus volumes waiting for M9.3: the game would have no way to set them
    meanwhile.
- **Revisit when:** a task of the three finds it needs a fourth package.

## ADR-0049 — M6.8: the sound file `.kpcm`; sounds cooked below `audio/`, by folder or sidecar, resampled to 48 kHz by a deterministic windowed sinc in nadir-codec

- **Status:** Accepted: the user accepted it on 2026-11-08, with M6.8's
  Accept as it reworded it; after M6.9, the cook holds a sound, not music,
  to what the game's `Sound` holds, its samples within 256 MiB as `f32`,
  as cooked and as the `Sound` converts it, and reads it back within half
  the game's limits, as the `Sound` does, in place of PCM16 within the
  game's 256 MiB (ADR-0050)
- **Date:** 2026-11-08
- **Context:**
  - M6.8 (ADR-0048) asks for `.kpcm`, PCM16 sounds, with a reader, a writer
    and a fuzz target, and the cook's WAV job, "PCM16 or QOA by a sidecar,
    with the resample policy the spec names". The spec's row: "Resample
    policy from the sidecar; long or music → QOA, short SFX → PCM16". No
    layout of `.kpcm` existed. A level file names sounds by their cooked
    paths, `"music": "audio/field.qoa"`.
  - nadir-audio takes 1 or 2 channels at 1 to 384,000 Hz, for sounds and
    music alike (`SoundData`, `MusicData`); the game's assets read within
    `Limits::default()`, 256 MiB, as the pack's writer does.
  - The mixer streams music, compressed, and resamples it as it plays by
    linear interpolation: at 44.1 to 48 kHz, the design critique measured
    −6.3 dB at 20 kHz and aliases up to −15 dB. Sounds, M6.9 converts at
    load. A resampler in the cook alone would be out of M6.9's reach:
    nadir-audio may depend on nadir-codec, not on the cook.
  - The cook's output must be the same bytes on every system, and a
    system's `sin`, `exp` and `powi` may differ in their last bit.
  - Resampling a loop with zeros past its ends, and a rounded length,
    breaks its seam: −33 dB at each pass, measured.
  - `CookError` is not `#[non_exhaustive]`; `Counts` is (ADR-0038).
- **Decision:**
  - **`.kpcm`, version 1:** a 16-byte header, `KPCM`, version `u16`,
    channels `u8` (1 or 2), a reserved `u8` (0), the rate `u32` (1 to
    384,000), the frames `u32` (1 or more); then the samples, `i16`,
    interleaved; little-endian, exactly that long. Canonical and lossless;
    no loop points (a sound loops whole, per play); a loop point would be
    version 2 and a change of `kpcm::Header`. `kpcm::decode` checks the
    magic first, then the header, the version, each field, the limit, the
    length; `kpcm::encode` refuses what a file cannot hold. Vectors written
    apart, a fuzz target that asserts the file encodes back to its bytes.
  - **Sounds are the WAVs below `audio/`**, each a job `audio/<path>`, its
    output at its path: `.kpcm`, or `.qoa`. A WAV or a sound's sidecar
    elsewhere is listed, not read; a sound's sidecar with no WAV is
    `CookError::Config` (no new variant). The cook owns `OUT/audio/`.
  - **The sidecar** `<stem>.sound.jsonc`: `format` (`"pcm16"` or `"qoa"`),
    `rate` (8,000 to 384,000), `loop` (true or false), strict. **Defaults
    by folder, never by length:** below `audio/music/`, QOA and looped;
    elsewhere PCM16, not looped; the rate 48,000, the engine's output's.
    A sound of 10 s or more kept as PCM16 by default gets a warning.
  - **The resampler is nadir-codec's `pcm`**, for the cook and M6.9: a
    windowed sinc (32 zero crossings, Kaiser β = 9, −6 dB at 0.91 of the
    lower Nyquist frequency, tabulated), weights normalized, a looped sound
    resampled as periodic with its length exact, 0 past a one-shot's ends;
    rates of 1 to 384,000 Hz falling by 48 times at most, which bounds
    its reach; `f64` with `+ − × ÷ sqrt floor` only, `sin(πx)` a
    polynomial of its own and I₀ a running product, so the same bits on
    every system, which pinned hashes of its output and of its kernel's
    `f64` bits check on CI's three systems. `pcm::to_i16` rounds half away
    from 0 and clamps.
  - **Bounds:** a WAV read within 512 MiB as `f32`, which its reader
    checks before it allocates; then, before the resample or the encoder
    allocates, no samples refused, a rate above 384,000 Hz refused
    whatever the sidecar, the resampled output within `u32::MAX` frames
    and 512 MiB, and PCM16 within the game's 256 MiB. Each written file is
    read back with the game's reader and limits (QOA a frame at a time).
  - **The end:** the cook does not change a sound's last samples (a fade
    would break a loop's seam, and ADR-0046 left the end to the data); it
    warns of a one-shot whose last frame is above −48 dBFS, and of samples
    clipped past full scale.
  - **`Counts::Sound { encoding, frames, sample_rate }`** of the output,
    with `SoundEncoding { Pcm16, Qoa }`, `#[non_exhaustive]`.
- **Consequences:**
  - Good: an output's name follows only its path and sidecar, so code and
    levels can name it; music reaches the mixer at the device's usual rate,
    resampled once, well; a loop keeps its seam; the cook is the same bytes
    everywhere; M6.9 can convert sounds with the same code.
  - Bad: every WAV not at 48 kHz is resampled by default, a few tenths of a
    second for ten seconds of sound; a sound longer than 23 minutes is
    refused; a job of a large sound holds up to about 1.5 GiB; a one-shot
    that starts or ends loud and is resampled has its edges against zeros;
    nadir-codec holds a resampler, which is not a file format.
  - M6.8's Accept, which ADR-0048's split wrote as "each WAV of
    `assets/`", says a source's `audio/`, as only those WAVs are sounds.
- **Alternatives considered:**
  - The format by length (10 s or more, QOA): the output's extension, and
    so its asset path, would change when a sound crossed 10 s, and every
    reference would go missing at run time with no error from the cook.
  - Keeping the WAV's rate by default: music would play through the
    mixer's linear interpolation at every callback.
  - The resampler in nadir-cook: M6.9 could not use it.
  - A fade at every sound's end: it breaks loops, and the cook cannot tell
    which sounds loop without a sidecar.
  - A new `CookError` variant for a sound's orphan sidecar: a break, as
    `CookError` is not `#[non_exhaustive]`.
- **Revisit when:**
  - a game needs loop points, or sounds longer than 23 minutes;
  - M6.9's conversion at load needs another trade of quality for time;
  - or a system's CI gives another hash for the pinned resample.

## ADR-0050 — M6.9: `Sound` and `Music` assets in nadir-audio; `play` looks a handle up in the `AssetServer`, beside calls that take the asset; sounds load at 48 kHz; the same track is the same bytes; nadir-audio for crates.io

- **Status:** Accepted in part: on 2026-11-08 the user chose the server as
  a parameter with calls that take the asset, the fixed load rate of
  48 kHz, and nadir-assets' docs as a fourth package; the rest is Proposed
- **Date:** 2026-11-08
- **Context:**
  - M6.9 (ADR-0048) asks for `Sound` and `Music` as nadir-assets assets,
    "decoded on a worker and converted to the device's rate", for
    `Audio::play`, `play_at` and `play_music` taking `Handle<Sound>` and
    `Handle<Music>`, a failed sound as silence where placeholders are on,
    and nadir-audio ready for crates.io.
  - The spec's `play(&self, sound: &Handle<Sound>, ..)` cannot be built as
    written: `Audio` holds no assets, a `Handle` owns nothing, and the
    `AssetServer` is neither `Send` nor `Sync`. `Asset::finalize` and
    `release` get no handle, so `Audio` cannot keep a table of its own
    without a change to nadir-assets.
  - `Asset::decode`, on the worker, sees only the path and the `Limits`;
    the device's rate is `AudioStats::rate`, 0 until the first callback.
    Every backend opens at `AudioOutputConfig`'s rate, 48 kHz by default,
    or converts to it (docs/03-platforms.md), and the cook writes sounds at
    48 kHz unless their sidecars give a rate (ADR-0049).
  - The mixer told tracks apart by `Arc`: a level that unloads its scope
    and loads the same track again would crossfade it into itself.
  - A design critique measured a 2.8 KB `.kpcm` at 1 Hz that converts for
    about a minute into 256 MiB, and a QOA that `qoa::decode` takes at 8
    channels or 16.7 MHz.
- **Decision:**
  - **The assets are nadir-audio's:** `Sound` (`Arc<SoundData>`), from
    `.kpcm` or `.qoa`, and `Music` (`Arc<MusicData>`), from `.qoa`, each
    implementing `nadir_assets::Asset`; nadir-audio depends on nadir-assets,
    as the matrix allows. `Sound::new` and `Music::new` make one in code.
  - **The API:** `play(&self, assets: &AssetServer, sound: Handle<Sound>,
    params)`, `play_at(.., position, ..)` and `play_music(&self, assets,
    track: Handle<Music>, crossfade_ms)`, handles by value, as the renderer
    takes the server and its handles; each looks the handle up on the main
    thread and calls `play_sound`, `play_sound_at` or `play_track`, which
    take the asset. A handle with nothing under it, loading, failed with no
    placeholder or unloaded, plays nothing, returns a `VoiceId` that names
    no voice, and is counted in `AudioStats::unplayed`; a one-off play of
    a sound still loading is lost, so M6.6 loads what it plays at once.
    Unloading a scope does not stop the voices that play its sounds.
  - **Sounds load at `LOAD_RATE`, 48 kHz:** a sound from 8,000 to 384,000
    Hz but 48,000 whose last 2 ms (36 frames at least, the resampler's
    reach) are within −48 dBFS, as the cook asks of a sound that does not
    loop, is converted on the worker by `nadir_codec::pcm`, as such a
    sound; clamped to ±1, with a warning naming the file when it clips. The
    files do not say whether a sound loops, and a seamless loop converted
    as a one-shot breaks at its seam: by −31 dBFS for a 441 Hz tone at
    −6 dBFS from 22,050 Hz cut at a zero, and up to −6 dBFS for higher
    tones, measured (a one-shot converted as a loop clicks at −3 dBFS). So
    a sound that ends louder, which may be a loop, keeps its rate, as does
    any sound at another rate, and the mixer resamples it as it plays,
    wrapping a loop's seam. A unit test ties `LOAD_RATE` to
    `AudioOutputConfig::default()`. A game that opens another rate hears
    the mixer resample its sounds, as it does music. Music keeps its rate
    and streams. Converting takes a worker about 0.4 s for 10 s of 44.1 kHz
    mono optimized, 8 to 14 times that unoptimized, and up to about a
    minute for a file at the limits; through `load_now`, the caller's
    thread.
  - **Bounds before allocation:** QOA's header first (1 or 2 channels, 1
    to 384,000 Hz); a sound's samples decoded within half the `Limits`, so
    that their `f32` copy is held to them whole; the converted samples
    held to them by the resampler; a track's file held to them before it
    is copied. At the limits, a worker holds about 640 MiB at once (the
    file, the `f32` samples, the converted ones). The cook holds a sound,
    PCM16 or QOA, not music, to the same, restating the rule, as it may
    not depend on nadir-audio: its samples within 256 MiB as `f32`, as
    cooked, and as converted where the `Sound` would convert it, read back
    within half the limits; it refused a PCM16 sound only past the whole
    256 MiB until a task of its own after M6.9.
  - **The same track is the same bytes:** `MusicData` keeps the FNV-1a
    hash of its file, made on the worker, and the mixer compares it and
    the length, not the `Arc`, so the track loaded again goes on, and a
    reloaded track that changed crossfades. A sound's `max_instances` is
    still per `Arc`: per load.
  - **Placeholders:** a failed sound is a frame of silence, mono, 48 kHz;
    a failed track, a QOA frame of silence, so that playing it fades the
    music out; one of each per failure.
  - **crates.io:** nadir-audio leaves `publish = false` for the metadata of
    the others, a description and categories of its own (`game-development`,
    `multimedia::audio`), copies of `LICENSE` and `NOTICE`, and a README in
    ADR-0039's shape, whose examples `src/lib.rs` tests; xtask's crates test
    lists it. It goes out with the next release, after the maintainer's
    raise of the version from 0.3.0, with the token's `publish-new`, as
    trusted publishing cannot make a crate.
  - **Four packages:** nadir-audio; xtask (its crates test); nadir-bench
    (the calls that change); and nadir-assets' README and crate docs,
    which list the engine's asset types, docs only, which M6.6's three
    could not take (ADR-0048's revisit).
- **Consequences:**
  - Good: games play sounds by handle, as the spec writes, with the
    server they hold already; a sound made in code still plays; the bench
    and the property test keep their sources at odd rates; most sounds
    reach the mixer at the output's rate, resampled once, well; a level
    change keeps its music going; a bad file is refused before it costs
    memory or time.
  - Bad: every play takes the server; the spec's `&Handle` changes; the
    roadmap's "the device's rate" becomes the output's configured default,
    so a game at another rate gets linear interpolation for its sounds; a
    sound cooked at another rate that ends loud keeps the mixer's linear
    interpolation; a sound takes 4 bytes a sample in memory, 66 MiB for
    three minutes of stereo, against 6.9 MB as music; a long sound at
    another rate takes seconds of a worker, up to about a minute at the
    limits, and the caller's thread through `load_now`; the cook restates
    the `Sound`'s bounds and its rule for converting, so a change to either
    is a change to the cook's `sound.rs` too.
- **Alternatives considered:**
  - A table of handles to `Arc`s in `Audio`, filled at finalize: finalize
    and release get no handle, so it needs nadir-assets to change.
  - `play(&Sound)` alone: does not take handles, as the task asks.
  - The device's rate through a new `DecodeCtx` setting: a change to the
    API of nadir-assets, on crates.io, for the rate the config gives
    anyway.
  - A process-wide rate: global state, shared by tests and by two
    `Audio`s.
  - Converting in `finalize`, on the main thread: 0.4 s for 10 s of sound,
    against a 2 ms budget.
  - Converting at a sound's first play, on a thread of `Audio`'s: a
    thread, a cache across threads, and two `Arc`s a sound.
  - Looped below `audio/music/`, as the cook's default: the sounds it
    would touch are those with a sidecar, which says what it says.
  - Every sound converted as a one-shot: a review measured seamless loops
    at 22,050 Hz clicking at their seams, by up to −11 dBFS.
  - A loud last frame as the sign of a loop, alone: a loop cut at a zero
    ends at 0, and broke at its seam by −31 dBFS.
- **Revisit when:**
  - a game opens its output at another rate than 48 kHz (then `DecodeCtx`
    gains a setting, in nadir-assets);
  - `.kpcm` gains a loop flag, in its reserved byte (then the conversion
    keeps a loop's seam);
  - crates.io's trusted publishing replaces the token (nadir-audio must
    exist there first).

## ADR-0051 — M6.6: audio opens in `App::run` alone; `nadir::sound` plays the listener, the emitters and the music areas each frame, from components not reflected; bus volumes in percent, heard squared; a level's own music waits; sounds of our own

- **Status:** Accepted in part: on 2026-11-08 the user chose nadir-cook's
  workspace test as a fourth package (ADR-0048's revisit), the percent
  volumes with the squared curve, the camera as the default listener, and
  48 kHz sources of about 1.9 MB; the rest is Proposed
- **Date:** 2026-11-08
- **Context:**
  - M6.6 (ADR-0048) asks the `App` to open audio with the mixer and call
    `Audio::update` once a frame; the `AudioEmitter` and `AudioListener`
    components and their systems; music that crossfades between areas;
    bus volumes in `Config`; the sandbox's footsteps and its areas' music,
    sounds of our own; and `libasound2` for the sandbox's `.deb`. Its
    Accept: sandbox audio plays; footsteps follow the player positionally;
    music crossfades between areas.
  - `open_audio` (ADR-0047) takes the render box whether or not it opens:
    `Unsupported` on Windows and macOS until M6.4 and M6.5, and on Linux
    `LibraryNotFound` or `MissingSymbols` without libasound. A missing
    device is no error, and dropping the stream waits 2 s at most.
  - Tests, benchmarks and `--headless` replays run `App::update` faster
    than real time; `--capture` runs `App::offscreen` in the release and
    demo page workflows, which install no libasound. A mixer that nothing
    renders fills its queue, and every command after is dropped, counted,
    and warned of once a second.
  - The queue drops a voice's changes and the listener's moves once it is
    half full, as the next frame sends them again, and anything once it is
    full (ADR-0046). The mixer tells the main thread nothing of a voice
    stolen, refused or ended, and a voice past `max_dist`, 800 px, is
    silent but holds one of its 48 slots. A play of a sound still loading
    is lost, and unloading a scope stops no voice (ADR-0050).
  - `Handle<T>` has no `Default`; `PlayParams` and `Bus` are not
    `Reflect`, as nadir-audio may not depend on nadir-scene; and each
    reflected type a game registers changes its world hash.
  - `Config` derives `Eq` and has public fields, and is not
    `#[non_exhaustive]`, so a field more breaks it, as ADR-0043 noted for
    `Sprite` and `Config`. The settings file that will set the volumes is
    M9.3's.
  - A level file has optional `music` and `ambience` paths (M5.10), but
    the cook writes neither, and drops a level's own LDtk fields with a
    warning (ADR-0038).
  - `tools/cook/tests/cook/workspace.rs` cooks the real `assets/`, and
    pinned its four jobs, the level job at index 3 and a pack of 6 files:
    the sandbox's sounds fail it, and it is nadir-cook's, a fourth package.
  - The sandbox had no sounds, and sounds not ours would need their
    licences beside them (ADR-0037).
- **Decision:**
  - **Audio opens in `App::run` alone.** `App::new` makes the mixer of
    `AudioConfig::default()`, which is valid, inserts `Audio`, always
    there, and the resource `AudioState`, queues each bus's volume, and
    keeps the `Mixer` in a private field. `App::run` opens the default
    output, 48 kHz stereo, once the window and the device are open, with
    the mixer in a private `MixerOutput`, an `AudioRender` whose
    `underrun` is `Mixer::count_underrun`. Where it does not open, a
    warning says why and the game runs silent; with no mixer left, the log
    says so. The stream drops as soon as the loop ends, before an error is
    shown; `Error` is unchanged.
    - **Every other path is silent:** `App::update`'s first call drops a
      mixer the app still holds, so tests, benchmarks, `--headless`
      replays, `App::offscreen` and `--capture` open no device, and
      `Audio` does nothing there. `App::take_mixer`, new, hands the mixer
      before then to a test or a tool that renders it itself.
  - **`nadir::sound`**, an engine frame system, first in `Extract`: after
    the game's camera, before `nadir::extract_sprites`. In order:
    1. **The listener:** where the first entity with an `AudioListener`
       and a `Transform` is drawn, or else the camera's position,
       `RenderFrame::camera.position` (the user's choice), sent when it
       is not where the queue last took it.
    2. **The emitters:** each `AudioEmitter` with a `Transform` starts
       once its sound has loaded and it is within `max_dist` of the
       listener, and its position is sent when it moves; it stops, fading
       over 100 ms, past `max_dist + 64`, or when its entity is despawned
       or loses the emitter or its `Transform`, and plays again back in
       range. A new `sound`, `bus`, `looped`, `priority` or
       `max_instances` stops the voice and starts another; a new `volume`
       or `pitch`, compared bit for bit, goes to the voice. A sound that
       failed, or that the asset server does not hold, is warned of once,
       naming the entity and the path, and not tried again until the
       emitter changes; one still loading waits.
    3. **The music areas:** a `MusicArea` is a box, its entity's position
       ± `half`. The area whose track was sent last stays while it holds
       the listener, so overlaps do not flap; else the first in the
       storage's order that holds it sends its track with `play_track`,
       over its `crossfade_ms`. Outside every area the music plays on, and
       no area is current, so coming back sends the track again, which the
       mixer carries on (ADR-0046). A track still loading waits; one that
       failed is warned of once, as a sound is, and an area playing whose
       music changes to it plays on as it was.
    4. `Audio::update`.
    - One position for all three: `PrevTransform.lerp(Transform, alpha)`
      where there is a `PrevTransform`, else `Transform`, as sprites are
      drawn.
    - Each play, stop, move and change of a voice, each move of the
      listener and each track is checked against `AudioStats::dropped`
      around its send: one the queue dropped is not recorded, and the
      next frame sends it again, so there is no dead voice, no voice twice
      and no stale position, and a frame where nothing moves sends
      nothing (the review measured 560 emitters, sent every frame, freeze
      the live voices at their start; more than about 500 moving in one
      frame still lose the same tail).
    - Each emitter costs O(1) a frame: a table of those it started, and a
      private marker component, not reflected, on their entities. Once
      they have room for the emitters, a frame allocates nothing.
    - It writes no reflected component and spawns nothing, so world hashes
      and replays are as they were.
  - **The components** are the facade's: `AudioListener`, a unit struct;
    `AudioEmitter { sound, params }`, with `new` and `looped`, a looped
    sound on `Bus::Ambience` at priority 192 with no limit to its voices,
    so that the sound's limit does not make emitters of one sound take
    one another's voices, and default plays, at 128, take none of theirs
    (when all the voices play, one of the same priority or higher still
    takes the oldest); and `MusicArea { music, half,
    crossfade_ms }`, with `new`, 1,500 ms; the last two are
    `#[non_exhaustive]`. All three are registered with the world and **not
    reflected**: a game gives them in code, not in prefabs or levels; a
    task that wants them in data reflects them by hand, as `Sprite` is.
    `AudioState::music_area()` names the area whose track was sent last.
  - **`Config::volumes: Volumes`**, a percent per bus (`u8`: music, sfx,
    ui, ambience), 100 each by default, more counting as 100. A percent is
    heard as its square (the user's choice): `Volumes::gain(bus)` is
    (percent / 100)², so 50 is −12 dB, nearer half as loud than half the
    gain's −6 dB; `get` and `set` serve menus that go through `Bus::ALL`.
    `App::new` applies them; a game changes one while it runs with
    `Audio::set_bus_volume(bus, volumes.gain(bus), fade)`. `Config` keeps
    `Eq`. **Breaking:** a `Config` literal without `..` no longer
    compiles; the release after 0.3.0 raises the version (ADR-0050). No
    master volume: the mixer has none, and M9.3 decides the settings.
  - **A level's own `music` and `ambience` wait,** with its camera
    bounds, for the roadmap's backlog: the cook writes none of them, which
    would be a fourth package more, and the task's music comes from areas.
    `load_level` neither plays nor stops them. The task that builds them
    decides how a level's music and its areas' share the music: the
    level's outside every area, or an area's over the level's.
  - **Sounds of our own:** `games/sandbox/make_sounds.py`, Python's
    standard library alone and seeded, writes the sandbox's four WAVs,
    mono at 48 kHz, so the cook resamples none of them: a footstep on
    grass, 80 ms; a pond's lapping water, 4 s, looped by its sidecar; and
    two tunes of 8 s, the north's bright and the south's darker and
    slower: about 1.9 MB in all (the user's choice). The WAVs are
    committed, and made again by hand.
  - **The sandbox:** `footsteps`, a tick system in `Cleanup` in every
    scene, plays a step at the player's feet every 12 ticks it moves, the
    first at once, at volume 0.6, its pitch 1 and 0.94 in turn. The field
    spawns, after its level and its player, two `MusicArea`s, the north
    (y 0 to 576) and the south (576 to 768), and the pond's looped emitter
    at (560, 112), at volume 0.5.
  - **The `.deb`:** the sandbox's depends on `libasound2` too, which it
    loads with `dlopen`, where `dpkg-shlibdeps` cannot see it; the release
    texts say the pack holds sounds, and that Windows and macOS play none
    until M6.4 and M6.5 (`docs/05-release.md`).
  - **The field's world hash changes:** its three new entities have a
    `Transform`, `298521738b5650ef` → `09b32c266f3b59f4` over the
    capture, its 600 frames the same bytes (SHA-256 `6a8ab445…`).
  - **Four packages:** the facade, the sandbox, xtask, and nadir-cook's
    workspace test, test only: its eight jobs, the level job found by
    name, the sounds' outputs, a pack of 10 files. No cook code changes.
- **Consequences:**
  - Good: tests, recordings and CI open no device and need no libasound,
    and nothing plays sped up; `Audio` is always a resource, so any system
    can play; a command the queue dropped is sent again; emitters out of
    range free their voices, and a stolen one comes back when it starts
    again; a game sets volumes in one unit and one curve, which M9.2's
    sliders and M9.3's settings can share; the sounds are ours, under the
    repository's licence.
  - Bad: a looped emitter whose voice is stolen is silent until it leaves
    range and comes back, or changes; a sound played once, as something
    happens, is lost if it has not loaded, so a game loads it first;
    `App::run` opens the output after the window, so a device slow to
    open holds the first frame up to 2 s; the components cannot be given
    in prefabs or levels; a `Config` literal without `..` breaks; a
    level's `music` and `ambience` are not played; with `dev`'s
    placeholders a failed sound plays as silence, so the emitter does not
    warn, and the asset server's log says why; while an area's track
    loads, `music_area()` still names the area before; the field's world
    hash changes.
  - The Accept is shown offline, through the mixer, by the facade's and
    the sandbox's tests; how it sounds only the user's check by ear shows
    (`docs/subsystems/audio.md`), so the task stays unticked until the
    user reports it, as M6.3 did.
  - The cook's QOA encoder starts each file with an empty history, so a
    looped track that starts loud errs on its first 20 samples at every
    pass: the tunes start at a zero crossing.
- **Alternatives considered:**
  - Audio opened in `App::new`: tests, captures and replays would open
    the device, the workflows' runners would need libasound, and a
    headless replay would play sped up.
  - The mixer kept in runs of `App::update` alone: nothing renders it, so
    its queue fills and `update` warns every second.
  - `NADIR_ALSA_DEVICE=null` in tests: Linux's alone, and a variable each
    test must set.
  - Positions sent only when they change: one that the half-full queue
    dropped would not be sent again.
  - The listener on the player: the player's own steps would always be
    centred.
  - Distant emitters left playing: each holds a slot, silent.
  - The components reflected: `Handle` has no `Default`, nadir-audio's
    types are not `Reflect`, and every game's world hash would change.
  - `f32` volumes in `Config`: they drop `Eq`, a break of more than
    literals; a linear percent: 50 is −6 dB, and saved settings would fix
    whatever curve came first.
  - A level's music played by `load_level` now: nothing writes it, and
    its share of the music with areas would be fixed before a game needs
    it. Areas as levels, each with its `music`: the cook writes none.
    Areas as prefabs of a trigger: LDtk cannot give one its size.
  - Sources at 22,050 Hz, about 1 MB: the cook would resample them,
    rounding a loop's seam, and its debug tests would take longer.
  - `Recommends: libasound2`, as the game runs silent without it: the
    other libraries it loads are `Depends`, as the roadmap asks of this
    one.
- **Revisit when:**
  - M6.4 or M6.5 opens output on Windows or macOS (`App::run` needs no
    change);
  - a game needs a level's own music, ambience or camera bounds (the
    roadmap's backlog), or emitters, listeners or areas in prefabs and
    levels;
  - a stolen emitter's silence is heard in a game (then the mixer tells
    the main thread of its voices);
  - M9.3's settings file sets `Config::volumes`, or a game asks for a
    master volume;
  - or the cook primes a looped track's first QOA frame with its last
    samples, so that a tune need not start at a zero crossing.
