# 04 — Quality: testing, checks, CI

Because we write our own bindings, codecs and graphics backends, testing has
to do the job that "battle-tested crate" would otherwise do. Every harness
below is our own code (dependency policy items 26–28).

## Test layers

| Layer | Where | Runs in | What it catches |
| :- | :- | :- | :- |
| Unit tests | `#[cfg(test)]` beside code | `cargo test` on all OSes | Logic errors in math, codecs, ECS, collision, mixer |
| Layout tests | `nadir-sys` tests | `cargo test` on each OS | FFI struct size/alignment/offset mismatches |
| Property tests | `nadir-core::testing::check` | `cargo test` | Edge cases: collision never leaves overlap, codecs round-trip, sort keys monotonic |
| Integration tests | `crates/*/tests/` | `cargo test` | Cross-module behavior (asset load → GPU upload with the headless device) |
| Golden-image tests | `xtask golden`; scenes in the GL test binaries, references in `tests/golden/` | Linux CI (llvmpipe); Windows and macOS locally before release | Rendering regressions, backend divergence |
| Determinism tests | `nadir/tests/determinism.rs` | `cargo test` | Replays producing different world state |
| Fuzz smoke | `xtask fuzz-smoke` | CI (time-boxed) | Panics, hangs, huge allocations in parsers |
| Soak tests | `xtask soak` (manual or nightly) | Real hardware | Audio underruns, memory growth, handle leaks |
| Benchmarks | `nadir-bench` | Nightly or manual | Performance regressions versus the stored baseline |

## Own harnesses

### Property testing — `nadir_core::testing`

```rust
/// Runs `property` on `cases` inputs generated from a seeded PCG32. On
/// failure, panics with the seed and case index so the failure can be
/// replayed exactly.
pub fn check<T: std::fmt::Debug>(
    name: &str,
    cases: u32,
    generate: impl Fn(&mut Pcg32) -> T,
    property: impl Fn(&T) -> Result<(), String>,
);

/// The same, for inputs with a natural size. `generate` also receives a
/// size from 0 to `MAX_SIZE` (100), which grows over the run.
pub fn check_sized<T: std::fmt::Debug>(
    name: &str,
    cases: u32,
    generate: impl Fn(&mut Pcg32, u32) -> T,
    property: impl Fn(&T) -> Result<(), String>,
);
```

- The default seed comes from `NADIR_SEED`, or from a fixed value in CI so
  runs are reproducible.
- `NADIR_EXPECT_GL_RENDERER`, when set and not empty, makes the headless
  OpenGL tests of `nadir-platform`, `nadir-gfx`, `nadir-render` and the
  facade fail unless the renderer's name contains it (`llvmpipe` in CI).
  Otherwise they log the renderer and take any. Where `nadir-platform` has
  no offscreen context yet (Windows and macOS, until M2.6 and M2.7), the
  gfx and render tests skip, and the facade's steady-frame test runs
  without drawing, but only while the variable is unset.
  - The facade's test is behind its `dev` feature, not `headless-test`, so
    `cargo test -p nadir --features dev` on Linux needs EGL and an OpenGL
    3.3 driver.
- A nightly job runs with random seeds and prints the seeds it used.
- Shrinking is deliberately minimal, and only `check_sized` does it: the
  harness retries a failing case at every smaller size and reports the
  smallest size that still fails.
- A property or generator that panics is reported like any other failure,
  with its seed.

### Golden-image tests

