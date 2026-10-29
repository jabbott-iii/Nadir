# Subsystem: `nadir-input`

Turns platform events into per-tick **action state**. Gameplay code never
reads keys or buttons directly.

Built so far (M3.1): keyboard and mouse bindings, latching, text mode, rebind
conflict detection, and replays. Gamepads, deadzones and the active device
come with M8.4, the rebind capture flow with M9.2, and typed text
(`Event::Text`) with the first text field, M7.2.

## Layers

1. **Device state.** Fed by `platform::Event` through `Input::handle`:
   - keyboard: a `Key` → down set
   - mouse: position, buttons, wheel accumulator
   - gamepads (M8): buttons and raw axes per `GamepadId`
2. **Edge accumulation.** Between ticks, every change to a key or button is
   kept in order.
3. **Latch per fixed tick.** `Input::latch(&map, &mut state)` produces the
   `ActionState` for the tick and starts the next one.
4. **Action map.** The latch reads the changes through the current bindings
   of an `ActionMap`.

## Edge latching (why it matters)

At 144 Hz rendering and a 60 Hz simulation, a quick tap can go down and back
up between two ticks. If you only sample the "is down" state at tick time,
the tap is lost. So each button tracks:

```rust
struct ButtonTrack { down: bool, presses: u16, releases: u16 }
// at latch:
//   just_pressed  = presses  > 0
//   just_released = releases > 0
//   held          = down || just_pressed      // a tap still counts as held for one tick
//   presses = 0; releases = 0
```

As built, the latch keeps the changes themselves rather than counts, because
counts per button lose their order:

- For each action, it starts from the keys and buttons that were down at the
  last latch, and takes the changes in order.
- The action is just pressed when it goes from none of its bound inputs down
  to some, and just released when it goes from some to none. Held is as
  above.
- With one binding, this is exactly the `ButtonTrack` above, and a property
  test checks it against that model.
- With several, the action goes down when the first input goes down and up
  when the last goes up. Clicking a second binding while the first is held
  is no new press, and rolling from one bound key to another is neither a
  press nor a release.
- Key repeats are ignored, and so are a press of a key that is down and a
  release of one that is up.
- Bindings changed between two latches apply to the whole tick. A key bound
  while it is held reads as held, with no edge, as latching the device state
  through the new bindings gives.
- If the changes are never latched, they stop at 1024. Then the oldest half
  is folded into the starting state, and its edges are lost.
- Call `latch` once a tick, with one map: it starts the next tick. A second
  reader, such as one that runs per frame, needs an `Input` of its own, fed
  the same events.

Focus loss resets every track (`down = false`), using the platform's
synthetic releases. `Input` also releases anything still down on
`Focused(false)`.

## Actions

```rust
pub enum ActionKind { Button, Axis1, Axis2 }

pub trait ActionSet: Copy + Eq + Debug + 'static {
    const ALL: &'static [Self];           // in index order
    fn index(self) -> usize;              // position in ALL
    fn name(self) -> &'static str;        // for settings files and replays
    fn kind(self) -> ActionKind;
    fn ui_allowed(self) -> bool { false } // stays live in text mode
    fn from_name(name: &str) -> Option<Self>;   // provided
}

pub struct ActionMap<A> { /* SLOTS = 4 bindings per action */ }
impl<A: ActionSet> ActionMap<A> {
    pub fn new() -> Result<Self, InputError>;   // checks the set's rules
    pub fn bind(&mut self, a: A, slot: usize, b: Binding) -> Result<Option<Binding>, InputError>;
    pub fn unbind(&mut self, a: A, slot: usize) -> Option<Binding>;
    pub fn bindings(&self, a: A) -> &[Option<Binding>; SLOTS];
    pub fn conflict(&self, a: A, slot: usize, b: &Binding) -> Option<(A, usize)>;
}

pub enum Binding {
    Key(Key),
    MouseButton(MouseButton),
    KeyComposite2 { up: Key, down: Key, left: Key, right: Key },   // WASD -> Axis2, normalized
    // From M8.4:
    // PadButton(GamepadButton),
    // PadAxis { axis: GamepadAxis, sign: AxisSign },   // half-axis as button / Axis1
    // PadStick { stick: Stick },                       // Axis2 with radial deadzone
}

pub struct ActionState<A> { /* per action: held, just_pressed, just_released, value */ }
impl<A: ActionSet> ActionState<A> {
    pub fn pressed(&self, a: A) -> bool;
    pub fn just_pressed(&self, a: A) -> bool;
    pub fn just_released(&self, a: A) -> bool;
    pub fn value(&self, a: A) -> f32;             // Axis1, 0..1 for buttons; an Axis2's length
    pub fn axis2(&self, a: A) -> Vec2;            // length ≤ 1
}
```

- **The game defines actions** as an enum implementing `ActionSet`.
  - The name, kind and `ui_allowed` of an action belong to the code, so
    they are on the trait.
  - Bindings are data, in the `ActionMap`. Their default table goes in
    `input.jsonc`, which a crate that can read JSON loads: `input` depends
    on `core` and `platform` only.
- **Bindings and kinds:** a key or mouse button drives a `Button` or an
  `Axis1`, which it holds at 1. A composite drives an `Axis2`. Anything else
  is refused.
- **Slots:** each action has four slots, so that the columns of a controls
  menu keep their places.
