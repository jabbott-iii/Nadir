// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The pack file, `game.kpak`, version 1: the cooked files of a game in one
//! file, each found by its path, KLZ-compressed where that makes it smaller
//! (ADR-0041). The cook writes it (roadmap M5.14), and the game reads it
//! through nadir-assets' `PackSource`.
//!
//! This module reads a pack and does not open files: [`header`] reads its
//! first 32 bytes, [`toc`] its table of contents (TOC), and
//! [`Toc::decode`] an entry's bytes, so that a reader takes from a file
//! only the parts it needs. [`parse`] and [`Toc::read`] do the same for a
//! pack held in memory.
//!
//! ```text
//! header (32 bytes):
//!   "KPAK" | version u16 (1) | flags u16 | entry_count u32 | toc_offset u64
//!   | toc_size u32 | toc_crc32 u32 | reserved u32 (0)
//! data: each entry's stored bytes, at a multiple of 16; identical blobs once
//! toc (toc_size bytes at toc_offset, to the end of the file):
//!   entry_count entries of 32 bytes, in ascending order of path_hash:
//!     path_hash u64 | offset u64 | stored_size u32 | raw_size u32
//!     | codec u8 (0 stored, 1 KLZ) | reserved [u8; 3] (0) | crc32 u32
//!   then, if flags bit 0 is set, the string table: for each entry in TOC
//!   order, its path's length u16 and its path's bytes
//! ```
//!
//! Numbers are little-endian. `path_hash` is the 64-bit FNV-1a of the
//! entry's path ([`nadir_core::hash::fnv1a64`]), `toc_crc32` the CRC-32 of
//! the TOC's `toc_size` bytes, and an entry's `crc32` the CRC-32 of its
//! stored bytes ([`nadir_core::hash::crc32`]). A path is the file's asset
//! path, such as `atlas/sprites_0.qoi`; the format checks only that it is
//! UTF-8 and not empty.
//!
//! The checks, each in the order given, the first that fails the error:
//!
//! - **Checked by [`header`]:** the magic, then the version (any other is
//!   not supported), the flags (bit 0 only), the reserved field; at most
//!   [`Limits::max_entries`] entries; a TOC that starts at a multiple of 16
//!   after the header, has room for its entries (exactly, with no string
//!   table), is at most [`Limits::max_decoded_bytes`], and ends where the
//!   file does.
//! - **Checked by [`toc`]:** the TOC's size, the header's, and its CRC-32;
//!   at most [`Limits::max_entries`] entries; then each entry in turn: its
//!   codec, its reserved bytes, its stored bytes between the header and the
//!   TOC at a multiple of 16, both its sizes at most
//!   [`Limits::max_decoded_bytes`], a stored entry's two sizes equal, and
//!   its path hash greater than the one before it; then two entries sharing
//!   bytes only when they are one blob (the same offset, sizes, codec and
//!   CRC); then each path of the string table UTF-8, not empty, and of its
//!   entry's hash; and nothing after the last.
//! - **Checked by [`Toc::decode`]:** the entry's CRC-32, before its bytes
//!   are decompressed, so that a damaged entry is [`ErrorKind::Damaged`],
//!   naming its path when the pack has a string table; then its KLZ stream.
//!
//! Reading an entry from a file:
//!
//! ```
//! use std::fs::File;
//! use std::io::{Read, Seek, SeekFrom};
//!
//! use nadir_codec::{Limits, kpak};
//!
//! let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/kpak/two.kpak");
//! let mut file = File::open(path)?;
//! let size = file.metadata()?.len();
//! let mut start = [0; kpak::HEADER_SIZE];
//! file.read_exact(&mut start)?;
//! let header = kpak::header(&start, size, Limits::default())?;
//!
//! // The header checked the TOC's size against the limits.
//! let mut bytes = vec![0; header.toc_size as usize];
//! file.seek(SeekFrom::Start(header.toc_offset))?;
//! file.read_exact(&mut bytes)?;
//! let toc = kpak::toc(&header, &bytes, Limits::default())?;
//!
//! let index = toc.find("notes/greeting.txt").expect("a file of the pack");
//! let entry = toc.entries()[index];
//! let mut stored = vec![0; entry.stored_size as usize];
//! file.seek(SeekFrom::Start(entry.offset))?;
//! file.read_exact(&mut stored)?;
//! assert_eq!(toc.decode(index, &stored, Limits::default())?, b"hello, hello, hello");
//! assert_eq!(toc.find("notes/missing.txt"), None);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::fmt;

