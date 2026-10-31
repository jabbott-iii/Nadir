//! BMFont, the AngelCode bitmap font format, in its text form: what font
//! generators write beside the pages of glyphs they pack. The cook tool
//! reads it into the game's font asset; the game does not read BMFont.
//!
//! [`parse`] reads the text into a [`Font`]: its metrics, the file of each
//! page, each glyph's rectangle and offsets, and the kerning pairs.
//!
//! ```text
//! info face="Pixel" size=16
//! common lineHeight=18 base=14 scaleW=128 scaleH=64 pages=1
//! page id=0 file="pixel_0.png"
//! chars count=1
//! char id=65 x=0 y=0 width=7 height=9 xoffset=0 yoffset=3 xadvance=8 page=0 chnl=15
//! kernings count=0
//! ```
//!
//! - **Lines:** a tag, then `key=value` pairs, separated by spaces or tabs.
//!   A value is an integer, a list of integers with commas, or a string in
//!   double quotes, which may hold spaces and has no escapes: it ends at
//!   the first quote that a space, a tab or the end of the line follows,
//!   so `letter="""` is a quote. Lines end with LF or CRLF; blank lines,
//!   trailing spaces and a leading byte-order mark are skipped.
//! - **Tags:** `info` first and `common` second, each once; then `page`,
//!   `chars`, `char`, `kernings` and `kerning`.
//!   Any other tag is rejected, which also catches the XML and binary
//!   forms of BMFont given by mistake.
//! - **Keys:** those the font asset needs must be there; any others,
//!   which generators add of their own, are ignored. A key twice on a line
//!   is rejected. `chnl` may be left out, meaning all four channels.
//! - **Ids:** code points when `info` has `unicode=1` or an empty
//!   `charset`; Windows-1252 bytes, mapped to code points, for
//!   `charset="ANSI"`. Any other charset is rejected. A `char` of id -1 is
//!   the fallback glyph, which BMFont writes when asked to.
//! - **Rejected, for what the game cannot draw:** `packed=1` (glyphs in
//!   each channel), `xFpBits` other than 0 (fixed-point advances), a number
//!   with a fraction, and a `distanceField` line (both from distance-field
//!   generators).
//! - **Checked:** a `page` line for every page, each once; a glyph's page
//!   exists, and its rectangle lies inside the page; glyph ids are
//!   characters, each once; each kerning pair's ids are characters, and the
//!   pair is there once; `chars count` and `kernings count`, where given,
//!   are the numbers of `char` and `kerning` lines in the file.

use std::fmt;

use crate::{CodecError, Limits};

/// A bitmap font.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Font {
    /// The typeface's name.
    pub face: String,
    /// The size it was rendered at, in pixels; negative when it was matched
    /// to the glyphs' height rather than the cell's, as BMFont writes it.
    pub size: i32,
    /// The distance from one line of text to the next, in pixels.
    pub line_height: u32,
    /// The distance from the top of a line to its baseline.
    pub base: u32,
    /// The width of each page, in pixels.
    pub scale_w: u32,
    /// The height of each page, in pixels.
    pub scale_h: u32,
    /// The file of each page, by page id.
    pub pages: Vec<String>,
    /// The glyphs, in the order of the file.
    pub glyphs: Vec<Glyph>,
    /// The kerning pairs, in the order of the file.
    pub kernings: Vec<Kerning>,
    /// What the alpha, red, green and blue channels of the pages hold, as
    /// `common` gives them: 0 the glyph, 1 its outline, 2 both, 3 zero, 4
    /// one. `None` where the file does not say, as Hiero's files do not.
    pub channels: [Option<u8>; 4],
    /// The glyph to draw for a character the font has none for, which
    /// AngelCode's BMFont writes with id -1 when asked to. Its `id` is 0.
    pub fallback: Option<Glyph>,
}

/// One glyph: where it is on its page, and how to place it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    /// The character, as a Unicode code point.
    pub id: u32,
    /// The left edge of its rectangle on the page.
    pub x: u32,
    /// The top edge of its rectangle on the page.
    pub y: u32,
    /// Its rectangle's width.
    pub width: u32,
    /// Its rectangle's height.
    pub height: u32,
    /// Where to draw the rectangle, right of the pen.
    pub x_offset: i32,
    /// Where to draw the rectangle, below the top of the line.
    pub y_offset: i32,
    /// How far the pen moves after the glyph.
    pub x_advance: i32,
    /// The page it is on.
    pub page: u32,
    /// The channels of the page it is in: 1 blue, 2 green, 4 red, 8 alpha,
    /// 15 all.
    pub channel: u8,
}

/// An adjustment to the pen's advance between two glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kerning {
    /// The glyph on the left.
    pub first: u32,
    /// The glyph on the right.
    pub second: u32,
    /// What to add to the first glyph's advance.
    pub amount: i32,
}

