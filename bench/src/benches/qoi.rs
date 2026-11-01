//! QOI benchmarks.

use std::hint::black_box;

use nadir_codec::Limits;
use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
use nadir_core::Pcg32;

use crate::harness::Bencher;

/// The side of the benchmark image, in pixels.
const SIDE: u32 = 2048;

/// The side of the square tiles the image is made of, in pixels.
const TILE: u32 = 64;

/// `qoi_decode_2048`: decodes a [`SIDE`] × [`SIDE`] RGBA image, 16 MiB of
/// pixels, as the game does a texture when it loads. The image is made of
/// tiles of five kinds, a fifth of each (see [`Fill`]), so that every kind
/// of chunk is common: runs, lookups, small and larger differences, and
/// full colors with and without alpha.
pub(crate) fn decode(bencher: &mut Bencher) {
    let file = file(SIDE);
    bencher.iter(|| qoi::decode(black_box(&file), Limits::default()));
}

/// What a tile of the image holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fill {
    /// Nothing: transparent black, which QOI writes as runs.
    Clear,
    /// Blocks of 4 × 4 pixels in 16 colors, like pixel art scaled up: a
    /// lookup or a full color, then a run.
    Blocks,
    /// A gradient with a little noise: mostly small differences.
    Shade,
    /// Noise: full colors.
    Noise,
    /// A disc on clear, shaded, whose edge fades out: changes of alpha
    /// around differences and runs.
    Disc,
}

/// The kinds of tile, as likely as each other.
const FILLS: [Fill; 5] = [
    Fill::Clear,
    Fill::Blocks,
    Fill::Shade,
    Fill::Noise,
    Fill::Disc,
];

/// The benchmark file: the image of `side` × `side` pixels, encoded.
fn file(side: u32) -> Vec<u8> {
    let header = Header {
        width: side,
        height: side,
        channels: Channels::Rgba,
        color_space: ColorSpace::Srgb,
    };
    qoi::encode(&header, &pixels(side)).expect("the pixels fill the image")
}

/// The image of `side` × `side` pixels, RGBA8: tiles of [`TILE`] pixels,
/// each of a kind and a color drawn at random. The same every time.
fn pixels(side: u32) -> Vec<u8> {
    let mut rng = Pcg32::new(0x716f_6966, 0);
    let tiles = side.div_ceil(TILE);
    let fills: Vec<(Fill, [u8; 4])> = (0..tiles * tiles)
        .map(|_| {
            let fill = rng.choose(&FILLS).copied().unwrap_or(Fill::Clear);
            (fill, opaque(&mut rng))
        })
        .collect();
    let palette: Vec<[u8; 4]> = (0..16).map(|_| opaque(&mut rng)).collect();

    let mut pixels = Vec::with_capacity(side as usize * side as usize * 4);
    for y in 0..side {
        for x in 0..side {
            let (fill, [r, g, b, _]) = fills[(y / TILE * tiles + x / TILE) as usize];
            let (u, v) = ((x % TILE) as u8, (y % TILE) as u8);
            let px = match fill {
                Fill::Clear => [0; 4],
                Fill::Blocks => {
                    let cell =
                        (x / 4).wrapping_mul(0x9e37_79b9) ^ (y / 4).wrapping_mul(0x85eb_ca6b);
                    palette[(cell >> 28) as usize]
                }
                Fill::Shade => {
                    let mut noisy = |base: u8, ramp: u8| {
                        base.wrapping_add(ramp)
                            .wrapping_add(rng.range(0..3u32) as u8)
                            .wrapping_sub(1)
                    };
                    [noisy(r, u), noisy(g, v), noisy(b, u / 2 + v / 2), 255]
                }
                Fill::Noise => opaque(&mut rng),
                Fill::Disc => {
                    let (du, dv) = (f32::from(u) - 31.5, f32::from(v) - 31.5);
                    let distance = (du * du + dv * dv).sqrt();
                    // Opaque to 22 px from the middle, clear from 30.
                    let alpha = ((30.0 - distance) / 8.0).clamp(0.0, 1.0);
                    if alpha == 0.0 {
                        [0; 4]
                    } else {
                        let light = (distance * 2.0) as u8;
                        [
                            r.wrapping_sub(light),
                            g.wrapping_sub(light),
                            b.wrapping_sub(light / 2),
                            (alpha * 255.0) as u8,
                        ]
                    }
                }
            };
            pixels.extend_from_slice(&px);
        }
    }
    pixels
}

/// A random opaque color.
fn opaque(rng: &mut Pcg32) -> [u8; 4] {
    let [r, g, b, _] = rng.next_u32().to_le_bytes();
    [r, g, b, 255]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Config;

    /// How many chunks of each kind a file has: full colors, full colors
    /// with alpha, lookups, differences, larger differences, and runs.
    fn chunks(file: &[u8]) -> [usize; 6] {
        let mut counts = [0; 6];
        let mut at = 14;
        while at < file.len() - 8 {
            let (kind, len) = match file[at] {
                0xfe => (0, 4),
                0xff => (1, 5),
                tag => match tag >> 6 {
                    0 => (2, 1),
                    1 => (3, 1),
                    2 => (4, 2),
                    _ => (5, 1),
                },
            };
            counts[kind] += 1;
            at += len;
        }
        counts
    }

    #[test]
    fn the_file_decodes_to_the_image() {
        let image = qoi::decode(&file(256), Limits::default()).unwrap();
        assert_eq!((image.header.width, image.header.height), (256, 256));
        assert!(image.pixels == pixels(256));
    }

    #[test]
    fn the_image_is_the_same_every_time() {
        assert!(pixels(128) == pixels(128));
    }

    #[test]
    fn every_kind_of_chunk_is_common_in_the_benchmark_file() {
        let file = file(SIDE);
        let counts = chunks(&file);
        let total: usize = counts.iter().sum();
        for (kind, &count) in counts.iter().enumerate() {
            assert!(count * 50 > total, "{counts:?}");
            assert!(count * 2 < total, "kind {kind}: {counts:?}");
        }
        // About two fifths of the size of its pixels.
        let pixels = SIDE as usize * SIDE as usize * 4;
        assert!(
            (pixels / 5..pixels / 2).contains(&file.len()),
            "{}",
            file.len()
        );
    }

    #[test]
    fn the_benchmark_times_the_decoder() {
        let mut bencher = Bencher::new(Config::fast());
        decode(&mut bencher);
        let measurement = bencher.finish().unwrap();
        assert!(measurement.median_ns() > 0.0);
    }
}
