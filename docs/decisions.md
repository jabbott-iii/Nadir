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

- **Status:** Accepted
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
