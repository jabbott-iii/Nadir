# Subsystem: `nadir-core`

Foundation types used by every other crate. No `unsafe`, no OS calls, no
dependencies.

## Contents

| Module | Provides |
| :- | :- |
| `math` | `Vec2`, `IVec2`, `UVec2`, `Mat3` (2D affine), `Affine2`, `Rect`, `IRect`, `lerp`, `approx_eq`, `clamp`, angle helpers. `#[repr(C)]` where used in GPU data |
| `color` | `Color` (sRGB `u8` RGBA), `LinearColor` (`f32`), conversion with exact sRGB transfer functions, hex parsing |
| `time` | `FixedClock` (accumulator, hitch clamp, `alpha`), `Tick` (index, dt), `Frame` (dt, alpha), `Stopwatch` |
| `log` | Macros `error!`, `warn!`, `info!`, `debug!`, `trace!` with a module target; sinks: stderr, rotating file, in-memory ring buffer (last 1000 lines, for console and crash reports); level filter per target from settings |
| `rng` | `Pcg32` (seedable, `next_u32`, `next_f32`, `range`, `shuffle`, `choose`); per-system streams derived from a world seed |
| `hash` | `fnv1a64`; `FxLikeHasher` with `FastHashMap` / `FastHashSet` aliases (deterministic, non-cryptographic); `crc32` (IEEE, table-driven), shared by `codec` (PNG chunks) and `assets` (pack integrity) |
| `handle` | `GenIndex { index: u32, generation: NonZeroU32 }`, `GenArena<T>` (insert/remove/get/iter, free list), typed `Handle<T>` built on it |
| `keys` | `Key` (physical keys, USB-HID-usage order), `MouseButton`, `GamepadButton`, `GamepadAxis` (standard layout, Xbox naming), `Modifiers` |
| `profile` | `profile_scope!("name")`: compiles to nothing unless feature `profile` is on; when on, records begin/end with `Instant` into a thread-local buffer; `profile::take_frame(&mut out)` moves the calling thread's records into a reused `Vec` |
| `testing` | Property-test harness (`check`, `check_sized`), golden comparison helpers (pure pixel diff), `assert_approx!` |
| `cli` | Minimal argument parser for `xtask`, `cook` and the game binary (`--flag`, `--key value`, positional) |
| `civil` | UTC date/time from `SystemTime` (days-to-civil conversion) for log and crash-file names |
| `error` | `CoreError` and the `ErrorContext` helper for adding "while doing X" context to errors |

## Invariants

- `GenArena` never hands out an index whose generation matches a removed
  entry. Generations wrap at `u32::MAX` back to 1, never to 0, and a slot that
  wraps is retired permanently.
- All math is `f32`. No implicit `f64` promotion in hot paths.
- `Pcg32` output is identical across OSes for the same seed (unit-tested
  against fixed vectors).
- `FastHashMap` iteration order depends only on insertion history, not on
  per-process random state. This keeps replays deterministic. Never use
  `std::collections::HashMap` with `RandomState` in simulation code. A lint
  helper in `xtask` greps for it under `crates/` (except tooling).

## Tests

- Property tests:
  - `Mat3` inverse round-trips
  - `Rect` intersection is symmetric
  - sRGB↔linear round-trips within 1 unit
- `GenArena` stress test: random insert/remove sequences against a reference
  model.
- Logger rotation, using a temporary directory under `target/`.
- `civil` against known timestamps, including leap years and the 2038
  boundary.