/// What is wrong with a BMFont file. See [`CodecError::BmFont`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The first line is not `info`.
    MissingInfo,
    /// A line's tag is not one of BMFont's.
    UnknownTag {
        /// The tag.
        tag: String,
    },
    /// A line comes where it may not, or a second time.
    Misplaced {
        /// The line's tag.
        tag: &'static str,
    },
    /// `common` is not the line after `info`, or there is none.
    MissingCommon,
    /// A pair has no `=`, or no key.
    BadPair,
    /// A string has no closing quote.
    UnterminatedString,
    /// A line lacks a key it needs.
    MissingKey {
        /// The key.
        key: &'static str,
    },
    /// A line has the same key twice.
    DuplicateKey {
        /// The key.
        key: String,
    },
    /// A value is not an integer.
    BadNumber {
        /// The key.
        key: &'static str,
    },
    /// An integer is out of its field's range.
    NumberOutOfRange {
        /// The key.
        key: &'static str,
    },
    /// A value is not a quoted string.
    NotAString {
        /// The key.
        key: &'static str,
    },
    /// A `page` line's id is past the pages the font has, or repeated.
    BadPageId {
        /// The id.
        id: u32,
    },
    /// A page has no `page` line.
    MissingPage {
        /// The page.
        id: u32,
    },
    /// A glyph's page does not exist.
    MissingGlyphPage {
        /// The glyph's id in the file: -1 for the fallback glyph.
        id: i64,
    },
    /// A glyph's rectangle runs off its page.
    GlyphOffPage {
        /// The glyph's id in the file: -1 for the fallback glyph.
        id: i64,
    },
    /// A glyph's or kerning pair's id is not a Unicode character: a
    /// surrogate, or past U+10FFFF.
    BadCodePoint {
        /// The id.
        id: u32,
    },
    /// Two glyphs have the same id.
    DuplicateGlyph {
        /// The id, as the file gives it.
        id: u32,
    },
    /// Two kerning pairs are the same pair.
    DuplicateKerning {
        /// The glyph on the left, as the file gives it.
        first: u32,
        /// The glyph on the right, as the file gives it.
        second: u32,
    },
    /// `chars count` or `kernings count` is not the number of `char` or
    /// `kerning` lines in the file.
    CountMismatch {
        /// `chars` or `kernings`.
        tag: &'static str,
        /// The count given.
        count: u32,
        /// The lines there are.
        found: usize,
    },
    /// More glyphs, kerning pairs or pages than [`Limits::max_entries`].
    TooManyEntries {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// The font's ids are in a code page other than Unicode or Windows-1252:
    /// `unicode` is not 1, and `charset` is neither empty nor `"ANSI"`.
    UnsupportedCharset {
        /// The charset.
        charset: String,
    },
    /// A Windows-1252 font's id has no character there, or is past 255.
    NotInCharset {
        /// The id.
        id: u32,
    },
    /// The font packs glyphs into each channel of its pages (`packed=1`).
    PackedFont,
    /// The font's advances are fixed-point numbers (`xFpBits`).
    FixedPoint,
    /// A number has a fraction, which a distance-field font's may.
    FractionalNumber {
        /// The key.
        key: &'static str,
    },
    /// The file is a distance-field font.
    DistanceField,
    /// The font has two glyphs of id -1.
    DuplicateFallback,
    /// A string is longer than [`Limits::max_string_bytes`].
    StringTooLong {
        /// The limit that was exceeded.
        limit: usize,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingInfo => f.write_str(
                "the first line is not `info`: this is not a BMFont file in the text format",
            ),
            Self::UnknownTag { tag } => write!(f, "`{tag}` is not a BMFont line"),
            Self::Misplaced { tag } => write!(f, "a `{tag}` line is out of place or repeated"),
            Self::MissingCommon => f.write_str("a `common` line does not follow `info`"),
            Self::BadPair => f.write_str("expected `key=value`"),
            Self::UnterminatedString => f.write_str("a string has no closing `\"`"),
            Self::MissingKey { key } => write!(f, "the line has no `{key}`"),
            Self::DuplicateKey { key } => write!(f, "the line has `{key}` twice"),
            Self::BadNumber { key } => write!(f, "`{key}` is not an integer"),
            Self::NumberOutOfRange { key } => write!(f, "`{key}` is out of range"),
            Self::NotAString { key } => write!(f, "`{key}` is not a quoted string"),
            Self::BadPageId { id } => {
                write!(f, "page {id} is past the font's pages, or listed twice")
            }
            Self::MissingPage { id } => write!(f, "page {id} has no `page` line"),
            Self::MissingGlyphPage { id } => {
                write!(f, "glyph {id} is on a page that does not exist")
            }
            Self::GlyphOffPage { id } => write!(f, "glyph {id}'s rectangle runs off its page"),
            Self::BadCodePoint { id } => write!(f, "id {id} is not a Unicode character"),
            Self::DuplicateGlyph { id } => write!(f, "glyph {id} is listed twice"),
            Self::DuplicateKerning { first, second } => {
                write!(f, "the kerning pair {first}, {second} is listed twice")
            }
            Self::CountMismatch { tag, count, found } => {
                write!(f, "`{tag} count` is {count}, but the font has {found}")
            }
            Self::TooManyEntries { limit } => write!(
                f,
                "more glyphs, kerning pairs or pages than the limit of {limit}"
            ),
            Self::StringTooLong { limit } => {
                write!(f, "a string is longer than the limit of {limit} bytes")
            }
            Self::UnsupportedCharset { charset } => write!(
                f,
                "the font's ids are in the code page {charset:?}: export it again as Unicode"
            ),
            Self::NotInCharset { id } => {
                write!(
                    f,
                    "id {id} is no character of Windows-1252 (`charset=\"ANSI\"`)"
                )
            }
            Self::PackedFont => f.write_str(
                "the font packs glyphs into each channel (`packed=1`), which is not supported",
            ),
            Self::FixedPoint => f.write_str(
                "the font's advances are fixed-point (`xFpBits`), which is not supported",
            ),
            Self::FractionalNumber { key } => write!(
                f,
                "`{key}` has a fraction; distance-field fonts are not supported"
            ),
            Self::DistanceField => f.write_str("distance-field fonts are not supported"),
            Self::DuplicateFallback => f.write_str("the glyph of id -1 is listed twice"),
        }
    }
}

const BYTE_ORDER_MARK: char = '\u{feff}';

/// The glyph's channels when `chnl` is left out: all four.
const ALL_CHANNELS: u8 = 15;

/// One `key=value` pair: the key, the value's text (a string without its
/// quotes), whether it was quoted, and the column it starts at.
struct Pair<'a> {
    key: &'a str,
    value: &'a str,
    quoted: bool,
    column: u32,
}

/// One line, split into its tag and pairs.
struct Line<'a> {
    number: u32,
    /// The tag's column, where errors about the whole line are placed.
    column: u32,
    tag: &'a str,
    pairs: Vec<Pair<'a>>,
}

