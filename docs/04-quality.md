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
| Golden-image tests | `xtask golden`, scenes in `tests/golden/` | Linux CI (llvmpipe); Windows and macOS locally before release | Rendering regressions, backend divergence |
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
  OpenGL tests of `nadir-platform` fail unless the renderer's name contains
  it (`llvmpipe` in CI). Otherwise they log the renderer and take any.
- A nightly job runs with random seeds and prints the seeds it used.
- Shrinking is deliberately minimal, and only `check_sized` does it: the
  harness retries a failing case at every smaller size and reports the
  smallest size that still fails.
- A property or generator that panics is reported like any other failure,
  with its seed.

### Golden-image tests

- A scene is a small Rust function that draws through the public `render`
  API into an offscreen render target of fixed size, e.g. 320×180.
- Readback uses `glReadPixels` or Metal `getBytes`, flipped to a top-left
  origin.
- The result is compared to `tests/golden/<scene>.qoi`, which is encoded
  with our own QOI encoder.
  - References are generated with the GL backend.
  - `--backend metal` renders with Metal and compares against the same
    references. Cross-backend parity is the point.
  - A scene may carry a per-backend override (`<scene>.metal.qoi`) only with
    a recorded reason.
- Tolerance: per-channel difference ≤ 2, and at most 0.1% of pixels may
  exceed it.
- On failure, write `target/golden/<scene>.actual.qoi` and a `diff.qoi`
  (differences in magenta).
- `cargo xtask golden --bless` rewrites the references. Run it only after a
  human has looked at the diff.

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
  - The allocation counter is not wired in yet: it arrives with the dev
    allocator in M3.5.
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
  under feature `dev`. It counts allocations per thread and per frame.
- The debug overlay shows allocations per frame.
- Integration tests assert **0 allocations** across 100 steady-state frames
  of the "10k sprites" and "5k entities" scenes.

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
  - To record a baseline, copy `latest.json` to `bench/baseline.json`. The
    numbers only mean something on the machine that recorded them.
- Required benchmarks:
  - sprite sort + batch for 10k sprites
  - tilemap chunk rebuild
  - ECS join over 5k entities
  - spatial hash rebuild and query
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
  4. Linux only:
     - install the rest (`libasound2`, `libx11-6`)
     - `cargo xtask golden`
     - `cargo xtask fuzz-smoke --secs 120`

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

**GPU availability on hosted runners:**

- Linux golden tests use software rendering (llvmpipe), which is reliable.
- Hosted Windows and macOS runners may not expose a usable GPU. Verify this
  at M2, and record what you find here. Until verified, Windows and macOS
  goldens are run locally before each release, and the release checklist
  requires it.

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
