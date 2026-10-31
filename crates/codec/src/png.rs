//! PNG (ISO/IEC 15948, PNG 1.2): the images artists export, which the cook
//! tool reads and turns into QOI textures. Behind the `png` feature: the
//! game does not read PNG.
//!
//! [`decode`] reads a file into an [`Image`] of RGBA8 pixels, whatever its
//! color type and bit depth.
//!
//! - **Supported:** every color type and bit depth: grayscale at 1, 2, 4, 8
//!   and 16 bits; RGB, grayscale with alpha and RGBA at 8 and 16; palettes
//!   at 1, 2, 4 and 8. A 16-bit sample becomes the nearest 8-bit value,
//!   `round(v / 257)` (libpng's `png_set_strip_16` truncates instead), and
//!   grayscale below 8 bits is scaled to 0–255. `tRNS`, Adam7 interlacing
//!   and all five filters. Every chunk's CRC, and the image data's Adler-32,
//!   are checked.
//! - **Ignored, and named in [`Image::ignored`]:** the chunks that say how
//!   the colors are encoded, `gAMA`, `cHRM`, `sRGB`, `iCCP`, `cICP`, `mDCV`
//!   and `cLLI`, and the text chunks `tEXt`, `zTXt` and `iTXt`. The pixels
//!   are taken to be sRGB. Other ancillary chunks are ignored without a
//!   word.
//! - **Rejected:** unknown critical chunks, Apple's `CgBI` among them (it
//!   comes before `IHDR`, so the error is that the first chunk is not
//!   `IHDR`), and the chunks of an animated PNG, `acTL`, `fcTL` and `fdAT`.
//!
//! What the decoder rejects, although libpng 1.6 loads it, with a warning
//! or none (each measured through GdkPixbuf): a file this damaged is better
//! exported again than cooked.
//!
//! - a CRC that does not match in an ancillary chunk
//! - bytes after `IEND`, or `IEND` with data
//! - bytes after the image data's zlib stream, or image data that inflates
//!   to more than the image needs
//! - an `IDAT` after another chunk once the image data has ended, empty or
//!   not
//! - a palette index past the end of the palette (libpng draws it black)
//! - `PLTE` in a grayscale image, after `IDAT`, or after `tRNS`; a suggested
//!   `PLTE` in an RGB or RGBA image of a length no palette has
//! - `tRNS` in an image with alpha, after `IDAT`, a second time, before the
//!   palette, of a length a gray or RGB value does not have, or with more
//!   alphas than the palette has entries that the bit depth can index
//!
//! Like libpng, it accepts a palette longer than the bit depth can index,
//! whose extra entries no pixel can use (Go's image/png rejects it); a
//! palette `tRNS` with no alphas, as if there were none; and a gray or RGB
//! `tRNS` value past the bit depth, read in the bits the depth has: in an
//! 8-bit image, 0x0100 makes 0x00 transparent. libpng warns about the last
//! two. Unlike libpng,
//! it does not hold the image data to the window its zlib header declares
//! (see [`inflate`]), and decodes a stream that reaches further back as
//! written.

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

use nadir_core::hash::crc32;

use crate::inflate::{self, Fault};
use crate::{CodecError, Limits};

/// A decoded image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// The width in pixels, at least 1.
    pub width: u32,
    /// The height in pixels, at least 1.
    pub height: u32,
    /// Its pixels: RGBA8, 4 bytes a pixel, rows from the top, `width × height
    /// × 4` bytes in all. Alpha is straight, not premultiplied.
    pub pixels: Vec<u8>,
    /// The chunks that say how the colors are encoded, or say something
    /// about the image, which the decoder ignored: the names of any of
    /// `gAMA`, `cHRM`, `sRGB`, `iCCP`, `cICP`, `mDCV`, `cLLI`, `tEXt`, `zTXt`
    /// and `iTXt`, each once, in the order first met. The cook tool warns
    /// once for a file that has any.
    pub ignored: Vec<&'static str>,
}

/// What is wrong with a PNG file. See [`CodecError::Png`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The input does not start with the PNG signature.
    NotPng,
    /// The input ends too soon.
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// A chunk's type is not four ASCII letters.
    BadChunkType {
        /// The type.
        found: [u8; 4],
    },
    /// A chunk claims more than 2³¹ − 1 bytes.
    ChunkTooLong {
        /// The length it claims.
        length: u32,
    },
    /// A chunk's CRC does not match its type and data.
    BadCrc {
        /// The chunk's type.
        chunk: [u8; 4],
        /// The CRC in the file.
        stored: u32,
        /// The CRC of the chunk's type and data.
        computed: u32,
    },
    /// The first chunk is not `IHDR`.
    MissingHeader {
        /// The first chunk's type.
        found: [u8; 4],
    },
    /// A chunk comes where it may not, or a second time.
    Misplaced {
        /// The chunk's type.
        chunk: [u8; 4],
    },
    /// A chunk's data is a length it cannot be.
    BadLength {
        /// The chunk's type.
        chunk: [u8; 4],
        /// Its length.
        length: u32,
    },
    /// The width or the height is 0.
    EmptyImage,
    /// The width or the height is more than
    /// [`Limits::max_texture_dimension`], or than PNG's 2³¹ − 1.
    DimensionTooLarge {
        /// The limit that was exceeded.
        limit: u32,
    },
    /// The pixels would take more than [`Limits::max_decoded_bytes`].
    TooManyBytes {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// The color type is not 0, 2, 3, 4 or 6.
    BadColorType {
        /// The value in the header.
        found: u8,
    },
    /// The bit depth is not one the color type has.
    BadBitDepth {
        /// The color type.
        color_type: u8,
        /// The bit depth.
        found: u8,
    },
    /// The compression method is not 0.
    BadCompressionMethod {
        /// The value in the header.
        found: u8,
    },
    /// The filter method is not 0.
    BadFilterMethod {
        /// The value in the header.
        found: u8,
    },
    /// The interlace method is not 0 or 1.
    BadInterlaceMethod {
        /// The value in the header.
        found: u8,
    },
    /// A grayscale image has a palette.
    UnexpectedPalette,
    /// A palette image has no palette before its image data.
    MissingPalette,
    /// An image with an alpha channel has a `tRNS` chunk.
    UnexpectedTransparency,
    /// A critical chunk the decoder does not know.
    UnknownCriticalChunk {
        /// The chunk's type.
        chunk: [u8; 4],
    },
    /// A chunk of an animated PNG.
    Animated {
        /// The chunk's type.
        chunk: [u8; 4],
    },
    /// There is no `IDAT` chunk.
    MissingImageData,
    /// The `IDAT` chunks are not all together.
    SplitImageData,
    /// The image data is malformed.
    BadImageData {
        /// What is wrong with it.
        kind: inflate::ErrorKind,
    },
    /// The image data the pixels are inflated from would take more bytes
    /// than a `usize` can count. Only limits raised near `usize::MAX`, or a
    /// 32-bit host, let the pixels fit while it does not.
    ImageDataTooLarge,
    /// The image data decompresses to fewer bytes than the image needs.
    TooLittleImageData {
        /// How many bytes the image needs.
        needed: usize,
    },
    /// The image data decompresses to more bytes than the image needs.
    TooMuchImageData {
        /// How many bytes the image needs.
        needed: usize,
    },
    /// Bytes follow the image data's zlib stream.
    TrailingImageData,
    /// A row's filter type is not 0 to 4.
    BadFilter {
        /// The row, counted from 0 through the image data; with Adam7, the
        /// rows of every pass in turn.
        row: usize,
        /// The filter type.
        found: u8,
    },
    /// A pixel's palette index is past the end of the palette.
    BadPaletteIndex {
        /// The index.
        index: u8,
        /// How many entries the palette has.
        entries: usize,
    },
    /// Bytes follow the `IEND` chunk.
    TrailingBytes,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPng => f.write_str("the file does not start with the PNG signature"),
            Self::UnexpectedEnd { expected } => {
                write!(f, "the file ends where {expected} should be")
            }
            Self::BadChunkType { found } => write!(
                f,
                "a chunk's type, `{}`, is not four ASCII letters",
                found.escape_ascii()
            ),
            Self::ChunkTooLong { length } => {
                write!(f, "a chunk claims {length} bytes, more than 2^31 - 1")
            }
            Self::BadCrc {
                chunk,
                stored,
                computed,
            } => write!(
                f,
                "the {} chunk's CRC is {stored:#010x}, but its contents' is {computed:#010x}",
                name(chunk)
            ),
            Self::MissingHeader { found } => {
                write!(f, "the first chunk is {}, not IHDR", name(found))
            }
            Self::Misplaced { chunk } => {
                write!(f, "a {} chunk is out of place or repeated", name(chunk))
            }
            Self::BadLength { chunk, length } => {
                write!(f, "a {} chunk cannot be {length} bytes long", name(chunk))
            }
            Self::EmptyImage => f.write_str("the image is 0 pixels wide or tall"),
            Self::DimensionTooLarge { limit } => write!(
                f,
                "the image is wider or taller than the limit of {limit} pixels"
            ),
            Self::TooManyBytes { limit } => write!(
                f,
                "the pixels would take more than the limit of {limit} bytes"
            ),
            Self::BadColorType { found } => {
                write!(f, "the color type is {found}, not 0, 2, 3, 4 or 6")
            }
            Self::BadBitDepth { color_type, found } => {
                write!(f, "color type {color_type} has no bit depth of {found}")
            }
            Self::BadCompressionMethod { found } => {
                write!(f, "the compression method is {found}, not 0")
            }
            Self::BadFilterMethod { found } => write!(f, "the filter method is {found}, not 0"),
            Self::BadInterlaceMethod { found } => {
                write!(f, "the interlace method is {found}, not 0 or 1")
            }
            Self::UnexpectedPalette => f.write_str("a grayscale image has a palette"),
            Self::MissingPalette => {
                f.write_str("a palette image has no palette before its image data")
            }
            Self::UnexpectedTransparency => {
                f.write_str("an image with an alpha channel has a tRNS chunk")
            }
            Self::UnknownCriticalChunk { chunk } => {
                write!(f, "the critical chunk {} is not known", name(chunk))
            }
            Self::Animated { chunk } => write!(
                f,
                "the {} chunk belongs to an animated PNG, which is not supported",
                name(chunk)
            ),
            Self::MissingImageData => f.write_str("there is no IDAT chunk"),
            Self::SplitImageData => f.write_str("the IDAT chunks are not all together"),
            Self::BadImageData { kind } => write!(f, "the image data is malformed: {kind}"),
            Self::ImageDataTooLarge => {
                f.write_str("the image data would take more bytes than a usize can count")
            }
            Self::TooLittleImageData { needed } => write!(
                f,
                "the image data inflates to fewer than the {needed} bytes the image needs"
            ),
            Self::TooMuchImageData { needed } => write!(
                f,
                "the image data inflates to more than the {needed} bytes the image needs"
            ),
            Self::TrailingImageData => {
                f.write_str("there are bytes after the end of the image data's zlib stream")
            }
            Self::BadFilter { row, found } => {
                write!(f, "row {row} has filter type {found}, not 0 to 4")
            }
            Self::BadPaletteIndex { index, entries } => write!(
                f,
                "a pixel has palette index {index}, but the palette has {entries} entries"
            ),
            Self::TrailingBytes => f.write_str("there are bytes after the IEND chunk"),
        }
    }
}

