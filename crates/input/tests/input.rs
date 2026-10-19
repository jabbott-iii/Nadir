//! Property tests of latching and replays, over random bindings and random
//! event streams. The hand-worked cases are beside the code.

use nadir_core::testing::{check, check_sized};
use nadir_core::{Key, MouseButton, Pcg32};
use nadir_input::{
    ActionKind, ActionMap, ActionSet, ActionState, Binding, Input, InputError, InputPlayer,
    InputRecorder, ReplayErrorKind, SLOTS,
};
use nadir_platform::Event;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Move,
    Jump,
    Fire,
    Menu,
    Throttle,
}

impl ActionSet for Act {
    const ALL: &'static [Self] = &[Act::Move, Act::Jump, Act::Fire, Act::Menu, Act::Throttle];

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        ["move", "jump", "fire", "menu", "throttle"][self as usize]
    }

    fn kind(self) -> ActionKind {
        match self {
            Act::Move => ActionKind::Axis2,
            Act::Jump | Act::Fire | Act::Menu => ActionKind::Button,
            Act::Throttle => ActionKind::Axis1,
        }
    }

    fn ui_allowed(self) -> bool {
        self == Act::Menu
    }
}

/// The header of a replay of [`Act`]: 16 bytes, and a kind per action.
const HEADER: usize = 16 + 5;
/// A record of [`Act`]: 15 bits of flags, Move's x and y, Throttle's value.
const RECORD: usize = 2 + 4 + 2;

/// A few keys, so that bindings overlap and streams hit them often. The
/// last is bound to nothing in some maps.
const KEYS: [Key; 8] = [
    Key::W,
    Key::A,
    Key::S,
    Key::D,
    Key::Space,
    Key::Enter,
    Key::Escape,
    Key::LeftShift,
];
const BUTTONS: [MouseButton; 2] = [MouseButton::Left, MouseButton::Right];

/// One step of a run.
#[derive(Clone, Copy, Debug)]
enum Step {
    Key {
        key: Key,
        pressed: bool,
        repeat: bool,
    },
    Mouse {
        button: MouseButton,
        pressed: bool,
    },
    FocusLost,
    TextMode(bool),
    Tick,
}

fn key(rng: &mut Pcg32) -> Key {
    *rng.choose(&KEYS).expect("not empty")
}

fn random_binding(rng: &mut Pcg32, kind: ActionKind) -> Binding {
    match kind {
        ActionKind::Axis2 => Binding::KeyComposite2 {
            up: key(rng),
            down: key(rng),
            left: key(rng),
            right: key(rng),
        },
        ActionKind::Button | ActionKind::Axis1 => {
            if rng.range(0..4u32) == 0 {
                Binding::MouseButton(*rng.choose(&BUTTONS).expect("not empty"))
            } else {
                Binding::Key(key(rng))
            }
        }
    }
}

/// A map whose actions have up to [`SLOTS`] bindings each, often sharing
/// keys.
fn random_map(rng: &mut Pcg32) -> Vec<(Act, usize, Binding)> {
    let mut bindings = Vec::new();
    for &action in Act::ALL {
        for slot in 0..SLOTS {
            if rng.range(0..3u32) != 0 {
                bindings.push((action, slot, random_binding(rng, action.kind())));
            }
        }
    }
    bindings
}

fn build(bindings: &[(Act, usize, Binding)]) -> ActionMap<Act> {
    let mut map = ActionMap::new().expect("a valid set");
    for &(action, slot, binding) in bindings {
        map.bind(action, slot, binding).expect("fits");
    }
    map
}

fn random_steps(rng: &mut Pcg32, size: u32, text_mode: bool) -> Vec<Step> {
    (0..size * 4)
        .map(|_| match rng.range(0..20u32) {
            0..=9 => Step::Key {
                key: key(rng),
                pressed: rng.range(0..2u32) == 0,
                repeat: rng.range(0..8u32) == 0,
            },
            10..=11 => Step::Mouse {
                button: *rng.choose(&BUTTONS).expect("not empty"),
                pressed: rng.range(0..2u32) == 0,
            },
            12 => Step::FocusLost,
            13 if text_mode => Step::TextMode(rng.range(0..2u32) == 0),
            _ => Step::Tick,
        })
        .collect()
}

