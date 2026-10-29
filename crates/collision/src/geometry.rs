//! The geometry under the queries, casts and moves: overlap tests, how far
//! one shape reaches past another along an axis, and rays against the
//! Minkowski sum of two shapes.
//!
//! Two shapes overlap when the distance between them is negative: shapes
//! that only touch do not overlap, and neither do two points.

use nadir_core::{IVec2, Rect, Vec2};

use crate::{EPSILON, Shape};

/// X or Y.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    X,
    Y,
}

impl Axis {
    /// The component of `v` along the axis.
    pub(crate) fn of(self, v: Vec2) -> f32 {
        match self {
            Self::X => v.x,
            Self::Y => v.y,
        }
    }

    /// The component of `v` across the axis.
    pub(crate) fn across(self, v: Vec2) -> f32 {
        match self {
            Self::X => v.y,
            Self::Y => v.x,
        }
    }

    /// `v` with its component along the axis set to `value`.
    pub(crate) fn with(self, v: Vec2, value: f32) -> Vec2 {
        match self {
            Self::X => Vec2::new(value, v.y),
            Self::Y => Vec2::new(v.x, value),
        }
    }

    /// The unit vector along the axis, pointing `sign`'s way.
    pub(crate) fn unit(self, sign: f32) -> Vec2 {
        match self {
            Self::X => Vec2::new(sign, 0.0),
            Self::Y => Vec2::new(0.0, sign),
        }
    }
}

/// `v` rounded down to whole numbers, saturating at the `i32` range; NaN
/// becomes 0. As `v.floor().as_ivec2()`, without the call to the C
/// library that `floor` costs on targets without SSE4.1.
pub(crate) fn floor(v: Vec2) -> IVec2 {
    let down = |x: f32| {
        let truncated = x as i32;
        truncated.saturating_sub(i32::from(truncated as f32 > x))
    };
    IVec2::new(down(v.x), down(v.y))
}

/// A shape where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placed {
    /// The shape's center.
    pub(crate) center: Vec2,
    /// The shape.
    pub(crate) shape: Shape,
}

impl Placed {
    pub(crate) fn new(center: Vec2, shape: Shape) -> Self {
        Self { center, shape }
    }
}

/// Whether `a` and `b` overlap.
pub(crate) fn overlaps(a: Placed, b: Placed) -> bool {
    let apart = (b.center - a.center).abs();
    match (a.shape, b.shape) {
        (Shape::Aabb { half: a }, Shape::Aabb { half: b }) => {
            apart.x < a.x + b.x && apart.y < a.y + b.y
        }
        (Shape::Circle { radius: a }, Shape::Circle { radius: b }) => {
            apart.length_squared() < (a + b) * (a + b)
        }
        (Shape::Aabb { half }, Shape::Circle { radius })
        | (Shape::Circle { radius }, Shape::Aabb { half }) => box_circle(apart, half, radius),
    }
}

/// Whether the boxes `a` and `b` overlap, either of them perhaps no more
/// than a point: boxes that only touch do not. False if either is not a
/// number.
pub(crate) fn boxes_overlap(a: Rect, b: Rect) -> bool {
    a.min.x < b.max.x && a.max.x > b.min.x && a.min.y < b.max.y && a.max.y > b.min.y
}

/// Whether `placed` overlaps the box `rect`, tested against the box's
/// edges, so that a box far wider than the shape keeps the shape's
/// precision.
pub(crate) fn overlaps_rect(placed: Placed, rect: Rect) -> bool {
    let center = placed.center;
    match placed.shape {
        Shape::Aabb { half } => {
            center.x - half.x < rect.max.x
                && center.x + half.x > rect.min.x
                && center.y - half.y < rect.max.y
                && center.y + half.y > rect.min.y
        }
        Shape::Circle { radius } => {
            let inside = center.x > rect.min.x
                && center.x < rect.max.x
                && center.y > rect.min.y
                && center.y < rect.max.y;
            inside || (center - center.clamp(rect.min, rect.max)).length_squared() < radius * radius
        }
    }
}