/// A chunk type for an error message. The types in messages other than
/// [`ErrorKind::BadChunkType`]'s are ASCII letters.
fn name(chunk: &[u8; 4]) -> impl fmt::Display + '_ {
    chunk.escape_ascii()
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// The chunks whose meaning the decoder ignores but reports: those that say
/// how the colors are encoded, and the text chunks.
const REPORTED: [&str; 10] = [
    "gAMA", "cHRM", "sRGB", "iCCP", "cICP", "mDCV", "cLLI", "tEXt", "zTXt", "iTXt",
];

/// The chunks of an animated PNG.
const ANIMATION: [&[u8; 4]; 3] = [b"acTL", b"fcTL", b"fdAT"];

/// The most bytes DEFLATE can give for one byte of input: a length of 258
/// in two bits, a one-bit length code and a one-bit distance code.
const MOST_INFLATED_PER_BYTE: usize = 1032;

/// The largest width or height, and chunk length, that PNG allows.
const PNG_MAX: u32 = 0x7fff_ffff;

/// Where IHDR's fields are in the file, which always starts with it.
const IHDR_WIDTH: usize = 16;
const IHDR_HEIGHT: usize = 20;
const IHDR_BIT_DEPTH: usize = 24;
const IHDR_COLOR_TYPE: usize = 25;
const IHDR_COMPRESSION: usize = 26;
const IHDR_FILTER: usize = 27;
const IHDR_INTERLACE: usize = 28;

/// The Adam7 passes: where each starts, and how far apart its pixels are.
const ADAM7: [Pass; 7] = [
    Pass::new(0, 0, 8, 8),
    Pass::new(4, 0, 8, 8),
    Pass::new(0, 4, 4, 8),
    Pass::new(2, 0, 4, 4),
    Pass::new(0, 2, 2, 4),
    Pass::new(1, 0, 2, 2),
    Pass::new(0, 1, 1, 2),
];

/// An image without interlacing is one pass of every pixel.
const WHOLE: [Pass; 1] = [Pass::new(0, 0, 1, 1)];

fn error(offset: usize, kind: ErrorKind) -> CodecError {
    CodecError::Png { offset, kind }
}

/// What a pixel holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Color {
    Gray,
    Rgb,
    Palette,
    GrayAlpha,
    Rgba,
}

impl Color {
    const fn channels(self) -> u32 {
        match self {
            Self::Gray | Self::Palette => 1,
            Self::GrayAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }
}

/// What `IHDR` says.
#[derive(Clone, Copy, Debug)]
struct Header {
    width: u32,
    height: u32,
    color: Color,
    depth: u8,
    interlaced: bool,
}

impl Header {
    const fn bits_per_pixel(&self) -> u32 {
        self.color.channels() * self.depth as u32
    }

    fn passes(&self) -> &'static [Pass] {
        if self.interlaced { &ADAM7 } else { &WHOLE }
    }

    /// The bytes of one filtered row `width` pixels wide, its filter type
    /// included.
    fn row_len(&self, width: u32) -> u64 {
        1 + (u64::from(width) * u64::from(self.bits_per_pixel())).div_ceil(8)
    }

    /// The bytes the image data inflates to, unless that overflows.
    fn image_data_len(&self) -> Option<u64> {
        self.passes().iter().try_fold(0u64, |sum, pass| {
            let (width, height) = pass.size(self);
            if width == 0 || height == 0 {
                return Some(sum);
            }
            sum.checked_add(u64::from(height).checked_mul(self.row_len(width))?)
        })
    }
}

/// One pass of an interlaced image, or all of one that is not.
#[derive(Clone, Copy, Debug)]
struct Pass {
    x: u32,
    y: u32,
    dx: u32,
    dy: u32,
}

impl Pass {
    const fn new(x: u32, y: u32, dx: u32, dy: u32) -> Self {
        Self { x, y, dx, dy }
    }

    /// How many pixels wide and tall the pass is.
    fn size(&self, header: &Header) -> (u32, u32) {
        let span = |side: u32, start: u32, step: u32| {
            if side > start {
                (side - start).div_ceil(step)
            } else {
                0
            }
        };
        (
            span(header.width, self.x, self.dx),
            span(header.height, self.y, self.dy),
        )
    }
}

/// One chunk, as found in the file.
struct Chunk<'a> {
    kind: [u8; 4],
    data: &'a [u8],
    /// Where the chunk starts: its length field.
    at: usize,
    /// Where its data starts.
    data_at: usize,
    /// Where the next chunk starts.
    end: usize,
}

/// Reads the chunk at `at`, checking its length, its type and its CRC.
fn read_chunk(bytes: &[u8], at: usize) -> Result<Chunk<'_>, CodecError> {
    let cut_short = |expected| error(bytes.len(), ErrorKind::UnexpectedEnd { expected });
    let Some(&[l0, l1, l2, l3, t0, t1, t2, t3]) =
        bytes.get(at..).and_then(<[u8]>::first_chunk::<8>)
    else {
        return Err(cut_short("a chunk's length and type"));
    };
    let length = u32::from_be_bytes([l0, l1, l2, l3]);
    let kind = [t0, t1, t2, t3];
    if length > PNG_MAX {
        return Err(error(at, ErrorKind::ChunkTooLong { length }));
    }
    if !kind.iter().all(u8::is_ascii_alphabetic) {
        return Err(error(at + 4, ErrorKind::BadChunkType { found: kind }));
    }
    let data_at = at + 8;
    let Some(data) = bytes
        .get(data_at..)
        .and_then(|rest| rest.get(..length as usize))
    else {
        return Err(cut_short("a chunk's data"));
    };
    let crc_at = data_at + data.len();
    let Some(&stored) = bytes.get(crc_at..).and_then(<[u8]>::first_chunk::<4>) else {
        return Err(cut_short("a chunk's CRC"));
    };
    let stored = u32::from_be_bytes(stored);
    let computed = crc32(bytes.get(at + 4..crc_at).unwrap_or_default());
    if stored != computed {
        return Err(error(
            crc_at,
            ErrorKind::BadCrc {
                chunk: kind,
                stored,
                computed,
            },
        ));
    }
    Ok(Chunk {
        kind,
        data,
        at,
        data_at,
        end: crc_at + 4,
    })
}

/// A palette: an RGBA color for each index, alpha from `tRNS`.
struct Palette {
    colors: [[u8; 4]; 256],
    entries: usize,
}

/// The color that `tRNS` makes transparent in a grayscale or RGB image, at
/// the image's bit depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Gray(u16),
    Rgb([u16; 3]),
}

/// Decodes a PNG file.
///
/// # Errors
///
/// [`CodecError::Png`], with the byte offset of the problem, for a file
/// that is malformed, outside the subset the module describes, or breaks a
/// limit of `limits`. A problem inside the image data is placed at its
/// byte in the file; one found after inflating it (a filter type or a
/// palette index) at the first `IDAT` chunk.
pub fn decode(bytes: &[u8], limits: Limits) -> Result<Image, CodecError> {
    if !SIGNATURE.starts_with(bytes.get(..SIGNATURE.len()).unwrap_or(bytes)) {
        return Err(error(0, ErrorKind::NotPng));
    }
    if bytes.len() < SIGNATURE.len() {
        return Err(error(
            bytes.len(),
            ErrorKind::UnexpectedEnd {
                expected: "the rest of the signature",
            },
        ));
    }

    let first = read_chunk(bytes, SIGNATURE.len())?;
    if first.kind != *b"IHDR" {
        return Err(error(
            first.at,
            ErrorKind::MissingHeader { found: first.kind },
        ));
    }
    let header = read_header(&first, limits)?;

    let mut palette: Option<Palette> = None;
    let mut key: Option<Key> = None;
    let mut transparency_seen = false;
    let mut image_data = ImageData::default();
    let mut after_image_data = false;
    let mut ignored: Vec<&'static str> = Vec::new();
    let mut at = first.end;
    let end = loop {
        let chunk = read_chunk(bytes, at)?;
        at = chunk.end;
        let misplaced = || error(chunk.at, ErrorKind::Misplaced { chunk: chunk.kind });
        if image_data.first.is_some() && &chunk.kind != b"IDAT" {
            after_image_data = true;
        }
        match &chunk.kind {
            b"IHDR" => return Err(misplaced()),
            b"PLTE" => {
                if palette.is_some() || transparency_seen || image_data.first.is_some() {
                    return Err(misplaced());
                }
                palette = Some(read_palette(&chunk, header)?);
            }
            b"tRNS" => {
                if transparency_seen || image_data.first.is_some() {
                    return Err(misplaced());
                }
                transparency_seen = true;
                key = read_transparency(&chunk, header, palette.as_mut())?;
            }
            b"IDAT" => {
                if after_image_data {
                    return Err(error(chunk.at, ErrorKind::SplitImageData));
                }
                if header.color == Color::Palette && palette.is_none() {
                    return Err(error(chunk.at, ErrorKind::MissingPalette));
                }
                image_data.first.get_or_insert(chunk.at);
                image_data.end = chunk.data_at + chunk.data.len();
            }
            b"IEND" => {
                if !chunk.data.is_empty() {
                    return Err(error(
                        chunk.at,
                        ErrorKind::BadLength {
                            chunk: chunk.kind,
                            length: chunk.data.len() as u32,
                        },
                    ));
                }
                if image_data.first.is_none() {
                    return Err(error(chunk.at, ErrorKind::MissingImageData));
                }
                break chunk.end;
            }
            kind if ANIMATION.contains(&kind) => {
                return Err(error(chunk.at, ErrorKind::Animated { chunk: chunk.kind }));
            }
            kind if kind[0].is_ascii_uppercase() => {
                return Err(error(
                    chunk.at,
                    ErrorKind::UnknownCriticalChunk { chunk: chunk.kind },
                ));
            }
            kind => {
                if let Some(&name) = REPORTED.iter().find(|name| name.as_bytes() == kind)
                    && !ignored.contains(&name)
                {
                    ignored.push(name);
                }
            }
        }
    };
    if end < bytes.len() {
        return Err(error(end, ErrorKind::TrailingBytes));
    }

    let raw = inflate_image_data(bytes, &image_data, header)?;
    let palette = palette.unwrap_or(Palette {
        colors: [[0, 0, 0, 255]; 256],
        entries: 0,
    });
    // Where problems found after inflating are placed.
    let data_at = image_data.first.unwrap_or(0);
    let pixels =
        unfilter_and_place(raw, header, &palette, key).map_err(|kind| error(data_at, kind))?;
    Ok(Image {
        width: header.width,
        height: header.height,
        pixels,
        ignored,
    })
}

