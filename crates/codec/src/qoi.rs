//! QOI, the "Quite OK Image" format (specification 1.0, qoiformat.org):
//! lossless images, which the game loads as textures and the golden tests
//! keep their references in.
//!
//! [`decode`] reads a file into an [`Image`], and [`encode`] writes one,
//! byte for byte as the reference encoder (`qoi.h`) does.
//!
//! ```
//! use nadir_codec::Limits;
//! use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
//!
//! let header = Header {
//!     width: 2,
//!     height: 1,
//!     channels: Channels::Rgba,
//!     color_space: ColorSpace::Srgb,
//! };
//! let pixels = [255, 0, 0, 255, 255, 0, 0, 128];
//! let file = qoi::encode(&header, &pixels)?;
//! let image = qoi::decode(&file, Limits::default())?;
//! assert_eq!(image.header, header);
//! assert_eq!(image.pixels, pixels);
//! # Ok::<(), nadir_codec::CodecError>(())
//! ```
//!
//! Pixels are always RGBA8, 4 bytes each, rows from the top. The header's
//! channel count and color space describe the image to its reader; they do
//! not change how the pixels are stored. So a file with [`Channels::Rgb`]
//! still decodes to RGBA, with the alpha its chunks give: 255, unless one
//! of them sets another.
//!
//! What the decoder rejects, although the reference decoder accepts it:
//!
//! - A run that goes past the last pixel.
//! - Chunks that run out before the last pixel, where the reference
//!   repeats the last pixel decoded. Here the end marker is then read as
//!   chunks, and the file is cut short or its end marker is wrong.
//! - An end marker that is missing or wrong, and anything after it.
//!
//! An encoder that follows the specification writes none of these. Like the
//! reference, the decoder also rejects a width or height of 0, and it
//! rejects images larger than its [`Limits`].

use std::fmt;

use crate::{CodecError, Limits};

/// What the pixels of an image hold, as its header says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channels {
    /// Red, green and blue: the header says every alpha is 255. The
    /// encoder holds to it; the decoder gives whatever alpha the chunks do.
    /// Header value 3.
    Rgb,
    /// Red, green, blue and alpha. Header value 4.
    Rgba,
}

/// How the color channels are encoded, as the header says. Alpha is always
/// linear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    /// sRGB-encoded color. Header value 0.
    Srgb,
    /// Linear color. Header value 1.
    Linear,
}

/// The description of an image, from the 14 bytes that start a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Header {
    /// The width in pixels, at least 1.
    pub width: u32,
    /// The height in pixels, at least 1.
    pub height: u32,
    /// What the pixels hold.
    pub channels: Channels,
    /// How the color channels are encoded.
    pub color_space: ColorSpace,
}

/// A decoded image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Its header.
    pub header: Header,
    /// Its pixels: RGBA8, 4 bytes a pixel, rows from the top, `width × height
    /// × 4` bytes in all.
    pub pixels: Vec<u8>,
}

/// What is wrong with a QOI file. See [`CodecError::Qoi`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The input does not start with `qoif`.
    NotQoi,
    /// The width or the height is 0.
    EmptyImage,
    /// The width or the height is more than
    /// [`Limits::max_texture_dimension`].
    DimensionTooLarge {
        /// The limit that was exceeded.
        limit: u32,
    },
    /// The pixels would take more than [`Limits::max_decoded_bytes`].
    TooManyBytes {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// The channel count is neither 3 nor 4.
    BadChannels {
        /// The value in the header.
        found: u8,
    },
    /// The color space is neither 0 nor 1.
    BadColorSpace {
        /// The value in the header.
        found: u8,
    },
    /// The input ends too soon.
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// A run goes past the last pixel.
    RunPastEnd,
    /// The 8 bytes after the last pixel are not the end marker.
    BadEndMarker,
    /// Bytes follow the end marker.
    TrailingBytes,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotQoi => f.write_str("the file does not start with `qoif`"),
            Self::EmptyImage => f.write_str("the image is 0 pixels wide or tall"),
            Self::DimensionTooLarge { limit } => write!(
                f,
                "the image is wider or taller than the limit of {limit} pixels"
            ),
            Self::TooManyBytes { limit } => write!(
                f,
                "the pixels would take more than the limit of {limit} bytes"
            ),
            Self::BadChannels { found } => {
                write!(f, "the channel count is {found}, not 3 or 4")
            }
            Self::BadColorSpace { found } => {
                write!(f, "the color space is {found}, not 0 or 1")
            }
            Self::UnexpectedEnd { expected } => {
                write!(f, "the file ends where {expected} should be")
            }
            Self::RunPastEnd => f.write_str("a run goes past the last pixel"),
            Self::BadEndMarker => f.write_str("the end marker is missing or wrong"),
            Self::TrailingBytes => f.write_str("there are bytes after the end marker"),
        }
    }
}