- A scene is a test in the OpenGL test binary of the crate that draws it
  (ADR-0017). It draws into an offscreen render target of fixed size, e.g.
  320×180, and checks a few of its pixels.
  - The `gfx` scenes, from M2.4, draw through the `gfx` API, in
    `crates/gfx/tests/headless/golden.rs`: clear, textured quad (nearest,
    linear, and repeat through an indexed draw), alpha blend (all four blend
    modes), 1000 instanced quads, scissor and viewport, and the render
    target round trip.
  - The `render` scenes, from M3.3, draw through the `render` API, in
    `crates/render/tests/headless/golden.rs`:
    - `y_sort`: a person behind a tree and one in front, a flipped sprite, a
      ground decal, and a missing texture's placeholder
    - the upscale of a 64×36 image, with a dot in each corner, at three
      output sizes: `upscale_320x180` (an exact fit),
      `upscale_300x200` (bars all round) and `upscale_333x147` (bars of
      uneven widths)
    - `tile_layers` (M4.5): a ground layer across a chunk edge, with
      flipped tiles and animated water at time 0, a decal on it, a tall
      tile between two people, a roof layer over a third, and a failed
      tile set's placeholder
    - `animated_tiles_0ms` and `animated_tiles_350ms` (M4.5): four
      animated or flipped tiles at two fixed times, one of them wrapping
      past the tile set's last tile

    Each sprite is drawn at a whole number of pixels per texel, with its
    corner on a whole pixel, so that every GPU draws it alike.
  - A scene is defined with the `scene!` macro, which names it after its
    test, and saves its pixels before its own checks can fail it.
- Readback uses `glReadPixels` or Metal `getBytes`, flipped to a top-left
  origin.
- `cargo xtask golden` empties `target/golden/`, then runs the scene tests
  with `NADIR_GOLDEN_DIR` set to `target/golden/rendered/`. Each scene
  writes its pixels there as `<scene>.pam` (Netpbm PAM, RGBA8), with the
  renderer in a `# renderer` comment. Without the variable, as under
  `cargo xtask ci`, the scenes draw and check but write nothing.
- xtask compares each scene with `tests/golden/<scene>.qoi`, which is
  encoded with our own QOI encoder.
  - References are generated with the GL backend, on llvmpipe, as CI draws
    them. `--bless` refuses any other renderer.
  - `--backend metal` is to render with Metal and compare against the same
    references, from M10.4. Cross-backend parity is the point. Until then
    the command takes only `--bless`.
  - A scene may carry a per-backend override (`<scene>.metal.qoi`) only with
    a recorded reason. Until M10.4 such a file is listed and skipped. Any
    other name with a dot, such as a copied `<scene>.actual.qoi`, fails the
    run.
  - Every scene of a run must come from one renderer, and from the one
    `NADIR_EXPECT_GL_RENDERER` names, if it is set.
- Tolerance: per-channel difference ≤ 2 (alpha included), and at most 0.1%
  of pixels may exceed it: 57 of the 57 600 in a 320×180 scene.
- A scene fails when it has no reference, its reference has another size
  or cannot be decoded, or it is over the tolerance. A reference that no
  scene drew fails too: its scene is gone, its test failed before it saved
  (on a GL error, say), or the tests skipped for want of an offscreen
  context.
- On failure, write `target/golden/<scene>.actual.qoi`, and for a scene
  over the tolerance `<scene>.diff.qoi`: magenta where a pixel is over,
  and elsewhere the reference in dim gray, opaque throughout.
- If the scene tests fail, xtask still compares what they drew and writes
  the images, then fails. A scene whose own pixel checks fail, or whose
  renderer is not the one asked for, has saved its pixels first. One that
  stops earlier, on a GL error, has saved nothing, and its reference is
  reported as not drawn.
- `cargo xtask golden --bless` writes the reference of every scene that
  fails, and keeps those within tolerance byte for byte, so blessing
  on another machine does not churn them. It blesses nothing when the scene
  tests fail, and it cannot mend a reference without a scene: remove that
  file by hand. Run it only after a human has looked at the images.
- The exit status is 0 when every scene matches (or, with `--bless`, now
  does), 1 otherwise, and 2 for a command line not understood.
- Commands, where libglvnd also has NVIDIA's EGL:

  ```text
  __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 NADIR_EXPECT_GL_RENDERER=llvmpipe cargo xtask golden [--bless]

  __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/10_nvidia.json \
  NADIR_EXPECT_GL_RENDERER=NVIDIA cargo xtask golden
  ```

  Without the first variable, `LIBGL_ALWAYS_SOFTWARE=1` does not choose
  llvmpipe there: NVIDIA takes the surfaceless platform.