/// Runs the steps live, and returns the state of every tick.
fn live(map: &ActionMap<Act>, steps: &[Step]) -> Vec<ActionState<Act>> {
    let mut input = Input::new();
    let mut state = ActionState::new();
    let mut ticks = Vec::new();
    for &step in steps {
        match step {
            Step::Key {
                key,
                pressed,
                repeat,
            } => input.handle(&Event::Key {
                key,
                pressed: pressed || repeat,
                repeat,
            }),
            Step::Mouse { button, pressed } => {
                input.handle(&Event::MouseButton { button, pressed });
            }
            Step::FocusLost => input.handle(&Event::Focused(false)),
            Step::TextMode(on) => input.set_text_mode(on),
            Step::Tick => {
                input.latch(map, &mut state);
                ticks.push(state.clone());
            }
        }
    }
    ticks
}

#[derive(Debug)]
struct Run {
    bindings: Vec<(Act, usize, Binding)>,
    steps: Vec<Step>,
}

fn random_run(rng: &mut Pcg32, size: u32) -> Run {
    Run {
        bindings: random_map(rng),
        steps: random_steps(rng, size, true),
    }
}

fn record(ticks: &[ActionState<Act>]) -> Vec<u8> {
    let mut recorder = InputRecorder::<Act>::new().expect("a valid set");
    for state in ticks {
        recorder.record(state);
    }
    recorder.into_bytes()
}

/// The spec's model of one button: `docs/subsystems/input.md`, "Edge
/// latching".
#[derive(Clone, Copy, Debug, Default)]
struct ButtonTrack {
    down: bool,
    presses: u16,
    releases: u16,
}

#[test]
fn with_one_binding_each_latching_is_the_spec_s_button_track() {
    // Jump, Fire and Menu on one key each; Space, Enter and Escape.
    const BOUND: [(Act, Key); 3] = [
        (Act::Jump, Key::Space),
        (Act::Fire, Key::Enter),
        (Act::Menu, Key::Escape),
    ];
    let map = build(&BOUND.map(|(action, key)| (action, 0, Binding::Key(key))));
    check_sized(
        "latching against ButtonTrack",
        300,
        |rng, size| random_steps(rng, size, false),
        |steps| {
            let ticks = live(&map, steps);
            let mut tracks = [ButtonTrack::default(); 3];
            let mut tick = 0;
            for &step in steps {
                match step {
                    Step::Key {
                        key,
                        pressed,
                        repeat: false,
                    } => {
                        if let Some(i) = BOUND.iter().position(|&(_, bound)| bound == key) {
                            let track = &mut tracks[i];
                            if track.down != pressed {
                                track.down = pressed;
                                if pressed {
                                    track.presses += 1;
                                } else {
                                    track.releases += 1;
                                }
                            }
                        }
                    }
                    Step::FocusLost => {
                        for track in &mut tracks {
                            if track.down {
                                track.down = false;
                                track.releases += 1;
                            }
                        }
                    }
                    Step::Tick => {
                        let state = &ticks[tick];
                        for (track, &(action, _)) in tracks.iter_mut().zip(&BOUND) {
                            let just_pressed = track.presses > 0;
                            let just_released = track.releases > 0;
                            let held = track.down || just_pressed;
                            let got = (
                                state.pressed(action),
                                state.just_pressed(action),
                                state.just_released(action),
                            );
                            if got != (held, just_pressed, just_released) {
                                return Err(format!(
                                    "tick {tick}, {action:?}: latched {got:?}, the track gives {:?}",
                                    (held, just_pressed, just_released)
                                ));
                            }
                            track.presses = 0;
                            track.releases = 0;
                        }
                        tick += 1;
                    }
                    Step::Key { repeat: true, .. } | Step::Mouse { .. } | Step::TextMode(_) => {}
                }
            }
            Ok(())
        },
    );
}

#[test]
fn replays_play_back_bit_identically() {
    check_sized("replay round trip", 300, random_run, |run| {
        let ticks = live(&build(&run.bindings), &run.steps);
        let bytes = record(&ticks);
        if bytes.len() != HEADER + RECORD * ticks.len() {
            return Err(format!("{} bytes for {} ticks", bytes.len(), ticks.len()));
        }
        let mut player = InputPlayer::<Act>::new(&bytes).map_err(|error| error.to_string())?;
        let mut state = ActionState::new();
        let mut played = Vec::new();
        for (tick, live) in ticks.iter().enumerate() {
            match player.next_tick(&mut state) {
                Ok(true) if state == *live => played.push(state.clone()),
                other => {
                    return Err(format!(
                        "tick {tick}: {other:?}, played {state:?}, live {live:?}"
                    ));
                }
            }
        }
        if player.next_tick(&mut state) != Ok(false) {
            return Err("more ticks than were recorded".to_string());
        }
        if record(&played) != bytes {
            return Err("recording the played ticks again gives other bytes".to_string());
        }
        Ok(())
    });
}

