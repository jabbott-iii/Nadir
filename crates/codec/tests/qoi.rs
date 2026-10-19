//! QOI property tests: random images survive being encoded and decoded,
//! and damaged files are rejected without a panic. The hand-computed
//! vectors are beside the codec, in `src/qoi.rs`.

use nadir_codec::qoi::{self, Channels, ColorSpace, ErrorKind, Header, Image};
use nadir_codec::{CodecError, Limits};
use nadir_core::Pcg32;
use nadir_core::testing::{check, check_sized};

/// A random image of about `size` pixels a row, made of the patterns each
/// chunk is for: repeats (runs), small and medium changes (diffs and luma),
/// colors seen before (lookups), and noise.
fn random_image(rng: &mut Pcg32, size: u32) -> Image {
    let width = 1 + rng.range(0..size + 1);
    let height = 1 + rng.range(0..size / 4 + 1);
    let channels = if rng.range(0..2u32) == 0 {
        Channels::Rgb
    } else {
        Channels::Rgba
    };
    let color_space = if rng.range(0..2u32) == 0 {
        ColorSpace::Srgb
    } else {
        ColorSpace::Linear
    };
    let palette: Vec<[u8; 4]> = (0..1 + rng.range(0..8u32))
        .map(|_| byte4(rng, channels))
        .collect();
    let mut px = [0, 0, 0, 255];
    let mut pixels = Vec::new();
    for _ in 0..width * height {
        px = match rng.range(0..6u32) {
            0 | 1 => px,
            2 => nudge(rng, px, 2),
            3 => nudge(rng, px, 40),
            4 => *rng.choose(&palette).unwrap_or(&px),
            _ => byte4(rng, channels),
        };
        if channels == Channels::Rgba && rng.range(0..16u32) == 0 {
            px[3] = rng.range(0..256u32) as u8;
        }
        pixels.extend_from_slice(&px);
    }
    Image {
        header: Header {
            width,
            height,
            channels,
            color_space,
        },
        pixels,
    }
}

fn byte4(rng: &mut Pcg32, channels: Channels) -> [u8; 4] {
    let mut byte = || rng.range(0..256u32) as u8;
    let alpha = match channels {
        Channels::Rgb => 255,
        Channels::Rgba => byte(),
    };
    [byte(), byte(), byte(), alpha]
}

/// Moves each color channel by up to `by` either way, wrapping.
fn nudge(rng: &mut Pcg32, [r, g, b, a]: [u8; 4], by: i32) -> [u8; 4] {
    let mut step = |channel: u8| channel.wrapping_add_signed(rng.range(-by..by + 1) as i8);
    [step(r), step(g), step(b), a]
}

fn decode(bytes: &[u8]) -> Result<Image, CodecError> {
    qoi::decode(bytes, Limits::default())
}

#[test]
fn random_images_survive_being_encoded_and_decoded() {
    check_sized("qoi round-trip", 400, random_image, |image| {
        let encoded = qoi::encode(&image.header, &image.pixels)
            .map_err(|error| format!("cannot encode: {error}"))?;
        let decoded = decode(&encoded).map_err(|error| format!("cannot decode: {error}"))?;
        if decoded != *image {
            return Err(format!("decodes as {decoded:?}"));
        }
        // Encoding is deterministic, and what it wrote is what it reads.
        let again = qoi::encode(&decoded.header, &decoded.pixels)
            .map_err(|error| format!("cannot encode again: {error}"))?;
        if again != encoded {
            return Err("a second encoding differs".to_string());
        }
        Ok(())
    });
}

#[test]
fn every_cut_short_file_is_rejected() {
    check_sized(
        "qoi prefixes",
        100,
        |rng, size| {
            let image = random_image(rng, size);
            qoi::encode(&image.header, &image.pixels).expect("a random image encodes")
        },
        |encoded| {
            // Each prefix fails where it ends, even a prefix of the magic.
            for len in 0..encoded.len() {
                match decode(&encoded[..len]) {
                    Err(CodecError::Qoi {
                        offset,
                        kind: ErrorKind::UnexpectedEnd { .. },
                    }) if offset == len => {}
                    other => return Err(format!("{len} bytes give {other:?}")),
                }
            }
            Ok(())
        },
    );
}

#[test]
fn damaged_files_are_decoded_or_rejected_without_a_panic() {
    check(
        "qoi damage",
        2000,
        |rng| {
            let image = random_image(rng, 16);
            let mut bytes =
                qoi::encode(&image.header, &image.pixels).expect("a random image encodes");
            for _ in 0..1 + rng.range(0..4u32) {
                let at = rng.range(0..bytes.len() as u32) as usize;
                bytes[at] = rng.range(0..256u32) as u8;
            }
            bytes
        },
        |bytes| match decode(bytes) {
            Ok(image) => {
                let expected = image.header.width as usize * image.header.height as usize * 4;
                if image.pixels.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "{} bytes of pixels, not {expected}",
                        image.pixels.len()
                    ))
                }
            }
            Err(CodecError::Qoi { offset, .. }) if offset <= bytes.len() => Ok(()),
            Err(error) => Err(format!("{error} points outside the file")),
        },
    );
}
