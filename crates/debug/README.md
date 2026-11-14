# nadir-debug

Developer tools for Nadir: an immediate-mode debug UI of windows and
widgets, drawn with the engine's own renderer and bitmap font, and fed by
the window's events.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Games use it through
[nadir_engine](https://crates.io/crates/nadir_engine)'s `dev` feature,
which re-exports it as `nadir::debug`, feeds it the window's events, draws
it over the game, holds the game's input off while it has the pointer or
the keyboard, and saves its windows' places; depend on it directly to use
it on its own. It is for development builds: keep it out of releases.

## Status

Version 0.3: the API will change before 1.0.

Built:

- windows that drag by their title bar, collapse, resize from a grip,
  scroll by the wheel or a thumb, and come to the front when pressed;
- labels, buttons, checkboxes, sliders, drags, a one-line text field,
  combo boxes, tree nodes, separators, line plots and color edits;
- identity by text, or by an explicit ID after `##`;
- what the game should leave alone: the pointer over a window or a widget
  held, and the keyboard while a text field has it;
- each window's place, given and taken as data for saving;
- a stats overlay of numbers the engine copies in, and a profiler of the
  last 300 frames: graphs of their processor time and of the GPU's time
  over their drawing, a frame's scopes as a tree, and a Chrome trace of
  them all, with a track of the GPU's passes, which Perfetto UI opens;
- an inspector of a `nadir-ecs` world: its entities, a page at a time,
  filtered by number and component; the components of the one clicked,
  each field edited through `nadir-scene`'s reflection; components added
  and removed by name, the entity despawned, or written as a prefab;
- a console: a command line, with history and Tab completion, under a log
  of what its commands print and the records the caller adds; commands
  registered by a usage that names their arguments, which it checks before
  handing the command back to run.

Not yet: tooltips, a scroll area inside a window, copy and paste. The
debug drawing (F4) and the time controls (F5 to F7) are nadir_engine's,
under its `dev` feature (ADR-0064).

## Usage

```toml
[dependencies]
nadir-debug = "0.3"
# For `Handle`, `Rect`, `Vec2` and the mouse, and for the UI list it draws into.
nadir-core = "0.3"
nadir-render = "0.3"
```

Each frame, the UI takes the frame's events, code fills its windows with
widgets, and the frame is pushed into the frame's UI list. Here a test font
measures the text, and a click lands on a checkbox where the frame before
drew it:

```rust
use std::num::NonZeroU32;

use nadir_core::{GenIndex, Handle, MouseButton, Rect, UVec2, Vec2};
use nadir_debug::{DebugInput, DebugUi, TextMetrics};
use nadir_render::UiList;

/// A font of 8 units a line, each character 6 wide.
struct Mono;

impl TextMetrics for Mono {
    fn line_height(&self) -> f32 {
        8.0
    }

    fn caret_x(&self, text: &str, byte: usize) -> f32 {
        6.0 * text[..byte].chars().count() as f32
    }
}

// The font and the white texture are the game's loaded assets; here, two
// handles that name nothing.
fn handle<T>(index: u32) -> Handle<T> {
    Handle::new(GenIndex::new(index, NonZeroU32::MIN))
}
let mut ui = DebugUi::new(handle(0), Mono, handle(1));
let mut input = DebugInput::new();
let mut out = UiList::new();
let output = UVec2::new(640, 360);
let mut paused = false;
for _ in 0..2 {
    let mut frame = ui.frame(&mut input, output, output);
    frame.window("Tools", Rect::new(Vec2::ZERO, Vec2::new(200.0, 120.0)), |ui| {
        ui.checkbox("Paused", &mut paused);
    });
    out.clear();
    frame.finish(&mut out);
    // A click on the checkbox's row, below the title bar.
    input.pointer_moved(Vec2::new(8.0, 18.0));
    input.button(MouseButton::Left, true);
    input.button(MouseButton::Left, false);
}
assert!(paused);
// The pointer is over the window: the game should leave the mouse alone.
assert!(ui.wants_mouse());
assert!(!out.is_empty());
```

A console hands back the command a line names, with its arguments checked
against its usage, for the caller to run; `Console::show` fills a window
with it, and Enter runs the line typed:

```rust
use nadir_debug::{Console, ConsoleError};

#[derive(Clone, Copy)]
enum Command {
    Teleport,
}

let mut console = Console::new();
console
    .register("tp <x> <y>", "moves the player", Command::Teleport)
    .expect("a usage starts with a name");
let mut player = (0.0, 0.0);
console.run_line("tp 10 -4", |command, args, out| {
    match command {
        Command::Teleport => player = (args.f32(0)?, args.f32(1)?),
    }
    out.print("moved");
    Ok::<(), ConsoleError>(())
});
assert_eq!(player, (10.0, -4.0));
// A line that does not fit the usage does not run.
console.run_line("tp 10", |_, _, _| Ok(()));
let lines: Vec<&str> = console.lines().collect();
assert_eq!(lines, ["> tp 10 -4", "moved", "> tp 10", "<y> is missing: tp <x> <y>"]);
```

## Documentation

- API reference: https://docs.rs/nadir-debug
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/debug-tools.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