/// Where the `IDAT` chunks are.
#[derive(Default)]
struct ImageData {
    /// Where the first one starts. The rest follow it: `decode` checked
    /// them all, and that nothing comes between them.
    first: Option<usize>,
    /// Where the last one's data ends.
    end: usize,
}

impl ImageData {
    /// The data of each of the chunks, in order, read from their headers
    /// again rather than kept, so that nothing grows with their number.
    fn pieces<'a>(&self, bytes: &'a [u8]) -> impl Iterator<Item = Range<usize>> + 'a {
        let mut at = self.first;
        std::iter::from_fn(move || {
            let start = at?;
            let &[l0, l1, l2, l3, t0, t1, t2, t3] = bytes.get(start..)?.first_chunk::<8>()?;
            if [t0, t1, t2, t3] != *b"IDAT" {
                return None;
            }
            let data = start + 8..start + 8 + u32::from_be_bytes([l0, l1, l2, l3]) as usize;
            at = Some(data.end + 4);
            Some(data)
        })
    }
}

/// Reads and checks `IHDR`, and checks the image against `limits`.
fn read_header(chunk: &Chunk<'_>, limits: Limits) -> Result<Header, CodecError> {
    let Ok(
        &[
            w0,
            w1,
            w2,
            w3,
            h0,
            h1,
            h2,
            h3,
            depth,
            color_type,
            compression,
            filter,
            interlace,
        ],
    ) = <&[u8; 13]>::try_from(chunk.data)
    else {
        return Err(error(
            chunk.at,
            ErrorKind::BadLength {
                chunk: chunk.kind,
                length: chunk.data.len() as u32,
            },
        ));
    };
    let most = limits.max_texture_dimension.min(PNG_MAX);
    let check_side = |side: u32, offset: usize| match side {
        0 => Err(error(offset, ErrorKind::EmptyImage)),
        side if side > most => Err(error(offset, ErrorKind::DimensionTooLarge { limit: most })),
        side => Ok(side),
    };
    let width = check_side(u32::from_be_bytes([w0, w1, w2, w3]), IHDR_WIDTH)?;
    let height = check_side(u32::from_be_bytes([h0, h1, h2, h3]), IHDR_HEIGHT)?;
    let (color, depths): (Color, &[u8]) = match color_type {
        0 => (Color::Gray, &[1, 2, 4, 8, 16]),
        2 => (Color::Rgb, &[8, 16]),
        3 => (Color::Palette, &[1, 2, 4, 8]),
        4 => (Color::GrayAlpha, &[8, 16]),
        6 => (Color::Rgba, &[8, 16]),
        found => return Err(error(IHDR_COLOR_TYPE, ErrorKind::BadColorType { found })),
    };
    if !depths.contains(&depth) {
        return Err(error(
            IHDR_BIT_DEPTH,
            ErrorKind::BadBitDepth {
                color_type,
                found: depth,
            },
        ));
    }
    if compression != 0 {
        return Err(error(
            IHDR_COMPRESSION,
            ErrorKind::BadCompressionMethod { found: compression },
        ));
    }
    if filter != 0 {
        return Err(error(
            IHDR_FILTER,
            ErrorKind::BadFilterMethod { found: filter },
        ));
    }
    let interlaced = match interlace {
        0 => false,
        1 => true,
        found => {
            return Err(error(
                IHDR_INTERLACE,
                ErrorKind::BadInterlaceMethod { found },
            ));
        }
    };
    let fits = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .is_some_and(|len| u64::try_from(limits.max_decoded_bytes).is_ok_and(|limit| len <= limit));
    if !fits {
        return Err(error(
            IHDR_WIDTH,
            ErrorKind::TooManyBytes {
                limit: limits.max_decoded_bytes,
            },
        ));
    }
    Ok(Header {
        width,
        height,
        color,
        depth,
        interlaced,
    })
}

/// Reads `PLTE`. A color image may have one, as a suggestion it ignores.
fn read_palette(chunk: &Chunk<'_>, header: Header) -> Result<Palette, CodecError> {
    if matches!(header.color, Color::Gray | Color::GrayAlpha) {
        return Err(error(chunk.at, ErrorKind::UnexpectedPalette));
    }
    let (colors, rest) = chunk.data.as_chunks::<3>();
    if colors.is_empty() || colors.len() > 256 || !rest.is_empty() {
        return Err(error(
            chunk.at,
            ErrorKind::BadLength {
                chunk: chunk.kind,
                length: chunk.data.len() as u32,
            },
        ));
    }
    let mut palette = Palette {
        colors: [[0, 0, 0, 255]; 256],
        entries: colors.len(),
    };
    for (entry, &[r, g, b]) in palette.colors.iter_mut().zip(colors) {
        *entry = [r, g, b, 255];
    }
    Ok(palette)
}

/// Reads `tRNS`: alphas for a palette's first entries, or the one color of
/// a grayscale or RGB image that is transparent.
fn read_transparency(
    chunk: &Chunk<'_>,
    header: Header,
    palette: Option<&mut Palette>,
) -> Result<Option<Key>, CodecError> {
    let bad_length = || {
        error(
            chunk.at,
            ErrorKind::BadLength {
                chunk: chunk.kind,
                length: chunk.data.len() as u32,
            },
        )
    };
    // A gray or RGB value is read in the bits the bit depth has, as libpng
    // reads it: in an 8-bit image, 0x0100 is 0x00.
    let sample = |hi, lo| u16::from_be_bytes([hi, lo]) & (u16::MAX >> (16 - header.depth));
    match (header.color, chunk.data) {
        (Color::Gray, &[hi, lo]) => Ok(Some(Key::Gray(sample(hi, lo)))),
        (Color::Rgb, &[r0, r1, g0, g1, b0, b1]) => Ok(Some(Key::Rgb([
            sample(r0, r1),
            sample(g0, g1),
            sample(b0, b1),
        ]))),
        (Color::Palette, alphas) => {
            // tRNS must follow PLTE.
            let Some(palette) = palette else {
                return Err(error(chunk.at, ErrorKind::Misplaced { chunk: chunk.kind }));
            };
            // Only the entries the bit depth can index can be used. No
            // alphas at all is as if there were no tRNS, as libpng and Go
            // read it.
            let usable = palette.entries.min(1 << header.depth);
            if alphas.len() > usable {
                return Err(bad_length());
            }
            for (entry, &alpha) in palette.colors.iter_mut().zip(alphas) {
                entry[3] = alpha;
            }
            Ok(None)
        }
        (Color::Gray | Color::Rgb, _) => Err(bad_length()),
        (Color::GrayAlpha | Color::Rgba, _) => {
            Err(error(chunk.at, ErrorKind::UnexpectedTransparency))
        }
    }
}

/// Inflates the image data of the `IDAT` chunks, which must give exactly
/// the bytes the image needs.
fn inflate_image_data(
    bytes: &[u8],
    image_data: &ImageData,
    header: Header,
) -> Result<Vec<u8>, CodecError> {
    let first_at = image_data.first.unwrap_or(0);
    // The pixels fit `max_decoded_bytes`, and the image data is at most
    // 2.25 times their size (16-bit RGBA one pixel wide: 9 bytes a row for
    // 4), so this fails only when the limit is raised past `usize::MAX`
    // divided by that, or on a 32-bit host.
    let needed = header
        .image_data_len()
        .and_then(|len| usize::try_from(len).ok())
        .ok_or_else(|| error(IHDR_WIDTH, ErrorKind::ImageDataTooLarge))?;
    let too_little = || error(first_at, ErrorKind::TooLittleImageData { needed });
    let total: usize = image_data.pieces(bytes).map(|range| range.len()).sum();
    // A file far too short for its image is cut short, however it goes on:
    // checking that first keeps a small file from claiming a large
    // allocation.
    if needed > total.saturating_mul(MOST_INFLATED_PER_BYTE) {
        return Err(too_little());
    }
    let piece = |range: Range<usize>| bytes.get(range).unwrap_or_default();
    let mut with_data = image_data.pieces(bytes).filter(|range| !range.is_empty());
    let data: Cow<'_, [u8]> = match (with_data.next(), with_data.next()) {
        (None, _) => Cow::Borrowed(&[]),
        (Some(only), None) => Cow::Borrowed(piece(only)),
        _ => {
            let mut data = Vec::with_capacity(total);
            for range in image_data.pieces(bytes) {
                data.extend_from_slice(piece(range));
            }
            Cow::Owned(data)
        }
    };
    // Where a byte of `data` is in the file.
    let in_file = |offset: usize| {
        let mut start = 0;
        for range in image_data.pieces(bytes) {
            if offset < start + range.len() {
                return range.start + (offset - start);
            }
            start += range.len();
        }
        image_data.end
    };
    let mut raw = Vec::with_capacity(needed);
    match inflate::zlib_into(&data, &mut raw, needed) {
        Ok(end) if end < data.len() => Err(error(in_file(end), ErrorKind::TrailingImageData)),
        Ok(_) if raw.len() < needed => Err(too_little()),
        Ok(_) => Ok(raw),
        Err(Fault {
            offset,
            kind: inflate::ErrorKind::TooMuchOutput { .. },
        }) => Err(error(
            in_file(offset),
            ErrorKind::TooMuchImageData { needed },
        )),
        Err(Fault { offset, kind }) => {
            Err(error(in_file(offset), ErrorKind::BadImageData { kind }))
        }
    }
}

