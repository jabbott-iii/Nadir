//! Colors: [`Color`] (8-bit sRGB) and [`LinearColor`] (`f32` linear light),
//! with the exact sRGB transfer functions between them.
//!
//! Authored colors are sRGB; blending happens in linear space (ADR-0013).
//! Alpha is straight (not premultiplied) and is never gamma-encoded.

use crate::error::CoreError;

/// An sRGB-encoded color with 8 bits per channel and straight alpha.
///
/// This is how colors are authored and stored. Convert with
/// [`Color::to_linear`] before blending or sending to the GPU as a vertex
/// color.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red, sRGB-encoded.
    pub r: u8,
    /// Green, sRGB-encoded.
    pub g: u8,
    /// Blue, sRGB-encoded.
    pub b: u8,
    /// Alpha: 0 is transparent, 255 is opaque.
    pub a: u8,
}

impl Color {
    /// Opaque white. As a tint, it leaves a sprite unchanged.
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    /// Opaque black.
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    /// Creates an opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// Creates a color with alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parses `RRGGBB` or `RRGGBBAA` hex digits, with an optional leading
    /// `#`. Digits may be upper or lower case; without `AA` the color is
    /// opaque.
    pub fn from_hex(hex: &str) -> Result<Self, CoreError> {
        let digits = hex.strip_prefix('#').unwrap_or(hex).as_bytes();
        let (pairs, rest) = digits.as_chunks::<2>();
        if !rest.is_empty() || !matches!(pairs.len(), 3 | 4) {
            return Err(CoreError::InvalidHexColor);
        }
        let mut channels = [0, 0, 0, u8::MAX];
        for (channel, &[high, low]) in channels.iter_mut().zip(pairs) {
            *channel = (hex_digit(high)? << 4) | hex_digit(low)?;
        }
        let [r, g, b, a] = channels;
        Ok(Self::rgba(r, g, b, a))
    }

    /// Converts to linear light. Alpha is rescaled to `[0, 1]` unchanged.
    pub fn to_linear(self) -> LinearColor {
        let decode = |channel: u8| srgb_to_linear(f32::from(channel) / 255.0);
        LinearColor::new(
            decode(self.r),
            decode(self.g),
            decode(self.b),
            f32::from(self.a) / 255.0,
        )
    }
}

impl From<LinearColor> for Color {
    fn from(color: LinearColor) -> Self {
        color.to_srgb()
    }
}

/// A color in linear light with `f32` channels and straight alpha.
///
/// This is the space where blending and lighting math is correct. Channels
/// are nominally in `[0, 1]`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearColor {
    /// Red, linear.
    pub r: f32,
    /// Green, linear.
    pub g: f32,
    /// Blue, linear.
    pub b: f32,
    /// Alpha: 0 is transparent, 1 is opaque.
    pub a: f32,
}

impl LinearColor {
    /// Opaque white.
    pub const WHITE: Self = Self::new(1.0, 1.0, 1.0, 1.0);
    /// Opaque black.
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);
    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);

    /// Creates a color from linear channels and alpha.
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Converts to 8-bit sRGB, rounding to nearest. Channels outside
    /// `[0, 1]` are clamped, and NaN becomes 0.
    pub fn to_srgb(self) -> Color {
        let encode = |channel: f32| quantize(linear_to_srgb(channel));
        Color::rgba(
            encode(self.r),
            encode(self.g),
            encode(self.b),
            quantize(self.a),
        )
    }

    /// Returns the channels as `[r, g, b, a]`.
    pub const fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

impl From<Color> for LinearColor {
    fn from(color: Color) -> Self {
        color.to_linear()
    }
}