use nadir_core::hash::{crc32, fnv1a64};

use crate::{CodecError, Limits, klz};

/// The first four bytes of a pack.
pub const MAGIC: [u8; 4] = *b"KPAK";

/// The version this module reads.
pub const VERSION: u16 = 1;

/// The size of the header, in bytes.
pub const HEADER_SIZE: usize = 32;

/// The size of one TOC entry, in bytes.
pub const ENTRY_SIZE: usize = 32;

/// What an entry's offset, and the TOC's, are a multiple of.
pub const ALIGN: u64 = 16;

/// The flag that says a string table follows the TOC's entries.
pub const NAMES: u16 = 1;

/// A pack's header, read with [`header`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Header {
    /// How many entries the TOC has.
    pub entry_count: u32,
    /// Where the TOC starts, in bytes from the start of the file.
    pub toc_offset: u64,
    /// How long the TOC is, string table included: it ends where the file
    /// does.
    pub toc_size: u32,
    /// The CRC-32 of the TOC.
    pub toc_crc32: u32,
    /// Whether the TOC has a string table, with each entry's path.
    pub names: bool,
}

/// How an entry's bytes are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Codec {
    /// As they are (0).
    Stored,
    /// As a KLZ stream (1, [`klz`]).
    Klz,
}

/// One entry of a TOC: a file of the pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Entry {
    /// The 64-bit FNV-1a of its path.
    pub path_hash: u64,
    /// Where its stored bytes start, in bytes from the start of the file.
    pub offset: u64,
    /// How many bytes it is stored in.
    pub stored_size: u32,
    /// How many bytes it is.
    pub raw_size: u32,
    /// How it is stored.
    pub codec: Codec,
    /// The CRC-32 of its stored bytes.
    pub crc32: u32,
}

impl Entry {
    /// Whether `other` is the same blob: the same bytes, stored alike.
    fn same_blob(&self, other: &Entry) -> bool {
        self.offset == other.offset
            && self.stored_size == other.stored_size
            && self.raw_size == other.raw_size
            && self.codec == other.codec
            && self.crc32 == other.crc32
    }
}

/// A pack's TOC, read with [`toc`] or [`parse`]: its entries, and their
/// paths if it has a string table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toc {
    entries: Vec<Entry>,
    names: Option<Vec<String>>,
}

impl Toc {
    /// Its entries, in ascending order of their path hashes.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// How many entries it has.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether it has a string table, with each entry's path.
    pub fn has_names(&self) -> bool {
        self.names.is_some()
    }

    /// The path of the entry `index`, if the pack has a string table and
    /// such an entry.
    pub fn name(&self, index: usize) -> Option<&str> {
        self.names.as_ref()?.get(index).map(String::as_str)
    }

    /// The index of the entry of `path`, found by a binary search of the
    /// path hashes. With a string table, an entry whose path is another,
    /// of the same hash, is not `path`'s; without one, the hash decides.
    pub fn find(&self, path: &str) -> Option<usize> {
        let hash = fnv1a64(path.as_bytes());
        let index = self
            .entries
            .binary_search_by_key(&hash, |entry| entry.path_hash)
            .ok()?;
        match &self.names {
            Some(names) if names.get(index).map(String::as_str) != Some(path) => None,
            _ => Some(index),
        }
    }