/// Whether a box of half size `half` and a circle of `radius` overlap,
/// their centers `apart` (each component non-negative).
fn box_circle(apart: Vec2, half: Vec2, radius: f32) -> bool {
    if apart.x < half.x && apart.y < half.y {
        // The circle's center is inside the box.
        return true;
    }
    let outside = (apart - half).max(Vec2::ZERO);
    outside.length_squared() < radius * radius
}

/// The distance from `point` to the shape `placed`: 0 inside it.
pub(crate) fn distance_to(placed: Placed, point: Vec2) -> f32 {
    let apart = (point - placed.center).abs();
    match placed.shape {
        Shape::Aabb { half } => (apart - half).max(Vec2::ZERO).length(),
        Shape::Circle { radius } => (apart.length() - radius).max(0.0),
    }
}

/// How far along `axis` the center of `mover` reaches past the center of
/// `other` while they overlap, when their centers are `across` apart
/// across the axis: they overlap while the centers are less than this far
/// apart along it.
///
/// `None` if they overlap across the axis by [`EPSILON`] or less, so that
/// a shape touching a wall, or sunk into it by rounding, slides along it.
pub(crate) fn reach(mover: Shape, other: Shape, axis: Axis, across: f32) -> Option<f32> {
    let (mover_half, other_half) = (mover.half_extents(), other.half_extents());
    let depth = axis.across(mover_half) + axis.across(other_half) - across;
    if depth.is_nan() || depth <= EPSILON {
        return None;
    }
    let along = |half: Vec2| axis.of(half);
    let round = |radius: f32, gap: f32| (radius * radius - gap * gap).max(0.0).sqrt();
    Some(match (mover, other) {
        (Shape::Aabb { .. }, Shape::Aabb { .. }) => along(mover_half) + along(other_half),
        (Shape::Circle { radius: a }, Shape::Circle { radius: b }) => round(a + b, across),
        (Shape::Aabb { half }, Shape::Circle { radius })
        | (Shape::Circle { radius }, Shape::Aabb { half }) => {
            along(half) + round(radius, (across - axis.across(half)).max(0.0))
        }
    })
}

/// Which sides of a box are closed. A tile's side is closed when the tile
/// beyond it is solid too: the two are one wall there.
///
/// A cast running exactly along an axis grazes a box it overlaps across
/// the motion by [`EPSILON`] or less, as [`reach`] has it for moves: it
/// counts as inside only from `EPSILON` in past an open side, but from
/// `EPSILON` out past a closed one. So a line along the outside of a wall
/// passes, and one along the seam between two of its tiles, or near it,
/// does not slip through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Closed {
    /// The left and right sides.
    pub(crate) x: [bool; 2],
    /// The top and bottom sides.
    pub(crate) y: [bool; 2],
}

impl Closed {
    fn of(self, axis: Axis) -> [bool; 2] {
        match axis {
            Axis::X => self.x,
            Axis::Y => self.y,
        }
    }
}

/// `v` scaled to length 1, or zero if it is zero or not finite. It does not
/// overflow for vectors too long to square.
pub(crate) fn unit(v: Vec2) -> Vec2 {
    let largest = v.x.abs().max(v.y.abs());
    if !(v.x.is_finite() && v.y.is_finite()) || largest == 0.0 {
        return Vec2::ZERO;
    }
    let scaled = v / largest;
    scaled / scaled.length()
}

/// The length of `v`, without overflow for vectors too long to square: it
/// is infinite only past `f32::MAX`.
fn length(v: Vec2) -> f32 {
    let largest = v.x.abs().max(v.y.abs());
    if largest == 0.0 {
        return 0.0;
    }
    largest * (v / largest).length()
}

/// Where a ray from `origin` along `delta` first meets a shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Ray {
    /// It misses within `0..=1`.
    Miss,
    /// It starts inside.
    Inside,
    /// It enters at `t`, a fraction of `delta`, through a surface facing
    /// `normal`.
    Hit { t: f32, normal: Vec2 },
}

