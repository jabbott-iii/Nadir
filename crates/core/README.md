# nadir-core

Foundation types for the Nadir engine: 2D math, color, fixed-step time,
logging, random numbers, hashing, generational handles, key names, profile
scopes, a command-line parser and a property-test harness.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Most of Nadir's other crates build on it. Games use the
common parts through [nadir_engine](https://crates.io/crates/nadir_engine),
which re-exports the logger as `nadir::log`, the argument parser as
`nadir::cli`, and `Vec2`, `IVec2`, `UVec2`, `Rect`, `Color`, `Key`,
`MouseButton`, `Pcg32`, `Tick`, `Frame` and `Stopwatch` in `nadir::prelude`.
Depend on it directly for the rest, such as `FastHashMap`, `GenArena` or
`Affine2`, or to use it on its own.

## Status

Version 0.3: the API will change before 1.0.

What is built:

- `f32` vectors, rectangles and transforms (`Vec2`, `Rect`, `Affine2`,
  `Mat3`), and their integer forms (`IVec2`, `UVec2`, `IRect`);
- `Color` (8-bit sRGB) and `LinearColor`, with exact sRGB conversion and hex
  parsing;
- `FixedClock`, which turns frames into fixed ticks plus an interpolation
  factor, and `Stopwatch`;
- logging macros with a level filter per module, and three sinks: stderr, a
  file that rotates by size, and a ring buffer of the last 1000 lines;
- `Pcg32`, a seeded generator that gives the same numbers on every OS;
- FNV-1a, CRC-32, and `FastHashMap` / `FastHashSet` with a hasher that has
  no random state;
- `GenArena`, `GenIndex` and the typed `Handle<T>`;
- `Key` (118 physical keys, numbered by USB HID usage) and `MouseButton`;
- `profile_scope!` timers, behind the `profile` feature;
- `cli::Args`, a small argument parser;
- `civil::DateTime`, UTC calendar time for log lines and file names;
- `testing::check` and `testing::check_sized`, a property-test harness.

Not yet:

- names for gamepad buttons and axes, which come with gamepad support
  (roadmap M8); media keys, and the keys that only Japanese and Korean
  keyboards have;
- collecting profile records from other threads, and the profiler that
  shows them (M7.3);
- the debug console and crash reports, which are to read the log's ring
  buffer (M7.5, M9.6).

## Usage

```toml
[dependencies]
nadir-core = "0.3"
```

Math, color and random numbers. World space is +X right and +Y down, in
pixels:

```rust
use nadir_core::{Color, CoreError, IVec2, Pcg32, Rect, Vec2};

fn main() -> Result<(), CoreError> {
    let player = Rect::from_center_size(Vec2::new(40.0, 40.0), Vec2::splat(16.0));
    let door = Rect::from_min_size(Vec2::new(44.0, 30.0), Vec2::new(8.0, 24.0));
    assert!(player.intersects(door));

    // The 16-pixel tile under the player's center.
    let tile = (player.center() / 16.0).floor().as_ivec2();
    assert_eq!(tile, IVec2::new(2, 2));

    // The same seed and stream give the same numbers on every OS.
    let mut rng = Pcg32::new(1234, 0);
    let mut replay = Pcg32::new(1234, 0);
    assert_eq!(rng.range(0..100), replay.range(0..100));

    let tint = Color::from_hex("#ff8000")?;
    assert_eq!(tint, Color::rgb(255, 128, 0));
    Ok(())
}
```

A fixed-step loop with logging. The clock does not read the time itself: the
caller says how long each frame took.

```rust
use std::time::Duration;

use nadir_core::FixedClock;
use nadir_core::log::info;

// 60 ticks a second, and no frame counts for more than 250 ms.
let mut clock = FixedClock::new(60, Duration::from_millis(250));

// A frame of 50 ms holds three ticks of 1/60 s.
let frame = clock.begin_frame(Duration::from_millis(50));
while let Some(tick) = clock.consume_tick() {
    // Step the simulation by `tick.dt` seconds here.
    info!("tick {}", tick.index);
}
assert_eq!(clock.ticks(), 3);
// Then draw, blending the last two ticks by `frame.alpha`.
assert_eq!(frame.alpha, 0.0);
```

## Features

- `profile`: `profile_scope!` records how long each scope ran, and
  `profile::take_frame` collects the records. Off by default, when the
  scopes compile to nothing. Cargo builds nadir-core once, with every
  feature that any crate in the build asks for, so this also times each
  system that nadir-ecs's schedule runs:

  ```toml
  [dependencies]
  nadir-core = { version = "0.3", features = ["profile"] }
  ```

## Documentation

- API reference: https://docs.rs/nadir-core
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/core.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
