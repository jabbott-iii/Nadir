//! Axis-aligned rectangles: [`Rect`] (`f32`) and [`IRect`] (`i32`).

use super::{IVec2, Vec2};

/// An axis-aligned rectangle with `f32` corners.
///
/// `min` is the top-left corner and `max` the bottom-right, because +Y is
/// down. The rectangle is half-open: it contains `min` but not `max`, so
/// rectangles that only share an edge do not intersect.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// The top-left corner (inclusive).
    pub min: Vec2,
    /// The bottom-right corner (exclusive).
    pub max: Vec2,
}

impl Rect {
    /// Creates a rectangle from its top-left and bottom-right corners.
    pub const fn new(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }

    /// Creates a rectangle from its top-left corner and size.
    pub fn from_min_size(min: Vec2, size: Vec2) -> Self {
        Self::new(min, min + size)
    }

    /// Creates a rectangle from its center and size.
    pub fn from_center_size(center: Vec2, size: Vec2) -> Self {
        let half = size * 0.5;
        Self::new(center - half, center + half)
    }

    /// Returns the width.
    pub fn width(self) -> f32 {
        self.max.x - self.min.x
    }

    /// Returns the height.
    pub fn height(self) -> f32 {
        self.max.y - self.min.y
    }

    /// Returns the width and height.
    pub fn size(self) -> Vec2 {
        self.max - self.min
    }

    /// Returns the center point.
    pub fn center(self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    /// Returns `true` if the width or height is zero or negative.
    pub fn is_empty(self) -> bool {
        self.max.x <= self.min.x || self.max.y <= self.min.y
    }

    /// Returns `true` if `point` is inside the rectangle. `min` is inside,
    /// `max` is not.
    pub fn contains(self, point: Vec2) -> bool {
        (self.min.x..self.max.x).contains(&point.x) && (self.min.y..self.max.y).contains(&point.y)
    }

    /// Returns `true` if the two rectangles overlap with a non-zero area.
    ///
    /// Empty rectangles intersect nothing.
    pub fn intersects(self, other: Self) -> bool {
        self.intersection(other).is_some()
    }

    /// Returns the overlapping region, or `None` if the rectangles do not
    /// overlap with a non-zero area.
    pub fn intersection(self, other: Self) -> Option<Self> {
        let overlap = Self::new(self.min.max(other.min), self.max.min(other.max));
        if overlap.is_empty() {
            None
        } else {
            Some(overlap)
        }
    }

    /// Returns the smallest rectangle that contains the corners of both.
    pub fn union(self, other: Self) -> Self {
        Self::new(self.min.min(other.min), self.max.max(other.max))
    }

    /// Returns the rectangle moved by `offset`.
    pub fn translate(self, offset: Vec2) -> Self {
        Self::new(self.min + offset, self.max + offset)
    }
}

/// An axis-aligned rectangle with `i32` corners: a pixel or tile region.
///
/// `min` is the top-left corner and `max` the bottom-right, because +Y is
/// down. The rectangle is half-open: it contains `min` but not `max`, so its
/// width in pixels is exactly `max.x - min.x`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct IRect {
    /// The top-left corner (inclusive).
    pub min: IVec2,
    /// The bottom-right corner (exclusive).
    pub max: IVec2,
}

impl IRect {
    /// Creates a rectangle from its top-left and bottom-right corners.
    pub const fn new(min: IVec2, max: IVec2) -> Self {
        Self { min, max }
    }

    /// Creates a rectangle from its top-left corner and size.
    pub fn from_min_size(min: IVec2, size: IVec2) -> Self {
        Self::new(min, min + size)
    }

    /// Returns the width.
    pub fn width(self) -> i32 {
        self.max.x - self.min.x
    }

    /// Returns the height.
    pub fn height(self) -> i32 {
        self.max.y - self.min.y
    }

    /// Returns the width and height.
    pub fn size(self) -> IVec2 {
        self.max - self.min
    }

    /// Returns `true` if the width or height is zero or negative.
    pub fn is_empty(self) -> bool {
        self.max.x <= self.min.x || self.max.y <= self.min.y
    }

    /// Returns `true` if `point` is inside the rectangle. `min` is inside,
    /// `max` is not.
    pub fn contains(self, point: IVec2) -> bool {
        (self.min.x..self.max.x).contains(&point.x) && (self.min.y..self.max.y).contains(&point.y)
    }

    /// Returns `true` if the two rectangles overlap with a non-zero area.
    ///
    /// Empty rectangles intersect nothing.
    pub fn intersects(self, other: Self) -> bool {
        self.intersection(other).is_some()
    }

    /// Returns the overlapping region, or `None` if the rectangles do not
    /// overlap with a non-zero area.
    pub fn intersection(self, other: Self) -> Option<Self> {
        let overlap = Self::new(self.min.max(other.min), self.max.min(other.max));
        if overlap.is_empty() {
            None
        } else {
            Some(overlap)
        }
    }