    /// The bytes of the entry `index`, from its stored bytes, `stored`,
    /// which a reader takes from the pack: [`Entry::stored_size`] bytes at
    /// [`Entry::offset`].
    ///
    /// # Errors
    ///
    /// A [`CodecError::Kpak`]: [`ErrorKind::NoEntry`] if there is no entry
    /// `index`, [`ErrorKind::StoredLength`] if `stored` is not its stored
    /// size, [`ErrorKind::Damaged`] if its CRC-32 is not the TOC's,
    /// [`ErrorKind::EntryTooLarge`] if it is more than
    /// [`Limits::max_decoded_bytes`], or [`ErrorKind::BadStream`] if its
    /// KLZ stream cannot be read.
    pub fn decode(
        &self,
        index: usize,
        stored: &[u8],
        limits: Limits,
    ) -> Result<Vec<u8>, CodecError> {
        let Some(entry) = self.entries.get(index) else {
            return Err(error(0, ErrorKind::NoEntry { index }));
        };
        let at = entry.offset;
        if stored.len() != entry.stored_size as usize {
            return Err(error(
                at,
                ErrorKind::StoredLength {
                    expected: entry.stored_size,
                    found: stored.len(),
                },
            ));
        }
        let found = crc32(stored);
        if found != entry.crc32 {
            return Err(error(
                at,
                ErrorKind::Damaged {
                    name: self.name(index).map(str::to_string),
                    index,
                    expected: entry.crc32,
                    found,
                },
            ));
        }
        if entry.raw_size as usize > limits.max_decoded_bytes {
            return Err(error(
                at,
                ErrorKind::EntryTooLarge {
                    size: entry.raw_size,
                    limit: limits.max_decoded_bytes,
                },
            ));
        }
        match entry.codec {
            Codec::Stored => Ok(stored.to_vec()),
            Codec::Klz => {
                klz::decompress(stored, entry.raw_size as usize, limits).map_err(|klz_error| {
                    match klz_error {
                        CodecError::Klz { offset, kind } => error(
                            at.saturating_add(offset as u64),
                            ErrorKind::BadStream {
                                name: self.name(index).map(str::to_string),
                                index,
                                kind,
                            },
                        ),
                        other => other,
                    }
                })
            }
        }
    }

    /// The bytes of the entry `index` of `pack`, a whole pack in memory,
    /// the one this TOC was read from: [`Toc::decode`] of its stored bytes.
    ///
    /// # Errors
    ///
    /// Those of [`Toc::decode`], and [`ErrorKind::UnexpectedEnd`] if
    /// `pack` ends before the entry's stored bytes do.
    pub fn read(&self, pack: &[u8], index: usize, limits: Limits) -> Result<Vec<u8>, CodecError> {
        let Some(entry) = self.entries.get(index) else {
            return Err(error(0, ErrorKind::NoEntry { index }));
        };
        let stored = usize::try_from(entry.offset).ok().and_then(|start| {
            let end = start.checked_add(entry.stored_size as usize)?;
            pack.get(start..end)
        });
        let Some(stored) = stored else {
            return Err(error(
                pack.len() as u64,
                ErrorKind::UnexpectedEnd {
                    expected: "an entry's bytes",
                },
            ));
        };
        self.decode(index, stored, limits)
    }
}