impl Line<'_> {
    fn error(&self, column: u32, kind: ErrorKind) -> CodecError {
        CodecError::BmFont {
            line: self.number,
            column,
            kind,
        }
    }

    /// An error about the whole line, at its tag.
    fn at_tag(&self, kind: ErrorKind) -> CodecError {
        self.error(self.column, kind)
    }

    fn find(&self, key: &str) -> Option<&Pair<'_>> {
        self.pairs.iter().find(|pair| pair.key == key)
    }

    fn require(&self, key: &'static str) -> Result<&Pair<'_>, CodecError> {
        self.find(key)
            .ok_or_else(|| self.at_tag(ErrorKind::MissingKey { key }))
    }

    /// An integer field, in the range of `T`.
    fn number<T: TryFrom<i64>>(&self, key: &'static str) -> Result<T, CodecError> {
        let pair = self.require(key)?;
        self.parse_number(pair, key)
    }

    fn optional_number<T: TryFrom<i64>>(&self, key: &'static str) -> Result<Option<T>, CodecError> {
        self.find(key)
            .map(|pair| self.parse_number(pair, key))
            .transpose()
    }

    fn parse_number<T: TryFrom<i64>>(
        &self,
        pair: &Pair<'_>,
        key: &'static str,
    ) -> Result<T, CodecError> {
        let digits = pair.value.strip_prefix('-').unwrap_or(pair.value);
        if !pair.quoted
            && digits.split_once('.').is_some_and(|(whole, fraction)| {
                !whole.is_empty()
                    && !fraction.is_empty()
                    && (whole.bytes().chain(fraction.bytes())).all(|byte| byte.is_ascii_digit())
            })
        {
            return Err(self.error(pair.column, ErrorKind::FractionalNumber { key }));
        }
        if pair.quoted || digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(self.error(pair.column, ErrorKind::BadNumber { key }));
        }
        let wide: i64 = pair
            .value
            .parse()
            .map_err(|_| self.error(pair.column, ErrorKind::NumberOutOfRange { key }))?;
        T::try_from(wide).map_err(|_| self.error(pair.column, ErrorKind::NumberOutOfRange { key }))
    }

    /// A quoted string field, within the length limit.
    fn string(&self, key: &'static str, limits: Limits) -> Result<String, CodecError> {
        let pair = self.require(key)?;
        if !pair.quoted {
            return Err(self.error(pair.column, ErrorKind::NotAString { key }));
        }
        if pair.value.len() > limits.max_string_bytes {
            return Err(self.error(
                pair.column,
                ErrorKind::StringTooLong {
                    limit: limits.max_string_bytes,
                },
            ));
        }
        Ok(pair.value.to_string())
    }
}

/// Lines of up to this many pairs are checked for a repeated key as each
/// pair is read; longer ones once at the end, by sorting, so that a long
/// line cannot make the check quadratic.
const SMALL_LINE: usize = 16;

fn is_space(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Moves `rest` on to `to`, which is a suffix of it, and `column` on by
/// the characters passed over.
fn advance<'a>(rest: &mut &'a str, column: &mut usize, to: &'a str) {
    *column += rest[..rest.len() - to.len()].chars().count();
    *rest = to;
}

/// Splits one line into its tag and pairs. `None` for a blank line.
fn split(number: u32, text: &str) -> Result<Option<Line<'_>>, CodecError> {
    let error = |column: usize, kind| CodecError::BmFont {
        line: number,
        column: u32::try_from(column).unwrap_or(u32::MAX),
        kind,
    };
    // Columns count characters: `column` is that of `rest`'s first.
    let mut rest = text;
    let mut column = 1;
    advance(&mut rest, &mut column, text.trim_start_matches(is_space));
    if rest.is_empty() {
        return Ok(None);
    }
    let tag_column = u32::try_from(column).unwrap_or(u32::MAX);
    let tag_end = rest.find(is_space).unwrap_or(rest.len());
    let tag = &rest[..tag_end];
    let after = rest[tag_end..].trim_start_matches(is_space);
    advance(&mut rest, &mut column, after);
    let mut pairs: Vec<Pair<'_>> = Vec::new();
    while !rest.is_empty() {
        let start = column;
        let Some((key, after)) = rest.split_once('=') else {
            return Err(error(start, ErrorKind::BadPair));
        };
        if key.is_empty() || key.contains(is_space) {
            return Err(error(start, ErrorKind::BadPair));
        }
        let (value, quoted, after) = if let Some(quoted) = after.strip_prefix('"') {
            // The closing quote is the first one that ends the pair, so
            // that `letter="""`, as exporters write the quote character,
            // reads as `"`.
            let close = quoted.match_indices('"').map(|(at, _)| at).find(|&at| {
                quoted
                    .get(at + 1..)
                    .is_none_or(|rest| rest.is_empty() || rest.starts_with(is_space))
            });
            let Some(close) = close else {
                return Err(error(start, ErrorKind::UnterminatedString));
            };
            (&quoted[..close], true, &quoted[close + 1..])
        } else {
            let end = after.find(is_space).unwrap_or(after.len());
            let (value, after) = after.split_at(end);
            (value, false, after)
        };
        if pairs.len() < SMALL_LINE && pairs.iter().any(|pair| pair.key == key) {
            return Err(error(
                start,
                ErrorKind::DuplicateKey {
                    key: key.to_string(),
                },
            ));
        }
        pairs.push(Pair {
            key,
            value,
            quoted,
            column: u32::try_from(start).unwrap_or(u32::MAX),
        });
        advance(&mut rest, &mut column, after.trim_start_matches(is_space));
    }
    if pairs.len() > SMALL_LINE
        && let Some((index, key)) = first_repeat(pairs.iter().map(|pair| pair.key))
    {
        let column = pairs.get(index).map_or(1, |pair| pair.column);
        return Err(CodecError::BmFont {
            line: number,
            column,
            kind: ErrorKind::DuplicateKey {
                key: key.to_string(),
            },
        });
    }
    Ok(Some(Line {
        number,
        column: tag_column,
        tag,
        pairs,
    }))
}

