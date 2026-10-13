//! Test-only input generation for property tests.
//!
//! A stand-in until `Pcg32` (M0.3) and `testing::check` (M0.4) exist.

use std::f32::consts::PI;

use crate::math::{Affine2, Vec2};

/// Base seed. Case `n` of every property uses `BASE_SEED + n`.
const BASE_SEED: u64 = 0x4E41_4449_5200_0001;

/// SplitMix64 generator: tiny, and well mixed even for sequential seeds.
pub(crate) struct TestRng(u64);

impl TestRng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub(crate) fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform in `[lo, hi)`.
    pub(crate) fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        // 24 random bits are exactly representable in an f32 mantissa.
        let unit = (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32;
        lo + (hi - lo) * unit
    }

    /// Uniform in `[lo, hi)`. Requires `lo < hi`.
    pub(crate) fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        let span = (i64::from(hi) - i64::from(lo)) as u64;
        (i64::from(lo) + (self.next_u64() % span) as i64) as i32
    }

    /// A vector with both components uniform in `[lo, hi)`.
    pub(crate) fn vec2(&mut self, lo: f32, hi: f32) -> Vec2 {
        Vec2::new(self.range_f32(lo, hi), self.range_f32(lo, hi))
    }

    /// A well-conditioned transform: scale magnitudes in `[0.25, 4)` with
    /// random signs (so mirrored transforms are covered), any angle, and a
    /// translation within ±10.
    pub(crate) fn affine2(&mut self) -> Affine2 {
        let scale = Vec2::new(
            self.sign() * self.range_f32(0.25, 4.0),
            self.sign() * self.range_f32(0.25, 4.0),
        );
        Affine2::from_scale_angle_translation(
            scale,
            self.range_f32(-PI, PI),
            self.vec2(-10.0, 10.0),
        )
    }

    fn sign(&mut self) -> f32 {
        if self.next_u32() & 1 == 0 { 1.0 } else { -1.0 }
    }
}

/// Runs `property` against `cases` independently seeded generators.
///
/// Panics on the first failure with the property name, case index and seed,
/// so the failing case can be replayed with `TestRng::new(seed)`.
pub(crate) fn for_cases(
    name: &str,
    cases: u64,
    property: impl Fn(&mut TestRng) -> Result<(), String>,
) {
    for case in 0..cases {
        let seed = BASE_SEED.wrapping_add(case);
        if let Err(message) = property(&mut TestRng::new(seed)) {
            panic!("property `{name}` failed at case {case} (seed {seed:#018x}): {message}");
        }
    }
}
