//! Deterministic random numbers: [`Pcg32`].

use std::ops::Range;

/// A PCG32 random number generator (PCG-XSH-RR 64/32).
///
/// It is small, fast and fully deterministic: the same seed and stream give
/// the same sequence on every OS, which replays and property tests rely on.
/// It is not cryptographically secure.
///
/// The type is deliberately not `Copy`, so a generator cannot be duplicated
/// by accident and replay the same numbers twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcg32 {
    state: u64,
    /// The stream selector. Always odd.
    increment: u64,
}

impl Pcg32 {
    const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

    /// Creates a generator from a seed and a stream number.
    ///
    /// Generators with the same seed but different streams produce unrelated
    /// sequences. Give each system its own stream derived from one world
    /// seed, so that adding a random call in one system does not shift the
    /// numbers another system sees. The top bit of `stream` is ignored.
    pub const fn new(seed: u64, stream: u64) -> Self {
        let mut rng = Self {
            state: 0,
            increment: (stream << 1) | 1,
        };
        rng.step();
        rng.state = rng.state.wrapping_add(seed);
        rng.step();
        rng
    }

    /// Returns the next 32 random bits.
    pub const fn next_u32(&mut self) -> u32 {
        let old = self.step();
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        xorshifted.rotate_right((old >> 59) as u32)
    }

    /// Returns a uniform `f32` in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        // 24 random bits are exactly representable in an f32 mantissa.
        (self.next_u32() >> 8) as f32 * (1.0 / (1u32 << 24) as f32)
    }

    /// Returns a uniform value from the half-open `range`.
    ///
    /// Works for `u32`, `i32` and `f32` ranges. An empty or inverted range
    /// returns `range.start` without consuming any randomness.
    pub fn range<R: SampleRange>(&mut self, range: R) -> R::Output {
        range.sample(self)
    }

    /// Shuffles `slice` in place, with every permutation equally likely.
    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        // Fisher–Yates, from the back.
        for last in (1..slice.len()).rev() {
            slice.swap(last, self.index_below(last + 1));
        }
    }

    /// Returns a uniformly chosen element of `slice`, or `None` if it is
    /// empty.
    pub fn choose<'a, T>(&mut self, slice: &'a [T]) -> Option<&'a T> {
        if slice.is_empty() {
            return None;
        }
        slice.get(self.index_below(slice.len()))
    }

    /// Advances the state and returns the previous one.
    const fn step(&mut self) -> u64 {
        let old = self.state;
        self.state = old
            .wrapping_mul(Self::MULTIPLIER)
            .wrapping_add(self.increment);
        old
    }

    /// Returns a uniform value in `0..bound`, without modulo bias.
    /// `bound` must not be zero.
    fn below(&mut self, bound: u32) -> u32 {
        // Reject the few lowest values so that the remaining count is a
        // multiple of `bound`. The loop almost always exits first time.
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let value = self.next_u32();
            if value >= threshold {
                return value % bound;
            }
        }
    }

    /// Returns a uniform index in `0..bound`. `bound` must not be zero.
    fn index_below(&mut self, bound: usize) -> usize {
        match u32::try_from(bound) {
            Ok(bound) => self.below(bound) as usize,
            // More than u32::MAX elements: combine two draws. The modulo
            // bias is below 2⁻³² here, which is not worth a rejection loop.
            Err(_) => {
                let wide = (u64::from(self.next_u32()) << 32) | u64::from(self.next_u32());
                (wide % bound as u64) as usize
            }
        }
    }
}

/// A half-open range that [`Pcg32::range`] can draw a value from.
pub trait SampleRange {
    /// The type of the value drawn.
    type Output;

    /// Draws one uniform value from the range. An empty or inverted range
    /// yields its start.
    fn sample(self, rng: &mut Pcg32) -> Self::Output;
}

impl SampleRange for Range<u32> {
    type Output = u32;

    fn sample(self, rng: &mut Pcg32) -> u32 {
        if self.is_empty() {
            return self.start;
        }
        self.start + rng.below(self.end - self.start)
    }
}