/// Undoes the filters of `raw`, the inflated image data, and puts its
/// pixels in place as RGBA8.
fn unfilter_and_place(
    mut raw: Vec<u8>,
    header: Header,
    palette: &Palette,
    key: Option<Key>,
) -> Result<Vec<u8>, ErrorKind> {
    // `read_header` checked that this fits `max_decoded_bytes`.
    let mut pixels = vec![0; header.width as usize * header.height as usize * 4];
    let bytes_per_pixel = (header.bits_per_pixel() as usize).div_ceil(8);
    let needed = raw.len();
    let mut rest = raw.as_mut_slice();
    let mut row = 0;
    for pass in header.passes() {
        let (width, height) = pass.size(&header);
        if width == 0 || height == 0 {
            continue;
        }
        let row_len = header.row_len(width) as usize;
        let Some((rows, after)) = rest.split_at_mut_checked(row_len * height as usize) else {
            // `inflate_image_data` gave exactly the bytes the passes need.
            return Err(ErrorKind::TooLittleImageData { needed });
        };
        rest = after;
        unfilter(rows, row_len, bytes_per_pixel, &mut row)?;
        let targets = pixels
            .chunks_exact_mut(header.width as usize * 4)
            .skip(pass.y as usize)
            .step_by(pass.dy as usize);
        for (line, target) in rows.chunks_exact(row_len).zip(targets) {
            let line = line.get(1..).unwrap_or_default();
            let targets = target
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .skip(pass.x as usize)
                .step_by(pass.dx as usize)
                .take(width as usize);
            place(line, targets, header, palette, key)?;
        }
    }
    Ok(pixels)
}

/// Undoes the filter of each row of one pass, in place. `row` counts the
/// rows of every pass, for errors.
fn unfilter(
    rows: &mut [u8],
    row_len: usize,
    bytes_per_pixel: usize,
    row: &mut usize,
) -> Result<(), ErrorKind> {
    let mut previous: Option<&[u8]> = None;
    let mut rest = rows;
    while let Some((filtered, after)) = rest.split_at_mut_checked(row_len) {
        rest = after;
        let Some((&mut filter, line)) = filtered.split_first_mut() else {
            break;
        };
        match (filter, previous) {
            (0, _) | (2, None) => {}
            (1, _) => {
                for i in bytes_per_pixel..line.len() {
                    line[i] = line[i].wrapping_add(line[i - bytes_per_pixel]);
                }
            }
            (2, Some(above)) => {
                for (byte, &up) in line.iter_mut().zip(above) {
                    *byte = byte.wrapping_add(up);
                }
            }
            (3, _) => {
                for i in 0..line.len() {
                    let left = i.checked_sub(bytes_per_pixel).map_or(0, |j| line[j]);
                    let up = previous
                        .and_then(|above| above.get(i))
                        .copied()
                        .unwrap_or(0);
                    let average = ((u16::from(left) + u16::from(up)) / 2) as u8;
                    line[i] = line[i].wrapping_add(average);
                }
            }
            (4, _) => {
                for i in 0..line.len() {
                    let left = i.checked_sub(bytes_per_pixel).map_or(0, |j| line[j]);
                    let up = previous
                        .and_then(|above| above.get(i))
                        .copied()
                        .unwrap_or(0);
                    let up_left = i
                        .checked_sub(bytes_per_pixel)
                        .and_then(|j| previous.and_then(|above| above.get(j)))
                        .copied()
                        .unwrap_or(0);
                    line[i] = line[i].wrapping_add(paeth(left, up, up_left));
                }
            }
            (found, _) => return Err(ErrorKind::BadFilter { row: *row, found }),
        }
        previous = Some(line);
        *row += 1;
    }
    Ok(())
}

/// The Paeth predictor: whichever of left, up and up-left is nearest to
/// `left + up − up_left`, preferring them in that order.
fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let (a, b, c) = (i16::from(left), i16::from(up), i16::from(up_left));
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        left
    } else if pb <= pc {
        up
    } else {
        up_left
    }
}

/// Turns one unfiltered row of a pass into RGBA8 pixels in `targets`.
fn place<'a>(
    line: &[u8],
    targets: impl Iterator<Item = &'a mut [u8; 4]>,
    header: Header,
    palette: &Palette,
    key: Option<Key>,
) -> Result<(), ErrorKind> {
    let opaque = |matches: bool| if matches { 0 } else { 255 };
    match (header.color, header.depth) {
        (Color::Gray, depth @ (1 | 2 | 4)) => {
            let scale = 255 / ((1u8 << depth) - 1);
            for (target, v) in targets.zip(small_samples(line, depth)) {
                let g = v * scale;
                *target = [g, g, g, opaque(key == Some(Key::Gray(v.into())))];
            }
        }
        (Color::Gray, 8) => {
            for (target, &v) in targets.zip(line) {
                *target = [v, v, v, opaque(key == Some(Key::Gray(v.into())))];
            }
        }
        (Color::Gray, _) => {
            for (target, &v) in targets.zip(line.as_chunks::<2>().0) {
                let v = u16::from_be_bytes(v);
                let g = to_8(v);
                *target = [g, g, g, opaque(key == Some(Key::Gray(v)))];
            }
        }
        (Color::Rgb, 8) => {
            for (target, &[r, g, b]) in targets.zip(line.as_chunks::<3>().0) {
                let matches = key == Some(Key::Rgb([r.into(), g.into(), b.into()]));
                *target = [r, g, b, opaque(matches)];
            }
        }
        (Color::Rgb, _) => {
            for (target, &[r0, r1, g0, g1, b0, b1]) in targets.zip(line.as_chunks::<6>().0) {
                let rgb = [
                    u16::from_be_bytes([r0, r1]),
                    u16::from_be_bytes([g0, g1]),
                    u16::from_be_bytes([b0, b1]),
                ];
                let [r, g, b] = rgb.map(to_8);
                *target = [r, g, b, opaque(key == Some(Key::Rgb(rgb)))];
            }
        }
        (Color::Palette, 8) => look_up(targets, line.iter().copied(), palette)?,
        (Color::Palette, depth) => look_up(targets, small_samples(line, depth), palette)?,
        (Color::GrayAlpha, 8) => {
            for (target, &[g, a]) in targets.zip(line.as_chunks::<2>().0) {
                *target = [g, g, g, a];
            }
        }
        (Color::GrayAlpha, _) => {
            for (target, &[g0, g1, a0, a1]) in targets.zip(line.as_chunks::<4>().0) {
                let g = to_8(u16::from_be_bytes([g0, g1]));
                *target = [g, g, g, to_8(u16::from_be_bytes([a0, a1]))];
            }
        }
        (Color::Rgba, 8) => {
            for (target, &rgba) in targets.zip(line.as_chunks::<4>().0) {
                *target = rgba;
            }
        }
        (Color::Rgba, _) => {
            for (target, samples) in targets.zip(line.as_chunks::<8>().0) {
                let [r0, r1, g0, g1, b0, b1, a0, a1] = *samples;
                *target = [
                    to_8(u16::from_be_bytes([r0, r1])),
                    to_8(u16::from_be_bytes([g0, g1])),
                    to_8(u16::from_be_bytes([b0, b1])),
                    to_8(u16::from_be_bytes([a0, a1])),
                ];
            }
        }
    }
    Ok(())
}

/// Puts the palette's colors for `indices` in `targets`.
fn look_up<'a>(
    targets: impl Iterator<Item = &'a mut [u8; 4]>,
    indices: impl Iterator<Item = u8>,
    palette: &Palette,
) -> Result<(), ErrorKind> {
    for (target, index) in targets.zip(indices) {
        if usize::from(index) >= palette.entries {
            return Err(ErrorKind::BadPaletteIndex {
                index,
                entries: palette.entries,
            });
        }
        *target = palette.colors[usize::from(index)];
    }
    Ok(())
}

/// The samples of a row of 1, 2 or 4 bits each, leftmost first. The row's
/// last byte may hold padding past its last pixel, which the caller does
/// not take.
fn small_samples(line: &[u8], depth: u8) -> impl Iterator<Item = u8> + '_ {
    let mask = (1u8 << depth) - 1;
    line.iter()
        .flat_map(move |&byte| (1..=8 / depth).map(move |k| (byte >> (8 - depth * k)) & mask))
}