/// What is wrong with a pack. See [`CodecError::Kpak`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The input ends too soon.
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// The file does not start with [`MAGIC`].
    NotKpak,
    /// The version is not [`VERSION`].
    Version {
        /// The version the file gives.
        found: u16,
    },
    /// The flags set a bit other than [`NAMES`].
    UnknownFlags {
        /// The flags.
        flags: u16,
    },
    /// A reserved field, of the header or of an entry, is not 0.
    Reserved,
    /// There are more entries than [`Limits::max_entries`].
    TooManyEntries {
        /// How many the header says.
        count: u32,
        /// The limit.
        limit: usize,
    },
    /// The TOC starts inside the header, or not at a multiple of 16.
    TocOffset {
        /// Where it starts.
        toc_offset: u64,
    },
    /// The TOC is too small for its entries, or, with no string table,
    /// larger than they are.
    TocSize {
        /// Its size.
        toc_size: u32,
        /// What its entries need: 32 bytes each, and 2 more each for their
        /// paths' lengths with a string table.
        needed: u64,
    },
    /// The TOC is larger than [`Limits::max_decoded_bytes`].
    TocTooLarge {
        /// Its size.
        toc_size: u32,
        /// The limit.
        limit: usize,
    },
    /// The TOC does not end where the file does.
    TocPlace {
        /// The file's size.
        file_size: u64,
    },
    /// The TOC given to [`toc`] is not the size the header says.
    TocLength {
        /// The size the header says.
        expected: u32,
        /// The bytes given.
        found: usize,
    },
    /// The TOC's CRC-32 is not the header's.
    TocChecksum {
        /// The header's.
        expected: u32,
        /// The TOC's.
        found: u32,
    },
    /// An entry's path hash is not greater than the one before it.
    OutOfOrder,
    /// An entry's codec is not 0 or 1.
    UnknownCodec {
        /// The codec.
        codec: u8,
    },
    /// An entry's stored bytes are not all between the header and the TOC.
    EntryOutside {
        /// Where they start.
        offset: u64,
        /// How many there are.
        stored_size: u32,
    },
    /// An entry does not start at a multiple of 16.
    Misaligned {
        /// Where it starts.
        offset: u64,
    },
    /// A stored entry's raw size is not its stored size.
    SizeMismatch {
        /// Its stored size.
        stored_size: u32,
        /// Its raw size.
        raw_size: u32,
    },
    /// An entry is larger than [`Limits::max_decoded_bytes`], stored or
    /// raw.
    EntryTooLarge {
        /// Its size.
        size: u32,
        /// The limit.
        limit: usize,
    },
    /// An entry shares bytes with another that is not the same blob.
    Overlap,
    /// A path of the string table is empty, or not UTF-8.
    BadName,
    /// A path of the string table does not hash to its entry's path hash.
    NameHash {
        /// The path.
        name: String,
    },
    /// Bytes follow the TOC's last entry, or the string table's last path.
    TrailingBytes,
    /// [`Toc::decode`] or [`Toc::read`] was asked for an entry the TOC does
    /// not have.
    NoEntry {
        /// The index asked for.
        index: usize,
    },
    /// The stored bytes given to [`Toc::decode`] are not the entry's size.
    StoredLength {
        /// The entry's stored size.
        expected: u32,
        /// The bytes given.
        found: usize,
    },
    /// An entry's CRC-32 is not the TOC's: its bytes are damaged.
    Damaged {
        /// Its path, if the pack has a string table.
        name: Option<String>,
        /// Its index in the TOC.
        index: usize,
        /// The TOC's CRC-32.
        expected: u32,
        /// Its bytes' CRC-32.
        found: u32,
    },
    /// An entry's KLZ stream cannot be read, though its CRC-32 is right:
    /// what wrote the pack is wrong.
    BadStream {
        /// Its path, if the pack has a string table.
        name: Option<String>,
        /// Its index in the TOC.
        index: usize,
        /// What is wrong with the stream.
        kind: klz::ErrorKind,
    },
}

/// An entry, by its path, or by its index without one.
struct Named<'a>(Option<&'a str>, usize);

