//! The keys and buttons that are down, and latching them into the state of
//! the actions once a tick.

use nadir_core::Vec2;
use nadir_platform::Event;

use crate::code::{Code, CodeSet};
use crate::state::{self, HELD, JUST_PRESSED, JUST_RELEASED, Latched, ONE};
use crate::{ActionKind, ActionMap, ActionSet, ActionState, Binding};

/// A change to the keys and buttons, kept in the order it happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Press(Code),
    Release(Code),
    TextMode(bool),
}

/// The keys and buttons that are down, and which of those gameplay actions
/// cannot see.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Devices {
    down: CodeSet,
    /// Down, but hidden from gameplay actions: whatever was held when text
    /// mode began or was pressed during it, until it is released. Always
    /// part of `down`.
    hidden: CodeSet,
    text_mode: bool,
}

impl Devices {
    fn apply(&mut self, change: Change) {
        match change {
            Change::Press(code) => {
                self.down.insert(code);
                if self.text_mode {
                    self.hidden.insert(code);
                }
            }
            Change::Release(code) => {
                self.down.remove(code);
                self.hidden.remove(code);
            }
            Change::TextMode(on) => {
                if on {
                    self.hidden = self.down;
                }
                self.text_mode = on;
            }
        }
    }

    /// Whether an action sees `code` down. A `ui_allowed` action sees
    /// everything.
    fn sees_down(&self, code: Code, ui_allowed: bool) -> bool {
        self.down.contains(code) && (ui_allowed || !self.hidden.contains(code))
    }
}

/// The most changes kept between two latches. Past it, the oldest half is
/// folded into the state they started from, and their edges are lost: an
/// [`Input`] that is never latched still uses bounded memory.
const MAX_CHANGES: usize = 1024;

/// The keys and mouse buttons, fed with window events, and latched into an
/// [`ActionState`] once a tick.
///
/// It keeps every change since the last latch in order, so that a tap
/// between two ticks is not lost, and so that an action with several
/// bindings goes down when the first goes down and up when the last goes
/// up.
pub struct Input {
    /// As the changes so far left it.
    now: Devices,
    /// As the last latch left it: where `changes` start.
    latched: Devices,
    changes: Vec<Change>,
    pointer: Vec2,
    /// Turned since the last latch.
    pending_wheel: Vec2,
    /// Turned in the last latched tick.
    wheel: Vec2,
}

impl Input {
    /// Input with nothing down.
    pub fn new() -> Self {
        Self {
            now: Devices::default(),
            latched: Devices::default(),
            changes: Vec::with_capacity(64),
            pointer: Vec2::ZERO,
            pending_wheel: Vec2::ZERO,
            wheel: Vec2::ZERO,
        }
    }

    /// Takes in a window event.
    ///
    /// - Key repeats are ignored, and so are a press of a key that is down
    ///   and a release of one that is up.
    /// - When the window loses the focus, everything still down is
    ///   released. (The platform releases them first anyway.)
    /// - Other events than keys, mouse buttons, the pointer, the wheel and
    ///   the focus are ignored.
    pub fn handle(&mut self, event: &Event) {
        match *event {
            Event::Key {
                key,
                pressed,
                repeat,
            } => {
                if !repeat {
                    self.set_down(Code::key(key), pressed);
                }
            }
            Event::MouseButton { button, pressed } => self.set_down(Code::mouse(button), pressed),
            Event::Focused(false) => {
                let down = self.now.down;
                for code in down.iter() {
                    self.change(Change::Release(code));
                }
            }
            Event::MouseMoved(position) => self.pointer = position,
            Event::MouseWheel(delta) => self.pending_wheel += delta,
            _ => {}
        }
    }

    /// Turns text mode on or off, from this point among the events.
    ///
    /// In text mode, a text field has the keyboard: gameplay actions read as
    /// released, and only `ui_allowed` actions stay live. This covers mouse
    /// buttons too. An action that was held when text mode began is just
    /// released on the next tick. Keys and buttons pressed during text mode,
    /// and those still held when it ends, stay hidden from gameplay actions
    /// until they are released, so that leaving text mode presses nothing.
    pub fn set_text_mode(&mut self, on: bool) {
        if self.now.text_mode != on {
            self.change(Change::TextMode(on));
        }
    }

