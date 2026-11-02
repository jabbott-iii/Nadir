# nadir-input

Input for Nadir: turns window events into the state of the game's actions,
once a fixed tick, and records that state as replays that play back
exactly. Gameplay code reads actions, never keys or buttons.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Games use it through
[nadir_engine](https://crates.io/crates/nadir_engine), which re-exports it as
`nadir::input`; depend on it directly to use it on its own. The engine feeds
every window event to its `Input`, and latches the game's actions once a
tick after `App::add_actions`.

## Status

Version 0.1: the API will change before 1.0.

Built today:

- actions that are buttons, axes from 0 to 1, or directions, named by the
  game's own enum;
- bindings of keys, mouse buttons and four-key composites such as WASD, four
  slots per action, with a check for conflicts when the player rebinds;
- latching once a tick that keeps every change between two ticks, so that
  a tap between them is not lost;
- text mode, in which only the actions allowed in the UI stay live;
- replays of the actions' state, tick by tick, read strictly.

Not yet: gamepads, deadzones and the active device come with roadmap task
M8.4, the rebinding capture flow with M9.2, bindings in the settings file
with M9.3, and typed text with M7.2.

## Usage

```toml
[dependencies]
nadir-input = "0.1"
# For `Key`, `MouseButton` and `Vec2`, and for the window's `Event`.
nadir-core = "0.1"
nadir-platform = "0.1"
```

The game names its actions with an enum, binds keys and buttons to them,
and reads their state each tick:

```rust
use nadir_core::{Key, MouseButton};
use nadir_input::{
    ActionKind, ActionMap, ActionSet, ActionState, Binding, Input, InputPlayer, InputRecorder,
};
use nadir_platform::Event;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Walk,
    Fire,
}

impl ActionSet for Act {
    const ALL: &'static [Self] = &[Act::Walk, Act::Fire];

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        match self {
            Act::Walk => "walk",
            Act::Fire => "fire",
        }
    }

    fn kind(self) -> ActionKind {
        match self {
            Act::Walk => ActionKind::Axis2,
            Act::Fire => ActionKind::Button,
        }
    }
}

let wasd = Binding::KeyComposite2 { up: Key::W, down: Key::S, left: Key::A, right: Key::D };
let mut map = ActionMap::<Act>::new().expect("a valid action set");
map.bind(Act::Walk, 0, wasd).expect("a composite drives a direction");
map.bind(Act::Fire, 0, Binding::MouseButton(MouseButton::Left))
    .expect("a button drives a button");

let mut input = Input::new();
let mut state = ActionState::new();
let mut recorder = InputRecorder::<Act>::new().expect("a valid action set");

// Tick 1: W and D are held, so the walk is up and right, of length 1.
// +Y is down.
for key in [Key::W, Key::D] {
    input.handle(&Event::Key { key, pressed: true, repeat: false });
}
input.latch(&map, &mut state);
recorder.record(&state);
let walk = state.axis2(Act::Walk);
assert!((walk.length() - 1.0).abs() < 1e-3 && walk.x > 0.0 && walk.y < 0.0);

// Tick 2: a click that went down and up between the ticks is not lost.
for pressed in [true, false] {
    input.handle(&Event::MouseButton { button: MouseButton::Left, pressed });
}
input.latch(&map, &mut state);
recorder.record(&state);
assert!(state.just_pressed(Act::Fire) && state.just_released(Act::Fire));

// The replay gives back the same state, tick by tick.
let replay = recorder.into_bytes();
let mut player = InputPlayer::<Act>::new(&replay).expect("a replay of these actions");
let mut replayed = ActionState::new();
for _ in 0..2 {
    assert_eq!(player.next_tick(&mut replayed), Ok(true));
}
assert_eq!(replayed, state);
assert_eq!(player.next_tick(&mut replayed), Ok(false), "the end of the replay");
```

## Documentation

- API reference: https://docs.rs/nadir-input
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/input.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