impl fmt::Display for Named<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(name) => write!(f, "the entry `{name}`"),
            None => write!(f, "entry {}", self.1),
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd { expected } => {
                write!(f, "the input ends where {expected} should be")
            }
            Self::NotKpak => f.write_str("the file does not start with `KPAK`"),
            Self::Version { found } => {
                write!(f, "the version is {found}, not {VERSION}: it is not supported")
            }
            Self::UnknownFlags { flags } => {
                write!(f, "the flags {flags:#06x} set bits that version 1 does not have")
            }
            Self::Reserved => f.write_str("a reserved field is not 0"),
            Self::TooManyEntries { count, limit } => write!(
                f,
                "the pack has {count} entries, more than the limit of {limit}"
            ),
            Self::TocOffset { toc_offset } => write!(
                f,
                "the TOC starts at byte {toc_offset}, not at a multiple of 16 after the header"
            ),
            Self::TocSize { toc_size, needed } => write!(
                f,
                "the TOC is {toc_size} bytes, but its entries need {needed}"
            ),
            Self::TocTooLarge { toc_size, limit } => write!(
                f,
                "the TOC is {toc_size} bytes, more than the limit of {limit}"
            ),
            Self::TocPlace { file_size } => write!(
                f,
                "the TOC does not end where the file does, at byte {file_size}"
            ),
            Self::TocLength { expected, found } => write!(
                f,
                "the TOC given is {found} bytes, not the {expected} the header says"
            ),
            Self::TocChecksum { expected, found } => write!(
                f,
                "the TOC's CRC-32 is {found:08x}, not the {expected:08x} the header says"
            ),
            Self::OutOfOrder => f.write_str(
                "an entry's path hash is not greater than the one before it: the entries are out of order, or one is there twice",
            ),
            Self::UnknownCodec { codec } => {
                write!(f, "the codec is {codec}, not 0 (stored) or 1 (KLZ)")
            }
            Self::EntryOutside {
                offset,
                stored_size,
            } => write!(
                f,
                "an entry's {stored_size} bytes at byte {offset} are not all between the header and the TOC"
            ),
            Self::Misaligned { offset } => {
                write!(f, "an entry starts at byte {offset}, not at a multiple of 16")
            }
            Self::SizeMismatch {
                stored_size,
                raw_size,
            } => write!(
                f,
                "a stored entry is {stored_size} bytes, but its raw size is {raw_size}"
            ),
            Self::EntryTooLarge { size, limit } => write!(
                f,
                "an entry of {size} bytes is more than the limit of {limit}"
            ),
            Self::Overlap => {
                f.write_str("an entry shares bytes with another that is not the same blob")
            }
            Self::BadName => f.write_str("a path of the string table is empty, or not UTF-8"),
            Self::NameHash { name } => write!(
                f,
                "the path `{name}` does not hash to its entry's path hash"
            ),
            Self::TrailingBytes => f.write_str("bytes follow the TOC's last entry or path"),
            Self::NoEntry { index } => write!(f, "the pack has no entry {index}"),
            Self::StoredLength { expected, found } => write!(
                f,
                "the entry's bytes given are {found}, not the {expected} it is stored in"
            ),
            Self::Damaged {
                name,
                index,
                expected,
                found,
            } => write!(
                f,
                "{} is damaged: its CRC-32 is {found:08x}, not the TOC's {expected:08x}",
                Named(name.as_deref(), *index)
            ),
            Self::BadStream { name, index, kind } => {
                write!(f, "{}'s KLZ stream: {kind}", Named(name.as_deref(), *index))
            }
        }
    }
}

fn error(offset: u64, kind: ErrorKind) -> CodecError {
    CodecError::Kpak { offset, kind }
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    let mut word = [0; 2];
    word.copy_from_slice(&bytes[at..at + 2]);
    u16::from_le_bytes(word)
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    let mut word = [0; 4];
    word.copy_from_slice(&bytes[at..at + 4]);
    u32::from_le_bytes(word)
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut word = [0; 8];
    word.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(word)
}

/// Reads a pack's header from `start`, its first bytes (32 or more), given
/// the size of the whole file, `file_size`.
///
/// # Errors
///
/// A [`CodecError::Kpak`] for the first check of the module's that fails:
/// the offset is the field's in the header.
pub fn header(start: &[u8], file_size: u64, limits: Limits) -> Result<Header, CodecError> {
    let Some(start) = start.get(..HEADER_SIZE) else {
        return Err(error(
            start.len() as u64,
            ErrorKind::UnexpectedEnd {
                expected: "the header",
            },
        ));
    };
    if start[..4] != MAGIC {
        return Err(error(0, ErrorKind::NotKpak));
    }
    let version = u16_at(start, 4);
    if version != VERSION {
        return Err(error(4, ErrorKind::Version { found: version }));
    }
    let flags = u16_at(start, 6);
    if flags & !NAMES != 0 {
        return Err(error(6, ErrorKind::UnknownFlags { flags }));
    }
    if u32_at(start, 28) != 0 {
        return Err(error(28, ErrorKind::Reserved));
    }
    let header = Header {
        entry_count: u32_at(start, 8),
        toc_offset: u64_at(start, 12),
        toc_size: u32_at(start, 20),
        toc_crc32: u32_at(start, 24),
        names: flags & NAMES != 0,
    };
    if header.entry_count as usize > limits.max_entries {
        return Err(error(
            8,
            ErrorKind::TooManyEntries {
                count: header.entry_count,
                limit: limits.max_entries,
            },
        ));
    }
    if header.toc_offset < HEADER_SIZE as u64 || !header.toc_offset.is_multiple_of(ALIGN) {
        return Err(error(
            12,
            ErrorKind::TocOffset {
                toc_offset: header.toc_offset,
            },
        ));
    }
    let per_entry = ENTRY_SIZE as u64 + if header.names { 2 } else { 0 };
    let needed = u64::from(header.entry_count) * per_entry;
    let toc_size = u64::from(header.toc_size);
    if toc_size < needed || (!header.names && toc_size != needed) {
        return Err(error(
            20,
            ErrorKind::TocSize {
                toc_size: header.toc_size,
                needed,
            },
        ));
    }
    if header.toc_size as usize > limits.max_decoded_bytes {
        return Err(error(
            20,
            ErrorKind::TocTooLarge {
                toc_size: header.toc_size,
                limit: limits.max_decoded_bytes,
            },
        ));
    }
    if header.toc_offset.checked_add(toc_size) != Some(file_size) {
        return Err(error(12, ErrorKind::TocPlace { file_size }));
    }
    Ok(header)
}