/// Why an image cannot be encoded. See [`CodecError::QoiEncode`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeErrorKind {
    /// The width or the height is 0.
    EmptyImage,
    /// The pixels are not `width × height × 4` bytes.
    WrongDataSize {
        /// The width the header gives.
        width: u32,
        /// The height the header gives.
        height: u32,
        /// The number of bytes of pixels.
        found: usize,
    },
    /// A pixel of an image with [`Channels::Rgb`] has an alpha other than
    /// 255.
    AlphaInRgb {
        /// The pixel, counted from 0 in rows from the top.
        pixel: usize,
    },
}

impl fmt::Display for EncodeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyImage => f.write_str("the image is 0 pixels wide or tall"),
            Self::WrongDataSize {
                width,
                height,
                found,
            } => write!(
                f,
                "the pixels are {found} bytes, not {width} × {height} × 4"
            ),
            Self::AlphaInRgb { pixel } => write!(
                f,
                "pixel {pixel} of an RGB image has an alpha other than 255"
            ),
        }
    }
}

const MAGIC: [u8; 4] = *b"qoif";
const HEADER_LEN: usize = 14;
const END_MARKER: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];

// The tags. The two 8-bit ones are checked first: as 2-bit tags they would
// read as runs of 63 and 64, which therefore do not exist.
const OP_RGB: u8 = 0xfe;
const OP_RGBA: u8 = 0xff;
const OP_INDEX: u8 = 0x00;
const OP_DIFF: u8 = 0x40;
const OP_LUMA: u8 = 0x80;
const OP_RUN: u8 = 0xc0;
const TAG_MASK: u8 = 0xc0;

/// What a chunk cut short lacks.
const REST: &str = "the rest of a chunk";

/// The most pixels one run chunk gives.
const MAX_RUN: u8 = 62;

/// Where a pixel is kept in the table of recently seen pixels.
fn slot([r, g, b, a]: [u8; 4]) -> usize {
    (usize::from(r) * 3 + usize::from(g) * 5 + usize::from(b) * 7 + usize::from(a) * 11) % 64
}

fn error(offset: usize, kind: ErrorKind) -> CodecError {
    CodecError::Qoi { offset, kind }
}

/// Decodes a QOI file.
///
/// # Errors
///
/// [`CodecError::Qoi`], with the byte offset of the problem, for a file
/// that is malformed or breaks a limit of `limits`.
pub fn decode(bytes: &[u8], limits: Limits) -> Result<Image, CodecError> {
    let header = read_header(bytes, limits)?;
    // The header check keeps this within `max_decoded_bytes`.
    let len = pixel_bytes(&header)
        .and_then(|len| usize::try_from(len).ok())
        .ok_or_else(|| error(4, too_many_bytes(limits)))?;
    let count = len / 4;
    // A chunk byte gives at most one run of 62 pixels, so a file too short
    // to hold that many is cut off, however it goes on. Checking this first
    // keeps a small file from claiming a large allocation.
    let room = bytes.len().saturating_sub(HEADER_LEN);
    if count.div_ceil(usize::from(MAX_RUN)) > room {
        return Err(cut_short(bytes, "more chunks"));
    }

    let mut pixels = Vec::with_capacity(len);
    let mut seen = [[0u8; 4]; 64];
    let mut px = [0, 0, 0, 255];
    let mut at = HEADER_LEN;
    while pixels.len() < len {
        let start = at;
        let [tag] = take(bytes, &mut at, "the next chunk")?;
        let mut run = 1;
        match tag {
            OP_RGB => {
                let [r, g, b] = take(bytes, &mut at, REST)?;
                px = [r, g, b, px[3]];
            }
            OP_RGBA => px = take(bytes, &mut at, REST)?,
            _ => match tag & TAG_MASK {
                OP_INDEX => px = seen[usize::from(tag & 0x3f)],
                OP_DIFF => {
                    let [r, g, b, a] = px;
                    px = [
                        r.wrapping_add((tag >> 4) & 3).wrapping_sub(2),
                        g.wrapping_add((tag >> 2) & 3).wrapping_sub(2),
                        b.wrapping_add(tag & 3).wrapping_sub(2),
                        a,
                    ];
                }
                OP_LUMA => {
                    let [second] = take(bytes, &mut at, REST)?;
                    let dg = (tag & 0x3f).wrapping_sub(32);
                    let [r, g, b, a] = px;
                    px = [
                        r.wrapping_add(dg).wrapping_sub(8).wrapping_add(second >> 4),
                        g.wrapping_add(dg),
                        b.wrapping_add(dg)
                            .wrapping_sub(8)
                            .wrapping_add(second & 0x0f),
                        a,
                    ];
                }
                _ => {
                    run = usize::from(tag & 0x3f) + 1;
                    if run > (len - pixels.len()) / 4 {
                        return Err(error(start, ErrorKind::RunPastEnd));
                    }
                }
            },
        }
        // The reference decoder stores the pixel after every chunk, runs
        // and lookups included, and encoders may count on it.
        seen[slot(px)] = px;
        for _ in 0..run {
            pixels.extend_from_slice(&px);
        }
    }

    let tail = bytes.get(at..).unwrap_or_default();
    match tail.split_first_chunk::<8>() {
        Some((marker, rest)) if *marker == END_MARKER => match rest {
            [] => Ok(Image { header, pixels }),
            _ => Err(error(at + END_MARKER.len(), ErrorKind::TrailingBytes)),
        },
        None if END_MARKER.starts_with(tail) => Err(cut_short(bytes, "the end marker")),
        _ => Err(error(at, ErrorKind::BadEndMarker)),
    }
}