### Determinism

- A recording is an initial seed plus the per-tick `ActionState` stream.
- The test runs the sandbox headless (no window; render extract skipped) for
  N ticks.
- Each tick, it hashes the serialized world (reflection → binary → FNV-1a 64)
  and compares the hash sequence against a stored one.
- Scope: same build, same OS. Cross-OS float determinism is not a goal
  (single-player).

### Fuzz smoke (stable Rust, no dependencies)

- `xtask fuzz-smoke --secs 60` loads `fuzz/corpus/<parser>/` seeds.
  - `--secs N` is the time for **each** target (default 60).
    `--target NAME` runs one target instead of all of them.
  - Every seed runs unchanged first, even with `--secs 0`. After that each
    input is a random seed with 1 to 16 random mutations.
- Mutations: bit flips, byte insert/delete, splice, length-field tweaks,
  "interesting" integers (0, 1, 255, 65535, `u32::MAX`).
- Each mutated input goes to the parser inside `catch_unwind`, with a
  watchdog for hangs and the allocation counter for blow-ups.
  - A panic does not stop the run. Panics raised at the same source
    location count as one finding.
  - A hang is an input the target does not finish within 10 s. It ends the
    whole run, because the stuck thread cannot be stopped.
  - The allocation counter is not wired in. It lives in `nadir-platform`
    (M3.5), which `xtask` may not depend on (the dependency matrix).
    Wiring it needs a decision: run the targets in a test binary that links
    `nadir-platform`, as the goldens run in GL test binaries (ADR-0017), or
    a new matrix edge.
  - A stack overflow or an aborted allocation kills the fuzzer, and the
    input is then not saved.
- Crashes are saved to `fuzz/crashes/`, which Git ignores, as
  `<target>/panic-<hash>.bin` or `<target>/hang-<hash>.bin`. Only the first
  input of each finding is saved.
  - To see a finding again, or to keep it as a regression test, copy the
    file into `fuzz/corpus/<target>/`.
- The run is reproducible: the seed is fixed unless `NADIR_SEED` is set,
  as for property tests, and it is printed. How far a run gets in its time
  still depends on the machine.
- The exit status is 0 for a clean run and 1 if there is a finding.
- Every parser in `nadir-codec`, `nadir-assets`, the save loader and the
  settings loader registers a fuzz target.
- The optional coverage-guided fuzzing (`cargo fuzz`) is a separate,
  **unapproved** proposal (policy item 29).

### Allocation counter (dev)

- `nadir-platform` provides a `#[global_allocator]` wrapper around `System`
  under feature `dev` (`crates/platform/src/alloc_counter.rs`).
  - It counts allocations (reallocations included, as that is how a `Vec`
    grows), deallocations and the bytes asked for, per thread
    (`thread_alloc_counts`) and for the process (`process_alloc_counts`).
  - It sees only Rust's global allocator: C libraries that call `malloc`
    themselves (Mesa, Xlib, EGL) are not counted.
  - Every binary that links `nadir-platform` with `dev` allocates through
    it, so no other crate may declare a `#[global_allocator]`. Turn `dev` on
    only from a `[features]` table or a dev-dependency, never from a normal
    dependency, so that no release build carries it.
- The facade re-exports it under its own `dev`, and keeps the main thread's
  count for the last frame in the resource `FrameAllocs`: from the end of
  the frame before to the end of this one. The sandbox reports it once a
  second.