/// A ray against the box `rect`, by slabs. A ray that only grazes an edge
/// or a corner misses, and so does one along a side with `closed` as
/// [`Closed`] says.
pub(crate) fn ray_box(origin: Vec2, delta: Vec2, rect: Rect, closed: Closed) -> Ray {
    let mut enter = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    let mut normal = Vec2::ZERO;
    for axis in [Axis::X, Axis::Y] {
        let (from, step) = (axis.of(origin), axis.of(delta));
        let (min, max) = (axis.of(rect.min), axis.of(rect.max));
        if step == 0.0 {
            // Parallel to this slab: inside it all along, or never.
            let [low, high] = closed.of(axis);
            let low = if low { min - EPSILON } else { min + EPSILON };
            let high = if high { max + EPSILON } else { max - EPSILON };
            if !(from > low && from < high) {
                return Ray::Miss;
            }
            continue;
        }
        let (near, far) = if step > 0.0 {
            ((min - from) / step, (max - from) / step)
        } else {
            ((max - from) / step, (min - from) / step)
        };
        if near > enter {
            enter = near;
            normal = axis.unit(-step.signum());
        }
        exit = exit.min(far);
    }
    classify(enter, exit, normal)
}

/// A ray against the open circle of `radius` around `center`, worked from
/// the point of the ray nearest the center, so that long rays keep their
/// precision.
///
/// A ray that passes within [`EPSILON`] of the circle's surface grazes
/// it, as [`reach`] has it for moves. One that starts within `EPSILON`
/// inside hits at once if it goes deeper than that, and grazes otherwise.
pub(crate) fn ray_circle(origin: Vec2, delta: Vec2, center: Vec2, radius: f32) -> Ray {
    let from = origin - center;
    let distance = from.length();
    if distance < radius && radius - distance > EPSILON {
        return Ray::Inside;
    }
    let span = length(delta);
    if span == 0.0 {
        return Ray::Miss;
    }
    let direction = unit(delta);
    // How far along the ray its nearest point to the center is, and how
    // near.
    let along = -from.dot(direction);
    let nearest = (from + direction * along).length();
    if distance < radius {
        let t = (along / span).clamp(0.0, 1.0);
        let deepest = (from + delta * t).length();
        return if radius - deepest > EPSILON {
            Ray::Hit {
                t: 0.0,
                normal: unit(from),
            }
        } else {
            Ray::Miss
        };
    }
    if nearest.is_nan() || radius - nearest <= EPSILON {
        return Ray::Miss;
    }
    let half_chord = ((radius - nearest) * (radius + nearest)).sqrt();
    let (enter, exit) = ((along - half_chord) / span, (along + half_chord) / span);
    let normal = unit(from + direction * (along - half_chord));
    classify(enter, exit, normal)
}

/// A ray against the box `rect` grown by `radius` all round, with rounded
/// corners: two crossed boxes and a circle at each corner.
pub(crate) fn ray_rounded(
    origin: Vec2,
    delta: Vec2,
    rect: Rect,
    radius: f32,
    closed: Closed,
) -> Ray {
    let mut best = Ray::Miss;
    let mut consider = |ray: Ray| match (ray, best) {
        (Ray::Inside, _) | (_, Ray::Inside) => best = Ray::Inside,
        (Ray::Hit { t, .. }, Ray::Hit { t: best_t, .. }) if t >= best_t => {}
        (Ray::Hit { .. }, _) => best = ray,
        (Ray::Miss, _) => {}
    };
    let wide = Vec2::new(radius, 0.0);
    let tall = Vec2::new(0.0, radius);
    consider(ray_box(
        origin,
        delta,
        Rect::new(rect.min - wide, rect.max + wide),
        closed,
    ));
    consider(ray_box(
        origin,
        delta,
        Rect::new(rect.min - tall, rect.max + tall),
        closed,
    ));
    for corner in [
        rect.min,
        Vec2::new(rect.max.x, rect.min.y),
        Vec2::new(rect.min.x, rect.max.y),
        rect.max,
    ] {
        consider(ray_circle(origin, delta, corner, radius));
    }
    best
}

