//! Foundation types for the Nadir engine: math, color, rects, time, logging,
//! RNG, hashing, generational handles, key enums, profiling scopes, errors and
//! test helpers.

pub mod color;
pub mod error;
pub mod handle;
pub mod hash;
pub mod math;
pub mod rng;

#[cfg(test)]
mod test_support;

pub use color::{Color, LinearColor};
pub use error::CoreError;
pub use handle::{GenArena, GenIndex, Handle};
pub use hash::{FastHashMap, FastHashSet};
pub use math::{Affine2, IRect, IVec2, Mat3, Rect, UVec2, Vec2};
pub use rng::Pcg32;
