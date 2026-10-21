//! Sort keys: the order sprites are drawn in.
//!
//! ```text
//!  63        56 55                              24 23              8 7       0
//! ┌───────────┬──────────────────────────────────┬─────────────────┬─────────┐
//! │ layer u8  │ depth u32                        │ texture id u16  │ seq u8  │
//! └───────────┴──────────────────────────────────┴─────────────────┴─────────┘
//! ```

use crate::SortMode;

/// `f`'s bits, made to order as unsigned numbers as the floats order:
/// negatives below positives, `-0.0` just below `+0.0`. This is the IEEE
/// total order, the order of [`f32::total_cmp`].
pub(crate) const fn sortable_bits(f: f32) -> u32 {
    let bits = f.to_bits();
    if bits >> 31 == 1 {
        !bits
    } else {
        bits | 0x8000_0000
    }
}

/// The key of a sprite.
///
/// `texture` is the low 16 bits of its texture's handle, so that sprites
/// of one depth group by texture; two textures that share them only split a
/// batch. `seq` is its submission number, modulo 256. The renderer sorts
/// on the key without `seq`, then on the full submission number, so that
/// ties keep their submission order exactly.
pub(crate) const fn sort_key(layer: u8, sort: SortMode, y: f32, texture: u16, seq: u8) -> u64 {
    let depth = match sort {
        SortMode::YSort => sortable_bits(y),
        SortMode::Fixed(depth) => depth,
    };
    (layer as u64) << 56 | (depth as u64) << 24 | (texture as u64) << 8 | seq as u64
}

#[cfg(test)]
mod tests {
    use nadir_core::testing::check;

    use super::*;

    #[test]
    fn the_fields_are_where_the_layout_says() {
        let key = sort_key(0xAB, SortMode::Fixed(0x1234_5678), 0.0, 0x9ABC, 0xDE);
        assert_eq!(key, 0xAB12_3456_789A_BCDE);
    }

    #[test]
    fn keys_order_as_the_floats_do() {
        check(
            "sortable bits",
            20_000,
            |rng| {
                let random = |rng: &mut nadir_core::Pcg32| match rng.range(0..3u32) {
                    // Any bit pattern but a NaN: extremes, subnormals, ±0.
                    0 => loop {
                        let f = f32::from_bits(rng.next_u32());
                        if !f.is_nan() {
                            break f;
                        }
                    },
                    // Values of a screen, negatives included.
                    1 => (rng.range(0..2_000_000u32) as f32 - 1_000_000.0) / 100.0,
                    _ => *rng
                        .choose(&[0.0, -0.0, 1.0, -1.0, f32::INFINITY, f32::NEG_INFINITY])
                        .expect("not empty"),
                };
                (random(rng), random(rng))
            },
            |&(a, b)| {
                let order = sortable_bits(a).cmp(&sortable_bits(b));
                if order == a.total_cmp(&b) {
                    Ok(())
                } else {
                    Err(format!("{a:?} vs {b:?}: the keys order {order:?}"))
                }
            },
        );
    }

    #[test]
    fn minus_zero_is_just_below_plus_zero() {
        assert_eq!(sortable_bits(-0.0) + 1, sortable_bits(0.0));
        assert!(sortable_bits(-f32::MIN_POSITIVE) < sortable_bits(-0.0));
        assert!(sortable_bits(0.0) < sortable_bits(f32::MIN_POSITIVE));
    }

    #[test]
    fn the_layer_comes_before_everything_else() {
        let below = sort_key(1, SortMode::Fixed(u32::MAX), f32::MAX, u16::MAX, u8::MAX);
        let above = sort_key(2, SortMode::Fixed(0), f32::MIN, 0, 0);
        assert!(below < above);
    }

    #[test]
    fn fixed_depths_bound_the_y_sorted_ones() {
        let behind = sort_key(1, SortMode::Fixed(0), 0.0, 0, 0);
        let front = sort_key(1, SortMode::Fixed(u32::MAX), 0.0, 0, 0);
        for y in [f32::MIN, -1.0, 0.0, 100.0, f32::MAX] {
            let sorted = sort_key(1, SortMode::YSort, y, 0, 0);
            assert!(behind < sorted && sorted < front, "{y}");
        }
    }
}