    /// Returns the smallest rectangle that contains the corners of both.
    pub fn union(self, other: Self) -> Self {
        Self::new(self.min.min(other.min), self.max.max(other.max))
    }

    /// Returns the rectangle moved by `offset`.
    pub fn translate(self, offset: IVec2) -> Self {
        Self::new(self.min + offset, self.max + offset)
    }

    /// Converts to a [`Rect`]. Coordinates beyond ±2²⁴ lose precision.
    pub fn as_rect(&self) -> Rect {
        Rect::new(self.min.as_vec2(), self.max.as_vec2())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestRng, for_cases};

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::new(Vec2::new(x0, y0), Vec2::new(x1, y1))
    }

    fn irect(x0: i32, y0: i32, x1: i32, y1: i32) -> IRect {
        IRect::new(IVec2::new(x0, y0), IVec2::new(x1, y1))
    }

    /// A rectangle with whole-number coordinates, so that shared edges and
    /// exact containment come up often. One in four has unordered corners
    /// and is usually inverted, and so empty; the rest are never empty.
    fn random_irect(rng: &mut TestRng) -> IRect {
        if rng.next_u32().is_multiple_of(4) {
            let mut corner = || IVec2::new(rng.range_i32(-8, 9), rng.range_i32(-8, 9));
            IRect::new(corner(), corner())
        } else {
            random_solid_irect(rng)
        }
    }

    /// A rectangle that is never empty.
    fn random_solid_irect(rng: &mut TestRng) -> IRect {
        let min = IVec2::new(rng.range_i32(-8, 8), rng.range_i32(-8, 8));
        let size = IVec2::new(rng.range_i32(1, 9), rng.range_i32(1, 9));
        IRect::from_min_size(min, size)
    }

    fn random_rect(rng: &mut TestRng) -> Rect {
        random_irect(rng).as_rect()
    }

    #[test]
    fn constructors_and_accessors() {
        let r = Rect::from_min_size(Vec2::new(1.0, 2.0), Vec2::new(4.0, 6.0));
        assert_eq!(r, rect(1.0, 2.0, 5.0, 8.0));
        assert_eq!(
            Rect::from_center_size(Vec2::new(3.0, 5.0), Vec2::new(4.0, 6.0)),
            r
        );
        assert_eq!(r.width(), 4.0);
        assert_eq!(r.height(), 6.0);
        assert_eq!(r.size(), Vec2::new(4.0, 6.0));
        assert_eq!(r.center(), Vec2::new(3.0, 5.0));
        assert_eq!(Rect::default(), rect(0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn emptiness() {
        assert!(!rect(0.0, 0.0, 1.0, 1.0).is_empty());
        assert!(rect(0.0, 0.0, 0.0, 1.0).is_empty());
        assert!(rect(0.0, 0.0, 1.0, 0.0).is_empty());
        assert!(rect(2.0, 0.0, 1.0, 1.0).is_empty());
        assert!(Rect::default().is_empty());
    }

    #[test]
    fn contains_is_half_open() {
        let r = rect(0.0, 0.0, 2.0, 2.0);
        assert!(r.contains(Vec2::new(0.0, 0.0)));
        assert!(r.contains(Vec2::new(1.5, 0.5)));
        assert!(!r.contains(Vec2::new(2.0, 1.0)));
        assert!(!r.contains(Vec2::new(1.0, 2.0)));
        assert!(!r.contains(Vec2::new(-0.1, 1.0)));
    }

    #[test]
    fn intersection_cases() {
        let a = rect(0.0, 0.0, 4.0, 4.0);
        // Partial overlap.
        assert_eq!(
            a.intersection(rect(2.0, 1.0, 6.0, 3.0)),
            Some(rect(2.0, 1.0, 4.0, 3.0))
        );
        // Fully contained.
        assert_eq!(
            a.intersection(rect(1.0, 1.0, 2.0, 2.0)),
            Some(rect(1.0, 1.0, 2.0, 2.0))
        );
        // Sharing an edge or a corner is not an overlap.
        assert_eq!(a.intersection(rect(4.0, 0.0, 8.0, 4.0)), None);
        assert_eq!(a.intersection(rect(4.0, 4.0, 8.0, 8.0)), None);
        // Disjoint.
        assert_eq!(a.intersection(rect(10.0, 10.0, 12.0, 12.0)), None);
        // Empty and inverted rectangles intersect nothing, even inside `a`.
        assert_eq!(a.intersection(rect(1.0, 1.0, 1.0, 3.0)), None);
        assert_eq!(a.intersection(rect(3.0, 3.0, 1.0, 1.0)), None);
        assert!(a.intersects(a));
        assert!(!Rect::default().intersects(Rect::default()));
    }

    #[test]
    fn union_and_translate() {
        let a = rect(0.0, 0.0, 2.0, 2.0);
        let b = rect(1.0, -3.0, 5.0, 1.0);
        assert_eq!(a.union(b), rect(0.0, -3.0, 5.0, 2.0));
        assert_eq!(
            a.translate(Vec2::new(10.0, -1.0)),
            rect(10.0, -1.0, 12.0, 1.0)
        );
    }

    #[test]
    fn irect_basics() {
        let r = IRect::from_min_size(IVec2::new(1, 2), IVec2::new(4, 6));
        assert_eq!(r, irect(1, 2, 5, 8));
        assert_eq!(r.width(), 4);
        assert_eq!(r.height(), 6);
        assert_eq!(r.size(), IVec2::new(4, 6));
        assert!(!r.is_empty());
        assert!(irect(0, 0, 0, 5).is_empty());
        assert!(irect(3, 0, 1, 5).is_empty());
        assert!(IRect::default().is_empty());
        assert!(r.contains(IVec2::new(1, 2)));
        assert!(r.contains(IVec2::new(4, 7)));
        assert!(!r.contains(IVec2::new(5, 7)));
        assert!(!r.contains(IVec2::new(4, 8)));
        assert_eq!(r.intersection(irect(3, 0, 10, 4)), Some(irect(3, 2, 5, 4)));
        assert_eq!(r.intersection(irect(5, 2, 9, 8)), None);
        assert!(r.intersects(irect(4, 7, 9, 9)));
        assert!(!r.intersects(irect(5, 8, 9, 9)));
        assert_eq!(r.union(irect(-1, 3, 2, 10)), irect(-1, 2, 5, 10));
        assert_eq!(r.translate(IVec2::new(-1, 1)), irect(0, 3, 4, 9));
        assert_eq!(r.as_rect(), rect(1.0, 2.0, 5.0, 8.0));
    }

    #[test]
    fn rect_intersection_is_symmetric() {
        for_cases("rect intersection symmetry", 2000, |rng| {
            let a = random_rect(rng);
            let b = random_rect(rng);
            let (ab, ba) = (a.intersection(b), b.intersection(a));
            if ab != ba {
                return Err(format!("{a:?} ∩ {b:?} = {ab:?}, but reversed = {ba:?}"));
            }
            if a.intersects(b) != b.intersects(a) || a.intersects(b) != ab.is_some() {
                return Err(format!(
                    "intersects disagrees with intersection for {a:?} and {b:?}"
                ));
            }
            if a.union(b) != b.union(a) {
                return Err(format!("union is not symmetric for {a:?} and {b:?}"));
            }
            Ok(())
        });
    }

    #[test]
    fn irect_intersection_is_symmetric() {
        for_cases("irect intersection symmetry", 2000, |rng| {
            let a = random_irect(rng);
            let b = random_irect(rng);
            let (ab, ba) = (a.intersection(b), b.intersection(a));
            if ab != ba {
                return Err(format!("{a:?} ∩ {b:?} = {ab:?}, but reversed = {ba:?}"));
            }
            if a.intersects(b) != b.intersects(a) || a.intersects(b) != ab.is_some() {
                return Err(format!(
                    "intersects disagrees with intersection for {a:?} and {b:?}"
                ));
            }
            Ok(())
        });
    }

    #[test]
    fn intersection_matches_point_membership() {
        for_cases("intersection matches point membership", 500, |rng| {
            let a = random_solid_irect(rng);
            let b = random_solid_irect(rng);
            let overlap = a.intersection(b);
            let (fa, fb) = (a.as_rect(), b.as_rect());
            let foverlap = fa.intersection(fb);
            if foverlap != overlap.map(|r| r.as_rect()) {
                return Err(format!("Rect and IRect disagree for {a:?} and {b:?}"));
            }
            // A point is in the overlap exactly when it is in both inputs.
            for y in -9..18 {
                for x in -9..18 {
                    let p = IVec2::new(x, y);
                    let in_both = a.contains(p) && b.contains(p);
                    if overlap.is_some_and(|r| r.contains(p)) != in_both {
                        return Err(format!(
                            "{p:?} membership is wrong for {a:?} ∩ {b:?} = {overlap:?}"
                        ));
                    }
                    let fp = p.as_vec2() + Vec2::splat(0.5);
                    if foverlap.is_some_and(|r| r.contains(fp))
                        != (fa.contains(fp) && fb.contains(fp))
                    {
                        return Err(format!(
                            "{fp:?} membership is wrong for {fa:?} ∩ {fb:?} = {foverlap:?}"
                        ));
                    }
                }
            }
            if let Some(r) = overlap {
                let bounds = a.union(b);
                if r.union(bounds) != bounds || r.is_empty() {
                    return Err(format!(
                        "{a:?} ∩ {b:?} = {r:?} is empty or outside the union"
                    ));
                }
            }
            Ok(())
        });
    }
}