/// Reads a pack's TOC from `bytes`, the [`Header::toc_size`] bytes at
/// [`Header::toc_offset`], as [`header`] read it.
///
/// # Errors
///
/// A [`CodecError::Kpak`] for the first check of the module's that fails,
/// at its byte in the file.
pub fn toc(header: &Header, bytes: &[u8], limits: Limits) -> Result<Toc, CodecError> {
    let base = header.toc_offset;
    let at = |offset: usize| base.saturating_add(offset as u64);
    if bytes.len() != header.toc_size as usize {
        return Err(error(
            base,
            ErrorKind::TocLength {
                expected: header.toc_size,
                found: bytes.len(),
            },
        ));
    }
    let found = crc32(bytes);
    if found != header.toc_crc32 {
        return Err(error(
            base,
            ErrorKind::TocChecksum {
                expected: header.toc_crc32,
                found,
            },
        ));
    }
    let count = header.entry_count as usize;
    if count > limits.max_entries {
        return Err(error(
            8,
            ErrorKind::TooManyEntries {
                count: header.entry_count,
                limit: limits.max_entries,
            },
        ));
    }
    let Some(table) = count
        .checked_mul(ENTRY_SIZE)
        .and_then(|size| bytes.get(..size))
    else {
        return Err(error(
            at(bytes.len()),
            ErrorKind::UnexpectedEnd {
                expected: "a TOC entry",
            },
        ));
    };

    let mut entries: Vec<Entry> = Vec::with_capacity(count);
    for (index, raw) in table.as_chunks::<ENTRY_SIZE>().0.iter().enumerate() {
        let here = index * ENTRY_SIZE;
        let entry = read_entry(raw, header, limits)
            .map_err(|(field, kind)| error(at(here + field), kind))?;
        if let Some(last) = entries.last()
            && entry.path_hash <= last.path_hash
        {
            return Err(error(at(here), ErrorKind::OutOfOrder));
        }
        entries.push(entry);
    }
    check_overlaps(&entries)
        .map_err(|index| error(at(index * ENTRY_SIZE + 8), ErrorKind::Overlap))?;

    let mut rest = &bytes[table.len()..];
    let names = if header.names {
        let mut names = Vec::with_capacity(count);
        for entry in &entries {
            let here = bytes.len() - rest.len();
            let Some((length, after)) = rest.split_first_chunk::<2>() else {
                return Err(error(
                    at(bytes.len()),
                    ErrorKind::UnexpectedEnd {
                        expected: "a path's length",
                    },
                ));
            };
            let length = usize::from(u16::from_le_bytes(*length));
            let Some(name) = after.get(..length) else {
                return Err(error(
                    at(bytes.len()),
                    ErrorKind::UnexpectedEnd { expected: "a path" },
                ));
            };
            let Ok(name) = std::str::from_utf8(name) else {
                return Err(error(at(here + 2), ErrorKind::BadName));
            };
            if name.is_empty() {
                return Err(error(at(here), ErrorKind::BadName));
            }
            if fnv1a64(name.as_bytes()) != entry.path_hash {
                return Err(error(
                    at(here),
                    ErrorKind::NameHash {
                        name: name.to_string(),
                    },
                ));
            }
            names.push(name.to_string());
            rest = &after[length..];
        }
        Some(names)
    } else {
        None
    };
    if !rest.is_empty() {
        return Err(error(
            at(bytes.len() - rest.len()),
            ErrorKind::TrailingBytes,
        ));
    }
    Ok(Toc { entries, names })
}