- **Values:**
  - A `Button` or `Axis1` driven by keys is 1 while held.
  - An `Axis2` is the sum of its composites' directions, scaled down to
    length 1 if it is longer. +Y is down.
  - In a composite, a key counts while it is down, or for the tick in
    which it was tapped. So an `Axis2` can have a direction in a tick in
    which it is just released and not held: W held, then D tapped while W
    goes up, moves along D for that tick.
  - Values are stored as `i16` steps of 1/32767, rounded toward zero. A
    direction is shortened by a step if rounding left it longer than 1.
    Replays store the same steps, so a replayed state equals the live one.
- **Gamepad deadzones** (M8.4):
  - Sticks: radial inner deadzone 0.2 and outer 0.95, rescaled to 0..1.
  - Triggers: a 0.1 threshold.
  - All are configurable per player in settings.
- **Active device** (M8.4): whichever device last produced meaningful input
  (keyboard/mouse or a specific gamepad). It drives UI prompts, e.g.
  showing "A" versus "Space".
- **Aim:** for twin-stick, the mouse world position (via
  `render::screen_to_world`) or the right stick, chosen by active device.
  The facade exposes this as `AimState`.
  - The pointer and the wheel are not actions: `Input::pointer()` and
    `Input::wheel()` (the last tick's total) are for the UI and for aiming.
  - Text mode does not hide them, and replays do not hold them.

## Rebinding and settings

- Bindings are saved in the `input` section of `settings.jsonc` (M9.3), by
  action name.
- **Rebind flow:**
  1. The UI calls `begin_capture(action, slot)` (M9.2).
  2. The next non-modifier press from the active device becomes the
     binding. Escape cancels.
  3. A conflict with another action is reported back to the UI, which
     decides whether to swap or reject.
- `ActionMap::conflict` finds that conflict now. It returns the first other
  slot, of any action or of the same one, whose binding reads a key or
  button the new binding reads. A composite shares each of its four keys.
- `bind` itself refuses no conflict, so a default table may share a key on
  purpose: Escape for both pause and a menu's back.

## Text input mode

- When a text field is focused (debug console, name entry), the input crate
  enters text mode (`Input::set_text_mode`), in order with the events:
  - `Event::Text` goes to the text consumer (from M7.2, when the platform
    reports typed text)
  - gameplay actions read as released
  - only actions flagged `ui_allowed` stay live
- Leaving text mode does not generate phantom presses for keys still held.
- As built:
  - Text mode covers mouse buttons as well as keys.
  - A gameplay action held when text mode begins is just released on the
    next tick.
  - Keys and buttons held then, and those pressed during text mode, stay
    hidden from gameplay actions until they are released.
  - A press just before text mode begins, in the same tick, is a tap.

## Replays

- `ActionState` for each tick serializes compactly: bitsets for buttons,
  quantized axes as `i16`.
- `InputRecorder` writes the per-tick stream. `InputPlayer` replaces live
  latching with recorded states.
- This powers determinism tests and bug reproductions.
- The facade plays one with `App::play_replay` (M4.7, ADR-0025):
  - It reads one tick ahead, so its `Replay` resource is `Ended` as soon
    as the last tick has run; the ticks after it are latched live.
  - While it plays, the live input is latched into a set of its own:
    `Input` keeps its ticks, and nothing reaches the actions. The pointer
    and the wheel stay live: fixed systems read input only through the
    `ActionState` if they are to replay.
  - At the handover the live ticks' edges are the live keys' alone: an
    action held in the replay's last tick is not just released, and a key
    held through it is held but not just pressed. `ActionState` has no
    public way to latch from a given state.
- The sandbox records with `--record FILE`, writing each tick as it comes,
  and plays with `--replay FILE`, in the window or `--headless`. Both log
  the ticks and the world's hash after them: a recording when its window
  is closed, a replay when it ends.
- Format version 1, little endian:
  - The header:
    - the magic `NRPL`, the version (`u16`) and the action count (`u16`)
    - a `u64` FNV-1a hash of each action's name, a 0 byte and its kind byte,
      in index order
    - one kind byte per action: 0 button, 1 `Axis1`, 2 `Axis2`
  - Then one record per tick, to the end of the file:
    - three bits per action: held, just pressed, just released, from the
      least significant bit of the first byte
    - an `i16` per `Axis1` and two per `Axis2`, in index order
    - a button's value is 1 while it is held, so it is not stored
- There is no tick count: a replay cut short at the end of a tick plays the
  ticks before the cut.
- The player reads strictly. Each error has a byte offset:
  - a cut-short header or tick
  - another magic, version, action count, kind, or names
  - flag bits no action uses
  - just pressed without held, or held and just released without just
    pressed, which latching never gives
  - an `Axis1` below 0, an `i16::MIN`, or a direction longer than 1
- `xtask` may depend only on `core` and `codec`, so the player has no fuzz
  target. Property tests stand in: a replay cut inside its header or a tick
  fails where it ends, one cut at the end of a tick plays the ticks before
  the cut, and damaged ones never panic.

## Tests

- Tap between ticks registers exactly one tick with `just_pressed &&
  just_released && pressed`.
- Holding across N ticks gives `just_pressed` only on the first tick.
- Composite WASD diagonals are normalized to length 1. Opposite keys cancel.
- Deadzone math (property test, M8.4): output is in 0..1 and monotonic in
  input magnitude.
- Rebind conflicts are detected.
- Replay round-trip is bit-identical.