/// Parses a BMFont file in the text format.
///
/// # Errors
///
/// [`CodecError::BmFont`], with the line and column of the problem, for a
/// file that is malformed, misses what the font asset needs, or exceeds
/// one of `limits`.
pub fn parse(text: &str, limits: Limits) -> Result<Font, CodecError> {
    let text = text.strip_prefix(BYTE_ORDER_MARK).unwrap_or(text);
    let mut raw_lines = text.split('\n').enumerate().map(|(index, raw)| {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        (number, raw.strip_suffix('\r').unwrap_or(raw))
    });
    // The first line says whether this is BMFont text at all: look at its
    // tag before its pairs, so that XML or binary is named as such.
    let first = raw_lines.find(|(_, raw)| !raw.trim_start_matches(is_space).is_empty());
    let not_bmfont = |line, column| CodecError::BmFont {
        line,
        column,
        kind: ErrorKind::MissingInfo,
    };
    let Some((number, raw)) = first else {
        return Err(not_bmfont(1, 1));
    };
    if raw.split(is_space).find(|word| !word.is_empty()) != Some("info") {
        let indent = raw.chars().take_while(|&c| is_space(c)).count();
        return Err(not_bmfont(
            number,
            u32::try_from(indent + 1).unwrap_or(u32::MAX),
        ));
    }
    // Each line is split as it is reached, so that a limit or an error
    // stops the parse there.
    let mut lines = std::iter::once((number, raw))
        .chain(raw_lines)
        .filter_map(|(number, raw)| split(number, raw).transpose());
    let end = CodecError::BmFont {
        line: u32::try_from(text.split('\n').count()).unwrap_or(u32::MAX),
        column: 1,
        kind: ErrorKind::MissingCommon,
    };

    let Some(info) = lines.next().transpose()? else {
        return Err(not_bmfont(number, 1));
    };
    let face = info.string("face", limits)?;
    let size = info.number("size")?;
    // The ids are code points when `unicode=1`, as Snowb writes it beside
    // a charset that lists the ids, or when the charset is empty, as
    // AngelCode's BMFont writes it, and Hiero with `unicode=0`. They are
    // Windows-1252 bytes for BMFont's `charset="ANSI"`.
    let unicode = info.optional_number::<i64>("unicode")? == Some(1);
    let ansi = match info.find("charset") {
        _ if unicode => false,
        None => false,
        Some(pair) if pair.value.is_empty() => false,
        Some(pair) if pair.value == "ANSI" => true,
        Some(pair) => {
            return Err(info.error(
                pair.column,
                ErrorKind::UnsupportedCharset {
                    charset: pair.value.to_string(),
                },
            ));
        }
    };
    // The character of an id in the font's charset.
    let character = |line: &Line<'_>, key: &'static str, id: u32| -> Result<u32, CodecError> {
        let column = line.find(key).map_or(1, |pair| pair.column);
        if !ansi {
            return match char::from_u32(id) {
                Some(_) => Ok(id),
                None => Err(line.error(column, ErrorKind::BadCodePoint { id })),
            };
        }
        match id {
            0..=0x7f | 0xa0..=0xff => Ok(id),
            0x80..=0x9f => match WINDOWS_1252[(id - 0x80) as usize] {
                0 => Err(line.error(column, ErrorKind::NotInCharset { id })),
                code_point => Ok(u32::from(code_point)),
            },
            _ => Err(line.error(column, ErrorKind::NotInCharset { id })),
        }
    };

    let Some(common) = lines.next().transpose()? else {
        return Err(end);
    };
    match common.tag {
        "common" => {}
        "info" => return Err(common.at_tag(ErrorKind::Misplaced { tag: "info" })),
        _ => return Err(common.at_tag(ErrorKind::MissingCommon)),
    }
    let line_height = common.number("lineHeight")?;
    let base = common.number("base")?;
    let scale_w: u32 = common.number("scaleW")?;
    let scale_h: u32 = common.number("scaleH")?;
    let page_count: u32 = common.number("pages")?;
    match common.optional_number::<u8>("packed")? {
        None | Some(0) => {}
        Some(packed) => {
            let column = common.find("packed").map_or(1, |pair| pair.column);
            let kind = if packed == 1 {
                ErrorKind::PackedFont
            } else {
                ErrorKind::NumberOutOfRange { key: "packed" }
            };
            return Err(common.error(column, kind));
        }
    }
    if common
        .optional_number::<i64>("xFpBits")?
        .is_some_and(|bits| bits != 0)
    {
        let column = common.find("xFpBits").map_or(1, |pair| pair.column);
        return Err(common.error(column, ErrorKind::FixedPoint));
    }
    let channels = [
        channel_content(&common, "alphaChnl")?,
        channel_content(&common, "redChnl")?,
        channel_content(&common, "greenChnl")?,
        channel_content(&common, "blueChnl")?,
    ];
    let too_many = |line: &Line<'_>| {
        line.at_tag(ErrorKind::TooManyEntries {
            limit: limits.max_entries,
        })
    };
    if page_count as usize > limits.max_entries {
        return Err(too_many(&common));
    }

    // Each page's id, file, line and column, in the order of the file.
    let mut pages: Vec<(u32, String, u32, u32)> = Vec::new();
    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut kernings: Vec<Kerning> = Vec::new();
    // Where each glyph and kerning pair is, and its ids as the file gives
    // them, which errors name.
    let mut glyph_lines: Vec<(u32, u32, u32)> = Vec::new();
    let mut kerning_lines: Vec<(u32, u32, u32, u32)> = Vec::new();
    let mut fallback: Option<Glyph> = None;
    // Each count, with the line and column of its `count`.
    let mut chars_count: Option<(u32, u32, u32)> = None;
    let mut kernings_count: Option<(u32, u32, u32)> = None;
    for line in lines {
        let line = line?;
        match line.tag {
            "page" => {
                let id: u32 = line.number("id")?;
                let file = line.string("file", limits)?;
                let column = line.require("id")?.column;
                if id >= page_count {
                    return Err(line.error(column, ErrorKind::BadPageId { id }));
                }
                pages.push((id, file, line.number, column));
                // More page lines than pages: one is a repeat.
                if pages.len() > page_count as usize {
                    return Err(repeated_page(&pages));
                }
            }
            "chars" | "kernings" => {
                let count = line.number("count")?;
                let column = line.require("count")?.column;
                let (seen, tag) = if line.tag == "chars" {
                    (&mut chars_count, "chars")
                } else {
                    (&mut kernings_count, "kernings")
                };
                if seen.is_some() {
                    return Err(line.at_tag(ErrorKind::Misplaced { tag }));
                }
                *seen = Some((count, line.number, column));
            }
            "char" => {
                if glyphs.len() >= limits.max_entries {
                    return Err(too_many(&line));
                }
                let channel: Option<u8> = line.optional_number("chnl")?;
                // Any of the four channels, 1, 2, 4 and 8, together; Hiero
                // writes 0.
                if channel.is_some_and(|channel| channel > ALL_CHANNELS) {
                    let column = line.find("chnl").map_or(1, |pair| pair.column);
                    return Err(line.error(column, ErrorKind::NumberOutOfRange { key: "chnl" }));
                }
                // AngelCode writes the glyph for characters the font lacks
                // as id -1.
                let raw_id: i64 = line.number("id")?;
                let is_fallback = raw_id == -1;
                let id = if is_fallback {
                    0
                } else {
                    let id = u32::try_from(raw_id).map_err(|_| {
                        let column = line.find("id").map_or(1, |pair| pair.column);
                        line.error(column, ErrorKind::NumberOutOfRange { key: "id" })
                    })?;
                    character(&line, "id", id)?
                };
                let glyph = Glyph {
                    id,
                    x: line.number("x")?,
                    y: line.number("y")?,
                    width: line.number("width")?,
                    height: line.number("height")?,
                    x_offset: line.number("xoffset")?,
                    y_offset: line.number("yoffset")?,
                    x_advance: line.number("xadvance")?,
                    page: line.number("page")?,
                    channel: channel.unwrap_or(ALL_CHANNELS),
                };
                let at = |key| line.find(key).map_or(1, |pair| pair.column);
                // Errors name the glyph by the id in the file, -1 too.
                if glyph.page >= page_count {
                    return Err(line.error(at("page"), ErrorKind::MissingGlyphPage { id: raw_id }));
                }
                let inside = |start: u32, size: u32, page: u32| {
                    start.checked_add(size).is_some_and(|end| end <= page)
                };
                if !inside(glyph.x, glyph.width, scale_w) || !inside(glyph.y, glyph.height, scale_h)
                {
                    return Err(line.error(at("x"), ErrorKind::GlyphOffPage { id: raw_id }));
                }
                if is_fallback {
                    if fallback.is_some() {
                        return Err(line.error(at("id"), ErrorKind::DuplicateFallback));
                    }
                    fallback = Some(glyph);
                } else {
                    glyphs.push(glyph);
                    let file_id = u32::try_from(raw_id).unwrap_or(glyph.id);
                    glyph_lines.push((line.number, line.column, file_id));
                }
            }
            "kerning" => {
                if kernings.len() >= limits.max_entries {
                    return Err(too_many(&line));
                }
                let first = line.number("first")?;
                let second = line.number("second")?;
                kernings.push(Kerning {
                    first: character(&line, "first", first)?,
                    second: character(&line, "second", second)?,
                    amount: line.number("amount")?,
                });
                kerning_lines.push((line.number, line.column, first, second));
            }
            "distanceField" => return Err(line.at_tag(ErrorKind::DistanceField)),
            "info" => return Err(line.at_tag(ErrorKind::Misplaced { tag: "info" })),
            "common" => return Err(line.at_tag(ErrorKind::Misplaced { tag: "common" })),
            tag => {
                return Err(line.at_tag(ErrorKind::UnknownTag {
                    tag: tag.to_string(),
                }));
            }
        }
    }

    let glyph_count = glyphs.len() + usize::from(fallback.is_some());
    for (count, line, column, found, tag) in [
        (chars_count, glyph_count, "chars"),
        (kernings_count, kernings.len(), "kernings"),
    ]
    .into_iter()
    .filter_map(|(seen, found, tag)| {
        seen.map(|(count, line, column)| (count, line, column, found, tag))
    }) {
        if count as usize != found {
            return Err(CodecError::BmFont {
                line,
                column,
                kind: ErrorKind::CountMismatch { tag, count, found },
            });
        }
    }
    if first_repeat(pages.iter().map(|page| page.0)).is_some() {
        return Err(repeated_page(&pages));
    }
    // No repeats, and every id below the count: the ids in order are 0, 1,
    // 2, … up to the first that is missing.
    pages.sort_unstable_by_key(|page| page.0);
    if pages.len() < page_count as usize {
        let id = pages
            .iter()
            .zip(0..)
            .find(|((id, ..), want)| id != want)
            .map_or(pages.len() as u32, |(_, want)| want);
        return Err(common.at_tag(ErrorKind::MissingPage { id }));
    }
    if let Some((index, _)) = first_repeat(glyphs.iter().map(|glyph| glyph.id)) {
        let (line, column, id) = glyph_lines.get(index).copied().unwrap_or((1, 1, 0));
        return Err(CodecError::BmFont {
            line,
            column,
            kind: ErrorKind::DuplicateGlyph { id },
        });
    }
    if let Some((index, _)) = first_repeat(kernings.iter().map(|pair| (pair.first, pair.second))) {
        let (line, column, first, second) =
            kerning_lines.get(index).copied().unwrap_or((1, 1, 0, 0));
        return Err(CodecError::BmFont {
            line,
            column,
            kind: ErrorKind::DuplicateKerning { first, second },
        });
    }
    Ok(Font {
        face,
        size,
        line_height,
        base,
        scale_w,
        scale_h,
        pages: pages.into_iter().map(|(_, file, ..)| file).collect(),
        glyphs,
        kernings,
        channels,
        fallback,
    })
}