/// Sorts a ray's slab or root interval `enter..exit` into a [`Ray`].
fn classify(enter: f32, exit: f32, normal: Vec2) -> Ray {
    if enter.is_nan() || exit.is_nan() || enter >= exit || exit <= 0.0 || enter > 1.0 {
        Ray::Miss
    } else if enter < 0.0 {
        Ray::Inside
    } else {
        Ray::Hit { t: enter, normal }
    }
}

/// What a cast is against: a box by its corners, so that neighbouring
/// tiles share their edges exactly, or a circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Target {
    Box(Rect),
    Circle { center: Vec2, radius: f32 },
}

impl Target {
    /// The target of a placed shape.
    pub(crate) fn of(placed: Placed) -> Self {
        match placed.shape {
            Shape::Aabb { half } => {
                Self::Box(Rect::new(placed.center - half, placed.center + half))
            }
            Shape::Circle { radius } => Self::Circle {
                center: placed.center,
                radius,
            },
        }
    }

    fn center(self) -> Vec2 {
        match self {
            Self::Box(rect) => (rect.min + rect.max) * 0.5,
            Self::Circle { center, .. } => center,
        }
    }
}

/// Where the shape `moving`, its center going from `origin` along `delta`,
/// first meets `target`, a box with `closed` sides or a circle: the ray
/// against their Minkowski sum.
///
/// If it starts inside `target`, it stops at once if it moves toward
/// `target`'s center, facing back along `delta`, and passes otherwise: so
/// a cast can leave what it starts in, but not go deeper into it.
pub(crate) fn cast(
    moving: Shape,
    origin: Vec2,
    delta: Vec2,
    target: Target,
    closed: Closed,
) -> Option<(f32, Vec2)> {
    let ray = match (moving, target) {
        (Shape::Aabb { half }, Target::Box(rect)) => ray_box(
            origin,
            delta,
            Rect::new(rect.min - half, rect.max + half),
            closed,
        ),
        (Shape::Circle { radius }, Target::Box(rect)) => {
            ray_rounded(origin, delta, rect, radius, closed)
        }
        (Shape::Aabb { half }, Target::Circle { center, radius }) => {
            let core = Rect::new(center - half, center + half);
            ray_rounded(origin, delta, core, radius, Closed::default())
        }
        (Shape::Circle { radius: a }, Target::Circle { center, radius: b }) => {
            ray_circle(origin, delta, center, a + b)
        }
    };
    match ray {
        Ray::Miss => None,
        Ray::Hit { t, normal } => Some((t, normal)),
        Ray::Inside => {
            // Along the direction, not `delta`, so that a long motion does
            // not overflow.
            let direction = unit(delta);
            (direction.dot(origin - target.center()) < 0.0).then_some((0.0, -direction))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aabb(x: f32, y: f32) -> Shape {
        Shape::Aabb {
            half: Vec2::new(x, y),
        }
    }

    fn circle(radius: f32) -> Shape {
        Shape::Circle { radius }
    }

    #[test]
    fn floor_rounds_down_and_saturates() {
        let cases = [
            (Vec2::new(-1.5, 1.5), IVec2::new(-2, 1)),
            (Vec2::new(-2.0, 2.0), IVec2::new(-2, 2)),
            (Vec2::new(-0.0, 0.25), IVec2::new(0, 0)),
            (Vec2::new(f32::NAN, -1e-30), IVec2::new(0, -1)),
            (Vec2::new(3e9, -3e9), IVec2::new(i32::MAX, i32::MIN)),
            (
                Vec2::new(f32::INFINITY, f32::NEG_INFINITY),
                IVec2::new(i32::MAX, i32::MIN),
            ),
        ];
        for (v, expected) in cases {
            assert_eq!(floor(v), expected, "{v:?}");
            assert_eq!(floor(v), v.floor().as_ivec2(), "{v:?}");
        }
    }

    #[test]
    fn touching_shapes_do_not_overlap() {
        let at = |x: f32, y: f32, shape| Placed::new(Vec2::new(x, y), shape);
        assert!(!overlaps(
            at(0.0, 0.0, aabb(1.0, 1.0)),
            at(2.0, 0.0, aabb(1.0, 1.0))
        ));
        assert!(overlaps(
            at(0.0, 0.0, aabb(1.0, 1.0)),
            at(1.99, 0.0, aabb(1.0, 1.0))
        ));
        assert!(!overlaps(
            at(0.0, 0.0, circle(1.0)),
            at(0.0, 2.0, circle(1.0))
        ));
        assert!(!overlaps(
            at(0.0, 0.0, aabb(1.0, 1.0)),
            at(2.0, 2.0, circle(1.4))
        ));
        assert!(overlaps(
            at(0.0, 0.0, aabb(1.0, 1.0)),
            at(2.0, 2.0, circle(1.42))
        ));
        // A point inside a box overlaps it; two points never overlap.
        assert!(overlaps(
            at(0.0, 0.0, aabb(1.0, 1.0)),
            at(0.5, 0.5, circle(0.0))
        ));
        assert!(!overlaps(
            at(0.0, 0.0, circle(0.0)),
            at(0.0, 0.0, circle(0.0))
        ));
    }

    #[test]
    fn a_circle_reaches_round_a_box_corner() {
        // A circle of radius 5 beside a box 2 tall, its center 4 below the
        // box's: 3 below the box's edge, so it reaches 4 past the edge.
        let reach = reach(circle(5.0), aabb(1.0, 2.0), Axis::X, 4.0 + 1.0);
        assert_eq!(reach, Some(1.0 + 4.0));
        // Across by less than EPSILON, nothing blocks.
        assert_eq!(
            super::reach(aabb(1.0, 1.0), aabb(1.0, 1.0), Axis::X, 1.995),
            None
        );
        assert_eq!(
            super::reach(aabb(1.0, 1.0), aabb(1.0, 1.0), Axis::Y, 1.5),
            Some(2.0)
        );
    }

    #[test]
    fn rays_enter_through_the_facing_side() {
        let open = Closed::default();
        let block = Rect::new(Vec2::new(4.0, -1.0), Vec2::new(6.0, 1.0));
        let hit = ray_box(Vec2::ZERO, Vec2::new(10.0, 0.0), block, open);
        assert_eq!(
            hit,
            Ray::Hit {
                t: 0.4,
                normal: Vec2::new(-1.0, 0.0)
            }
        );
        // Along the top edge, or within EPSILON inside it, it grazes,
        // unless the top is closed: then within EPSILON outside it hits.
        let graze =
            |y: f32, closed| ray_box(Vec2::new(0.0, y), Vec2::new(10.0, 0.0), block, closed);
        let top = Closed {
            y: [true, false],
            ..open
        };
        let bottom = Closed {
            y: [false, true],
            ..open
        };
        for y in [-1.0, -0.995, -1.005] {
            assert_eq!(graze(y, open), Ray::Miss, "{y}");
            assert!(matches!(graze(y, top), Ray::Hit { .. }), "{y}");
            assert_eq!(graze(y, bottom), Ray::Miss, "{y}");
        }
        assert!(matches!(graze(-0.985, open), Ray::Hit { .. }));
        assert_eq!(graze(-1.015, top), Ray::Miss);
        let circle = ray_circle(Vec2::ZERO, Vec2::new(0.0, 10.0), Vec2::new(0.0, 5.0), 1.0);
        assert_eq!(
            circle,
            Ray::Hit {
                t: 0.4,
                normal: Vec2::new(0.0, -1.0)
            }
        );
        assert_eq!(
            ray_circle(Vec2::ZERO, Vec2::ZERO, Vec2::ONE, 2.0),
            Ray::Inside
        );
        assert_eq!(
            ray_circle(Vec2::ZERO, Vec2::ZERO, Vec2::splat(5.0), 2.0),
            Ray::Miss
        );
    }

    #[test]
    fn a_rounded_box_has_round_corners() {
        // Diagonally at the corner of a box of half 1 grown by 1: the
        // corner circle, not the bounding square.
        let origin = Vec2::new(-3.0, -3.0);
        let open = Closed::default();
        let core = Rect::new(-Vec2::ONE, Vec2::ONE);
        let Ray::Hit { t, normal } = ray_rounded(origin, Vec2::splat(3.0), core, 1.0, open) else {
            panic!("it hits");
        };
        let expected = (2.0 - 1.0 / 2f32.sqrt()) / 3.0;
        assert!((t - expected).abs() < 1e-6, "{t} vs {expected}");
        assert!(normal.approx_eq(Vec2::splat(-1.0).normalize_or_zero(), 1e-6));
    }

    #[test]
    fn a_cast_leaves_what_it_starts_in_but_goes_no_deeper() {
        let wall = Target::of(Placed::new(Vec2::ZERO, aabb(1.0, 1.0)));
        let point = aabb(0.0, 0.0);
        let open = Closed::default();
        assert_eq!(
            cast(point, Vec2::new(0.5, 0.0), Vec2::new(5.0, 0.0), wall, open),
            None
        );
        assert_eq!(
            cast(point, Vec2::new(0.5, 0.0), Vec2::new(-5.0, 0.0), wall, open),
            Some((0.0, Vec2::new(1.0, 0.0)))
        );
    }

    #[test]
    fn a_ray_that_passes_within_epsilon_of_a_circle_grazes_it() {
        let ray = |y: f32| ray_circle(Vec2::new(-20.0, y), Vec2::new(40.0, 0.0), Vec2::ZERO, 4.0);
        assert_eq!(ray(4.0), Ray::Miss);
        assert_eq!(ray(3.995), Ray::Miss);
        assert!(matches!(ray(3.98), Ray::Hit { .. }));
        // Starting within EPSILON inside: it hits only by going deeper.
        let start = Vec2::new(0.0, 3.995);
        let along = ray_circle(start, Vec2::new(10.0, 0.0), Vec2::ZERO, 4.0);
        assert_eq!(along, Ray::Miss);
        let down = ray_circle(start, Vec2::new(0.0, -10.0), Vec2::ZERO, 4.0);
        assert_eq!(
            down,
            Ray::Hit {
                t: 0.0,
                normal: Vec2::Y
            }
        );
    }

    #[test]
    fn long_rays_keep_their_precision() {
        // Through the center of a circle from 60,000 px away, and past it
        // by 2 px.
        let through = ray_circle(
            Vec2::new(-60_000.0, 0.0),
            Vec2::new(120_000.0, 0.0),
            Vec2::ZERO,
            8.0,
        );
        let Ray::Hit { t, normal } = through else {
            panic!("{through:?}");
        };
        assert!((t * 120_000.0 - 59_992.0).abs() < 0.02, "{t}");
        assert_eq!(normal, Vec2::new(-1.0, 0.0));
        let near = ray_circle(
            Vec2::new(-30_000.0, -29_994.0),
            Vec2::new(60_000.0, 60_000.0),
            Vec2::ZERO,
            8.0,
        );
        assert!(matches!(near, Ray::Hit { .. }), "{near:?}");
        // A motion too long to square still hits, with a unit normal.
        let huge = ray_circle(
            Vec2::ZERO,
            Vec2::new(1e30, 0.0),
            Vec2::new(100.0, 0.0),
            10.0,
        );
        assert!(
            matches!(huge, Ray::Hit { t, normal } if t > 0.0 && normal == Vec2::new(-1.0, 0.0)),
            "{huge:?}"
        );
        assert!(unit(Vec2::new(3e38, 2.25e38)).approx_eq(Vec2::new(0.8, 0.6), 1e-6));
        assert_eq!(unit(Vec2::new(f32::NAN, 1.0)), Vec2::ZERO);
        assert_eq!(unit(Vec2::ZERO), Vec2::ZERO);
    }
}