    /// Whether text mode is on.
    pub fn text_mode(&self) -> bool {
        self.now.text_mode
    }

    /// Where the pointer is: physical pixels from the top-left corner of
    /// the window, as the last [`Event::MouseMoved`] said.
    ///
    /// It is for the UI and for aiming. Text mode does not hide it, and
    /// replays do not hold it.
    pub fn pointer(&self) -> Vec2 {
        self.pointer
    }

    /// How far the wheel turned before the last latch and after the one
    /// before, in lines. As for [`Input::pointer`], it is not an action.
    pub fn wheel(&self) -> Vec2 {
        self.wheel
    }

    /// Makes the state of every action of `map` for one tick, then starts
    /// the next tick.
    ///
    /// For each action, starting from the keys and buttons that were down
    /// at the last latch, the changes since then are taken in order:
    ///
    /// - It is just pressed if it went from none of its bound keys and
    ///   buttons down to some, and just released if it went from some to
    ///   none.
    /// - It is held if some are down now, or if it was just pressed: a tap
    ///   between ticks is held for one tick.
    /// - A [`ActionKind::Button`] or [`ActionKind::Axis1`] is worth 1 while
    ///   held. An [`ActionKind::Axis2`] is the sum of its composites'
    ///   directions, no longer than 1. In a composite, a key counts if it
    ///   is down or was pressed during the tick. So an `Axis2` can have a
    ///   direction in a tick in which it is just released and not held:
    ///   when W is held, and D is tapped while W goes up, the direction is
    ///   D's for that tick.
    ///
    /// Call it once a tick, with one map: it starts a new tick, so a second
    /// call sees nothing of this one. Bindings changed since the last latch
    /// apply to the whole tick.
    pub fn latch<A: ActionSet>(&mut self, map: &ActionMap<A>, state: &mut ActionState<A>) {
        for &action in A::ALL {
            state.set(action, self.latch_action(map, action));
        }
        if cfg!(debug_assertions) {
            let mut replayed = self.latched;
            for &change in &self.changes {
                replayed.apply(change);
            }
            debug_assert_eq!(
                replayed, self.now,
                "the changes lead from the last latch to now"
            );
        }
        self.latched = self.now;
        self.changes.clear();
        self.wheel = self.pending_wheel;
        self.pending_wheel = Vec2::ZERO;
    }

    fn latch_action<A: ActionSet>(&self, map: &ActionMap<A>, action: A) -> Latched {
        let ui_allowed = action.ui_allowed();
        let bindings = || map.slots(action.index());
        // How many of the action's bound keys and buttons it sees down.
        let count = |devices: &Devices| -> usize {
            bindings()
                .map(|binding| {
                    let inputs = binding.inputs();
                    inputs
                        .as_slice()
                        .iter()
                        .filter(|&&code| devices.sees_down(code, ui_allowed))
                        .count()
                })
                .sum()
        };

        let mut devices = self.latched;
        let mut before = count(&devices);
        let (mut pressed, mut released) = (false, false);
        // Keys and buttons that went down where the action could see them.
        let mut tapped = CodeSet::default();
        for &change in &self.changes {
            devices.apply(change);
            if let Change::Press(code) = change
                && devices.sees_down(code, ui_allowed)
            {
                tapped.insert(code);
            }
            let after = count(&devices);
            pressed |= before == 0 && after > 0;
            released |= before > 0 && after == 0;
            before = after;
        }
        let held = before > 0 || pressed;

        let mut flags = 0;
        for (on, flag) in [
            (held, HELD),
            (pressed, JUST_PRESSED),
            (released, JUST_RELEASED),
        ] {
            if on {
                flags |= flag;
            }
        }
        let axes = match action.kind() {
            ActionKind::Button | ActionKind::Axis1 => [if held { ONE } else { 0 }, 0],
            ActionKind::Axis2 => {
                let held_now = |code: Code| -> f32 {
                    let down = devices.sees_down(code, ui_allowed) || tapped.contains(code);
                    if down { 1.0 } else { 0.0 }
                };
                let mut sum = Vec2::ZERO;
                for binding in bindings() {
                    if let &Binding::KeyComposite2 {
                        up,
                        down,
                        left,
                        right,
                    } = binding
                    {
                        let [up, down, left, right] = [up, down, left, right].map(Code::key);
                        sum += Vec2::new(
                            held_now(right) - held_now(left),
                            held_now(down) - held_now(up),
                        )
                        .normalize_or_zero();
                    }
                }
                if sum.length() > 1.0 {
                    sum = sum.normalize_or_zero();
                }
                state::quantize2(sum)
            }
        };
        Latched { flags, axes }
    }