#[test]
fn live_directions_and_values_stay_in_range() {
    check_sized("live ranges", 300, random_run, |run| {
        for (tick, state) in live(&build(&run.bindings), &run.steps).iter().enumerate() {
            let direction = state.axis2(Act::Move);
            let throttle = state.value(Act::Throttle);
            if direction.length() > 1.0 || !(0.0..=1.0).contains(&throttle) {
                return Err(format!("tick {tick}: {direction:?}, {throttle}"));
            }
            for &action in Act::ALL {
                if state.just_pressed(action) && !state.pressed(action) {
                    return Err(format!("tick {tick}: {action:?} pressed but not held"));
                }
            }
        }
        Ok(())
    });
}

#[test]
fn a_cut_short_replay_fails_where_it_ends_or_plays_its_whole_ticks() {
    check_sized(
        "replay prefixes",
        100,
        |rng, size| {
            let run = random_run(rng, size);
            record(&live(&build(&run.bindings), &run.steps))
        },
        |bytes| {
            for len in 0..=bytes.len() {
                let prefix = &bytes[..len];
                let mut player = match InputPlayer::<Act>::new(prefix) {
                    Ok(player) if len >= HEADER => player,
                    Err(InputError::Replay {
                        offset,
                        kind: ReplayErrorKind::UnexpectedEnd { .. },
                    }) if len < HEADER && offset == len => continue,
                    other => return Err(format!("{len} bytes: the header gives {other:?}")),
                };
                let mut state = ActionState::new();
                let whole = (len - HEADER) / RECORD;
                for tick in 0..whole {
                    if player.next_tick(&mut state) != Ok(true) {
                        return Err(format!("{len} bytes: tick {tick} does not play"));
                    }
                }
                let end = player.next_tick(&mut state);
                let cut = !(len - HEADER).is_multiple_of(RECORD);
                let fine = match end {
                    Ok(false) => !cut,
                    Err(InputError::Replay {
                        offset,
                        kind: ReplayErrorKind::UnexpectedEnd { .. },
                    }) => cut && offset == len,
                    _ => false,
                };
                if !fine {
                    return Err(format!("{len} bytes: after {whole} ticks, {end:?}"));
                }
            }
            Ok(())
        },
    );
}

#[test]
fn damaged_replays_are_played_or_refused_without_a_panic() {
    check(
        "damaged replays",
        2000,
        |rng| {
            let run = random_run(rng, 12);
            let mut bytes = record(&live(&build(&run.bindings), &run.steps));
            for _ in 0..1 + rng.range(0..4u32) {
                // Mostly in the records, past a header left whole.
                let from = if rng.range(0..4u32) == 0 { 0 } else { HEADER };
                if bytes.len() > from {
                    let at = rng.range(from as u32..bytes.len() as u32) as usize;
                    bytes[at] = rng.range(0..256u32) as u8;
                }
            }
            if rng.range(0..4u32) == 0 {
                let len = rng.range(0..bytes.len() as u32 + 1) as usize;
                bytes.truncate(len);
            }
            bytes
        },
        |bytes| {
            let inside = |error: &InputError| match error {
                InputError::Replay { offset, .. } if *offset <= bytes.len() => Ok(()),
                other => Err(format!("{other} points outside the replay")),
            };
            let mut player = match InputPlayer::<Act>::new(bytes) {
                Ok(player) => player,
                Err(error) => return inside(&error),
            };
            let mut state = ActionState::new();
            loop {
                match player.next_tick(&mut state) {
                    Ok(true) => {
                        if state.axis2(Act::Move).length() > 1.0
                            || !(0.0..=1.0).contains(&state.value(Act::Throttle))
                        {
                            return Err(format!("played {state:?}"));
                        }
                    }
                    Ok(false) => return Ok(()),
                    Err(error) => return inside(&error),
                }
            }
        },
    );
}