/// The error for the first `page` line, in the file's order, whose id an
/// earlier one has.
fn repeated_page(pages: &[(u32, String, u32, u32)]) -> CodecError {
    let (line, column, id) = first_repeat(pages.iter().map(|page| page.0))
        .and_then(|(index, id)| pages.get(index).map(|page| (page.2, page.3, id)))
        .unwrap_or((1, 1, 0));
    CodecError::BmFont {
        line,
        column,
        kind: ErrorKind::BadPageId { id },
    }
}

/// What one channel of the pages holds, as `common` gives it: 0 to 4.
fn channel_content(common: &Line<'_>, key: &'static str) -> Result<Option<u8>, CodecError> {
    let content: Option<u8> = common.optional_number(key)?;
    if content.is_some_and(|content| content > 4) {
        let column = common.find(key).map_or(1, |pair| pair.column);
        return Err(common.error(column, ErrorKind::NumberOutOfRange { key }));
    }
    Ok(content)
}

/// Windows-1252's characters for 0x80 to 0x9f, where it differs from
/// Latin-1; 0 where it has none.
const WINDOWS_1252: [u16; 32] = [
    0x20ac, 0, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160, 0x2039,
    0x0152, 0, 0x017d, 0, 0, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x02dc,
    0x2122, 0x0161, 0x203a, 0x0153, 0, 0x017e, 0x0178,
];