    fn set_down(&mut self, code: Code, down: bool) {
        if self.now.down.contains(code) != down {
            self.change(if down {
                Change::Press(code)
            } else {
                Change::Release(code)
            });
        }
    }

    fn change(&mut self, change: Change) {
        if self.changes.len() == MAX_CHANGES {
            for old in self.changes.drain(..MAX_CHANGES / 2) {
                self.latched.apply(old);
            }
        }
        self.now.apply(change);
        self.changes.push(change);
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use nadir_core::{Key, MouseButton};

    use super::*;
    use crate::tests::Act;

    fn wasd() -> Binding {
        Binding::KeyComposite2 {
            up: Key::W,
            down: Key::S,
            left: Key::A,
            right: Key::D,
        }
    }

    /// Input, a map and the state of the last tick.
    struct Game {
        input: Input,
        map: ActionMap<Act>,
        state: ActionState<Act>,
    }

    impl Game {
        fn new() -> Self {
            let mut map = ActionMap::new().expect("a valid set");
            let bindings = [
                (Act::Move, 0, wasd()),
                (Act::Jump, 0, Binding::Key(Key::Space)),
                (Act::Jump, 1, Binding::MouseButton(MouseButton::Left)),
                (Act::Pause, 0, Binding::Key(Key::Escape)),
                (Act::Throttle, 0, Binding::Key(Key::LeftShift)),
            ];
            for (action, slot, binding) in bindings {
                map.bind(action, slot, binding).expect("fits");
            }
            Self {
                input: Input::new(),
                map,
                state: ActionState::new(),
            }
        }

        fn key(&mut self, key: Key, pressed: bool) -> &mut Self {
            self.input.handle(&Event::Key {
                key,
                pressed,
                repeat: false,
            });
            self
        }

        fn press(&mut self, key: Key) -> &mut Self {
            self.key(key, true)
        }

        fn release(&mut self, key: Key) -> &mut Self {
            self.key(key, false)
        }

        fn mouse(&mut self, button: MouseButton, pressed: bool) -> &mut Self {
            self.input.handle(&Event::MouseButton { button, pressed });
            self
        }

        fn text_mode(&mut self, on: bool) -> &mut Self {
            self.input.set_text_mode(on);
            self
        }

        /// Latches a tick.
        fn tick(&mut self) -> &mut Self {
            self.input.latch(&self.map, &mut self.state);
            self
        }

        /// Held, just pressed, just released.
        fn edges(&self, action: Act) -> (bool, bool, bool) {
            (
                self.state.pressed(action),
                self.state.just_pressed(action),
                self.state.just_released(action),
            )
        }

        fn movement(&self) -> Vec2 {
            self.state.axis2(Act::Move)
        }
    }

    const UP: (bool, bool, bool) = (false, false, false);
    const PRESSED: (bool, bool, bool) = (true, true, false);
    const HOLDING: (bool, bool, bool) = (true, false, false);
    const RELEASED: (bool, bool, bool) = (false, false, true);
    const TAPPED: (bool, bool, bool) = (true, true, true);

    #[test]
    fn a_tap_between_ticks_is_held_for_exactly_one_tick() {
        let mut game = Game::new();
        game.press(Key::Space).release(Key::Space).tick();
        assert_eq!(game.edges(Act::Jump), TAPPED);
        assert_eq!(game.state.value(Act::Jump), 1.0);
        game.tick();
        assert_eq!(game.edges(Act::Jump), UP);
        assert_eq!(game.state.value(Act::Jump), 0.0);
    }

    #[test]
    fn holding_across_ticks_is_just_pressed_only_on_the_first() {
        let mut game = Game::new();
        game.press(Key::Space).tick();
        assert_eq!(game.edges(Act::Jump), PRESSED);
        for _ in 0..5 {
            game.tick();
            assert_eq!(game.edges(Act::Jump), HOLDING);
            assert_eq!(game.state.value(Act::Jump), 1.0);
        }
        game.release(Key::Space).tick();
        assert_eq!(game.edges(Act::Jump), RELEASED);
        game.tick();
        assert_eq!(game.edges(Act::Jump), UP);
    }

    #[test]
    fn a_release_and_a_press_within_a_tick_are_both_seen() {
        let mut game = Game::new();
        game.press(Key::Space).tick();
        game.release(Key::Space).press(Key::Space).tick();
        assert_eq!(game.edges(Act::Jump), TAPPED);
        game.tick();
        assert_eq!(game.edges(Act::Jump), HOLDING);
    }

    #[test]
    fn repeats_and_repeated_events_change_nothing() {
        let mut game = Game::new();
        game.press(Key::Space).tick();
        game.input.handle(&Event::Key {
            key: Key::Space,
            pressed: true,
            repeat: true,
        });
        game.press(Key::Space).tick();
        assert_eq!(game.edges(Act::Jump), HOLDING);
        game.release(Key::Space).release(Key::Space).tick();
        assert_eq!(game.edges(Act::Jump), RELEASED);
    }

    #[test]
    fn with_several_bindings_the_first_down_presses_and_the_last_up_releases() {
        let mut game = Game::new();
        game.press(Key::Space).tick();
        assert_eq!(game.edges(Act::Jump), PRESSED);
        // A click of the other binding while Space is held is no new press.
        game.mouse(MouseButton::Left, true)
            .mouse(MouseButton::Left, false)
            .tick();
        assert_eq!(game.edges(Act::Jump), HOLDING);
        // Rolling from Space to the mouse button neither releases nor
        // presses.
        game.mouse(MouseButton::Left, true)
            .release(Key::Space)
            .tick();
        assert_eq!(game.edges(Act::Jump), HOLDING);
        // Down from one binding, then up from the other, within a tick: a
        // release, and the value follows the action, not the binding that
        // went down.
        game.press(Key::Space)
            .mouse(MouseButton::Left, false)
            .release(Key::Space)
            .tick();
        assert_eq!(game.edges(Act::Jump), RELEASED);
        assert_eq!(game.state.value(Act::Jump), 0.0);
    }

    #[test]
    fn composite_diagonals_are_normalized_and_opposites_cancel() {
        let mut game = Game::new();
        game.press(Key::W).press(Key::D).tick();
        let diagonal = game.movement();
        assert!(diagonal.x > 0.0 && diagonal.y < 0.0, "{diagonal:?}");
        assert_eq!(diagonal.x, -diagonal.y);
        let length = diagonal.length();
        assert!(length <= 1.0 && length > 1.0 - 1e-4, "{length}");
        assert_eq!(game.state.value(Act::Move), length);

        game.press(Key::S).tick();
        assert_eq!(game.movement(), Vec2::new(1.0, 0.0));
        game.press(Key::A).tick();
        assert_eq!(game.movement(), Vec2::ZERO);
        // All four keys are held, so the action is held though it points
        // nowhere.
        assert!(game.state.pressed(Act::Move));
        game.release(Key::W).release(Key::A).release(Key::D).tick();
        assert_eq!(game.movement(), Vec2::new(0.0, 1.0), "+Y is down");
    }

    #[test]
    fn a_tapped_composite_key_moves_for_one_tick() {
        let mut game = Game::new();
        game.press(Key::A).release(Key::A).tick();
        assert_eq!(game.movement(), Vec2::new(-1.0, 0.0));
        assert_eq!(game.edges(Act::Move), TAPPED);
        game.tick();
        assert_eq!(game.movement(), Vec2::ZERO);
    }

    #[test]
    fn a_key_tapped_while_the_composite_lets_go_moves_for_that_tick() {
        let mut game = Game::new();
        game.press(Key::W).tick();
        // D goes down and up while W goes up: the action is released, but
        // the tap of D still moves, as a tap of any composite key does.
        game.press(Key::D).release(Key::W).release(Key::D).tick();
        assert_eq!(game.edges(Act::Move), RELEASED);
        assert_eq!(game.movement(), Vec2::new(1.0, 0.0));
        game.tick();
        assert_eq!(game.movement(), Vec2::ZERO);
    }

    #[test]
    fn composites_in_two_slots_add_up_to_no_more_than_1() {
        let mut game = Game::new();
        let arrows = Binding::KeyComposite2 {
            up: Key::Up,
            down: Key::Down,
            left: Key::Left,
            right: Key::Right,
        };
        game.map.bind(Act::Move, 1, arrows).expect("fits");
        game.press(Key::W).press(Key::Up).tick();
        assert_eq!(game.movement(), Vec2::new(0.0, -1.0));
        game.release(Key::Up).press(Key::Right).tick();
        let diagonal = game.movement();
        assert_eq!(diagonal.x, -diagonal.y);
        assert!(diagonal.length() <= 1.0);
    }

    #[test]
    fn a_key_holds_an_axis1_at_1() {
        let mut game = Game::new();
        game.tick();
        assert_eq!(game.state.value(Act::Throttle), 0.0);
        game.press(Key::LeftShift).tick();
        assert_eq!(game.state.value(Act::Throttle), 1.0);
        assert_eq!(game.edges(Act::Throttle), PRESSED);
        assert_eq!(game.state.axis2(Act::Throttle), Vec2::ZERO);
    }

    #[test]
    fn losing_the_focus_releases_everything() {
        let mut game = Game::new();
        game.press(Key::Space).press(Key::D).tick();
        game.input.handle(&Event::Focused(false));
        game.tick();
        assert_eq!(game.edges(Act::Jump), RELEASED);
        assert_eq!(game.movement(), Vec2::ZERO);
        // The key's real release, when the window sees it again, is a
        // release of a key that is up.
        game.release(Key::Space).tick();
        assert_eq!(game.edges(Act::Jump), UP);
    }

    #[test]
    fn text_mode_releases_gameplay_actions_and_keeps_ui_actions() {
        let mut game = Game::new();
        game.press(Key::Space)
            .press(Key::W)
            .press(Key::Escape)
            .tick();
        game.text_mode(true).tick();
        assert!(game.input.text_mode());
        assert_eq!(game.edges(Act::Jump), RELEASED);
        assert_eq!(game.edges(Act::Move), RELEASED);
        assert_eq!(game.movement(), Vec2::ZERO);
        assert_eq!(
            game.edges(Act::Pause),
            HOLDING,
            "a ui_allowed action stays live"
        );
        // Typing presses nothing in the game, but a UI key works.
        game.release(Key::Escape)
            .press(Key::A)
            .press(Key::Space)
            .tick();
        assert_eq!(game.edges(Act::Jump), UP);
        assert_eq!(game.movement(), Vec2::ZERO);
        assert_eq!(game.edges(Act::Pause), RELEASED);
        game.press(Key::Escape).tick();
        assert_eq!(game.edges(Act::Pause), PRESSED);
    }

    #[test]
    fn leaving_text_mode_presses_nothing_that_is_still_held() {
        let mut game = Game::new();
        game.press(Key::W).tick();
        // Space goes down during text mode, and W was down when it began.
        game.text_mode(true)
            .press(Key::Space)
            .press(Key::Escape)
            .tick();
        game.text_mode(false).tick();
        assert!(!game.input.text_mode());
        assert_eq!(game.edges(Act::Jump), UP);
        assert_eq!(game.movement(), Vec2::ZERO);
        // The UI key that was live stays held, with no edge.
        assert_eq!(game.edges(Act::Pause), HOLDING);
        // Released and pressed again, the keys count.
        game.release(Key::Space).release(Key::W).tick();
        assert_eq!(game.edges(Act::Jump), UP);
        game.press(Key::Space).press(Key::W).tick();
        assert_eq!(game.edges(Act::Jump), PRESSED);
        assert_eq!(game.movement(), Vec2::new(0.0, -1.0));
    }

    #[test]
    fn text_mode_hides_mouse_buttons_too() {
        let mut game = Game::new();
        game.text_mode(true).mouse(MouseButton::Left, true).tick();
        assert_eq!(game.edges(Act::Jump), UP);
        game.text_mode(false).tick();
        assert_eq!(game.edges(Act::Jump), UP, "still held, so still hidden");
        game.mouse(MouseButton::Left, false)
            .mouse(MouseButton::Left, true)
            .tick();
        assert_eq!(game.edges(Act::Jump), PRESSED);
    }

    #[test]
    fn a_press_just_before_text_mode_is_a_tap() {
        let mut game = Game::new();
        game.press(Key::Space).text_mode(true).tick();
        assert_eq!(game.edges(Act::Jump), TAPPED);
        game.tick();
        assert_eq!(game.edges(Act::Jump), UP);
    }

    #[test]
    fn text_mode_on_and_off_within_a_tick_hides_what_was_held() {
        let mut game = Game::new();
        game.press(Key::Space).tick();
        game.text_mode(true).text_mode(false).tick();
        assert_eq!(game.edges(Act::Jump), RELEASED);
        game.tick();
        assert_eq!(game.edges(Act::Jump), UP);
    }

    #[test]
    fn the_wheel_is_counted_per_tick_and_the_pointer_is_the_latest() {
        let mut game = Game::new();
        for delta in [Vec2::new(0.0, 1.0), Vec2::new(0.5, 2.0)] {
            game.input.handle(&Event::MouseWheel(delta));
        }
        game.input.handle(&Event::MouseMoved(Vec2::new(10.0, 20.0)));
        game.input.handle(&Event::MouseMoved(Vec2::new(12.0, 21.0)));
        assert_eq!(game.input.wheel(), Vec2::ZERO, "not latched yet");
        game.tick();
        assert_eq!(game.input.wheel(), Vec2::new(0.5, 3.0));
        assert_eq!(game.input.pointer(), Vec2::new(12.0, 21.0));
        game.tick();
        assert_eq!(game.input.wheel(), Vec2::ZERO);
    }

    #[test]
    fn other_events_are_ignored() {
        let mut game = Game::new();
        game.input.handle(&Event::Focused(true));
        game.input.handle(&Event::CloseRequested);
        game.input
            .handle(&Event::Resized(nadir_core::UVec2::new(10, 10)));
        assert!(game.input.changes.is_empty());
        game.tick();
        assert_eq!(ActionState::<Act>::new(), game.state);
    }

    #[test]
    fn an_unbound_key_drives_nothing_and_bindings_apply_from_the_next_latch() {
        let mut game = Game::new();
        game.press(Key::K).tick();
        assert_eq!(game.state, ActionState::new());
        // Bound while held: held from the next tick on, with no edge, as
        // the spec's latch of the device state gives.
        game.map
            .bind(Act::Jump, 2, Binding::Key(Key::K))
            .expect("fits");
        game.tick();
        assert_eq!(game.edges(Act::Jump), HOLDING);
        game.map.unbind(Act::Jump, 2);
        game.tick();
        assert_eq!(game.edges(Act::Jump), UP);
    }

    #[test]
    fn changes_never_latched_use_bounded_memory() {
        let mut game = Game::new();
        for _ in 0..MAX_CHANGES {
            game.press(Key::Space).release(Key::Space);
        }
        assert!(game.input.changes.len() <= MAX_CHANGES);
        // The newest changes are kept, presses and releases among them.
        game.press(Key::Space).tick();
        assert_eq!(game.edges(Act::Jump), TAPPED);
        game.release(Key::Space).tick();
        assert_eq!(game.edges(Act::Jump), RELEASED);
    }
}
