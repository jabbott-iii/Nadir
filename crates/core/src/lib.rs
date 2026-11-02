// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Foundation types for the Nadir engine: math, color, rects, time, logging,
//! RNG, hashing, generational handles, key enums, profiling scopes, errors and
//! test helpers.

pub mod civil;
pub mod cli;
pub mod color;
pub mod error;
pub mod handle;
pub mod hash;
pub mod keys;
pub mod log;
pub mod math;
pub mod profile;
pub mod rng;
pub mod testing;
pub mod time;

#[cfg(test)]
mod test_support;

pub use color::{Color, LinearColor};
pub use error::CoreError;
pub use handle::{GenArena, GenIndex, Handle};
pub use hash::{FastHashMap, FastHashSet};
pub use keys::{Key, MouseButton};
pub use math::{Affine2, IRect, IVec2, Mat3, Rect, UVec2, Vec2};
pub use rng::Pcg32;
pub use time::{FixedClock, Frame, Stopwatch, Tick};
