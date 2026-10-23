//! [`Interpolated`]: a value's state at the last two ticks, for drawing
//! between them.

use nadir_core::Vec2;

/// What [`Interpolated`] can blend.
pub trait Lerp: Copy {
    /// The value `t` of the way from `self` to `other`.
    fn lerp(self, other: Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(self, other: Self, t: f32) -> Self {
        nadir_core::math::lerp(self, other, t)
    }
}

impl Lerp for Vec2 {
    fn lerp(self, other: Self, t: f32) -> Self {
        Vec2::lerp(self, other, t)
    }
}

/// A value as the last two ticks left it, so that a frame between them can
/// draw it part of the way from one to the other, by the frame's `alpha`.
///
/// A fixed system calls [`Interpolated::snapshot`] at the start of each
/// tick, before it changes `current`. A frame system draws
/// [`Interpolated::lerp`] of the frame's `alpha`.
///
/// ```
/// use nadir::Interpolated;
/// use nadir::prelude::Vec2;
///
/// let mut position = Interpolated::new(Vec2::ZERO);
/// // A tick: remember where it was, then move.
/// position.snapshot();
/// position.current.x += 2.0;
/// // A frame a quarter of the way to the next tick draws it here.
/// assert_eq!(position.lerp(0.25), Vec2::new(0.5, 0.0));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Interpolated<T> {
    /// As the tick before the last left it.
    pub previous: T,
    /// As the last tick left it.
    pub current: T,
}

impl<T: Lerp> Interpolated<T> {
    /// A value that has not moved.
    pub fn new(value: T) -> Self {
        Self {
            previous: value,
            current: value,
        }
    }

    /// Remembers the current value as the previous one: call it at the start
    /// of each tick.
    pub fn snapshot(&mut self) {
        self.previous = self.current;
    }

    /// The value `alpha` of the way from the previous tick to the last.
    pub fn lerp(&self, alpha: f32) -> T {
        self.previous.lerp(self.current, alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_blends_from_its_previous_to_its_current_tick() {
        let mut value = Interpolated::new(1.0f32);
        assert_eq!(value.lerp(0.5), 1.0, "it has not moved");
        value.snapshot();
        value.current = 3.0;
        assert_eq!(
            [value.lerp(0.0), value.lerp(0.5), value.lerp(1.0)],
            [1.0, 2.0, 3.0]
        );
        value.snapshot();
        assert_eq!(value.lerp(0.5), 3.0, "a tick in which it did not move");
    }
}