/// The nearest 8-bit value to a 16-bit one: `round(v × 255 / 65535)`, which
/// is `round(v / 257)`. No value is half way.
fn to_8(v: u16) -> u8 {
    ((u32::from(v) + 128) / 257) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adler32;

    /// A chunk with its length and CRC.
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[4..]);
        out.extend_from_slice(&crc.to_be_bytes());
        out
    }

    fn ihdr(width: u32, height: u32, depth: u8, color_type: u8, interlace: u8) -> Vec<u8> {
        let mut data = width.to_be_bytes().to_vec();
        data.extend_from_slice(&height.to_be_bytes());
        data.extend_from_slice(&[depth, color_type, 0, 0, interlace]);
        chunk(b"IHDR", &data)
    }

    /// A zlib stream of one stored block.
    fn zlib_stored(data: &[u8]) -> Vec<u8> {
        let len = data.len() as u16;
        let mut out = vec![0x78, 0x01, 0x01];
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(&adler32::checksum(data).to_be_bytes());
        out
    }

    /// A file of these chunks.
    fn file(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut out = SIGNATURE.to_vec();
        for chunk in chunks {
            out.extend_from_slice(chunk);
        }
        out
    }

    /// A 2×1 RGB image, unfiltered: red, then green.
    fn rgb_2x1() -> Vec<Vec<u8>> {
        vec![
            ihdr(2, 1, 8, 2, 0),
            chunk(b"IDAT", &zlib_stored(&[0, 255, 0, 0, 0, 255, 0])),
            chunk(b"IEND", &[]),
        ]
    }

    fn ok(bytes: &[u8]) -> Image {
        decode(bytes, Limits::default()).unwrap()
    }

    fn err(bytes: &[u8]) -> (usize, ErrorKind) {
        err_with(bytes, Limits::default())
    }

    fn err_with(bytes: &[u8], limits: Limits) -> (usize, ErrorKind) {
        match decode(bytes, limits) {
            Err(CodecError::Png { offset, kind }) => (offset, kind),
            other => panic!("not a PNG error: {other:?}"),
        }
    }

    /// Where the chunk at `index` starts in `file(chunks)`.
    fn at(chunks: &[Vec<u8>], index: usize) -> usize {
        8 + chunks[..index].iter().map(Vec::len).sum::<usize>()
    }

    #[test]
    fn a_small_image_decodes() {
        let image = ok(&file(&rgb_2x1()));
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.pixels, [255, 0, 0, 255, 0, 255, 0, 255]);
        assert!(image.ignored.is_empty());
    }

    #[test]
    fn the_signature_is_checked() {
        assert_eq!(err(b"GIF89a"), (0, ErrorKind::NotPng));
        assert_eq!(
            err(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, 0]),
            (0, ErrorKind::NotPng)
        );
        // A prefix of the signature is cut short.
        assert_eq!(
            err(&SIGNATURE[..5]),
            (
                5,
                ErrorKind::UnexpectedEnd {
                    expected: "the rest of the signature"
                }
            )
        );
        assert_eq!(
            err(&SIGNATURE),
            (
                8,
                ErrorKind::UnexpectedEnd {
                    expected: "a chunk's length and type"
                }
            )
        );
    }

    #[test]
    fn chunks_are_checked() {
        let good = file(&rgb_2x1());
        // A CRC off by one bit.
        let mut bad = good.clone();
        bad[29] ^= 1;
        let (offset, kind) = err(&bad);
        assert_eq!(offset, 29);
        assert!(
            matches!(kind, ErrorKind::BadCrc { chunk, stored, computed } if chunk == *b"IHDR" && stored ^ computed == 1 << 24),
            "{kind:?}"
        );
        // An ancillary chunk's CRC is checked too, though libpng only
        // warns.
        let mut chunks = rgb_2x1();
        let mut text = chunk(b"tEXt", b"a\0b");
        let last = text.len() - 1;
        text[last] ^= 1;
        chunks.insert(1, text);
        let (offset, kind) = err(&file(&chunks));
        assert_eq!(offset, 33 + 8 + 3);
        assert!(
            matches!(kind, ErrorKind::BadCrc { chunk, .. } if chunk == *b"tEXt"),
            "{kind:?}"
        );
        // A length past 2^31 - 1.
        let mut bad = good.clone();
        bad[33..37].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        assert_eq!(
            err(&bad),
            (
                33,
                ErrorKind::ChunkTooLong {
                    length: 0x8000_0000
                }
            )
        );
        // A length past the end of the file.
        let mut bad = good.clone();
        bad[33..37].copy_from_slice(&1000u32.to_be_bytes());
        assert_eq!(
            err(&bad),
            (
                good.len(),
                ErrorKind::UnexpectedEnd {
                    expected: "a chunk's data"
                }
            )
        );
        // A type that is not letters.
        let chunks = [ihdr(2, 1, 8, 2, 0), chunk(b"ID4T", &[])];
        assert_eq!(
            err(&file(&chunks)),
            (37, ErrorKind::BadChunkType { found: *b"ID4T" })
        );
        // Cut short in the CRC.
        assert_eq!(
            err(&good[..good.len() - 1]),
            (
                good.len() - 1,
                ErrorKind::UnexpectedEnd {
                    expected: "a chunk's CRC"
                }
            )
        );
        // No IEND at all.
        let chunks = &rgb_2x1()[..2];
        let bytes = file(chunks);
        assert_eq!(
            err(&bytes),
            (
                bytes.len(),
                ErrorKind::UnexpectedEnd {
                    expected: "a chunk's length and type"
                }
            )
        );
    }

    #[test]
    fn the_header_comes_first_and_is_checked() {
        let mut chunks = rgb_2x1();
        chunks.insert(0, chunk(b"tEXt", b"a\0b"));
        assert_eq!(
            err(&file(&chunks)),
            (8, ErrorKind::MissingHeader { found: *b"tEXt" })
        );
        let header = |width, height, depth, color_type, interlace| {
            let mut chunks = rgb_2x1();
            chunks[0] = ihdr(width, height, depth, color_type, interlace);
            err(&file(&chunks))
        };
        assert_eq!(header(0, 1, 8, 2, 0), (16, ErrorKind::EmptyImage));
        assert_eq!(header(2, 0, 8, 2, 0), (20, ErrorKind::EmptyImage));
        assert_eq!(
            header(8193, 1, 8, 2, 0),
            (16, ErrorKind::DimensionTooLarge { limit: 8192 })
        );
        assert_eq!(
            header(2, 1, 8, 1, 0),
            (25, ErrorKind::BadColorType { found: 1 })
        );
        assert_eq!(
            header(2, 1, 8, 7, 0),
            (25, ErrorKind::BadColorType { found: 7 })
        );
        for (color_type, depth) in [(0, 3), (2, 4), (3, 16), (4, 4), (6, 1), (0, 32)] {
            assert_eq!(
                header(2, 1, depth, color_type, 0),
                (
                    24,
                    ErrorKind::BadBitDepth {
                        color_type,
                        found: depth
                    }
                ),
                "{color_type} {depth}"
            );
        }
        assert_eq!(
            header(2, 1, 8, 2, 2),
            (28, ErrorKind::BadInterlaceMethod { found: 2 })
        );
        let mut chunks = rgb_2x1();
        chunks[0] = chunk(b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 1, 8, 2, 1, 0, 0]);
        assert_eq!(
            err(&file(&chunks)),
            (26, ErrorKind::BadCompressionMethod { found: 1 })
        );
        chunks[0] = chunk(b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 1, 8, 2, 0, 1, 0]);
        assert_eq!(
            err(&file(&chunks)),
            (27, ErrorKind::BadFilterMethod { found: 1 })
        );
        chunks[0] = chunk(b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 1, 8, 2, 0, 0]);
        assert_eq!(
            err(&file(&chunks)),
            (
                8,
                ErrorKind::BadLength {
                    chunk: *b"IHDR",
                    length: 12
                }
            )
        );
        // A second header.
        let mut chunks = rgb_2x1();
        chunks.insert(1, ihdr(2, 1, 8, 2, 0));
        assert_eq!(
            err(&file(&chunks)),
            (at(&chunks, 1), ErrorKind::Misplaced { chunk: *b"IHDR" })
        );
    }

    #[test]
    fn images_beyond_the_limits_are_rejected() {
        let mut chunks = rgb_2x1();
        let limits = Limits {
            max_texture_dimension: 1,
            ..Limits::default()
        };
        assert_eq!(
            err_with(&file(&chunks), limits),
            (16, ErrorKind::DimensionTooLarge { limit: 1 })
        );
        let limits = Limits {
            max_decoded_bytes: 7,
            ..Limits::default()
        };
        assert_eq!(
            err_with(&file(&chunks), limits),
            (16, ErrorKind::TooManyBytes { limit: 7 })
        );
        // PNG's own limit holds whatever the limits say.
        chunks[0] = ihdr(0x8000_0000, 1, 8, 2, 0);
        let limits = Limits {
            max_texture_dimension: u32::MAX,
            ..Limits::default()
        };
        assert_eq!(
            err_with(&file(&chunks), limits),
            (16, ErrorKind::DimensionTooLarge { limit: 0x7fff_ffff })
        );
    }

    /// A zlib stream of `len` zero bytes, nearly as small as DEFLATE can
    /// make it: one dynamic block whose length 258 and distance 1 take a
    /// bit each, so that 258 bytes take 2 bits.
    fn zlib_zeros(len: usize) -> Vec<u8> {
        let mut bits = Vec::new();
        let mut put = |value: u32, count: u32| {
            for i in 0..count {
                bits.push((value >> i) & 1);
            }
        };
        // Huffman codes go most significant bit first.
        let code = |put: &mut dyn FnMut(u32, u32), code: u32, length: u32| {
            for i in (0..length).rev() {
                put((code >> i) & 1, 1);
            }
        };
        // Final, dynamic; 286 literal/length codes, 1 distance code, 18
        // code-length code lengths.
        put(1, 1);
        put(2, 2);
        put(29, 5);
        put(0, 5);
        put(14, 4);
        // In the order 16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13,
        // 2, 14, 1: symbol 18 in 1 bit ("0"), 1 and 2 in 2 ("10", "11").
        for length in [0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2] {
            put(length, 3);
        }
        // Literal 0: 2 bits. 1–255: none (138 + 117). 256: 2 bits.
        // 257–284: none (28). 285, length 258: 1 bit. Distance 1: 1 bit.
        code(&mut put, 0b11, 2);
        code(&mut put, 0, 1);
        put(127, 7);
        code(&mut put, 0, 1);
        put(106, 7);
        code(&mut put, 0b11, 2);
        code(&mut put, 0, 1);
        put(17, 7);
        code(&mut put, 0b10, 2);
        code(&mut put, 0b10, 2);
        // The codes: 285 "0", literal 0 "10", 256 "11"; distance 1 "0".
        let matches = (len - 1) / 258;
        for _ in 0..len - 258 * matches {
            code(&mut put, 0b10, 2);
        }
        for _ in 0..matches {
            code(&mut put, 0, 1);
            code(&mut put, 0, 1);
        }
        code(&mut put, 0b11, 2);
        let mut out = vec![0x78, 0xda];
        out.extend(bits.chunks(8).map(|byte| {
            byte.iter()
                .enumerate()
                .fold(0u8, |acc, (i, &bit)| acc | (bit as u8) << i)
        }));
        out.extend_from_slice(&adler32::checksum(&vec![0; len]).to_be_bytes());
        out
    }

    #[test]
    fn image_data_compressed_nearly_as_far_as_deflate_goes_decodes() {
        // 258 × 1291 gray: 1291 rows of 259 bytes is 1 + 258 × 1296, so the
        // image data is one literal and 1296 matches of 258.
        let data = zlib_zeros(259 * 1291);
        let ratio = (259 * 1291) / data.len();
        assert!(ratio > 900, "{ratio}");
        let chunks = [
            ihdr(258, 1291, 8, 0, 0),
            chunk(b"IDAT", &data),
            chunk(b"IEND", &[]),
        ];
        let image = ok(&file(&chunks));
        assert!(image.pixels.chunks(4).all(|px| px == [0, 0, 0, 255]));
    }

    #[test]
    fn a_huge_image_with_little_data_is_rejected_before_allocating() {
        // 8192 × 8192 RGBA needs 268 MB of image data, which 27 bytes
        // cannot inflate to.
        let chunks = [
            ihdr(8192, 8192, 8, 6, 0),
            chunk(b"IDAT", &zlib_stored(&[0; 16])),
            chunk(b"IEND", &[]),
        ];
        assert_eq!(
            err(&file(&chunks)),
            (
                at(&chunks, 1),
                ErrorKind::TooLittleImageData {
                    needed: 8192 * (1 + 8192 * 4)
                }
            )
        );
    }

    #[test]
    fn palettes_are_checked() {
        let palette_image = |chunks_between: Vec<Vec<u8>>, data: &[u8]| {
            let mut chunks = vec![ihdr(2, 1, 8, 3, 0)];
            chunks.extend(chunks_between);
            chunks.push(chunk(b"IDAT", &zlib_stored(data)));
            chunks.push(chunk(b"IEND", &[]));
            chunks
        };
        let plte = chunk(b"PLTE", &[1, 2, 3, 4, 5, 6]);
        assert_eq!(
            ok(&file(&palette_image(vec![plte.clone()], &[0, 1, 0]))).pixels,
            [4, 5, 6, 255, 1, 2, 3, 255]
        );
        // No palette.
        let chunks = palette_image(vec![], &[0, 1, 0]);
        assert_eq!(
            err(&file(&chunks)),
            (at(&chunks, 1), ErrorKind::MissingPalette)
        );
        // An index past the palette.
        let chunks = palette_image(vec![plte.clone()], &[0, 1, 2]);
        assert_eq!(
            err(&file(&chunks)),
            (
                at(&chunks, 2),
                ErrorKind::BadPaletteIndex {
                    index: 2,
                    entries: 2
                }
            )
        );
        // Lengths: empty, not a multiple of 3, more than 256 entries.
        for length in [0, 5, 257 * 3] {
            let chunks = palette_image(vec![chunk(b"PLTE", &vec![0; length])], &[0, 0, 0]);
            assert_eq!(
                err(&file(&chunks)),
                (
                    at(&chunks, 1),
                    ErrorKind::BadLength {
                        chunk: *b"PLTE",
                        length: length as u32
                    }
                ),
                "{length}"
            );
        }
        // Twice.
        let chunks = palette_image(vec![plte.clone(), plte.clone()], &[0, 0, 0]);
        assert_eq!(
            err(&file(&chunks)),
            (at(&chunks, 2), ErrorKind::Misplaced { chunk: *b"PLTE" })
        );
        // In a grayscale image.
        let chunks = [ihdr(1, 1, 8, 0, 0), plte.clone()];
        assert_eq!(err(&file(&chunks)), (33, ErrorKind::UnexpectedPalette));
        // In an RGB image: a suggestion, ignored.
        let mut chunks = rgb_2x1();
        chunks.insert(1, plte);
        assert_eq!(ok(&file(&chunks)).pixels, [255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn transparency_is_checked() {
        let plte = chunk(b"PLTE", &[1, 2, 3, 4, 5, 6]);
        let idat = chunk(b"IDAT", &zlib_stored(&[0, 1, 0]));
        let iend = chunk(b"IEND", &[]);
        let palette = |between: &[Vec<u8>]| {
            let mut chunks = vec![ihdr(2, 1, 8, 3, 0)];
            chunks.extend_from_slice(between);
            chunks.extend([idat.clone(), iend.clone()]);
            chunks
        };
        // Alphas for the first entry only.
        let chunks = palette(&[plte.clone(), chunk(b"tRNS", &[7])]);
        assert_eq!(ok(&file(&chunks)).pixels, [4, 5, 6, 255, 1, 2, 3, 7]);
        // More alphas than entries.
        let chunks = palette(&[plte.clone(), chunk(b"tRNS", &[1, 2, 3])]);
        assert_eq!(
            err(&file(&chunks)),
            (
                at(&chunks, 2),
                ErrorKind::BadLength {
                    chunk: *b"tRNS",
                    length: 3
                }
            )
        );
        // None: as if there were no tRNS, as libpng and Go read it.
        let chunks = palette(&[plte.clone(), chunk(b"tRNS", &[])]);
        assert_eq!(ok(&file(&chunks)).pixels, [4, 5, 6, 255, 1, 2, 3, 255]);
        // Before the palette.
        let chunks = palette(&[chunk(b"tRNS", &[7]), plte.clone()]);
        assert_eq!(
            err(&file(&chunks)),
            (at(&chunks, 1), ErrorKind::Misplaced { chunk: *b"tRNS" })
        );
        // Twice.
        let chunks = palette(&[plte.clone(), chunk(b"tRNS", &[7]), chunk(b"tRNS", &[7])]);
        assert_eq!(
            err(&file(&chunks)),
            (at(&chunks, 3), ErrorKind::Misplaced { chunk: *b"tRNS" })
        );
        // After the image data.
        let mut chunks = rgb_2x1();
        chunks.insert(2, chunk(b"tRNS", &[0, 0, 0, 255, 0, 0]));
        assert_eq!(
            err(&file(&chunks)),
            (at(&chunks, 2), ErrorKind::Misplaced { chunk: *b"tRNS" })
        );
        // RGB: the red pixel is transparent. A wrong length.
        let mut chunks = rgb_2x1();
        chunks.insert(1, chunk(b"tRNS", &[0, 255, 0, 0, 0, 0]));
        assert_eq!(ok(&file(&chunks)).pixels, [255, 0, 0, 0, 0, 255, 0, 255]);
        chunks[1] = chunk(b"tRNS", &[0, 255]);
        assert_eq!(
            err(&file(&chunks)),
            (
                33,
                ErrorKind::BadLength {
                    chunk: *b"tRNS",
                    length: 2
                }
            )
        );
        // An alpha channel and a tRNS.
        let chunks = [ihdr(1, 1, 8, 6, 0), chunk(b"tRNS", &[0, 0])];
        assert_eq!(err(&file(&chunks)), (33, ErrorKind::UnexpectedTransparency));
    }

    #[test]
    fn the_image_data_is_checked() {
        let with_data = |data: &[u8]| {
            let mut chunks = rgb_2x1();
            chunks[1] = chunk(b"IDAT", data);
            chunks
        };
        let data_at = 33 + 8;
        // One byte too few and one too many.
        let chunks = with_data(&zlib_stored(&[0, 255, 0, 0, 0, 255]));
        assert_eq!(
            err(&file(&chunks)),
            (33, ErrorKind::TooLittleImageData { needed: 7 })
        );
        let chunks = with_data(&zlib_stored(&[0, 255, 0, 0, 0, 255, 0, 9]));
        // The stored block's data starts after the zlib header and the
        // block's 5 bytes.
        assert_eq!(
            err(&file(&chunks)),
            (data_at + 7, ErrorKind::TooMuchImageData { needed: 7 })
        );
        // Bytes after the zlib stream.
        let mut data = zlib_stored(&[0, 255, 0, 0, 0, 255, 0]);
        let end = data.len();
        data.push(0);
        let chunks = with_data(&data);
        assert_eq!(
            err(&file(&chunks)),
            (data_at + end, ErrorKind::TrailingImageData)
        );
        // A bad Adler-32.
        let mut data = zlib_stored(&[0, 255, 0, 0, 0, 255, 0]);
        let last = data.len() - 1;
        data[last] ^= 1;
        let chunks = with_data(&data);
        let (offset, kind) = err(&file(&chunks));
        assert_eq!(offset, data_at + last - 3);
        assert!(
            matches!(
                kind,
                ErrorKind::BadImageData {
                    kind: inflate::ErrorKind::BadChecksum { .. }
                }
            ),
            "{kind:?}"
        );
        // A filter type of 5, in the image's only row.
        let chunks = with_data(&zlib_stored(&[5, 255, 0, 0, 0, 255, 0]));
        assert_eq!(
            err(&file(&chunks)),
            (33, ErrorKind::BadFilter { row: 0, found: 5 })
        );
        // No image data at all.
        let mut chunks = rgb_2x1();
        chunks.remove(1);
        assert_eq!(err(&file(&chunks)), (33, ErrorKind::MissingImageData));
    }

    #[test]
    fn image_data_may_be_split_but_not_apart() {
        let data = zlib_stored(&[0, 255, 0, 0, 0, 255, 0]);
        // In three, one of them empty.
        let mut chunks = rgb_2x1();
        chunks.splice(
            1..2,
            [
                chunk(b"IDAT", &data[..3]),
                chunk(b"IDAT", &[]),
                chunk(b"IDAT", &data[3..]),
            ],
        );
        assert_eq!(ok(&file(&chunks)).pixels, [255, 0, 0, 255, 0, 255, 0, 255]);
        // A problem in the third chunk is placed in it: change the last
        // byte of the Adler-32.
        let mut bytes = file(&chunks);
        let crc_at = at(&chunks, 3) + 8 + data.len() - 3;
        bytes[crc_at - 1] ^= 1;
        let mut repaired = chunks.clone();
        repaired[3] = chunk(b"IDAT", &bytes[at(&chunks, 3) + 8..crc_at]);
        let (offset, _) = err(&file(&repaired));
        assert_eq!(offset, at(&chunks, 3) + 8 + data.len() - 3 - 4);
        // Another chunk between them.
        let mut chunks = rgb_2x1();
        chunks.splice(
            1..2,
            [
                chunk(b"IDAT", &data[..3]),
                chunk(b"tEXt", b"a\0b"),
                chunk(b"IDAT", &data[3..]),
            ],
        );
        assert_eq!(
            err(&file(&chunks)),
            (at(&chunks, 3), ErrorKind::SplitImageData)
        );
    }

    #[test]
    fn critical_and_animation_chunks_are_rejected() {
        let mut chunks = rgb_2x1();
        chunks.insert(1, chunk(b"ABCD", &[]));
        assert_eq!(
            err(&file(&chunks)),
            (33, ErrorKind::UnknownCriticalChunk { chunk: *b"ABCD" })
        );
        for kind in [b"acTL", b"fcTL", b"fdAT"] {
            let mut chunks = rgb_2x1();
            chunks.insert(1, chunk(kind, &[0; 8]));
            assert_eq!(
                err(&file(&chunks)),
                (33, ErrorKind::Animated { chunk: *kind })
            );
        }
    }

    #[test]
    fn ancillary_chunks_are_ignored_and_some_reported() {
        let mut chunks = rgb_2x1();
        chunks.insert(1, chunk(b"pHYs", &[0; 9]));
        chunks.insert(1, chunk(b"gAMA", &[0, 0, 0xb1, 0x8f]));
        chunks.insert(3, chunk(b"tEXt", b"a\0b"));
        chunks.insert(4, chunk(b"zzZz", b"anything"));
        chunks.insert(5, chunk(b"gAMA", &[0; 4]));
        let image = ok(&file(&chunks));
        assert_eq!(image.ignored, ["gAMA", "tEXt"]);
        assert_eq!(image.pixels, [255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn iend_ends_the_file() {
        let mut bytes = file(&rgb_2x1());
        let end = bytes.len();
        bytes.push(0);
        assert_eq!(err(&bytes), (end, ErrorKind::TrailingBytes));
        let mut chunks = rgb_2x1();
        chunks[2] = chunk(b"IEND", &[0]);
        assert_eq!(
            err(&file(&chunks)),
            (
                at(&chunks, 2),
                ErrorKind::BadLength {
                    chunk: *b"IEND",
                    length: 1
                }
            )
        );
    }

    #[test]
    fn sixteen_bit_samples_round_to_the_nearest() {
        assert_eq!(to_8(0), 0);
        assert_eq!(to_8(65535), 255);
        for v in 0..=u16::MAX {
            let exact = f64::from(v) * 255.0 / 65535.0;
            assert!((f64::from(to_8(v)) - exact).abs() <= 0.5, "{v}");
        }
        // 0x8100 is 128.498 and 0x8101 is 128.502, in 8-bit steps.
        assert_eq!(to_8(0x8100), 128);
        assert_eq!(to_8(0x8101), 129);
    }

    #[test]
    fn paeth_picks_the_nearest() {
        // 10 + 20 - 15 = 15: up-left is nearest.
        assert_eq!(paeth(10, 20, 15), 15);
        // 0 + 5 - 10 = -5: left is 5 away, up 10, up-left 15.
        assert_eq!(paeth(0, 5, 10), 0);
        // 5 + 0 - 10 = -5: up is 5 away, left 10, up-left 15.
        assert_eq!(paeth(5, 0, 10), 0);
        assert_eq!(paeth(5, 5, 5), 5);
        assert_eq!(paeth(255, 0, 0), 255);
    }

    #[test]
    fn paeth_breaks_ties_as_the_specification_does() {
        // 0 + 3 - 1 = 2: up and up-left are both 1 away, left 2. Up wins,
        // where a strict comparison would pick up-left.
        assert_eq!(paeth(0, 3, 1), 3);
        // 0 + 3 - 2 = 1: left and up-left are both 1 away. Left wins.
        assert_eq!(paeth(0, 3, 2), 0);
        // p is computed without wrapping: 200 + 200 - 0 = 400.
        assert_eq!(paeth(200, 200, 0), 200);
        assert_eq!(paeth(10, 250, 255), 10);
    }

    #[test]
    fn average_adds_without_wrapping_and_rounds_down() {
        // One row of two RGB8 pixels under one above it: average of left
        // (a) and up (b), the sum in 9 bits, rounded down.
        let mut rows = vec![
            0, 200, 100, 7, 255, 1, 2, //
            3, 0, 0, 0, 0, 0, 0,
        ];
        let mut row = 0;
        unfilter(&mut rows, 7, 3, &mut row).unwrap();
        // Row 1, byte 0: left 0, up 200 → 100. Byte 3: left 100, up 255 →
        // 177 (355 / 2, rounded down).
        assert_eq!(&rows[8..], [100, 50, 3, 177, 25, 2]);
        assert_eq!(row, 2);
    }

    #[test]
    fn adam7_puts_each_pass_where_the_specification_does() {
        // An 8×8 gray image whose every pixel is the number of its pass, as
        // the specification's table gives them, then filled pass by pass:
        // each pass's pixels hold its number, so the decoded image must be
        // the table.
        const TABLE: [[u8; 8]; 8] = [
            [1, 6, 4, 6, 2, 6, 4, 6],
            [7, 7, 7, 7, 7, 7, 7, 7],
            [5, 6, 5, 6, 5, 6, 5, 6],
            [7, 7, 7, 7, 7, 7, 7, 7],
            [3, 6, 4, 6, 3, 6, 4, 6],
            [7, 7, 7, 7, 7, 7, 7, 7],
            [5, 6, 5, 6, 5, 6, 5, 6],
            [7, 7, 7, 7, 7, 7, 7, 7],
        ];
        // Pass sizes for 8×8: 1×1, 1×1, 2×1, 2×2, 4×2, 4×4, 8×4.
        let sizes = [(1, 1), (1, 1), (2, 1), (2, 2), (4, 2), (4, 4), (8, 4)];
        let mut raw = Vec::new();
        for (pass, (width, height)) in sizes.into_iter().enumerate() {
            for _ in 0..height {
                raw.push(0);
                raw.extend(std::iter::repeat_n(pass as u8 + 1, width));
            }
        }
        let chunks = [
            ihdr(8, 8, 8, 0, 1),
            chunk(b"IDAT", &zlib_stored(&raw)),
            chunk(b"IEND", &[]),
        ];
        let image = ok(&file(&chunks));
        let gray: Vec<u8> = image.pixels.chunks(4).map(|px| px[0]).collect();
        assert_eq!(gray, TABLE.concat());
    }

    #[test]
    fn interlaced_passes_without_pixels_have_no_rows() {
        // The image data's length, worked by hand: a pass with no columns
        // or no rows has no filter bytes either.
        let len = |width, height, depth, color_type| {
            let header = Header {
                width,
                height,
                color: match color_type {
                    0 => Color::Gray,
                    _ => Color::Rgba,
                },
                depth,
                interlaced: true,
            };
            header.image_data_len()
        };
        // 1×1: only pass 1, one row of 1 + 1 byte.
        assert_eq!(len(1, 1, 1, 0), Some(2));
        assert_eq!(len(1, 1, 16, 6), Some(9));
        // 2×1: passes 1 and 6.
        assert_eq!(len(2, 1, 1, 0), Some(4));
        // 1×2: passes 1 and 7.
        assert_eq!(len(1, 2, 16, 6), Some(18));
        // 1×5: passes 1 (1 row), 3 (1 row, from y 4), 5 (1 row, y 2) and
        // 7 (2 rows, y 1 and 3).
        assert_eq!(len(1, 5, 1, 0), Some(10));
        // 5×5: every pass has pixels. Widths 1, 1, 2, 1, 3, 2, 5; heights
        // 1, 1, 1, 2, 1, 3, 2: 2 + 2 + 2 + 4 + 2 + 6 + 4 bytes at 1 bit.
        assert_eq!(len(5, 5, 1, 0), Some(22));
        assert_eq!(
            len(5, 5, 16, 6),
            Some(9 + 9 + 17 + 2 * 9 + 25 + 3 * 17 + 2 * 41)
        );
    }

    #[test]
    fn samples_below_8_bits_are_read_from_the_high_bits_first() {
        // A 4×1 2-bit gray image: 0b00_01_10_11 is 0, 1, 2, 3, scaled by
        // 85.
        let chunks = [
            ihdr(4, 1, 2, 0, 0),
            chunk(b"IDAT", &zlib_stored(&[0, 0b0001_1011])),
            chunk(b"IEND", &[]),
        ];
        let gray: Vec<u8> = ok(&file(&chunks))
            .pixels
            .chunks(4)
            .map(|px| px[0])
            .collect();
        assert_eq!(gray, [0, 85, 170, 255]);
        // 1-bit and 4-bit, with padding bits set.
        let chunks = [
            ihdr(3, 1, 1, 0, 0),
            chunk(b"IDAT", &zlib_stored(&[0, 0b1011_1111])),
            chunk(b"IEND", &[]),
        ];
        let gray: Vec<u8> = ok(&file(&chunks))
            .pixels
            .chunks(4)
            .map(|px| px[0])
            .collect();
        assert_eq!(gray, [255, 0, 255]);
        let chunks = [
            ihdr(3, 1, 4, 0, 0),
            chunk(b"IDAT", &zlib_stored(&[0, 0x1f, 0x3f])),
            chunk(b"IEND", &[]),
        ];
        let gray: Vec<u8> = ok(&file(&chunks))
            .pixels
            .chunks(4)
            .map(|px| px[0])
            .collect();
        assert_eq!(gray, [17, 255, 51]);
    }

    #[test]
    fn a_transparent_value_counts_only_in_the_bits_the_depth_has() {
        // An 8-bit gray image of 0, then 1: tRNS 0x0100 is 0x00, as libpng
        // reads it.
        let gray = |trns: &[u8]| {
            let chunks = [
                ihdr(2, 1, 8, 0, 0),
                chunk(b"tRNS", trns),
                chunk(b"IDAT", &zlib_stored(&[0, 0, 1])),
                chunk(b"IEND", &[]),
            ];
            ok(&file(&chunks)).pixels
        };
        assert_eq!(gray(&[1, 0]), [0, 0, 0, 0, 1, 1, 1, 255]);
        assert_eq!(gray(&[0, 1]), [0, 0, 0, 255, 1, 1, 1, 0]);
        assert_eq!(gray(&[0xff, 0xff]), [0, 0, 0, 255, 1, 1, 1, 255]);
        // A 16-bit sample is compared whole, before it is rounded: 0x0101
        // and 0x0100 round to the same byte but only one is transparent.
        let chunks = [
            ihdr(2, 1, 16, 0, 0),
            chunk(b"tRNS", &[1, 1]),
            chunk(b"IDAT", &zlib_stored(&[0, 1, 1, 1, 0])),
            chunk(b"IEND", &[]),
        ];
        assert_eq!(ok(&file(&chunks)).pixels, [1, 1, 1, 0, 1, 1, 1, 255]);
    }

    #[test]
    fn a_palette_may_be_longer_than_its_bit_depth_needs_but_not_its_alphas() {
        // A 1-bit image with 4 entries: indices 0 and 1 only.
        let plte = chunk(b"PLTE", &[1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4]);
        let image = |between: &[Vec<u8>]| {
            let mut chunks = vec![ihdr(2, 1, 1, 3, 0), plte.clone()];
            chunks.extend_from_slice(between);
            chunks.push(chunk(b"IDAT", &zlib_stored(&[0, 0b0100_0000])));
            chunks.push(chunk(b"IEND", &[]));
            chunks
        };
        assert_eq!(ok(&file(&image(&[]))).pixels, [1, 1, 1, 255, 2, 2, 2, 255]);
        assert_eq!(
            ok(&file(&image(&[chunk(b"tRNS", &[9, 8])]))).pixels,
            [1, 1, 1, 9, 2, 2, 2, 8]
        );
        // Alphas for entries no pixel can use: libpng drops the tRNS, so
        // that the image decodes opaque there; this rejects it.
        let chunks = image(&[chunk(b"tRNS", &[9, 8, 7])]);
        assert_eq!(
            err(&file(&chunks)),
            (
                at(&chunks, 2),
                ErrorKind::BadLength {
                    chunk: *b"tRNS",
                    length: 3
                }
            )
        );
    }

    #[test]
    fn the_color_and_text_chunks_are_reported_once_each() {
        let mut chunks = rgb_2x1();
        for kind in [
            b"cICP", b"mDCV", b"cLLI", b"sRGB", b"iCCP", b"cHRM", b"gAMA", b"cICP",
        ] {
            chunks.insert(1, chunk(kind, &[0; 4]));
        }
        chunks.insert(chunks.len() - 1, chunk(b"zTXt", b"k\0\0x"));
        chunks.insert(chunks.len() - 1, chunk(b"iTXt", b"k\0\0\0\0\0x"));
        chunks.insert(chunks.len() - 1, chunk(b"eXIf", b"MM\0*"));
        assert_eq!(
            ok(&file(&chunks)).ignored,
            [
                "cICP", "gAMA", "cHRM", "iCCP", "sRGB", "cLLI", "mDCV", "zTXt", "iTXt"
            ]
        );
    }

    #[test]
    fn faults_in_split_image_data_are_placed_in_their_chunk() {
        let data = zlib_stored(&[0, 255, 0, 0, 0, 255, 0]);
        // Split so that the Adler-32 straddles the last two chunks.
        let split = data.len() - 2;
        let pieces = |data: &[u8]| {
            let mut chunks = rgb_2x1();
            chunks.splice(
                1..2,
                [
                    chunk(b"IDAT", &data[..3]),
                    chunk(b"IDAT", &[]),
                    chunk(b"IDAT", &data[3..split]),
                    chunk(b"IDAT", &data[split..]),
                ],
            );
            chunks
        };
        assert_eq!(
            ok(&file(&pieces(&data))).pixels,
            [255, 0, 0, 255, 0, 255, 0, 255]
        );
        // A bad Adler-32 is placed at its first byte, in the third chunk.
        let mut bad = data.clone();
        bad[data.len() - 1] ^= 1;
        let chunks = pieces(&bad);
        let (offset, kind) = err(&file(&chunks));
        assert_eq!(offset, at(&chunks, 3) + 8 + (data.len() - 4 - 3));
        assert!(
            matches!(
                kind,
                ErrorKind::BadImageData {
                    kind: inflate::ErrorKind::BadChecksum { .. }
                }
            ),
            "{kind:?}"
        );
        // A stream cut short is placed at the end of the last chunk's
        // data: its CRC.
        let chunks = pieces(&data[..data.len() - 2]);
        let last = at(&chunks, 4);
        let (offset, kind) = err(&file(&chunks));
        assert_eq!(offset, last + 8 + chunks[4].len() - 12);
        assert!(
            matches!(
                kind,
                ErrorKind::BadImageData {
                    kind: inflate::ErrorKind::UnexpectedEnd { .. }
                }
            ),
            "{kind:?}"
        );
    }

    #[test]
    fn source_sheets_wider_than_a_texture_decode_with_raised_limits() {
        let mut raw = vec![0];
        raw.extend((0..10_000u32).flat_map(|x| [x as u8, (x >> 8) as u8, 7]));
        let chunks = [
            ihdr(10_000, 1, 8, 2, 0),
            chunk(b"IDAT", &zlib_stored(&raw)),
            chunk(b"IEND", &[]),
        ];
        assert_eq!(
            err(&file(&chunks)),
            (16, ErrorKind::DimensionTooLarge { limit: 8192 })
        );
        let limits = Limits {
            max_texture_dimension: 16_384,
            ..Limits::default()
        };
        let image = decode(&file(&chunks), limits).unwrap();
        assert_eq!((image.width, image.height), (10_000, 1));
        assert_eq!(image.pixels[4 * 9_999..], [0x0f, 0x27, 7, 255]);
    }

    #[test]
    fn the_image_data_may_be_larger_than_the_limit_if_the_pixels_are_not() {
        // A 1×1 RGBA8 image: 4 bytes of pixels, 5 of image data. The
        // largest texture, 8192 × 8192 RGBA8, is the same case at the
        // default limit, by 8 KiB of filter bytes.
        let chunks = [
            ihdr(1, 1, 8, 6, 0),
            chunk(b"IDAT", &zlib_stored(&[0, 1, 2, 3, 4])),
            chunk(b"IEND", &[]),
        ];
        let limits = Limits {
            max_decoded_bytes: 4,
            ..Limits::default()
        };
        assert_eq!(decode(&file(&chunks), limits).unwrap().pixels, [1, 2, 3, 4]);
    }

    #[test]
    fn image_data_too_large_to_count_is_rejected() {
        // 2³¹ − 1 square in 16-bit RGBA: the pixels, just under 2⁶⁴ bytes,
        // fit limits raised as far as they go, but the image data, 2.25
        // times as much, does not fit a 64-bit count.
        let chunks = [
            ihdr(0x7fff_ffff, 0x7fff_ffff, 16, 6, 0),
            chunk(b"IDAT", &zlib_stored(&[0; 8])),
            chunk(b"IEND", &[]),
        ];
        let limits = Limits {
            max_texture_dimension: u32::MAX,
            max_decoded_bytes: usize::MAX,
            ..Limits::default()
        };
        assert_eq!(
            err_with(&file(&chunks), limits),
            (16, ErrorKind::ImageDataTooLarge)
        );
    }

    #[test]
    fn many_empty_image_data_chunks_are_allowed() {
        let data = zlib_stored(&[0, 255, 0, 0, 0, 255, 0]);
        let mut chunks = rgb_2x1();
        chunks.remove(1);
        let empties = std::iter::repeat_n(chunk(b"IDAT", &[]), 10_000);
        let at = chunks.len() - 1;
        chunks.splice(at..at, empties.chain([chunk(b"IDAT", &data)]));
        assert_eq!(ok(&file(&chunks)).pixels, [255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn what_libpng_loads_but_this_rejects() {
        // An empty IDAT after another chunk, though the image data has
        // ended.
        let mut chunks = rgb_2x1();
        chunks.insert(2, chunk(b"tEXt", b"a\0b"));
        chunks.insert(3, chunk(b"IDAT", &[]));
        assert_eq!(
            err(&file(&chunks)),
            (at(&chunks, 3), ErrorKind::SplitImageData)
        );
        // A suggested palette of a length no palette has.
        let mut chunks = rgb_2x1();
        chunks.insert(1, chunk(b"PLTE", &[1, 2, 3, 4]));
        assert_eq!(
            err(&file(&chunks)),
            (
                33,
                ErrorKind::BadLength {
                    chunk: *b"PLTE",
                    length: 4
                }
            )
        );
        // A gray tRNS of the wrong length.
        let chunks = [
            ihdr(1, 1, 8, 0, 0),
            chunk(b"tRNS", &[0, 0, 0, 0]),
            chunk(b"IDAT", &zlib_stored(&[0, 0])),
            chunk(b"IEND", &[]),
        ];
        assert_eq!(
            err(&file(&chunks)),
            (
                33,
                ErrorKind::BadLength {
                    chunk: *b"tRNS",
                    length: 4
                }
            )
        );
    }

    #[test]
    fn error_kinds_explain_themselves() {
        assert_eq!(
            CodecError::Png {
                offset: 29,
                kind: ErrorKind::BadCrc {
                    chunk: *b"IHDR",
                    stored: 1,
                    computed: 2
                }
            }
            .to_string(),
            "PNG error at byte 29: the IHDR chunk's CRC is 0x00000001, but its contents' is 0x00000002"
        );
        assert_eq!(
            ErrorKind::BadChunkType {
                found: *b"a\x00b\xff"
            }
            .to_string(),
            "a chunk's type, `a\\x00b\\xff`, is not four ASCII letters"
        );
        assert_eq!(
            ErrorKind::BadImageData {
                kind: inflate::ErrorKind::BadBlockType
            }
            .to_string(),
            "the image data is malformed: a block has type 3, which does not exist"
        );
    }
}