impl SampleRange for Range<i32> {
    type Output = i32;

    fn sample(self, rng: &mut Pcg32) -> i32 {
        if self.is_empty() {
            return self.start;
        }
        // The span of any non-empty i32 range fits in a u32, and adding the
        // offset back with wrapping arithmetic lands inside the range.
        let span = self.end.wrapping_sub(self.start) as u32;
        self.start.wrapping_add(rng.below(span) as i32)
    }
}

/// The result is in `[start, end)`. Because of rounding, a very narrow range
/// can also yield `end` itself.
impl SampleRange for Range<f32> {
    type Output = f32;

    fn sample(self, rng: &mut Pcg32) -> f32 {
        if self.is_empty() {
            return self.start;
        }
        self.start + (self.end - self.start) * rng.next_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::for_cases;

    fn take<const N: usize>(rng: &mut Pcg32) -> [u32; N] {
        std::array::from_fn(|_| rng.next_u32())
    }

    #[test]
    fn matches_the_reference_implementation() {
        // Output of the PCG reference demo (pcg32-demo), seeded with
        // `pcg32_srandom_r(&rng, 42u, 54u)`.
        let mut rng = Pcg32::new(42, 54);
        assert_eq!(
            take(&mut rng),
            [
                0xa15c_02b7,
                0x7b47_f409,
                0xba1d_3330,
                0x83d2_f293,
                0xbfa4_784b,
                0xcbed_606e
            ]
        );

        // The demo then flips 65 coins with `pcg32_boundedrand_r(&rng, 2)`
        // and rolls 33 dice with `pcg32_boundedrand_r(&rng, 6) + 1`.
        for _ in 0..65 {
            rng.range(0..2u32);
        }
        let rolls: [u32; 33] = std::array::from_fn(|_| rng.range(1..7u32));
        assert_eq!(
            rolls,
            [
                3, 4, 1, 1, 2, 2, 3, 2, 4, 3, 2, 4, 3, 3, 5, 2, 3, 1, 3, 1, 5, 1, 4, 1, 5, 6, 4, 6,
                6, 2, 6, 3, 3
            ]
        );
    }

    #[test]
    fn matches_fixed_vectors_for_other_seeds() {
        // From an independent implementation of the published algorithm.
        assert_eq!(
            take(&mut Pcg32::new(0, 0)),
            [0xe4c1_4788, 0x379c_6516, 0x5c4a_b3bb, 0x601d_23e0]
        );
        assert_eq!(
            take(&mut Pcg32::new(1, 1)),
            [0xc982_8f91, 0x1592_e274, 0xc026_2657, 0xa5c2_b6d3]
        );
        assert_eq!(
            take(&mut Pcg32::new(0xdead_beef, 7)),
            [0x19bf_3fe4, 0x3b4e_25b8, 0x6c32_198a, 0x0571_5932]
        );
        assert_eq!(
            take(&mut Pcg32::new(u64::MAX, u64::MAX)),
            [0x2675_c047, 0x7779_a837, 0xa145_aa13, 0x5f6b_e726]
        );
    }

    #[test]
    fn bounded_draws_reject_instead_of_wrapping() {
        // With a bound just over 2³¹, almost half of all raw outputs must be
        // rejected to avoid bias. The second raw output for this seed
        // (0x7b47f409) is one of them. Expected values are from an
        // independent implementation of `pcg32_boundedrand_r`.
        let mut rng = Pcg32::new(42, 54);
        let draws: [u32; 6] = std::array::from_fn(|_| rng.range(0..0x8000_0001u32));
        assert_eq!(
            draws,
            [
                0x215c_02b6,
                0x3a1d_332f,
                0x03d2_f292,
                0x3fa4_784a,
                0x4bed_606d,
                0x3fc6_a3ac
            ]
        );
    }

    #[test]
    fn works_in_const_context() {
        const FIRST: u32 = {
            let mut rng = Pcg32::new(42, 54);
            rng.next_u32()
        };
        assert_eq!(FIRST, 0xa15c_02b7);
    }

    #[test]
    fn same_seed_same_sequence_and_streams_differ() {
        let mut a = Pcg32::new(7, 1);
        let mut b = Pcg32::new(7, 1);
        assert_eq!(a, b);
        assert_eq!(take::<16>(&mut a), take::<16>(&mut b));
        assert_eq!(a, b);

        let mut cloned = a.clone();
        assert_eq!(cloned.next_u32(), a.next_u32());

        let streams: [[u32; 8]; 3] = [
            take(&mut Pcg32::new(7, 1)),
            take(&mut Pcg32::new(7, 2)),
            take(&mut Pcg32::new(8, 1)),
        ];
        assert_ne!(streams[0], streams[1]);
        assert_ne!(streams[0], streams[2]);
        assert_ne!(streams[1], streams[2]);
    }

    #[test]
    fn next_f32_is_in_the_unit_interval_and_looks_uniform() {
        let mut rng = Pcg32::new(1, 1);
        let mut buckets = [0u32; 10];
        for _ in 0..10_000 {
            let value = rng.next_f32();
            assert!((0.0..1.0).contains(&value), "{value}");
            buckets[(value * 10.0) as usize] += 1;
        }
        // Each bucket expects 1000 with a standard deviation of 30.
        for (bucket, &count) in buckets.iter().enumerate() {
            assert!((850..1150).contains(&count), "bucket {bucket} has {count}");
        }
    }

    #[test]
    fn empty_ranges_return_start_and_consume_nothing() {
        // Callers compute their bounds, so an inverted range arrives as two
        // values, not as a literal.
        fn from_bounds<T>(start: T, end: T) -> Range<T> {
            start..end
        }

        let mut rng = Pcg32::new(3, 3);
        let untouched = rng.clone();
        assert_eq!(rng.range(from_bounds(5u32, 5)), 5);
        assert_eq!(rng.range(from_bounds(9u32, 2)), 9);
        assert_eq!(rng.range(from_bounds(-4i32, -4)), -4);
        assert_eq!(rng.range(from_bounds(4i32, -4)), 4);
        assert_eq!(rng.range(from_bounds(1.5f32, 1.5)), 1.5);
        assert_eq!(rng.range(from_bounds(2.0f32, 1.0)), 2.0);
        assert!(rng.range(from_bounds(f32::NAN, 1.0)).is_nan());
        assert_eq!(rng, untouched);
    }

    #[test]
    fn integer_ranges_cover_every_value_and_nothing_else() {
        let mut rng = Pcg32::new(11, 5);
        let mut seen_u32 = [false; 6];
        let mut seen_i32 = [false; 7];
        for _ in 0..500 {
            let value = rng.range(10..16u32);
            assert!((10..16).contains(&value), "{value}");
            seen_u32[(value - 10) as usize] = true;

            let value = rng.range(-3..4i32);
            assert!((-3..4).contains(&value), "{value}");
            seen_i32[(value + 3) as usize] = true;
        }
        assert_eq!(seen_u32, [true; 6]);
        assert_eq!(seen_i32, [true; 7]);
    }

    #[test]
    fn ranges_stay_in_bounds() {
        for_cases("pcg32 range bounds", 500, |case| {
            let mut rng = Pcg32::new(u64::from(case.next_u32()), u64::from(case.next_u32()));

            let (a, b) = (case.next_u32(), case.next_u32());
            let (low, high) = (a.min(b), a.max(b));
            let value = rng.range(low..high);
            if low < high && !(low..high).contains(&value) {
                return Err(format!("{value} is outside {low}..{high}"));
            }

            let (a, b) = (case.next_u32() as i32, case.next_u32() as i32);
            let (low, high) = (a.min(b), a.max(b));
            let value = rng.range(low..high);
            if low < high && !(low..high).contains(&value) {
                return Err(format!("{value} is outside {low}..{high}"));
            }

            let (a, b) = (
                case.range_f32(-1000.0, 1000.0),
                case.range_f32(-1000.0, 1000.0),
            );
            let (low, high) = (a.min(b), a.max(b));
            let value = rng.range(low..high);
            if !(low..=high).contains(&value) {
                return Err(format!("{value} is outside {low}..{high}"));
            }
            Ok(())
        });
    }

    #[test]
    fn extreme_integer_ranges_do_not_overflow() {
        let mut rng = Pcg32::new(99, 99);
        for _ in 0..100 {
            let full = rng.range(i32::MIN..i32::MAX);
            assert!(full < i32::MAX);
            let top = rng.range(u32::MAX - 2..u32::MAX);
            assert!((u32::MAX - 2..u32::MAX).contains(&top), "{top}");
            let low = rng.range(i32::MIN..i32::MIN + 3);
            assert!((i32::MIN..i32::MIN + 3).contains(&low), "{low}");
            assert_eq!(rng.range(41..42u32), 41);
        }
    }

    #[test]
    fn bounded_draws_look_uniform() {
        let mut rng = Pcg32::new(2024, 6);
        let mut counts = [0u32; 6];
        for _ in 0..12_000 {
            counts[rng.range(0..6u32) as usize] += 1;
        }
        // Each face expects 2000 with a standard deviation of about 41.
        for (face, &count) in counts.iter().enumerate() {
            assert!(
                (1800..2200).contains(&count),
                "face {face} came up {count} times"
            );
        }
    }

    #[test]
    fn shuffle_keeps_every_element() {
        for_cases("pcg32 shuffle is a permutation", 200, |case| {
            let mut rng = Pcg32::new(u64::from(case.next_u32()), 0);
            let len = case.next_u32() as usize % 50;
            let mut items: Vec<usize> = (0..len).collect();
            rng.shuffle(&mut items);
            let mut sorted = items.clone();
            sorted.sort_unstable();
            if sorted != (0..len).collect::<Vec<_>>() {
                return Err(format!("shuffling 0..{len} gave {items:?}"));
            }
            Ok(())
        });
    }

    #[test]
    fn shuffle_reaches_every_permutation_evenly() {
        let mut rng = Pcg32::new(5, 5);
        let mut counts = [0u32; 6];
        for _ in 0..6000 {
            let mut items = [0, 1, 2];
            rng.shuffle(&mut items);
            let which = match items {
                [0, 1, 2] => 0,
                [0, 2, 1] => 1,
                [1, 0, 2] => 2,
                [1, 2, 0] => 3,
                [2, 0, 1] => 4,
                [2, 1, 0] => 5,
                other => panic!("not a permutation: {other:?}"),
            };
            counts[which] += 1;
        }
        // Each permutation expects 1000 with a standard deviation of about 29.
        for (which, &count) in counts.iter().enumerate() {
            assert!(
                (850..1150).contains(&count),
                "permutation {which} came up {count} times"
            );
        }
    }

    #[test]
    fn shuffle_and_choose_handle_tiny_slices() {
        let mut rng = Pcg32::new(1, 2);
        let untouched = rng.clone();
        let mut empty: [u8; 0] = [];
        rng.shuffle(&mut empty);
        let mut one = [7];
        rng.shuffle(&mut one);
        assert_eq!(one, [7]);
        assert_eq!(rng.choose(&empty), None);
        assert_eq!(rng, untouched);
        assert_eq!(rng.choose(&one), Some(&7));
    }

    #[test]
    fn choose_picks_every_element() {
        let mut rng = Pcg32::new(8, 8);
        let items = ["a", "b", "c", "d"];
        let mut counts = [0u32; 4];
        for _ in 0..4000 {
            let picked = rng.choose(&items).expect("slice is not empty");
            let which = items
                .iter()
                .position(|item| item == picked)
                .expect("picked from items");
            counts[which] += 1;
        }
        // Each element expects 1000 with a standard deviation of about 27.
        for (which, &count) in counts.iter().enumerate() {
            assert!(
                (850..1150).contains(&count),
                "element {which} was picked {count} times"
            );
        }
    }
}