/// Encodes an image, as the reference encoder would. `pixels` holds RGBA8,
/// 4 bytes a pixel, rows from the top.
///
/// # Errors
///
/// [`CodecError::QoiEncode`] for a width or height of 0, for pixels that
/// are not `width × height × 4` bytes, and for [`Channels::Rgb`] with an
/// alpha other than 255.
pub fn encode(header: &Header, pixels: &[u8]) -> Result<Vec<u8>, CodecError> {
    let Header {
        width,
        height,
        channels,
        color_space,
    } = *header;
    let refuse = |kind| Err(CodecError::QoiEncode { kind });
    if width == 0 || height == 0 {
        return refuse(EncodeErrorKind::EmptyImage);
    }
    if pixel_bytes(header) != u64::try_from(pixels.len()).ok() {
        return refuse(EncodeErrorKind::WrongDataSize {
            width,
            height,
            found: pixels.len(),
        });
    }
    let pixels = pixels.as_chunks::<4>().0;
    if channels == Channels::Rgb
        && let Some(pixel) = pixels.iter().position(|&[.., a]| a != 255)
    {
        return refuse(EncodeErrorKind::AlphaInRgb { pixel });
    }

    let mut out = Vec::with_capacity(HEADER_LEN + pixels.len() + END_MARKER.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.push(match channels {
        Channels::Rgb => 3,
        Channels::Rgba => 4,
    });
    out.push(match color_space {
        ColorSpace::Srgb => 0,
        ColorSpace::Linear => 1,
    });

    let mut seen = [[0u8; 4]; 64];
    let mut previous = [0, 0, 0, 255];
    let mut run = 0u8;
    let last = pixels.len() - 1;
    for (index, &px) in pixels.iter().enumerate() {
        if px == previous {
            run += 1;
            if run == MAX_RUN || index == last {
                out.push(OP_RUN | (run - 1));
                run = 0;
            }
            continue;
        }
        if run > 0 {
            out.push(OP_RUN | (run - 1));
            run = 0;
        }
        // Like the reference encoder, this stores a pixel only when it
        // writes one of the four chunks below, never for a run.
        let slot = slot(px);
        if seen[slot] == px {
            // `slot` is below 64.
            out.push(OP_INDEX | slot as u8);
        } else {
            seen[slot] = px;
            push_chunk(&mut out, previous, px);
        }
        previous = px;
    }
    out.extend_from_slice(&END_MARKER);
    Ok(out)
}

/// Writes the smallest chunk that turns `previous` into `px`, other than a
/// run or a lookup, in the reference encoder's order of preference.
fn push_chunk(out: &mut Vec<u8>, previous: [u8; 4], px: [u8; 4]) {
    let [r, g, b, a] = px;
    if a != previous[3] {
        out.extend_from_slice(&[OP_RGBA, r, g, b, a]);
        return;
    }
    // Differences wrap around: 1 - 2 is -1, and 255 + 1 is 0.
    let dr = r.wrapping_sub(previous[0]).cast_signed();
    let dg = g.wrapping_sub(previous[1]).cast_signed();
    let db = b.wrapping_sub(previous[2]).cast_signed();
    let small = |d: i8| (-2..=1).contains(&d);
    if small(dr) && small(dg) && small(db) {
        let bias = |d: i8| (d + 2).cast_unsigned();
        out.push(OP_DIFF | (bias(dr) << 4) | (bias(dg) << 2) | bias(db));
        return;
    }
    let dr_dg = dr.wrapping_sub(dg);
    let db_dg = db.wrapping_sub(dg);
    let near = |d: i8| (-8..=7).contains(&d);
    if (-32..=31).contains(&dg) && near(dr_dg) && near(db_dg) {
        let bias = |d: i8| (d + 8).cast_unsigned();
        out.extend_from_slice(&[
            OP_LUMA | (dg + 32).cast_unsigned(),
            (bias(dr_dg) << 4) | bias(db_dg),
        ]);
        return;
    }
    out.extend_from_slice(&[OP_RGB, r, g, b]);
}

/// Reads and checks the header.
fn read_header(bytes: &[u8], limits: Limits) -> Result<Header, CodecError> {
    if !MAGIC.starts_with(bytes.get(..MAGIC.len()).unwrap_or(bytes)) {
        return Err(error(0, ErrorKind::NotQoi));
    }
    let Some(
        &[
            _,
            _,
            _,
            _,
            w0,
            w1,
            w2,
            w3,
            h0,
            h1,
            h2,
            h3,
            channels,
            color_space,
        ],
    ) = bytes.first_chunk::<HEADER_LEN>()
    else {
        return Err(cut_short(bytes, "the rest of the header"));
    };
    let check_side = |side: u32, offset: usize| match side {
        0 => Err(error(offset, ErrorKind::EmptyImage)),
        side if side > limits.max_texture_dimension => Err(error(
            offset,
            ErrorKind::DimensionTooLarge {
                limit: limits.max_texture_dimension,
            },
        )),
        side => Ok(side),
    };
    let width = check_side(u32::from_be_bytes([w0, w1, w2, w3]), 4)?;
    let height = check_side(u32::from_be_bytes([h0, h1, h2, h3]), 8)?;
    let channels = match channels {
        3 => Channels::Rgb,
        4 => Channels::Rgba,
        found => return Err(error(12, ErrorKind::BadChannels { found })),
    };
    let color_space = match color_space {
        0 => ColorSpace::Srgb,
        1 => ColorSpace::Linear,
        found => return Err(error(13, ErrorKind::BadColorSpace { found })),
    };
    let header = Header {
        width,
        height,
        channels,
        color_space,
    };
    let fits = pixel_bytes(&header)
        .is_some_and(|len| u64::try_from(limits.max_decoded_bytes).is_ok_and(|limit| len <= limit));
    if !fits {
        return Err(error(4, too_many_bytes(limits)));
    }
    Ok(header)
}

fn too_many_bytes(limits: Limits) -> ErrorKind {
    ErrorKind::TooManyBytes {
        limit: limits.max_decoded_bytes,
    }
}

/// `width × height × 4`, unless that overflows a `u64`.
fn pixel_bytes(header: &Header) -> Option<u64> {
    u64::from(header.width)
        .checked_mul(u64::from(header.height))?
        .checked_mul(4)
}

/// Takes the next `N` bytes from `at`, which it moves past them, or fails
/// with what they should have been.
fn take<const N: usize>(
    bytes: &[u8],
    at: &mut usize,
    expected: &'static str,
) -> Result<[u8; N], CodecError> {
    let next = bytes
        .get(*at..)
        .and_then(<[u8]>::first_chunk::<N>)
        .ok_or_else(|| cut_short(bytes, expected))?;
    *at += N;
    Ok(*next)
}

/// The input ends where `expected` should be.
fn cut_short(bytes: &[u8], expected: &'static str) -> CodecError {
    error(bytes.len(), ErrorKind::UnexpectedEnd { expected })
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn header(width: u32, height: u32) -> Header {
        Header {
            width,
            height,
            channels: Channels::Rgba,
            color_space: ColorSpace::Srgb,
        }
    }

    /// A file: a header with these fields, `body`, and the end marker.
    fn file(width: u32, height: u32, channels: u8, color_space: u8, body: &[u8]) -> Vec<u8> {
        let mut bytes = b"qoif".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[channels, color_space]);
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&END_MARKER);
        bytes
    }

    fn flat(pixels: &[[u8; 4]]) -> Vec<u8> {
        pixels.concat()
    }

    fn err(bytes: &[u8]) -> (usize, ErrorKind) {
        err_with(bytes, Limits::default())
    }

    fn err_with(bytes: &[u8], limits: Limits) -> (usize, ErrorKind) {
        match decode(bytes, limits) {
            Err(CodecError::Qoi { offset, kind }) => (offset, kind),
            other => panic!("not a QOI error: {other:?}"),
        }
    }

    fn encode_err(header: &Header, pixels: &[u8]) -> EncodeErrorKind {
        match encode(header, pixels) {
            Err(CodecError::QoiEncode { kind }) => kind,
            other => panic!("not a QOI encoding error: {other:?}"),
        }
    }

    const OPAQUE_BLACK: [u8; 4] = [0, 0, 0, 255];

    /// A name, the pixels of one row, and the chunks they encode to.
    type Vector = (&'static str, Vec<[u8; 4]>, Vec<u8>);

    /// Images of one row, and the chunks the reference encoder writes for
    /// them, worked out by hand from the specification. The first pixel is
    /// compared with opaque black, and the table of seen pixels starts as
    /// all zeros.
    fn vectors() -> Vec<Vector> {
        vec![
            // Equal to the starting pixel: a run of 1, 0b11_000000.
            ("starting pixel", vec![OPAQUE_BLACK], vec![0xc0]),
            // Transparent black is in slot 0 of the zeroed table:
            // QOI_OP_INDEX 0, which is the byte 0x00.
            ("zeroed table", vec![[0, 0, 0, 0]], vec![0x00]),
            // dr = 1, dg = -1 (0 - 1 wraps to 255), db = 0; biased by 2:
            // 0b01 11 01 10.
            ("diff", vec![[1, 255, 0, 255]], vec![0x76]),
            // dr = -1 (0 wraps to 255), dg = 1, db = 0: 0b01 01 11 10.
            ("diff wrapping", vec![[255, 1, 0, 255]], vec![0x5e]),
            // dg = 20, dr - dg = -8, db - dg = 7, the edges of the range:
            // 0b10 110100 (20 + 32), then 0000 1111 (-8 + 8, 7 + 8).
            ("luma", vec![[12, 20, 27, 255]], vec![0xb4, 0x0f]),
            // dg = -32 and 31, the edges of its range.
            ("luma low", vec![[224, 224, 224, 255]], vec![0x80, 0x88]),
            ("luma high", vec![[31, 31, 31, 255]], vec![0xbf, 0x88]),
            // dg = 32 is out of range, and so is dr - dg = 8.
            ("rgb", vec![[32, 32, 32, 255]], vec![0xfe, 32, 32, 32]),
            ("rgb dr", vec![[18, 10, 10, 255]], vec![0xfe, 18, 10, 10]),
            // A change of alpha always takes all four.
            ("rgba", vec![[1, 2, 3, 4]], vec![0xff, 1, 2, 3, 4]),
            // (100, 50, 200, 255) is in slot 19 and opaque black in slot 53.
            // The third pixel is found in the table.
            (
                "index",
                vec![[100, 50, 200, 255], OPAQUE_BLACK, [100, 50, 200, 255]],
                vec![0xfe, 100, 50, 200, 0xfe, 0, 0, 0, 0x13],
            ),
            // A starting run, then transparent black from the zeroed table.
            (
                "run, then index",
                vec![OPAQUE_BLACK, [0, 0, 0, 0]],
                vec![0xc0, 0x00],
            ),
            // (5, 5, 5, 255): luma 0b10 100101 (5 + 32) and 1000 1000; then
            // 62 more, the longest run, 0b11 111101.
            (
                "longest run",
                [[5, 5, 5, 255]; 63].to_vec(),
                vec![0xa5, 0x88, 0xfd],
            ),
            // One more is a second run, of 1.
            (
                "run of 63",
                [[5, 5, 5, 255]; 64].to_vec(),
                vec![0xa5, 0x88, 0xfd, 0xc0],
            ),
            // A lookup is tried before a diff, and a diff before luma.
            (
                "lookup first",
                vec![[1, 1, 1, 255], [2, 2, 2, 255], [1, 1, 1, 255]],
                vec![0x7f, 0x7f, 0x04],
            ),
            // The encoder stores no pixel for a run, so opaque black is not
            // in slot 53 after the starting run: a diff, not a lookup (0x35).
            (
                "no store for runs",
                vec![OPAQUE_BLACK, OPAQUE_BLACK, [2, 2, 2, 255], OPAQUE_BLACK],
                vec![0xc1, 0xa2, 0x88, 0x40],
            ),
            // All three hash to slot 53. The second takes the slot.
            (
                "slot collision",
                vec![[64, 0, 0, 255], [0, 64, 0, 255], [64, 0, 0, 255]],
                vec![0xfe, 64, 0, 0, 0xfe, 0, 64, 0, 0xfe, 64, 0, 0],
            ),
            // dr = 10, dg = 8, db = 6, each after wrapping: luma
            // 0b10 101000 (8 + 32), then 1010 0110 (2 + 8, -2 + 8).
            (
                "luma wrapping",
                vec![[250, 250, 250, 255], [4, 2, 0, 255]],
                vec![0x9a, 0x88, 0xa8, 0xa6],
            ),
            // Translucent pixels with the same alpha take a diff, luma and
            // RGB chunk, as opaque ones do.
            (
                "translucent",
                vec![
                    [10, 10, 10, 128],
                    [11, 10, 10, 128],
                    [21, 20, 20, 128],
                    [100, 20, 20, 128],
                ],
                vec![0xff, 10, 10, 10, 128, 0x7a, 0xaa, 0x88, 0xfe, 100, 20, 20],
            ),
            // A run ends at a change of alpha, and at the last pixel.
            (
                "run and alpha",
                vec![
                    [1, 1, 1, 255],
                    [1, 1, 1, 255],
                    [1, 1, 1, 255],
                    [1, 1, 1, 0],
                    [1, 1, 1, 0],
                ],
                vec![0x7f, 0xc1, 0xff, 1, 1, 1, 0, 0xc0],
            ),
            // A run ends before a pixel that differs.
            (
                "run, then diff",
                vec![
                    [5, 5, 5, 255],
                    [5, 5, 5, 255],
                    [5, 5, 5, 255],
                    [6, 5, 5, 255],
                ],
                vec![0xa5, 0x88, 0xc1, 0x7a],
            ),
        ]
    }

    #[test]
    fn the_encoder_writes_what_the_reference_encoder_writes() {
        for (name, pixels, body) in vectors() {
            let width = pixels.len() as u32;
            let encoded = encode(&header(width, 1), &flat(&pixels)).unwrap();
            assert_eq!(encoded, file(width, 1, 4, 0, &body), "{name}");
        }
    }

    #[test]
    fn the_decoder_reads_what_the_reference_encoder_writes() {
        for (name, pixels, body) in vectors() {
            let width = pixels.len() as u32;
            let image = decode(&file(width, 1, 4, 0, &body), Limits::default()).unwrap();
            assert_eq!(image.header, header(width, 1), "{name}");
            assert_eq!(image.pixels, flat(&pixels), "{name}");
        }
    }

    #[test]
    fn the_decoder_stores_every_pixel_even_from_runs_and_lookups() {
        // The encoder writes 0xc1 for two opaque black pixels. Another
        // encoder may look the second up, in slot 53, which the run filled.
        let image = decode(&file(2, 1, 4, 0, &[0xc0, 0x35]), Limits::default()).unwrap();
        assert_eq!(image.pixels, flat(&[OPAQUE_BLACK; 2]));
        let image = decode(&file(2, 1, 4, 0, &[0xc1]), Limits::default()).unwrap();
        assert_eq!(image.pixels, flat(&[OPAQUE_BLACK; 2]));
    }

    #[test]
    fn the_header_gives_the_channels_and_color_space() {
        let rgb_linear = Header {
            width: 2,
            height: 3,
            channels: Channels::Rgb,
            color_space: ColorSpace::Linear,
        };
        let pixels = flat(&[OPAQUE_BLACK; 6]);
        let encoded = encode(&rgb_linear, &pixels).unwrap();
        assert_eq!(encoded, file(2, 3, 3, 1, &[0xc5]));
        let image = decode(&encoded, Limits::default()).unwrap();
        assert_eq!(image.header, rgb_linear);
        // RGB still decodes to RGBA.
        assert_eq!(image.pixels, pixels);
        // The channel count does not change how chunks are read: an RGB
        // file may hold an alpha.
        let image = decode(&file(1, 1, 3, 0, &[0xff, 1, 2, 3, 4]), Limits::default()).unwrap();
        assert_eq!(image.pixels, [1, 2, 3, 4]);
    }

    #[test]
    fn rows_run_from_the_top() {
        let pixels = flat(&[
            [1, 0, 0, 255],
            [2, 0, 0, 255],
            [3, 0, 0, 255],
            [4, 0, 0, 255],
        ]);
        let image = decode(&encode(&header(2, 2), &pixels).unwrap(), Limits::default()).unwrap();
        assert_eq!(image.pixels, pixels);
        // Runs cross rows.
        let encoded = encode(&header(3, 2), &flat(&[OPAQUE_BLACK; 6])).unwrap();
        assert_eq!(encoded, file(3, 2, 4, 0, &[0xc5]));
    }

    #[test]
    fn a_file_that_is_not_qoi_is_rejected() {
        assert_eq!(err(b"\x89PNG\r\n\x1a\n"), (0, ErrorKind::NotQoi));
        assert_eq!(err(b"qoiF"), (0, ErrorKind::NotQoi));
        assert_eq!(err(b"x"), (0, ErrorKind::NotQoi));
        // A start of the magic is a file cut short.
        let header = ErrorKind::UnexpectedEnd {
            expected: "the rest of the header",
        };
        assert_eq!(err(b""), (0, header.clone()));
        assert_eq!(err(b"qo"), (2, header.clone()));
        let whole = file(1, 1, 4, 0, &[0xc0]);
        assert_eq!(err(&whole[..13]), (13, header));
    }

    #[test]
    fn header_fields_are_checked() {
        assert_eq!(err(&file(0, 1, 4, 0, &[])), (4, ErrorKind::EmptyImage));
        assert_eq!(err(&file(1, 0, 4, 0, &[])), (8, ErrorKind::EmptyImage));
        assert_eq!(
            err(&file(1, 1, 5, 0, &[0xc0])),
            (12, ErrorKind::BadChannels { found: 5 })
        );
        assert_eq!(
            err(&file(1, 1, 0, 0, &[0xc0])),
            (12, ErrorKind::BadChannels { found: 0 })
        );
        assert_eq!(
            err(&file(1, 1, 4, 2, &[0xc0])),
            (13, ErrorKind::BadColorSpace { found: 2 })
        );
    }

    #[test]
    fn images_beyond_the_limits_are_rejected() {
        let too_wide = ErrorKind::DimensionTooLarge { limit: 8192 };
        assert_eq!(err(&file(8193, 1, 4, 0, &[])), (4, too_wide.clone()));
        assert_eq!(err(&file(1, 8193, 4, 0, &[])), (8, too_wide));
        assert_eq!(
            err(&file(u32::MAX, u32::MAX, 4, 0, &[])),
            (4, ErrorKind::DimensionTooLarge { limit: 8192 })
        );
        // The limits are configurable.
        let limits = Limits {
            max_texture_dimension: 2,
            max_decoded_bytes: 12,
            ..Limits::default()
        };
        assert!(decode(&file(3, 1, 4, 0, &[0xc2]), Limits::default()).is_ok());
        assert_eq!(
            err_with(&file(3, 1, 4, 0, &[0xc2]), limits),
            (4, ErrorKind::DimensionTooLarge { limit: 2 })
        );
        assert!(decode(&file(1, 2, 4, 0, &[0xc1]), limits).is_ok());
        assert_eq!(
            err_with(&file(2, 2, 4, 0, &[0xc3]), limits),
            (4, ErrorKind::TooManyBytes { limit: 12 })
        );
    }

    #[test]
    fn a_file_too_short_for_its_size_is_rejected_before_decoding() {
        // The largest image the limits allow, in a file with no room for
        // its pixels: 64 Mi pixels need at least 1 082 402 chunk bytes. This
        // checks the error only; nothing here counts what is allocated.
        let more = ErrorKind::UnexpectedEnd {
            expected: "more chunks",
        };
        let huge = file(8192, 8192, 4, 0, &[0xfd; 1000]);
        assert_eq!(err(&huge), (huge.len(), more.clone()));
        // Shorter than a header and an end marker: nothing underflows.
        for len in [14, 21, 22] {
            assert_eq!(err(&huge[..len]), (len, more.clone()), "{len}");
        }
        // 64 pixels in two runs, the fewest bytes they fit in.
        assert!(decode(&file(8, 8, 4, 0, &[0xfd, 0xc1]), Limits::default()).is_ok());
        // One run is all the room a 23-byte file has. A 63rd pixel is read
        // from the end marker, which then is wrong.
        assert!(decode(&file(62, 1, 4, 0, &[0xfd]), Limits::default()).is_ok());
        assert_eq!(
            err(&file(63, 1, 4, 0, &[0xfd])),
            (16, ErrorKind::BadEndMarker)
        );
    }

    #[test]
    fn a_file_cut_short_is_rejected() {
        // The end inside a chunk, or before the next one.
        let rest = ErrorKind::UnexpectedEnd {
            expected: "the rest of a chunk",
        };
        let mut bytes = file(1, 1, 4, 0, &[0xfe, 1]);
        bytes.truncate(16);
        assert_eq!(err(&bytes), (16, rest.clone()));
        let mut bytes = file(1, 1, 4, 0, &[0xff, 1, 2, 3]);
        bytes.truncate(18);
        assert_eq!(err(&bytes), (18, rest.clone()));
        let mut bytes = file(1, 1, 4, 0, &[0x80]);
        bytes.truncate(15);
        assert_eq!(err(&bytes), (15, rest));
        let mut bytes = file(2, 1, 4, 0, &[0x00]);
        bytes.truncate(15);
        let next = ErrorKind::UnexpectedEnd {
            expected: "the next chunk",
        };
        assert_eq!(err(&bytes), (15, next));
        // Every pixel there, and none or part of the end marker.
        let marker = ErrorKind::UnexpectedEnd {
            expected: "the end marker",
        };
        let whole = file(1, 1, 4, 0, &[0xc0]);
        for len in 15..whole.len() {
            assert_eq!(err(&whole[..len]), (len, marker.clone()), "{len}");
        }
    }

    #[test]
    fn a_run_past_the_last_pixel_is_rejected() {
        assert_eq!(err(&file(1, 1, 4, 0, &[0xc1])), (14, ErrorKind::RunPastEnd));
        assert_eq!(
            err(&file(3, 1, 4, 0, &[0x00, 0xc2])),
            (15, ErrorKind::RunPastEnd)
        );
        assert!(decode(&file(3, 1, 4, 0, &[0x00, 0xc1]), Limits::default()).is_ok());
    }

    #[test]
    fn the_end_marker_must_be_there_and_be_last() {
        let mut bytes = file(1, 1, 4, 0, &[0xc0]);
        *bytes.last_mut().unwrap() = 2;
        assert_eq!(err(&bytes), (15, ErrorKind::BadEndMarker));
        // Every byte counts, not only the last.
        for at in [15, 18, 21] {
            let mut bytes = file(1, 1, 4, 0, &[0xc0]);
            bytes[at] = 0xff;
            assert_eq!(err(&bytes), (15, ErrorKind::BadEndMarker), "{at}");
        }
        // A wrong start, even if short.
        let bytes = [&file(1, 1, 4, 0, &[0xc0])[..15], &[0, 1]].concat();
        assert_eq!(err(&bytes), (15, ErrorKind::BadEndMarker));
        // A pixel too many is a wrong marker, as the image is complete.
        let bytes = file(1, 1, 4, 0, &[0xc0, 0xc0]);
        assert_eq!(err(&bytes), (15, ErrorKind::BadEndMarker));
        let mut bytes = file(1, 1, 4, 0, &[0xc0]);
        bytes.push(0);
        assert_eq!(err(&bytes), (23, ErrorKind::TrailingBytes));
        bytes.extend_from_slice(&END_MARKER);
        assert_eq!(err(&bytes), (23, ErrorKind::TrailingBytes));
    }

    #[test]
    fn the_encoder_checks_its_input() {
        assert_eq!(encode_err(&header(0, 1), &[]), EncodeErrorKind::EmptyImage);
        assert_eq!(encode_err(&header(1, 0), &[]), EncodeErrorKind::EmptyImage);
        let wrong = |found| EncodeErrorKind::WrongDataSize {
            width: 1,
            height: 1,
            found,
        };
        assert_eq!(encode_err(&header(1, 1), &[0; 3]), wrong(3));
        assert_eq!(encode_err(&header(1, 1), &[0; 8]), wrong(8));
        // No overflow, however large the header.
        assert_eq!(
            encode_err(&header(u32::MAX, u32::MAX), &[0; 4]),
            EncodeErrorKind::WrongDataSize {
                width: u32::MAX,
                height: u32::MAX,
                found: 4
            }
        );
        let rgb = Header {
            channels: Channels::Rgb,
            ..header(3, 1)
        };
        assert_eq!(
            encode_err(&rgb, &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 254]),
            EncodeErrorKind::AlphaInRgb { pixel: 2 }
        );
    }

    #[test]
    fn slots_follow_the_specified_hash() {
        assert_eq!(slot([0, 0, 0, 0]), 0);
        assert_eq!(slot(OPAQUE_BLACK), 53);
        assert_eq!(slot([100, 50, 200, 255]), 19);
        assert_eq!(slot([255, 255, 255, 255]), (255 * 26) % 64);
    }

    #[test]
    fn error_kinds_explain_themselves() {
        let cases = [
            (ErrorKind::NotQoi, "the file does not start with `qoif`"),
            (
                ErrorKind::DimensionTooLarge { limit: 8192 },
                "the image is wider or taller than the limit of 8192 pixels",
            ),
            (
                ErrorKind::TooManyBytes { limit: 16 },
                "the pixels would take more than the limit of 16 bytes",
            ),
            (
                ErrorKind::BadColorSpace { found: 9 },
                "the color space is 9, not 0 or 1",
            ),
            (
                ErrorKind::UnexpectedEnd {
                    expected: "the end marker",
                },
                "the file ends where the end marker should be",
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(kind.to_string(), expected);
        }
        for kind in [
            ErrorKind::EmptyImage,
            ErrorKind::RunPastEnd,
            ErrorKind::BadEndMarker,
            ErrorKind::TrailingBytes,
        ] {
            assert!(!kind.to_string().is_empty());
        }
        let cases = [
            (
                EncodeErrorKind::EmptyImage,
                "the image is 0 pixels wide or tall",
            ),
            (
                EncodeErrorKind::WrongDataSize {
                    width: 2,
                    height: 3,
                    found: 5,
                },
                "the pixels are 5 bytes, not 2 × 3 × 4",
            ),
            (
                EncodeErrorKind::AlphaInRgb { pixel: 7 },
                "pixel 7 of an RGB image has an alpha other than 255",
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(kind.to_string(), expected);
        }
    }
}
