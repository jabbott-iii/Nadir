//! The state of every action for one tick.

use std::fmt;
use std::marker::PhantomData;

use nadir_core::Vec2;

use crate::{ActionKind, ActionSet};

/// An axis value of 1, as stored: values are kept as `i16` steps of
/// 1/32767, so that a replay can hold them exactly.
pub(crate) const ONE: i16 = i16::MAX;

/// The action is held.
pub(crate) const HELD: u8 = 1;
/// The action went down since the last tick.
pub(crate) const JUST_PRESSED: u8 = 2;
/// The action went up since the last tick.
pub(crate) const JUST_RELEASED: u8 = 4;

/// One action's state for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Latched {
    /// [`HELD`], [`JUST_PRESSED`] and [`JUST_RELEASED`].
    pub(crate) flags: u8,
    /// The value: a button's and an `Axis1`'s in the first, and an
    /// `Axis2`'s x and y.
    pub(crate) axes: [i16; 2],
}

/// The state of every action of the set `A` for one tick: what gameplay
/// code reads.
///
/// [`Input::latch`](crate::Input::latch) fills it from live input, and
/// [`InputPlayer`](crate::InputPlayer) from a replay. Values are stored in
/// steps of 1/32767 either way, so a replayed state equals the live one.
pub struct ActionState<A> {
    actions: Vec<Latched>,
    set: PhantomData<A>,
}

impl<A: ActionSet> ActionState<A> {
    /// A state with every action released.
    pub fn new() -> Self {
        Self {
            actions: vec![Latched::default(); A::ALL.len()],
            set: PhantomData,
        }
    }

    /// Whether `action` is held this tick. A press and release between two
    /// ticks counts as held for one tick, so that the tap is not lost.
    pub fn pressed(&self, action: A) -> bool {
        self.flags(action) & HELD != 0
    }

    /// Whether `action` went down since the last tick.
    pub fn just_pressed(&self, action: A) -> bool {
        self.flags(action) & JUST_PRESSED != 0
    }

    /// Whether `action` went up since the last tick.
    pub fn just_released(&self, action: A) -> bool {
        self.flags(action) & JUST_RELEASED != 0
    }

    /// The value of `action`: 0 or 1 for a [`ActionKind::Button`], 0 to 1
    /// for an [`ActionKind::Axis1`], and the length of the direction for an
    /// [`ActionKind::Axis2`].
    pub fn value(&self, action: A) -> f32 {
        match action.kind() {
            ActionKind::Button | ActionKind::Axis1 => to_f32(self.get(action).axes[0]),
            ActionKind::Axis2 => self.axis2(action).length(),
        }
    }

    /// The direction of an [`ActionKind::Axis2`] action, no longer than 1,
    /// with +Y down. Zero for the other kinds.
    ///
    /// A key tapped during the tick counts, even in a tick in which the
    /// action as a whole is just released and not held.
    pub fn axis2(&self, action: A) -> Vec2 {
        match action.kind() {
            ActionKind::Axis2 => {
                let [x, y] = self.get(action).axes;
                Vec2::new(to_f32(x), to_f32(y))
            }
            ActionKind::Button | ActionKind::Axis1 => Vec2::ZERO,
        }
    }

    pub(crate) fn get(&self, action: A) -> Latched {
        self.actions[action.index()]
    }

    pub(crate) fn set(&mut self, action: A, latched: Latched) {
        self.actions[action.index()] = latched;
    }

    fn flags(&self, action: A) -> u8 {
        self.get(action).flags
    }
}

impl<A: ActionSet> Default for ActionState<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A> Clone for ActionState<A> {
    fn clone(&self) -> Self {
        Self {
            actions: self.actions.clone(),
            set: PhantomData,
        }
    }
}

impl<A> PartialEq for ActionState<A> {
    fn eq(&self, other: &Self) -> bool {
        self.actions == other.actions
    }
}

impl<A> Eq for ActionState<A> {}

impl<A: ActionSet> fmt::Debug for ActionState<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(
                A::ALL
                    .iter()
                    .map(|&action| (action.name(), self.get(action))),
            )
            .finish()
    }
}

fn to_f32(value: i16) -> f32 {
    f32::from(value) / f32::from(ONE)
}