/// Reads and checks the TOC entry `raw`, or gives the offset of the field
/// that is wrong, in the entry, and what is wrong.
fn read_entry(raw: &[u8], header: &Header, limits: Limits) -> Result<Entry, (usize, ErrorKind)> {
    let codec = match raw[24] {
        0 => Codec::Stored,
        1 => Codec::Klz,
        codec => return Err((24, ErrorKind::UnknownCodec { codec })),
    };
    if raw[25..28] != [0; 3] {
        return Err((25, ErrorKind::Reserved));
    }
    let entry = Entry {
        path_hash: u64_at(raw, 0),
        offset: u64_at(raw, 8),
        stored_size: u32_at(raw, 16),
        raw_size: u32_at(raw, 20),
        codec,
        crc32: u32_at(raw, 28),
    };
    let inside = entry.offset >= HEADER_SIZE as u64
        && entry
            .offset
            .checked_add(u64::from(entry.stored_size))
            .is_some_and(|end| end <= header.toc_offset);
    if !inside {
        return Err((
            8,
            ErrorKind::EntryOutside {
                offset: entry.offset,
                stored_size: entry.stored_size,
            },
        ));
    }
    if !entry.offset.is_multiple_of(ALIGN) {
        return Err((
            8,
            ErrorKind::Misaligned {
                offset: entry.offset,
            },
        ));
    }
    for (field, size) in [(16, entry.stored_size), (20, entry.raw_size)] {
        if size as usize > limits.max_decoded_bytes {
            return Err((
                field,
                ErrorKind::EntryTooLarge {
                    size,
                    limit: limits.max_decoded_bytes,
                },
            ));
        }
    }
    if codec == Codec::Stored && entry.stored_size != entry.raw_size {
        return Err((
            20,
            ErrorKind::SizeMismatch {
                stored_size: entry.stored_size,
                raw_size: entry.raw_size,
            },
        ));
    }
    Ok(entry)
}

/// Checks that entries share bytes only when they are one blob, or gives
/// the index of an entry that shares bytes with another blob.
fn check_overlaps(entries: &[Entry]) -> Result<(), usize> {
    let mut order: Vec<usize> = (0..entries.len())
        .filter(|&index| entries[index].stored_size > 0)
        .collect();
    order.sort_by_key(|&index| (entries[index].offset, entries[index].stored_size, index));
    // The entry, of those so far, whose bytes reach furthest. One that
    // starts before its end must be the same blob, which ends there too;
    // one that starts after it reaches further than all before it. Each
    // entry's end was checked to be inside the file.
    let mut cover: Option<&Entry> = None;
    for index in order {
        let entry = &entries[index];
        if let Some(last) = cover
            && entry.offset < last.offset + u64::from(last.stored_size)
        {
            if !entry.same_blob(last) {
                return Err(index);
            }
            continue;
        }
        cover = Some(entry);
    }
    Ok(())
}

