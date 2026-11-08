# Nadir

Nadir is a 2D game engine for top-down games, written in Rust for Windows,
macOS and Linux. It is made for games with tilemaps, y-sorted sprites,
kinematic movement and many entities.

It is written from the ground up, with no third-party crates. Its OS
bindings, OpenGL backend, math, ECS, codecs, asset pipeline and tools are all
in this repository. Beyond Rust's standard library, it calls only the
system's own libraries, such as Xlib, EGL and OpenGL.

The [demo page](https://jabbott.dev/Nadir/) shows recordings of what it draws
today and has a Linux download of the sandbox game.

## Status

Nadir is early (version 0.2), and its APIs will change. It is built one task
of the [roadmap](https://github.com/jabbott-iii/Nadir/blob/main/docs/roadmap.md)
at a time, alongside the game in `games/sandbox`, which pulls each feature in.

- **On Linux (X11),** these work:
  - a window, with keyboard and mouse input;
  - a fixed 60 Hz simulation, which rendering interpolates between ticks;
  - an OpenGL 3.3 renderer, with:
    - y-sorted, instanced sprite batches;
    - tilemaps;
    - animation;
    - a pixel-perfect camera;
  - an ECS, collision with move-and-slide, and replays checked for
    determinism;
  - assets loaded on worker threads;
  - an offline cook that packs PNG images, Aseprite exports and bitmap fonts
    into atlases, and turns LDtk levels into level files, which the game
    loads whole, with their prefabs; and puts all it cooks in one pack
    file, `game.kpak`, which release builds read.
- **On Windows and macOS,** everything builds and passes the tests, but the
  window stays empty until the OpenGL contexts land (roadmap tasks M2.6 and
  M2.7).
- **Not built yet:** audio, text, the debug tools, gamepads, hot reload,
  saves and the Metal backend.

## Build and run

You need [rustup](https://rustup.rs). `rust-toolchain.toml` pins the
toolchain, and rustup installs it the first time you run `cargo` here. Each
system also needs:

| System | You need |
| :- | :- |
| Linux | An X11 session (or Wayland with XWayland) and OpenGL 3.3. The engine loads `libX11`, `libEGL` and OpenGL at run time, so no development packages are needed. |
| Windows | The MSVC build tools and the Windows SDK. |
| macOS | The Xcode command-line tools. |

```bash
git clone https://github.com/jabbott-iii/Nadir.git
cd Nadir
cargo xtask cook
cargo run -p sandbox
```

`cargo xtask cook` cooks `assets/` into `target/cooked/`, which the sandbox
draws from. Run it again after you change the assets.

In the sandbox, WASD or the arrow keys walk. Close the window to quit.

Other ways to run it:

| Command | What it runs |
| :- | :- |
| `cargo run -p sandbox -- --scene stress-sprites` | 10,000 people walking about in view |
| `cargo run -p sandbox -- --scene stress-entities` | 5,000 people jostling in a maze of walls |
| `cargo dev` | A development build: placeholders for missing assets, allocation counts in the log, and hot reload of what `cargo xtask cook --watch` cooks |

## Use it in a game

Add the `nadir_engine` crate (see [Releases](#releases)); code names it `nadir`. A
game needs only this crate:

```rust
use nadir::prelude::*;

fn main() -> Result<(), nadir::Error> {
    App::new(Config::default())
        .add_fixed_system(FixedStage::Gameplay, "hello", |_, tick| {
            if tick.index == 0 {
                nadir::log::info!("the first tick");
            }
        })
        .run()
}
```

For a fuller example, see the sandbox's
[`main.rs`](https://github.com/jabbott-iii/Nadir/blob/main/games/sandbox/src/main.rs).
It has:

- a tile map;
- a walking character that collides;
- a camera that follows them;
- sprites cooked from an Aseprite export.

## Repository layout

| Path | Contents |
| :- | :- |
| `crates/` | The engine, one crate per subsystem. `crates/engine` is the facade a game uses. |
| `tools/cook` | `nadir-cook`, the offline asset pipeline |
| `games/sandbox` | The game that drives the engine's requirements |
| `assets/` | The sandbox's source assets, which the cook reads |
| `xtask/` | `cargo xtask`: the CI checks, golden images, fuzzing, cooking, packaging and the demo page |
| `bench/` | The benchmark harness |
| `fuzz/` | The fuzzer's seed corpora |
| `tests/golden/` | The renderer's golden reference images |
| `site/` | The demo page |
| `docs/` | The design |
| `diagrams/` | Diagrams of the architecture, the game loop and the main types |

## Documentation

| Document | Contents |
| :- | :- |
| [`docs/00-overview.md`](https://github.com/jabbott-iii/Nadir/blob/main/docs/00-overview.md) | Goals, non-goals and performance budgets |
| [`docs/01-architecture.md`](https://github.com/jabbott-iii/Nadir/blob/main/docs/01-architecture.md) | The crates and their dependency matrix, the main loop, threading |
| [`docs/subsystems/`](https://github.com/jabbott-iii/Nadir/tree/main/docs/subsystems) | One spec per subsystem |
| [`docs/roadmap.md`](https://github.com/jabbott-iii/Nadir/blob/main/docs/roadmap.md) | Milestones, with each task's acceptance criteria |
| [`docs/decisions.md`](https://github.com/jabbott-iii/Nadir/blob/main/docs/decisions.md) | Architecture decision records: why things are the way they are |
| [`docs/05-release.md`](https://github.com/jabbott-iii/Nadir/blob/main/docs/05-release.md) | Build profiles, releases and packaging |
| [`diagrams/`](https://github.com/jabbott-iii/Nadir/tree/main/diagrams) | C4 views of the system, its programs and its crates, the game loop's sequence, and class diagrams of the main types |

## Contributing

Open an issue before you open a pull request.
[`CONTRIBUTING.md`](https://github.com/jabbott-iii/Nadir/blob/main/CONTRIBUTING.md)
covers the setup, the rules and the checks. `cargo xtask ci` runs the same
checks as the pull request gate in CI.

Coding agents follow
[`AGENTS.md`](https://github.com/jabbott-iii/Nadir/blob/main/AGENTS.md).

Report vulnerabilities privately, as
[`SECURITY.md`](https://github.com/jabbott-iii/Nadir/blob/main/SECURITY.md)
describes.

## License

Apache License 2.0: see
[`LICENSE`](https://github.com/jabbott-iii/Nadir/blob/main/LICENSE) and
[`NOTICE`](https://github.com/jabbott-iii/Nadir/blob/main/NOTICE).

## Releases

A tag `v<version>` publishes a release (`docs/05-release.md`, "Engine
releases"; ADR-0036). Each one has:

- **Programs** on the
  [GitHub release](https://github.com/jabbott-iii/Nadir/releases/latest),
  for Linux, Windows and macOS on x86_64 and arm64: `nadir-cook`, the asset
  pipeline, and the sandbox game, which draws on Linux only so far (an
  empty window on Windows and macOS until roadmap tasks M2.6 and M2.7).
  They are not signed yet.
- **apt** packages `nadir-cook` and `nadir-sandbox`, for amd64 and arm64:

  ```bash
  curl -fsSL https://github.com/jabbott-iii/Nadir/releases/latest/download/nadir-archive-keyring.gpg \
    | sudo tee /usr/share/keyrings/nadir-archive-keyring.gpg > /dev/null
  echo "deb [signed-by=/usr/share/keyrings/nadir-archive-keyring.gpg] https://github.com/jabbott-iii/Nadir/releases/latest/download ./" \
    | sudo tee /etc/apt/sources.list.d/nadir.list
  sudo apt update && sudo apt install nadir-cook nadir-sandbox
  ```

- **A container image** of the cook, `ghcr.io/jabbott-iii/nadir-cook`, for
  `linux/amd64` and `linux/arm64`:

  ```bash
  docker run --rm --user "$(id -u):$(id -g)" --volume "$PWD:/work" \
    ghcr.io/jabbott-iii/nadir-cook assets cooked
  ```

- **Crates** on crates.io. The engine is `nadir_engine`, since `nadir` was
  taken, and code names it `nadir`:

  ```toml
  [dependencies]
  nadir_engine = "0.2"
  ```

  ```rust
  use nadir::prelude::*;
  ```

  The cook installs with `cargo install nadir-cook`.