/// Decodes one sRGB-encoded channel in `[0, 1]` to linear light, with the
/// exact piecewise sRGB transfer function (IEC 61966-2-1).
pub fn srgb_to_linear(encoded: f32) -> f32 {
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// Encodes one linear-light channel in `[0, 1]` to sRGB, with the exact
/// piecewise sRGB transfer function (IEC 61966-2-1).
pub fn linear_to_srgb(linear: f32) -> f32 {
    if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// Maps `[0, 1]` to `0..=255`, rounding to nearest and clamping. NaN maps to
/// 0, because float-to-int `as` casts saturate.
fn quantize(unit: f32) -> u8 {
    (unit.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn hex_digit(byte: u8) -> Result<u8, CoreError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(CoreError::InvalidHexColor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::approx_eq;
    use crate::test_support::for_cases;

    fn channel_error(a: Color, b: Color) -> u8 {
        [
            a.r.abs_diff(b.r),
            a.g.abs_diff(b.g),
            a.b.abs_diff(b.b),
            a.a.abs_diff(b.a),
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
    }

    #[test]
    fn constructors_and_constants() {
        assert_eq!(
            Color::rgb(1, 2, 3),
            Color {
                r: 1,
                g: 2,
                b: 3,
                a: 255
            }
        );
        assert_eq!(
            Color::rgba(1, 2, 3, 4),
            Color {
                r: 1,
                g: 2,
                b: 3,
                a: 4
            }
        );
        assert_eq!(Color::WHITE, Color::rgba(255, 255, 255, 255));
        assert_eq!(Color::BLACK, Color::rgba(0, 0, 0, 255));
        assert_eq!(Color::TRANSPARENT, Color::rgba(0, 0, 0, 0));
        assert_eq!(
            LinearColor::new(0.1, 0.2, 0.3, 0.4).to_array(),
            [0.1, 0.2, 0.3, 0.4]
        );
    }

    #[test]
    fn hex_parsing_accepts_six_or_eight_digits() {
        assert_eq!(Color::from_hex("#ff8000"), Ok(Color::rgb(255, 128, 0)));
        assert_eq!(Color::from_hex("FF8000"), Ok(Color::rgb(255, 128, 0)));
        assert_eq!(
            Color::from_hex("#0a1B2c3D"),
            Ok(Color::rgba(10, 27, 44, 61))
        );
        assert_eq!(Color::from_hex("00000000"), Ok(Color::TRANSPARENT));
        assert_eq!(Color::from_hex("#ffffff"), Ok(Color::WHITE));
    }

    #[test]
    fn hex_parsing_rejects_malformed_input() {
        for bad in [
            "",
            "#",
            "#fff",
            "#ffff",
            "#fffff",
            "#fffffff",
            "#fffffffff",
            "##ffffff",
            "#ffffff#",
            "#gg0000",
            "#ff 000",
            "#+f0000",
            " #ffffff",
            "#ffffff ",
            "#ééé",
            "0xffffff",
        ] {
            assert_eq!(
                Color::from_hex(bad),
                Err(CoreError::InvalidHexColor),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn transfer_functions_match_known_values() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert_eq!(linear_to_srgb(0.0), 0.0);
        assert!(approx_eq(srgb_to_linear(1.0), 1.0, 1e-6));
        assert!(approx_eq(linear_to_srgb(1.0), 1.0, 1e-6));
        assert!(approx_eq(srgb_to_linear(0.5), 0.214041, 1e-5));
        assert!(approx_eq(linear_to_srgb(0.5), 0.735357, 1e-5));
        // Inside the linear toe.
        assert!(approx_eq(srgb_to_linear(0.02), 0.02 / 12.92, 1e-7));
        assert!(approx_eq(linear_to_srgb(0.001), 0.01292, 1e-7));
        // The two pieces meet at the threshold.
        assert!(approx_eq(srgb_to_linear(0.04045), 0.0031308, 1e-6));
        assert!(approx_eq(srgb_to_linear(0.04046), 0.0031308, 1e-5));
        assert!(approx_eq(linear_to_srgb(0.0031308), 0.04045, 1e-5));
        assert!(approx_eq(linear_to_srgb(0.0031309), 0.04045, 1e-5));
    }

    #[test]
    fn color_conversion_matches_known_values() {
        assert_eq!(Color::WHITE.to_linear(), LinearColor::WHITE);
        assert_eq!(Color::BLACK.to_linear(), LinearColor::BLACK);
        assert_eq!(Color::TRANSPARENT.to_linear(), LinearColor::TRANSPARENT);
        assert_eq!(LinearColor::WHITE.to_srgb(), Color::WHITE);
        assert_eq!(LinearColor::BLACK.to_srgb(), Color::BLACK);

        let gray = Color::rgba(128, 128, 128, 128).to_linear();
        assert!(approx_eq(gray.r, 0.215861, 1e-5), "{gray:?}");
        assert_eq!(gray.r, gray.g);
        assert_eq!(gray.r, gray.b);
        // Alpha is not gamma-encoded.
        assert!(approx_eq(gray.a, 128.0 / 255.0, 1e-6), "{gray:?}");

        assert_eq!(
            LinearColor::new(0.1, 0.1, 0.1, 0.5).to_srgb(),
            Color::rgba(89, 89, 89, 128)
        );
        assert_eq!(LinearColor::from(Color::WHITE), LinearColor::WHITE);
        assert_eq!(Color::from(LinearColor::BLACK), Color::BLACK);
    }

    #[test]
    fn to_srgb_clamps_out_of_range_channels() {
        let wild = LinearColor::new(-1.0, 2.0, f32::NAN, f32::INFINITY);
        assert_eq!(wild.to_srgb(), Color::rgba(0, 255, 0, 255));
        let negative_alpha = LinearColor::new(0.0, 0.0, 0.0, -0.5);
        assert_eq!(negative_alpha.to_srgb(), Color::TRANSPARENT);
    }

    #[test]
    fn every_8_bit_value_round_trips_within_one() {
        for value in 0..=u8::MAX {
            let color = Color::rgba(value, value, value, value);
            let back = color.to_linear().to_srgb();
            assert!(
                channel_error(color, back) <= 1,
                "{color:?} came back as {back:?}"
            );
        }
    }

    #[test]
    fn srgb_round_trips_within_one() {
        for_cases("sRGB round-trip", 2000, |rng| {
            let [r, g, b, a] = rng.next_u32().to_le_bytes();
            let color = Color::rgba(r, g, b, a);
            let back = color.to_linear().to_srgb();
            if channel_error(color, back) > 1 {
                return Err(format!("{color:?} came back as {back:?}"));
            }
            Ok(())
        });
    }

    #[test]
    fn linear_round_trips_through_8_bit_within_one_step() {
        for_cases("linear round-trip", 2000, |rng| {
            let linear = LinearColor::new(
                rng.range_f32(0.0, 1.0),
                rng.range_f32(0.0, 1.0),
                rng.range_f32(0.0, 1.0),
                rng.range_f32(0.0, 1.0),
            );
            let encoded = linear.to_srgb();
            // Once quantized to 8 bits, decoding and re-encoding must stay
            // within one step.
            let again = encoded.to_linear().to_srgb();
            if channel_error(encoded, again) > 1 {
                return Err(format!(
                    "{linear:?} encoded to {encoded:?}, then to {again:?}"
                ));
            }
            Ok(())
        });
    }

    #[test]
    fn transfer_functions_are_inverses() {
        for_cases("transfer functions are inverses", 2000, |rng| {
            let x = rng.range_f32(0.0, 1.0);
            let there_and_back = srgb_to_linear(linear_to_srgb(x));
            if !approx_eq(there_and_back, x, 1e-5) {
                return Err(format!("linear {x} came back as {there_and_back}"));
            }
            let back_and_there = linear_to_srgb(srgb_to_linear(x));
            if !approx_eq(back_and_there, x, 1e-5) {
                return Err(format!("sRGB {x} came back as {back_and_there}"));
            }
            Ok(())
        });
    }
}