- The debug overlay shows allocations per frame.
- Tests assert **0 allocations** in steady state:
  - the facade's unit test `steady_frames` in `crates/engine/src/app.rs`
    draws 10,000 sprite entities of its own through the real loop,
    offscreen, for 100 frames (`cargo test -p nadir --features dev`), and
    checks `FrameAllocs` against its own count frame by frame. Since M4.5
    the ground is a tile layer of 64 × 64 cells, a quarter of them
    animated, whose view moves in the warm-up to where four chunks meet:
    the counted frames draw 4 chunks;
  - the facade's unit test `steady_ticks` in `crates/engine/src/glue.rs`
    runs 5,000 colliding entities, a tenth of them triggers, with 10
    entities spawned and 10 despawned through commands each tick. It runs
    three identical passes of 28 ticks, frames of two ticks among them,
    and counts the third: movement, triggers and their events, commands
    and drawing (M4.4);
  - the sandbox's stress scene, `--scene stress-sprites`, runs 100 frames
    through `App::update`, without a GPU.
- The sandbox's `--scene stress-entities` (5,000 colliding entities)
  reports its allocations once a second. As its crowd jams, its buffers
  grow now and then to a new peak, then stay.
- `App::update` drains the scope records of nadir-core's `profile`
  feature each frame, into buffers with room for 1,024 records, so a build
  with it allocates nothing for them in steady state, even in a frame of
  many ticks (`a_frame_of_many_ticks_after_frames_of_one_allocates_nothing`
  in `crates/engine/src/app.rs`, which `cargo xtask ci` runs with the
  feature on).
- `crates/engine/tests/allocations.rs` holds checks of code outside the
  loop, such as a QOI file too short for its size being refused before
  anything is allocated.

### Benchmarks — `nadir-bench`

- Run with `cargo xtask bench [filter]`, which builds in release mode and
  runs the benchmarks whose name contains the filter.
- Each benchmark: warm-up for 0.5 s, then 50 samples of an auto-scaled
  iteration count. Report median and p95.
  - The iteration count is scaled so that one sample takes about 10 ms.
  - The times are per iteration. p95 is the 48th of the 50 samples in
    ascending order.
- Results go to `target/bench/latest.json` and are compared against
  `bench/baseline.json`. A slowdown above 10% is flagged; it is not a failure.
  - Medians are compared. A benchmark without a baseline entry is reported
    as such.
  - To record a baseline, copy `latest.json` to `bench/baseline.json`
    after a run of every benchmark: a filtered run writes only what it ran.
    The numbers only mean something on the machine that recorded them.
- Required benchmarks:
  - sprite sort + batch for 10k sprites (`sprite_sort_batch`, M3.5)
  - tilemap chunk rebuild (`tile_chunk_rebuild`, M4.5: packing a full chunk
    of 1,024 tiles)
  - ECS join over 5k entities (`ecs_join2_5k`, `ecs_join3_5k`, with
    `ecs_spawn_despawn` for churn, M4.1)
  - spatial hash rebuild and query (`collision_hash_5k`, M4.3, with
    `collision_slide_5k` for a tick of movement, ADR-0021)
  - a whole tick of the facade for 5,000 colliding entities
    (`facade_tick_5k`, M4.4, ADR-0022)
  - A* on 256×256
  - mixer with 32 voices
  - QOI decode 2048²
  - inflate 10 MB

## `xtask` checks

| Check | Implementation |
| :- | :- |
| `fmt` | `cargo fmt --all -- --check` |
| `clippy` | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| `test` | `cargo test --workspace --all-features` |
| `deps` | `cargo metadata --format-version 1`, parsed with `nadir-codec::json`; every package with a non-null `source` must appear in `xtask/deps-allowlist.txt` with its exact version. A path package from outside the workspace also fails (vendored code) |
| `layers` | Same metadata; each workspace crate's dependencies (normal, dev and build) must be a subset of its row in the dependency matrix, and a workspace crate without a row fails. The matrix is a `const` table in `xtask` that mirrors `docs/01-architecture.md`; a unit test compares the two |
| `unsafe-audit` | Scans `*.rs`, ignoring comments and string literals: no `unsafe` outside `sys`/`platform`/`gfx`; `allow`, `expect` or `warn(unsafe_code)` only in those three crate roots; every `unsafe {` / `unsafe impl` preceded by a `// SAFETY:` comment that ends at most 3 lines above it; prints a per-crate `unsafe` count (reviewed if it grows) |
| `release-no-dev` | Runs `cargo tree -p sandbox -e normal --prefix none` without the `dev` feature and fails if `nadir-debug` appears |
| `ci` | All of the above in order; stops at the first failure and prints a summary table |