/// Reads the header and the TOC of `pack`, a whole pack in memory: its
/// entries are then [`Toc::read`] from it.
///
/// # Errors
///
/// Those of [`header`] and [`toc`].
pub fn parse(pack: &[u8], limits: Limits) -> Result<Toc, CodecError> {
    let header = header(pack, pack.len() as u64, limits)?;
    // The header checked that the TOC ends where the pack does.
    let start = usize::try_from(header.toc_offset).unwrap_or(usize::MAX);
    let bytes = pack.get(start..).unwrap_or_default();
    toc(&header, bytes, limits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> Entry {
        Entry {
            path_hash: fnv1a64(path.as_bytes()),
            offset: 32,
            stored_size: 0,
            raw_size: 0,
            codec: Codec::Stored,
            crc32: 0,
        }
    }

    #[test]
    fn find_takes_an_entry_by_its_hash_and_its_name() {
        let mut entries = vec![entry("b.txt"), entry("a.txt"), entry("c.txt")];
        entries.sort_by_key(|entry| entry.path_hash);
        let names: Vec<String> = entries
            .iter()
            .map(|entry| {
                ["a.txt", "b.txt", "c.txt"]
                    .into_iter()
                    .find(|path| fnv1a64(path.as_bytes()) == entry.path_hash)
                    .expect("a path")
                    .to_string()
            })
            .collect();
        let toc = Toc {
            entries: entries.clone(),
            names: Some(names.clone()),
        };
        for (index, name) in names.iter().enumerate() {
            assert_eq!(toc.find(name), Some(index));
        }
        assert_eq!(toc.find("d.txt"), None);

        // A path of another entry's hash, which a string table tells apart
        // and a pack without one cannot.
        let mut collided = names;
        collided[0] = "z.txt".to_string();
        let toc = Toc {
            entries: entries.clone(),
            names: Some(collided),
        };
        let first = toc.entries()[0].path_hash;
        let path = ["a.txt", "b.txt", "c.txt"]
            .into_iter()
            .find(|path| fnv1a64(path.as_bytes()) == first)
            .expect("a path");
        assert_eq!(toc.find(path), None);
        let toc = Toc {
            entries,
            names: None,
        };
        assert_eq!(toc.find(path), Some(0));
    }

    #[test]
    fn errors_say_where_and_what() {
        let error = CodecError::Kpak {
            offset: 4,
            kind: ErrorKind::Version { found: 2 },
        };
        assert_eq!(
            error.to_string(),
            "pack error at byte 4: the version is 2, not 1: it is not supported"
        );
        let kinds = [
            ErrorKind::UnexpectedEnd {
                expected: "the header",
            },
            ErrorKind::NotKpak,
            ErrorKind::UnknownFlags { flags: 2 },
            ErrorKind::Reserved,
            ErrorKind::TooManyEntries { count: 3, limit: 2 },
            ErrorKind::TocOffset { toc_offset: 8 },
            ErrorKind::TocSize {
                toc_size: 31,
                needed: 32,
            },
            ErrorKind::TocTooLarge {
                toc_size: 99,
                limit: 98,
            },
            ErrorKind::TocPlace { file_size: 100 },
            ErrorKind::TocLength {
                expected: 99,
                found: 98,
            },
            ErrorKind::TocChecksum {
                expected: 1,
                found: 2,
            },
            ErrorKind::OutOfOrder,
            ErrorKind::UnknownCodec { codec: 2 },
            ErrorKind::EntryOutside {
                offset: 16,
                stored_size: 3,
            },
            ErrorKind::Misaligned { offset: 40 },
            ErrorKind::SizeMismatch {
                stored_size: 3,
                raw_size: 4,
            },
            ErrorKind::EntryTooLarge { size: 5, limit: 4 },
            ErrorKind::Overlap,
            ErrorKind::BadName,
            ErrorKind::NameHash {
                name: "a.txt".to_string(),
            },
            ErrorKind::TrailingBytes,
            ErrorKind::NoEntry { index: 7 },
            ErrorKind::StoredLength {
                expected: 3,
                found: 2,
            },
            ErrorKind::Damaged {
                name: None,
                index: 1,
                expected: 1,
                found: 2,
            },
            ErrorKind::BadStream {
                name: Some("a.txt".to_string()),
                index: 0,
                kind: klz::ErrorKind::ZeroOffset,
            },
        ];
        let texts: Vec<String> = kinds.iter().map(ToString::to_string).collect();
        for (kind, text) in kinds.iter().zip(&texts) {
            assert!(!text.is_empty(), "{kind:?}");
        }
        assert_eq!(
            texts[23],
            "entry 1 is damaged: its CRC-32 is 00000002, not the TOC's 00000001"
        );
        assert_eq!(
            texts[24],
            "the entry `a.txt`'s KLZ stream: a match has offset 0"
        );
        assert_eq!(
            texts[2],
            "the flags 0x0002 set bits that version 1 does not have"
        );
    }
}