/// Rounds a direction no longer than 1 (give or take rounding) to the steps
/// it is stored in, so that the stored one is no longer than 1.
pub(crate) fn quantize2(direction: Vec2) -> [i16; 2] {
    let step = |value: f32| ((value * f32::from(ONE)) as i16).clamp(-ONE, ONE);
    let [mut x, mut y] = [step(direction.x), step(direction.y)];
    // Each component is rounded toward zero, but the f32 direction can be a
    // little longer than 1 to begin with. A step of the longer component
    // toward zero takes off at least 2 · 23170 − 1 from the squared length,
    // more than f32 rounding adds, so this runs at most once or twice.
    while longer_than_one(x, y) {
        let toward_zero = |value: i16| value - value.signum();
        if x.unsigned_abs() >= y.unsigned_abs() {
            x = toward_zero(x);
        } else {
            y = toward_zero(y);
        }
    }
    [x, y]
}

/// Whether a stored direction is longer than 1.
pub(crate) fn longer_than_one(x: i16, y: i16) -> bool {
    let square = |value: i16| i64::from(value) * i64::from(value);
    square(x) + square(y) > square(ONE)
}

#[cfg(test)]
mod tests {
    use nadir_core::testing::check;

    use super::*;
    use crate::tests::Act;

    #[test]
    fn a_new_state_has_everything_released() {
        let state = ActionState::<Act>::new();
        for &action in Act::ALL {
            assert!(!state.pressed(action));
            assert!(!state.just_pressed(action));
            assert!(!state.just_released(action));
            assert_eq!(state.value(action), 0.0);
            assert_eq!(state.axis2(action), Vec2::ZERO);
        }
        assert_eq!(state, ActionState::default());
    }

    #[test]
    fn values_read_back_by_kind() {
        let mut state = ActionState::<Act>::new();
        state.set(
            Act::Jump,
            Latched {
                flags: HELD | JUST_PRESSED,
                axes: [ONE, 0],
            },
        );
        state.set(
            Act::Throttle,
            Latched {
                flags: HELD,
                axes: [ONE / 2, 0],
            },
        );
        state.set(
            Act::Move,
            Latched {
                flags: HELD,
                axes: [-ONE, 0],
            },
        );
        assert!(state.pressed(Act::Jump) && state.just_pressed(Act::Jump));
        assert!(!state.just_released(Act::Jump));
        assert_eq!(state.value(Act::Jump), 1.0);
        assert_eq!(state.axis2(Act::Jump), Vec2::ZERO);
        assert!((state.value(Act::Throttle) - 0.5).abs() < 1e-4);
        assert_eq!(state.axis2(Act::Move), Vec2::new(-1.0, 0.0));
        assert_eq!(state.value(Act::Move), 1.0);
        let text = format!("{state:?}");
        assert!(text.starts_with("{\"move\": Latched { flags: 1"), "{text}");
    }

    #[test]
    fn directions_round_toward_zero() {
        // 32767 / √2 = 23169.77…
        let diagonal = Vec2::new(1.0, -1.0).normalize_or_zero();
        assert_eq!(quantize2(diagonal), [23169, -23169]);
        assert_eq!(quantize2(Vec2::new(0.0, 1.0)), [0, ONE]);
        assert_eq!(quantize2(Vec2::new(f32::NAN, -2.0)), [0, -ONE]);
    }

    #[test]
    fn a_direction_just_over_1_is_shortened() {
        // Its f32 length is 1, but rounded toward zero it is
        // 32767² + 3², over 1.
        assert_eq!(quantize2(Vec2::new(1.0, 1e-4)), [32766, 3]);
    }

    #[test]
    fn stored_directions_are_never_longer_than_1() {
        check(
            "quantize2 length",
            20_000,
            |rng| {
                let angle = rng.range(0..1_000_000u32) as f32 / 1_000_000.0 * std::f32::consts::TAU;
                let length = 1.0 + (rng.range(0..3u32) as f32 - 1.0) * 1e-6;
                Vec2::new(angle.cos(), angle.sin()) * length
            },
            |direction| {
                let [x, y] = quantize2(*direction);
                if longer_than_one(x, y) {
                    return Err(format!("[{x}, {y}] is longer than 1"));
                }
                if x.unsigned_abs() > (direction.x.abs() * f32::from(ONE)) as u16 + 1
                    || y.unsigned_abs() > (direction.y.abs() * f32::from(ONE)) as u16 + 1
                {
                    return Err(format!("[{x}, {y}] grew"));
                }
                Ok(())
            },
        );
    }
}