Each check is also a subcommand (`cargo xtask deps`). The exit status is 0
when the check passes, 1 when it fails, and 2 when the command line is not
understood.

## CI (GitHub Actions)

- **Every PR / push, matrix `ubuntu-latest`, `windows-latest`, `macos-latest`**
  (`.github/workflows/ci.yml`):
  1. Install the pinned toolchain (from `rust-toolchain.toml`).
  2. Linux only: install EGL and Mesa (`libegl1`, `libgl1-mesa-dri`; from
     M2.2, because `cargo xtask ci` runs the headless OpenGL tests).
  3. `cargo xtask ci`, with `LIBGL_ALWAYS_SOFTWARE=1` and, on Linux,
     `NADIR_EXPECT_GL_RENDERER=llvmpipe`: the OpenGL tests fail on any other
     renderer.
  4. Linux only, from M2.5: `cargo xtask golden`, with the same two
     variables, set on that step too.
     - It runs even when the checks fail: they draw the same scenes, and
       fail first when a scene's own pixel checks do.
     - If it fails, the job uploads `target/golden/*.qoi` as the artifact
       `golden-failures`: each failed scene's `actual` image, and its
       `diff` image if it was compared.
     - An `actual` image holds the bytes `--bless` would write on the
       runner's llvmpipe. Once a human has looked at it, it can become the
       reference.
  5. Linux only: `cargo xtask fuzz-smoke --secs 120`.

  The images already have `libx11-6`, through `xvfb`, which their
  software lists include. ALSA (`libasound2t64` on Ubuntu 24.04) waits
  for a test that needs it, from M6.3.

  Before the checkout, the workflow turns Git's `core.autocrlf` off. A
  Windows runner would otherwise check the files out with CRLF line
  endings, and tests that read committed files expect them as committed.
- **Nightly:**
  - `xtask bench` (report only)
  - fuzz smoke for 30 minutes per parser
  - property tests with random seeds
  - a full asset cook of `assets/`
- **Release (tag `v*`):** see `docs/05-release.md`.
- **Supply-chain hygiene for CI itself:**
  - Pin every action by full commit SHA, with the tag as a comment.
  - Grant `permissions: contents: read` by default, and elevate only in
    release jobs.
  - Use no third-party actions beyond `actions/checkout` and
    `actions/upload-artifact`/`download-artifact`. Install the toolchain with
    `rustup` commands directly.
- **Caching:** cache `~/.cargo/registry` and `target/` keyed on the
  `Cargo.lock` + `rust-toolchain.toml` hash using `actions/cache`
  (SHA-pinned).

**GPU availability on hosted runners.** This was looked into at M2.5, on
2026-10-18, from GitHub's docs, the `actions/runner-images` repository
and other projects' CI logs and issues. No Nadir job has drawn on a Windows
or macOS runner yet, so the "reported" points are other projects'.

