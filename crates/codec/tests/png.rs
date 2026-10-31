//! PNG tests: the hand-made vectors in `tests/png/`, random images through
//! an encoder of the test's own, and damaged files. The error cases, one by
//! one, are beside the decoder, in `src/png.rs`.
//!
//! The vectors were written by `tests/png/make_vectors.py`, which shares no
//! code with the decoder: its own filters, bit packing and Adam7 split,
//! zlib (or its own stored blocks) for the image data, and the expected
//! pixels computed from its source samples rather than by decoding. libpng,
//! through GdkPixbuf, decodes every vector to the same pixels, except that
//! it truncates 16-bit samples where this decoder rounds them; so does Go's
//! image/png, except on the two vectors where this decoder follows libpng
//! (`g2n_trns_masked`, `p2n_pal256`).
//! `vectors.txt` lists each vector: its name, width, height, and the chunks
//! it should report as ignored.

use nadir_codec::png::{self, ErrorKind, Image};
use nadir_codec::{CodecError, Limits, adler32};
use nadir_core::Pcg32;
use nadir_core::hash::crc32;
use nadir_core::testing::{check, check_sized};

const VECTORS: &str = include_str!("png/vectors.txt");

fn vector(name: &str, extension: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/png/{name}.{extension}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn decode(bytes: &[u8]) -> Result<Image, CodecError> {
    png::decode(bytes, Limits::default())
}

#[test]
fn every_vector_decodes_to_its_pixels() {
    let mut count = 0;
    for line in VECTORS.lines() {
        let mut fields = line.split_whitespace();
        let name = fields.next().expect("a name");
        let width: u32 = fields.next().and_then(|w| w.parse().ok()).expect("a width");
        let height: u32 = fields
            .next()
            .and_then(|h| h.parse().ok())
            .expect("a height");
        let ignored: Vec<&str> = fields.collect();
        let image = decode(&vector(name, "png")).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!((image.width, image.height), (width, height), "{name}");
        assert!(image.pixels == vector(name, "rgba"), "{name}: other pixels");
        assert_eq!(image.ignored, ignored, "{name}");
        count += 1;
    }
    // Every color type and bit depth, each with and without interlacing,
    // and the rest.
    assert_eq!(count, 109);
}

#[test]
fn the_vectors_cover_every_color_type_bit_depth_and_interlace() {
    let names: Vec<&str> = VECTORS
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    for kind in [
        "g1", "g2", "g4", "g8", "g16", "rgb8", "rgb16", "p1", "p2", "p4", "p8", "ga8", "ga16",
        "rgba8", "rgba16",
    ] {
        for interlace in ["n", "i"] {
            let name = format!("{kind}{interlace}");
            assert!(names.contains(&name.as_str()), "{name}");
        }
    }
}

/// What a random image is made of, before encoding.
#[derive(Debug)]
struct Source {
    width: u32,
    height: u32,
    color_type: u8,
    depth: u8,
    interlaced: bool,
    /// Samples at `depth`, `channels` a pixel, rows from the top.
    samples: Vec<u16>,
    palette: Vec<[u8; 3]>,
    /// For a palette: alphas. For gray and RGB: the transparent color.
    transparency: Option<Vec<u16>>,
    /// The filter of each row, in the order the rows are written.
    filters: Vec<u8>,
    /// Where to split the zlib stream between `IDAT` chunks.
    splits: Vec<usize>,
}

const KINDS: [(u8, u8); 15] = [
    (0, 1),
    (0, 2),
    (0, 4),
    (0, 8),
    (0, 16),
    (2, 8),
    (2, 16),
    (3, 1),
    (3, 2),
    (3, 4),
    (3, 8),
    (4, 8),
    (4, 16),
    (6, 8),
    (6, 16),
];

const fn channels(color_type: u8) -> usize {
    match color_type {
        0 | 3 => 1,
        4 => 2,
        2 => 3,
        _ => 4,
    }
}

fn random_source(rng: &mut Pcg32, size: u32) -> Source {
    let (color_type, depth) = KINDS[rng.range(0..KINDS.len() as u32) as usize];
    let width = 1 + rng.range(0..size / 3 + 2);
    let height = 1 + rng.range(0..size / 3 + 2);
    let top = (1u32 << depth) - 1;
    let entries = if color_type == 3 {
        1 + rng.range(0..top.min(255) + 1) as usize
    } else {
        0
    };
    let palette: Vec<[u8; 3]> = (0..entries)
        .map(|_| [byte(rng), byte(rng), byte(rng)])
        .collect();
    let count = (width * height) as usize * channels(color_type);
    let mut samples = Vec::with_capacity(count);
    let mut previous = 0;
    for _ in 0..count {
        let sample = if color_type == 3 {
            rng.range(0..entries as u32)
        } else if rng.range(0..3u32) == 0 {
            previous
        } else {
            rng.range(0..top + 1)
        };
        samples.push(sample as u16);
        previous = sample;
    }
    let transparency = match (color_type, rng.range(0..3u32)) {
        (3, 0) => Some(
            (0..1 + rng.range(0..entries as u32))
                .map(|_| u16::from(byte(rng)))
                .collect(),
        ),
        (0 | 2, 0) => {
            // A color some pixel has, so that something is transparent.
            let n = channels(color_type);
            let at = rng.range(0..(width * height)) as usize * n;
            Some(samples[at..at + n].to_vec())
        }
        _ => None,
    };
    let interlaced = rng.range(0..2u32) == 1;
    let rows = (0..height as usize * 2)
        .map(|_| rng.range(0..5u32) as u8)
        .collect();
    let splits = (0..rng.range(0..4u32))
        .map(|_| rng.range(0..64u32) as usize)
        .collect();
    Source {
        width,
        height,
        color_type,
        depth,
        interlaced,
        samples,
        palette,
        transparency,
        filters: rows,
        splits,
    }
}

fn byte(rng: &mut Pcg32) -> u8 {
    rng.range(0..256u32) as u8
}

const ADAM7: [(u32, u32, u32, u32); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// A zlib stream of stored blocks.
fn stored_zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = if data.is_empty() {
        vec![&[]]
    } else {
        data.chunks(65535).collect()
    };
    for (i, block) in blocks.iter().enumerate() {
        out.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32::checksum(data).to_be_bytes());
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn encode(source: &Source) -> Vec<u8> {
    let n = channels(source.color_type);
    let bits = n * usize::from(source.depth);
    let bpp = bits.div_ceil(8);
    let passes: Vec<(u32, u32, u32, u32)> = if source.interlaced {
        ADAM7.to_vec()
    } else {
        vec![(0, 0, 1, 1)]
    };
    let mut raw = Vec::new();
    let mut row_index = 0;
    for (x0, y0, dx, dy) in passes {
        let pw = if source.width > x0 {
            (source.width - x0).div_ceil(dx)
        } else {
            0
        };
        let ph = if source.height > y0 {
            (source.height - y0).div_ceil(dy)
        } else {
            0
        };
        if pw == 0 || ph == 0 {
            continue;
        }
        let mut previous: Option<Vec<u8>> = None;
        for r in 0..ph {
            let y = y0 + r * dy;
            let mut line = Vec::new();
            let mut acc = 0u32;
            let mut used = 0;
            for i in 0..pw {
                let x = x0 + i * dx;
                let at = (y * source.width + x) as usize * n;
                for &sample in &source.samples[at..at + n] {
                    match source.depth {
                        16 => line.extend_from_slice(&sample.to_be_bytes()),
                        8 => line.push(sample as u8),
                        depth => {
                            acc = (acc << depth) | u32::from(sample);
                            used += u32::from(depth);
                            if used == 8 {
                                line.push(acc as u8);
                                acc = 0;
                                used = 0;
                            }
                        }
                    }
                }
            }
            if used > 0 {
                line.push((acc << (8 - used)) as u8);
            }
            let filter = source.filters[row_index % source.filters.len()];
            row_index += 1;
            raw.push(filter);
            for i in 0..line.len() {
                let a = if i >= bpp { line[i - bpp] } else { 0 };
                let b = previous.as_ref().map_or(0, |above| above[i]);
                let c = if i >= bpp {
                    previous.as_ref().map_or(0, |above| above[i - bpp])
                } else {
                    0
                };
                let predicted = match filter {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    _ => paeth(a, b, c),
                };
                raw.push(line[i].wrapping_sub(predicted));
            }
            previous = Some(line);
        }
    }
    let z = stored_zlib(&raw);

    let mut out = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&source.width.to_be_bytes());
    ihdr.extend_from_slice(&source.height.to_be_bytes());
    ihdr.extend_from_slice(&[
        source.depth,
        source.color_type,
        0,
        0,
        u8::from(source.interlaced),
    ]);
    chunk(&mut out, b"IHDR", &ihdr);
    if source.color_type == 3 {
        chunk(&mut out, b"PLTE", &source.palette.concat());
    }
    if let Some(values) = &source.transparency {
        let data: Vec<u8> = if source.color_type == 3 {
            values.iter().map(|&alpha| alpha as u8).collect()
        } else {
            values
                .iter()
                .flat_map(|value| value.to_be_bytes())
                .collect()
        };
        chunk(&mut out, b"tRNS", &data);
    }
    let mut start = 0;
    let mut splits: Vec<usize> = source.splits.iter().map(|&at| at.min(z.len())).collect();
    splits.sort_unstable();
    for at in splits.into_iter().chain([z.len()]) {
        chunk(&mut out, b"IDAT", &z[start..at]);
        start = at;
    }
    chunk(&mut out, b"IEND", &[]);
    out
}

/// What the decoder should give for `source`.
fn expected(source: &Source) -> Vec<u8> {
    let n = channels(source.color_type);
    let to_8 = |v: u16| match source.depth {
        16 => ((u32::from(v) * 255 + 32767) / 65535) as u8,
        depth => (u32::from(v) * 255 / ((1 << depth) - 1)) as u8,
    };
    let key = |px: &[u16]| {
        if source.transparency.as_deref() == Some(px) {
            0
        } else {
            255
        }
    };
    let mut out = Vec::new();
    for px in source.samples.chunks(n) {
        let rgba = match source.color_type {
            0 => [to_8(px[0]), to_8(px[0]), to_8(px[0]), key(px)],
            2 => [to_8(px[0]), to_8(px[1]), to_8(px[2]), key(px)],
            3 => {
                let [r, g, b] = source.palette[usize::from(px[0])];
                let alpha = source
                    .transparency
                    .as_ref()
                    .and_then(|alphas| alphas.get(usize::from(px[0])))
                    .map_or(255, |&alpha| alpha as u8);
                [r, g, b, alpha]
            }
            4 => [to_8(px[0]), to_8(px[0]), to_8(px[0]), to_8(px[1])],
            _ => [to_8(px[0]), to_8(px[1]), to_8(px[2]), to_8(px[3])],
        };
        out.extend_from_slice(&rgba);
    }
    out
}

#[test]
fn random_images_decode_to_their_samples() {
    check_sized("png random images", 600, random_source, |source| {
        let bytes = encode(source);
        let image = decode(&bytes).map_err(|error| format!("cannot decode: {error}"))?;
        if (image.width, image.height) != (source.width, source.height) {
            return Err(format!("size {}×{}", image.width, image.height));
        }
        if image.pixels != expected(source) {
            return Err("other pixels".to_string());
        }
        Ok(())
    });
}

#[test]
fn every_cut_short_file_is_rejected() {
    check_sized(
        "png prefixes",
        60,
        |rng, size| encode(&random_source(rng, size / 4)),
        |bytes| {
            for len in 0..bytes.len() {
                match decode(&bytes[..len]) {
                    Err(CodecError::Png { .. }) => {}
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
        "png damage",
        3000,
        |rng| {
            let mut bytes = encode(&random_source(rng, 24));
            for _ in 0..1 + rng.range(0..4u32) {
                let at = rng.range(0..bytes.len() as u32) as usize;
                bytes[at] = byte(rng);
            }
            if rng.range(0..2u32) == 0 {
                fix_crcs(&mut bytes);
            }
            bytes
        },
        |bytes| match decode(bytes) {
            Ok(image) => {
                let expected = image.width as usize * image.height as usize * 4;
                if image.pixels.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "{} bytes of pixels, not {expected}",
                        image.pixels.len()
                    ))
                }
            }
            Err(CodecError::Png { .. }) => Ok(()),
            Err(other) => Err(format!("not a PNG error: {other:?}")),
        },
    );
}

/// Rewrites the CRC of every whole chunk, so that damage inside a chunk
/// reaches the decoding of its contents.
fn fix_crcs(bytes: &mut [u8]) {
    let mut at = 8;
    while let Some(length) = bytes.get(at..at + 4) {
        let length = u32::from_be_bytes(length.try_into().expect("four bytes")) as usize;
        let Some(end) = at
            .checked_add(8 + length)
            .filter(|&end| end + 4 <= bytes.len())
        else {
            return;
        };
        let crc = crc32(&bytes[at + 4..end]);
        bytes[end..end + 4].copy_from_slice(&crc.to_be_bytes());
        at = end + 4;
    }
}

#[test]
fn a_damaged_vector_is_rejected_with_its_place() {
    // One byte of the image data changed: the Adler-32 no longer matches,
    // or the stream breaks earlier. Both are found inside the IDAT chunk.
    let mut bytes = vector("rgb8n_stored", "png");
    let idat = bytes
        .windows(4)
        .position(|window| window == b"IDAT")
        .expect("an IDAT chunk")
        - 4;
    bytes[idat + 8 + 40] ^= 0x10;
    fix_crcs(&mut bytes);
    match decode(&bytes) {
        Err(CodecError::Png {
            offset,
            kind: ErrorKind::BadImageData { .. },
        }) => assert!(offset > idat + 8, "{offset}"),
        other => panic!("{other:?}"),
    }
}