/// The index of the first item equal to one before it, and the item: by
/// sorting, so that a large font cannot make the check quadratic.
fn first_repeat<T: Ord + Copy>(items: impl Iterator<Item = T>) -> Option<(usize, T)> {
    let mut order: Vec<(T, usize)> = items
        .enumerate()
        .map(|(index, item)| (item, index))
        .collect();
    order.sort_unstable();
    order
        .windows(2)
        .filter(|pair| pair[0].0 == pair[1].0)
        .map(|pair| (pair[1].1, pair[1].0))
        .min_by_key(|&(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "info face=\"F\" size=8\ncommon lineHeight=9 base=7 scaleW=32 scaleH=16 pages=1\npage id=0 file=\"f.png\"\n";

    fn err(text: &str) -> (u32, u32, ErrorKind) {
        err_with(text, Limits::default())
    }

    fn err_with(text: &str, limits: Limits) -> (u32, u32, ErrorKind) {
        match parse(text, limits) {
            Err(CodecError::BmFont { line, column, kind }) => (line, column, kind),
            other => panic!("not a BMFont error: {other:?}"),
        }
    }

    /// The head, then `rest`.
    fn font(rest: &str) -> String {
        format!("{HEAD}{rest}")
    }

    const CHAR: &str = "char id=65 x=0 y=0 width=8 height=8 xoffset=0 yoffset=0 xadvance=8 page=0";

    #[test]
    fn the_first_lines_are_info_and_common() {
        assert_eq!(err(""), (1, 1, ErrorKind::MissingInfo));
        assert_eq!(
            err("<?xml version=\"1.0\"?>\n<font>"),
            (1, 1, ErrorKind::MissingInfo)
        );
        assert_eq!(err("BMF\u{3}"), (1, 1, ErrorKind::MissingInfo));
        // At the first line that is not blank, and its tag.
        assert_eq!(err("\n \n\nchar id=1\n"), (4, 1, ErrorKind::MissingInfo));
        assert_eq!(err("\n  \tchar id=1\n"), (2, 4, ErrorKind::MissingInfo));
        // Errors about a whole line are at its tag, indented or not.
        assert_eq!(
            err(&font("    glyph id=1\n")),
            (
                4,
                5,
                ErrorKind::UnknownTag {
                    tag: "glyph".to_string()
                }
            )
        );
        assert_eq!(
            err(&font("\t\tcommon lineHeight=1\n")),
            (4, 3, ErrorKind::Misplaced { tag: "common" })
        );
        assert_eq!(
            err(&font("  char id=1\n")),
            (4, 3, ErrorKind::MissingKey { key: "x" })
        );
        assert_eq!(
            err("info face=\"F\" size=8\ninfo face=\"G\" size=8\n"),
            (2, 1, ErrorKind::Misplaced { tag: "info" })
        );
        assert_eq!(
            err("info face=\"F\" size=8\n"),
            (2, 1, ErrorKind::MissingCommon)
        );
        assert_eq!(
            err("info face=\"F\" size=8\npage id=0 file=\"f\"\n"),
            (2, 1, ErrorKind::MissingCommon)
        );
        assert_eq!(
            err(&font("info face=\"G\" size=1\n")),
            (4, 1, ErrorKind::Misplaced { tag: "info" })
        );
        assert_eq!(
            err(&font("glyph id=1\n")),
            (
                4,
                1,
                ErrorKind::UnknownTag {
                    tag: "glyph".to_string()
                }
            )
        );
    }

    #[test]
    fn pairs_are_checked_where_they_are() {
        // Columns count characters: "é" is one.
        assert_eq!(err("info face=\"é\" size\n"), (1, 15, ErrorKind::BadPair));
        assert_eq!(
            err("info face=\"F size=8\n"),
            (1, 6, ErrorKind::UnterminatedString)
        );
        assert_eq!(
            err("info face=\"F\"size=8\n"),
            (1, 6, ErrorKind::UnterminatedString)
        );
        assert_eq!(
            err("info face=\"F\" size=8 size=9\n"),
            (
                1,
                22,
                ErrorKind::DuplicateKey {
                    key: "size".to_string()
                }
            )
        );
        assert_eq!(
            err("info size=8\n"),
            (1, 1, ErrorKind::MissingKey { key: "face" })
        );
        assert_eq!(
            err("info face=F size=8\n"),
            (1, 6, ErrorKind::NotAString { key: "face" })
        );
    }

    #[test]
    fn lines_are_read_in_order_and_long_ones_in_linear_time() {
        // An error is the first one in the file, though a later line would
        // not split.
        assert_eq!(
            err("info face=\"F\" size=x\ncommon\npage\nthis is junk\n"),
            (1, 15, ErrorKind::BadNumber { key: "size" })
        );
        // A line of 200,000 keys of a tool's own, then one repeated: found
        // by sorting, at its column. Checking each key against those
        // before it, or counting each pair's column from the line's start,
        // would take minutes.
        let mut line = String::from("info face=\"F\" size=8");
        for i in 0..200_000 {
            line.push_str(&format!(" k{i}=0"));
        }
        let column = line.chars().count() as u32 + 2;
        line.push_str(" k7=1");
        let start = std::time::Instant::now();
        assert_eq!(
            err(&format!("{line}\n")),
            (
                1,
                column,
                ErrorKind::DuplicateKey {
                    key: "k7".to_string()
                }
            )
        );
        let took = start.elapsed();
        assert!(took.as_secs() < 30, "{took:?}");
    }

    #[test]
    fn numbers_are_checked_against_their_fields() {
        assert_eq!(
            err("info face=\"F\" size=8x\n"),
            (1, 15, ErrorKind::BadNumber { key: "size" })
        );
        assert_eq!(
            err("info face=\"F\" size=\"8\"\n"),
            (1, 15, ErrorKind::BadNumber { key: "size" })
        );
        assert_eq!(
            err("info face=\"F\" size=-\n"),
            (1, 15, ErrorKind::BadNumber { key: "size" })
        );
        assert_eq!(
            err("info face=\"F\" size=99999999999\n"),
            (1, 15, ErrorKind::NumberOutOfRange { key: "size" })
        );
        assert_eq!(
            err("info face=\"F\" size=99999999999999999999\n"),
            (1, 15, ErrorKind::NumberOutOfRange { key: "size" })
        );
        // A width cannot be negative.
        let text = font(&CHAR.replace("width=8", "width=-1"));
        assert_eq!(err(&text).2, ErrorKind::NumberOutOfRange { key: "width" });
    }

    #[test]
    fn pages_are_checked() {
        let two =
            "info face=\"F\" size=8\ncommon lineHeight=9 base=7 scaleW=32 scaleH=16 pages=2\n";
        assert_eq!(
            err(&format!("{two}page id=1 file=\"b\"\n")),
            (2, 1, ErrorKind::MissingPage { id: 0 })
        );
        assert_eq!(
            err(&format!(
                "{two}page id=0 file=\"a\"\npage id=0 file=\"b\"\n"
            )),
            (4, 6, ErrorKind::BadPageId { id: 0 })
        );
        assert_eq!(
            err(&format!("{two}page id=2 file=\"c\"\n")),
            (3, 6, ErrorKind::BadPageId { id: 2 })
        );
        // Pages past the limit; and more than the file has lines for,
        // which is the first page it lacks.
        let many = "info face=\"F\" size=8\ncommon lineHeight=9 base=7 scaleW=32 scaleH=16 pages=4000000000\n";
        assert!(matches!(err(many).2, ErrorKind::TooManyEntries { .. }));
        assert_eq!(
            err(&HEAD.replace("pages=1", "pages=200")),
            (2, 1, ErrorKind::MissingPage { id: 1 })
        );
        // A repeat among many pages, found at the end, at its line.
        let twenty = HEAD
            .replace("pages=1", "pages=20")
            .replace("page id=0 file=\"f.png\"\n", "");
        let lines: String = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 5,
        ]
        .iter()
        .map(|id| format!("page id={id} file=\"p{id}.png\"\n"))
        .collect();
        assert_eq!(
            err(&format!("{twenty}{lines}")),
            (21, 6, ErrorKind::BadPageId { id: 5 })
        );
        // Pages in any order, kept in the order of their ids.
        let text = format!("{two}page id=1 file=\"b\"\npage id=0 file=\"a\"\n");
        assert_eq!(parse(&text, Limits::default()).unwrap().pages, ["a", "b"]);
    }

    #[test]
    fn glyphs_are_checked() {
        let line = |text: &str| err(&font(&format!("{text}\n")));
        assert_eq!(
            line(&CHAR.replace("page=0", "page=1")),
            (4, 68, ErrorKind::MissingGlyphPage { id: 65 })
        );
        // 32 wide: x 25 + width 8 runs off.
        assert_eq!(
            line(&CHAR.replace("x=0", "x=25")),
            (4, 12, ErrorKind::GlyphOffPage { id: 65 })
        );
        assert_eq!(
            line(&CHAR.replace("y=0", "y=9")),
            (4, 12, ErrorKind::GlyphOffPage { id: 65 })
        );
        assert_eq!(
            line(&CHAR.replace("id=65", "id=55296")),
            (4, 6, ErrorKind::BadCodePoint { id: 55296 })
        );
        assert_eq!(
            line(&CHAR.replace("xadvance=8 ", "")),
            (4, 1, ErrorKind::MissingKey { key: "xadvance" })
        );
        assert_eq!(
            err(&font(&format!("{CHAR}\n{CHAR}\n"))),
            (5, 1, ErrorKind::DuplicateGlyph { id: 65 })
        );
        assert_eq!(
            line(&format!("{CHAR} chnl=256")).2,
            ErrorKind::NumberOutOfRange { key: "chnl" }
        );
        // Channels are 1, 2, 4 and 8, together or not; 0 too, from Hiero.
        assert_eq!(
            line(&format!("{CHAR} chnl=16")),
            (4, 75, ErrorKind::NumberOutOfRange { key: "chnl" })
        );
        for chnl in [0, 4, 15] {
            let text = font(&format!("{CHAR} chnl={chnl}\n"));
            assert_eq!(
                parse(&text, Limits::default()).unwrap().glyphs[0].channel,
                chnl
            );
        }
    }

    #[test]
    fn kernings_and_counts_are_checked() {
        // Ids are characters in a kerning pair too.
        for first in [55_296, 4_294_967_295] {
            assert_eq!(
                err(&font(&format!(
                    "kerning first={first} second=65 amount=1\n"
                ))),
                (4, 9, ErrorKind::BadCodePoint { id: first })
            );
        }
        // A count may come after its lines.
        let text = font(&format!("{CHAR}\nchars count=1\n"));
        assert!(parse(&text, Limits::default()).is_ok());
        let pair = "kerning first=65 second=86 amount=-1";
        assert_eq!(
            err(&font(&format!("{pair}\n{pair}\n"))),
            (
                5,
                1,
                ErrorKind::DuplicateKerning {
                    first: 65,
                    second: 86
                }
            )
        );
        assert_eq!(
            err(&font(&format!("chars count=2\n{CHAR}\n"))),
            (
                4,
                7,
                ErrorKind::CountMismatch {
                    tag: "chars",
                    count: 2,
                    found: 1
                }
            )
        );
        assert_eq!(
            err(&font("kernings count=1\n")),
            (
                4,
                10,
                ErrorKind::CountMismatch {
                    tag: "kernings",
                    count: 1,
                    found: 0
                }
            )
        );
        assert_eq!(
            err(&font("chars count=0\nchars count=0\n")),
            (5, 1, ErrorKind::Misplaced { tag: "chars" })
        );
    }

    #[test]
    fn limits_hold() {
        let limits = Limits {
            max_entries: 1,
            ..Limits::default()
        };
        let second = CHAR.replace("id=65", "id=66");
        assert_eq!(
            err_with(&font(&format!("{CHAR}\n{second}\n")), limits),
            (5, 1, ErrorKind::TooManyEntries { limit: 1 })
        );
        let limits = Limits {
            max_string_bytes: 3,
            ..Limits::default()
        };
        assert_eq!(
            err_with("info face=\"Long\" size=8\n", limits),
            (1, 6, ErrorKind::StringTooLong { limit: 3 })
        );
    }

    #[test]
    fn a_quote_inside_a_string_ends_it_only_before_a_space() {
        let text = font(&format!("{CHAR} letter=\"\"\" more=\"a\"b\"\n"));
        let parsed = parse(&text, Limits::default()).unwrap();
        assert_eq!(parsed.glyphs.len(), 1);
        assert_eq!(
            err("info face=\"F\"x size=8\n").2,
            ErrorKind::UnterminatedString
        );
    }

    #[test]
    fn what_changes_the_meaning_of_the_values_is_rejected() {
        let common = |extra: &str| {
            format!(
                "info face=\"F\" size=8\ncommon lineHeight=9 base=7 scaleW=32 scaleH=16 pages=1 {extra}\npage id=0 file=\"f.png\"\n"
            )
        };
        assert_eq!(err(&common("packed=1")), (2, 56, ErrorKind::PackedFont));
        assert!(parse(&common("packed=0"), Limits::default()).is_ok());
        assert_eq!(
            err(&common("packed=2")),
            (2, 56, ErrorKind::NumberOutOfRange { key: "packed" })
        );
        assert_eq!(err(&common("xFpBits=6")), (2, 56, ErrorKind::FixedPoint));
        assert_eq!(
            err(&common("alphaChnl=5")),
            (2, 56, ErrorKind::NumberOutOfRange { key: "alphaChnl" })
        );
        assert_eq!(
            err("info face=\"F\" size=8.5\n"),
            (1, 15, ErrorKind::FractionalNumber { key: "size" })
        );
        assert_eq!(
            err(&font("distanceField fieldType=\"msdf\"\n")),
            (4, 1, ErrorKind::DistanceField)
        );
    }

    #[test]
    fn charsets_other_than_unicode_and_windows_1252_are_rejected() {
        let ansi = |rest: &str| {
            format!(
                "info face=\"F\" size=8 charset=\"ANSI\"\ncommon lineHeight=9 base=7 scaleW=32 scaleH=16 pages=1\npage id=0 file=\"f.png\"\n{rest}"
            )
        };
        let line = CHAR.replace("id=65", "id=129");
        assert_eq!(
            err(&ansi(&format!("{line}\n"))),
            (4, 6, ErrorKind::NotInCharset { id: 129 })
        );
        let line = CHAR.replace("id=65", "id=300");
        assert_eq!(
            err(&ansi(&format!("{line}\n"))),
            (4, 6, ErrorKind::NotInCharset { id: 300 })
        );
        assert_eq!(
            err("info face=\"F\" size=8 charset=\"SHIFTJIS\"\n"),
            (
                1,
                22,
                ErrorKind::UnsupportedCharset {
                    charset: "SHIFTJIS".to_string()
                }
            )
        );
        assert_eq!(
            err("info face=\"F\" size=8 unicode=0 charset=\"32-126\"\n"),
            (
                1,
                32,
                ErrorKind::UnsupportedCharset {
                    charset: "32-126".to_string()
                }
            )
        );
        // With `unicode=1`, ids are code points whatever the charset says.
        let line = CHAR.replace("id=65", "id=145");
        for charset in ["32-126,145", "ANSI"] {
            let text = ansi(&format!("{line}\n")).replace(
                "charset=\"ANSI\"",
                &format!("unicode=1 charset=\"{charset}\""),
            );
            let font = parse(&text, Limits::default()).unwrap();
            assert_eq!(font.glyphs[0].id, 145, "{charset}");
        }
        // Without it, 145 in ANSI is the left single quotation mark.
        let font = parse(&ansi(&format!("{line}\n")), Limits::default()).unwrap();
        assert_eq!(font.glyphs[0].id, 0x2018);
        // Errors give the ids as the file does.
        assert_eq!(
            err(&ansi(&format!("{line}\n{line}\n"))),
            (5, 1, ErrorKind::DuplicateGlyph { id: 145 })
        );
        let pair = "kerning first=145 second=150 amount=-1";
        assert_eq!(
            err(&ansi(&format!("{pair}\n{pair}\n"))),
            (
                5,
                1,
                ErrorKind::DuplicateKerning {
                    first: 145,
                    second: 150
                }
            )
        );
    }

    #[test]
    fn the_fallback_glyph_is_one() {
        let fallback = CHAR.replace("id=65", "id=-1");
        assert_eq!(
            err(&font(&format!("{fallback}\n{fallback}\n"))),
            (5, 6, ErrorKind::DuplicateFallback)
        );
        let other = CHAR.replace("id=65", "id=-2");
        assert_eq!(
            err(&font(&format!("{other}\n"))).2,
            ErrorKind::NumberOutOfRange { key: "id" }
        );
        // Its errors name it as the file does, beside a glyph of id 0.
        let zero = CHAR.replace("id=65", "id=0");
        let off = fallback.replace("x=0", "x=40");
        assert_eq!(
            err(&font(&format!("{zero}\n{off}\n"))),
            (5, 12, ErrorKind::GlyphOffPage { id: -1 })
        );
        assert_eq!(
            err(&font(&format!(
                "{}\n",
                fallback.replace("page=0", "page=3")
            ))),
            (4, 68, ErrorKind::MissingGlyphPage { id: -1 })
        );
    }

    #[test]
    fn error_kinds_explain_themselves() {
        assert_eq!(
            CodecError::BmFont {
                line: 4,
                column: 12,
                kind: ErrorKind::GlyphOffPage { id: 65 }
            }
            .to_string(),
            "BMFont error at line 4, column 12: glyph 65's rectangle runs off its page"
        );
        assert_eq!(
            ErrorKind::MissingInfo.to_string(),
            "the first line is not `info`: this is not a BMFont file in the text format"
        );
    }
}