- **Linux** (`ubuntu-latest`): no GPU. llvmpipe, from Ubuntu's Mesa, draws
  the goldens.
  - The references were drawn on Mesa 26.1.6. Ubuntu 24.04 has 25.2.8 and
    26.04 has 26.0.8, so the tolerance has to cover the version
    difference.
  - `ubuntu-latest` moves to Ubuntu 26.04 between 2026-10-19 and
    2026-11-19 (runner-images #14748). A failure that starts with the
    move is the new Mesa: compare the uploaded images before blessing.
- **Windows** (`windows-latest` is Windows Server 2025, and `windows-2022`
  is the other x64 image):
  - No GPU. The display is Hyper-V Video. DXGI lists only Microsoft Basic
    Render Driver (WARP), so Direct3D 11 and 12 work in software.
  - OpenGL is Microsoft's GDI Generic 1.1. A job on `windows-2025` failed
    with "the graphics driver provides only OpenGL 1.1.0 (GDI Generic)"
    (tasksmack#2256). The same project found, on its own machine, that GDI
    Generic has no `WGL_ARB_create_context` (tasksmack#2113), so it has no
    3.3 core context.
  - The gfx and render GL tests skip only when there is no offscreen
    context at all (`PlatformError::Unsupported`, which
    `Device::new_headless` gives as `GfxError::NoOffscreenContext`) and
    `NADIR_EXPECT_GL_RENDERER` is empty. Any other error fails them. So
    once M2.6 has a WGL context, the Windows leg fails on this runner,
    unless M2.6 widens the skip or brings llvmpipe.
  - The image installs no Mesa and no other OpenGL driver.
  - Jobs run in an interactive desktop session, where windows can be
    created (reported, not documented).
  - Other projects get llvmpipe there, at GL 4.x, by putting Mesa's
    `opengl32.dll` next to the test binary (`target/<profile>/deps`). The
    builds:
    - `pal1000/mesa-dist-win`: built on a private server, with no build
      log. It needs `libgallium_wgl.dll` beside it, and before 25.0
      `libglapi.dll` too.
    - `mmozeiko/build-mesa`: built in public CI. It is one static
      `opengl32.dll`.
    - MSYS2: signed packages, but a rolling release, and the DLL needs
      MSYS2's other DLLs.
  - With `LIBGL_ALWAYS_SOFTWARE=1`, Mesa's WGL chooses llvmpipe over its
    D3D12 driver on WARP.
  - Each of these is a third-party binary in CI, which
    `docs/02-dependency-policy.md` does not cover: using one is the user's
    decision. Each is also another build of Mesa, with another LLVM, than
    the one that drew the references.
- **macOS** (`macos-latest` is macOS 26 on a 3-core M1 virtual machine).
  The other images are `macos-15`, `macos-15-intel` and `macos-26-intel`.
  `macos-14` is retired on 2026-11-02.
  - The virtual GPU, `Apple Paravirtual device`, accelerates Metal only.
    - Plain Metal works on the arm64 images: wgpu and Bevy run Metal tests
      there.
    - It reports Apple5 as its highest family, and no Metal 4.
    - On the Intel images, multisampled Metal work hangs or gives wrong
      results (runner-images #14836, open).
  - On the arm64 images, OpenGL runs on Apple's CPU `Apple Software
    Renderer`. No recent report gives the Intel images' GL renderer.
    - A 3.2 core request gave 4.1 core on `macos-14`.
    - A pixel format that asks for `NSOpenGLPFAAccelerated` is refused.
    - Its output has known defects (wrong lines and textures, closed as
      an Apple bug in runner-images #9712), and it is not llvmpipe, so
      it cannot be compared with the references.
  - The image logs the runner user in to a GUI session.
- **So:** goldens run in CI only on Linux. Windows and macOS goldens run
  locally before each release, and the release checklist requires it.
  Look again:
  - at M2.6: Mesa on the Windows runner, if the user approves the binary
  - at M2.7: the software renderer, which needs references of its own
  - at M10.4: Metal goldens on the arm64 runners, within Apple5

## Unsafe and FFI review checklist

Use this in code review for any diff touching `sys`, `platform` or `gfx`.

- [ ] Declarations match the official headers or documentation, and the
      source is cited in a comment (header name or doc URL).
- [ ] Every new `#[repr(C)]` struct has a `const` assertion on `size_of` and
      `align_of` against values from the header, plus offset checks for
      fields we read.
- [ ] Function pointer types match the calling convention: `"system"` on
      Windows, `"C"` elsewhere.
- [ ] Ownership is documented for every returned pointer: who frees it, and
      with what.
- [ ] Every `unsafe` block is minimal, with one unsafe operation per block
      and a `// SAFETY:` comment.
- [ ] Errors from the OS (HRESULT, `errno`, `NSError`, EGL error) are
      converted to our error type with context, never ignored.
- [ ] Threading assumptions are stated (e.g. "AppKit calls must happen on
      the main thread").
