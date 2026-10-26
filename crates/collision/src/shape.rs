//! [`Shape`], [`Collider`] and the [`LayerMask`]s that say who collides
//! with whom.

use std::ops::{BitAnd, BitOr, BitOrAssign};

use nadir_core::{Rect, Vec2};

/// A set of the 32 collision layers, as bits.
///
/// A collider's `layer` is what it is, and its `mask` what it collides with
/// or detects: a collider meets another when its mask
/// [intersects](LayerMask::intersects) the other's layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LayerMask(pub u32);

impl LayerMask {
    /// No layers.
    pub const NONE: Self = Self(0);

    /// Every layer.
    pub const ALL: Self = Self(u32::MAX);

    /// Layer `n` alone.
    ///
    /// # Panics
    ///
    /// If `n` is 32 or more.
    pub const fn bit(n: u32) -> Self {
        assert!(n < 32, "there are 32 collision layers, numbered 0 to 31");
        Self(1 << n)
    }

    /// Whether the two share a layer.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for LayerMask {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for LayerMask {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for LayerMask {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

/// The shape of a collider, around its center. Sizes are in pixels, and
/// must be finite and not negative.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// An axis-aligned box.
    Aabb {
        /// Half its width and height.
        half: Vec2,
    },
    /// A circle.
    Circle {
        /// Its radius.
        radius: f32,
    },
}

impl Shape {
    /// Half the width and height of the box around the shape.
    pub fn half_extents(self) -> Vec2 {
        match self {
            Self::Aabb { half } => half,
            Self::Circle { radius } => Vec2::splat(radius),
        }
    }
}

/// Whether a collider stops others or only notices them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColliderKind {
    /// It stops movers whose mask has its layer.
    #[default]
    Solid,
    /// It stops nothing, and reports the colliders it overlaps whose layer
    /// its mask has ([`Triggers`](crate::Triggers)).
    Trigger,
}

/// A shape, placed relative to its entity, with its layers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Collider {
    /// The shape.
    pub shape: Shape,
    /// From the entity's position, which is its feet, to the shape's
    /// center.
    pub offset: Vec2,
    /// What it is.
    pub layer: LayerMask,
    /// What it collides with, or detects.
    pub mask: LayerMask,
    /// Whether it stops others.
    pub kind: ColliderKind,
}

impl Collider {
    /// A solid `shape`, centered on the entity, on every layer and
    /// colliding with every layer.
    pub fn solid(shape: Shape) -> Self {
        Self {
            shape,
            offset: Vec2::ZERO,
            layer: LayerMask::ALL,
            mask: LayerMask::ALL,
            kind: ColliderKind::Solid,
        }
    }

    /// A trigger `shape`, centered on the entity, on every layer and
    /// detecting every layer.
    pub fn trigger(shape: Shape) -> Self {
        Self {
            kind: ColliderKind::Trigger,
            ..Self::solid(shape)
        }
    }

    /// The center of the shape, for an entity at `position`.
    pub fn center(&self, position: Vec2) -> Vec2 {
        position + self.offset
    }

    /// The box around the shape, for an entity at `position`.
    pub fn bounds(&self, position: Vec2) -> Rect {
        let center = self.center(position);
        let half = self.shape.half_extents();
        Rect::new(center - half, center + half)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_meet_when_they_share_a_layer() {
        let walls = LayerMask::bit(0);
        let enemies = LayerMask::bit(3);
        assert_eq!((walls | enemies).0, 0b1001);
        assert!((walls | enemies).intersects(enemies));
        assert!(!walls.intersects(enemies));
        assert!(!LayerMask::NONE.intersects(LayerMask::ALL));
        assert_eq!((walls | enemies) & enemies, enemies);
        assert_eq!(LayerMask::default(), LayerMask::NONE);
    }

    #[test]
    #[should_panic(expected = "32 collision layers")]
    fn there_are_32_layers() {
        let _ = LayerMask::bit(32);
    }

    #[test]
    fn bounds_follow_the_offset() {
        let collider = Collider {
            offset: Vec2::new(0.0, -4.0),
            ..Collider::solid(Shape::Circle { radius: 3.0 })
        };
        assert_eq!(
            collider.bounds(Vec2::new(10.0, 10.0)),
            Rect::new(Vec2::new(7.0, 3.0), Vec2::new(13.0, 9.0))
        );
        assert_eq!(
            Collider::trigger(Shape::Circle { radius: 1.0 }).kind,
            ColliderKind::Trigger
        );
    }
}
