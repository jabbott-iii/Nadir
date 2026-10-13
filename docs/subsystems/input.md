# Subsystem: `nadir-input`

Turns platform events into per-tick **action state**. Gameplay code never
reads keys or buttons directly.

## Layers

1. **Device state.** Fed by `platform::Event`:
   - keyboard: a `Key` → down set
   - mouse: position, buttons, wheel accumulator
   - gamepads: buttons and raw axes per `GamepadId`
2. **Edge accumulation.** Per button, between ticks, count presses and
   releases.
3. **Latch per fixed tick.** Produce `ActionState` for the tick and reset the
   counts.
4. **Action map.** Translate the latched device state into named actions
   using the current bindings.

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

Focus loss resets every track (`down = false`), using the platform's
synthetic releases.

## Actions

```rust
pub enum ActionKind { Button, Axis1, Axis2 }

pub struct ActionMap {
    actions: Vec<ActionDef>,                   // name, kind, bindings
}

pub enum Binding {
    Key(Key),
    MouseButton(MouseButton),
    PadButton(GamepadButton),
    PadAxis { axis: GamepadAxis, sign: AxisSign },     // half-axis as button / Axis1
    PadStick { stick: Stick },                         // Axis2 with radial deadzone
    KeyComposite2 { up: Key, down: Key, left: Key, right: Key },   // WASD -> Axis2, normalized
}

pub struct ActionState { /* per action: pressed/just_pressed/just_released/value/vec2 */ }
impl ActionState {
    pub fn pressed(&self, a: Action) -> bool;
    pub fn just_pressed(&self, a: Action) -> bool;
    pub fn just_released(&self, a: Action) -> bool;
    pub fn value(&self, a: Action) -> f32;             // Axis1, 0..1 for buttons
    pub fn axis2(&self, a: Action) -> Vec2;            // length ≤ 1
}
```

- **The game defines actions** as an enum implementing a small `ActionSet`
  trait (name ↔ index), plus a default binding table in data
  (`input.jsonc`).
- **Gamepad deadzones:**
  - Sticks: radial inner deadzone 0.2 and outer 0.95, rescaled to 0..1.
  - Triggers: a 0.1 threshold.
  - All are configurable per player in settings.
- **Active device:** whichever device last produced meaningful input
  (keyboard/mouse or a specific gamepad). It drives UI prompts, e.g.
  showing "A" versus "Space".
- **Aim:** for twin-stick, the mouse world position (via
  `render::screen_to_world`) or the right stick, chosen by active device.
  The facade exposes this as `AimState`.

## Rebinding and settings

- Bindings are saved in the `input` section of `settings.jsonc`.
- **Rebind flow:**
  1. The UI calls `begin_capture(action, slot)`.
  2. The next non-modifier press from the active device becomes the
     binding. Escape cancels.
  3. A conflict with another action is reported back to the UI, which
     decides whether to swap or reject.

## Text input mode

- When a text field is focused (debug console, name entry), the input crate
  enters text mode:
  - `Event::Text` goes to the text consumer
  - gameplay actions read as released
  - only actions flagged `ui_allowed` stay live
- Leaving text mode does not generate phantom presses for keys still held.

## Replays

- `ActionState` for each tick serializes compactly: bitsets for buttons,
  quantized axes as `i16`.
- `InputRecorder` writes the per-tick stream. `InputPlayer` replaces live
  latching with recorded states.
- This powers determinism tests and bug reproductions.

## Tests

- Tap between ticks registers exactly one tick with `just_pressed &&
  just_released && pressed`.
- Holding across N ticks gives `just_pressed` only on the first tick.
- Composite WASD diagonals are normalized to length 1. Opposite keys cancel.
- Deadzone math (property test): output is in 0..1 and monotonic in input
  magnitude.
- Rebind conflicts are detected.
- Replay round-trip is bit-identical.
