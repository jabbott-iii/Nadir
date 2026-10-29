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

- **Status:** Accepted
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

- **Status:** Accepted
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

- **Status:** Accepted
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

- **Status:** Accepted
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
