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

//! The pack, `OUT/game.kpak` (ADR-0041): every output the cache lists, in
//! one file of `nadir_codec::kpak`'s format, version 1, which nadir-assets'
//! `PackSource` reads.
//!
//! - The files are laid out in path order, each at a multiple of 16 after
//!   the header, a file of the same bytes as one before it stored once
//!   with it.
//! - A file is stored as KLZ when that makes it smaller, and as it is
//!   otherwise.
//! - The TOC is in order of the paths' hashes, with the string table, so
//!   that a damaged entry is named; two paths of one hash fail the cook.
//! - The pack is read back with the game's reader, and the game's limits,
//!   before it is written.

use std::collections::BTreeMap;

use nadir_codec::kpak::{self, Codec};
use nadir_codec::{CodecError, Limits, klz};
use nadir_core::hash::{crc32, fnv1a64};

use crate::{CookError, PACK};

/// Where a blob of the data region is, and how it is stored.
#[derive(Clone, Copy)]
struct Blob {
    offset: u64,
    stored_size: u32,
    raw_size: u32,
    codec: Codec,
    crc32: u32,
}

/// The pack of `files`, each a path and its bytes, in path order.
///
/// # Errors
///
/// [`CookError::OutputCollision`] for two paths of one hash;
/// [`CookError::BadPath`] for a path longer than the string table holds;
/// [`CookError::Encode`] for a file, or a TOC, larger than the game reads,
/// more files than it reads, or a pack its reader refuses.
pub(crate) fn write(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, CookError> {
    write_with(files, fnv1a64)
}

/// [`write`], with `hash` as the path hash, so that a test can make two
/// paths collide.
fn write_with(files: &[(String, Vec<u8>)], hash: fn(&[u8]) -> u64) -> Result<Vec<u8>, CookError> {
    let limits = Limits::default();
    let refused = |path: &str, offset: u64, kind: kpak::ErrorKind| CookError::Encode {
        path: path.to_string(),
        error: CodecError::Kpak { offset, kind },
    };
    let size = |bytes: usize| u32::try_from(bytes).unwrap_or(u32::MAX);
    if files.len() > limits.max_entries {
        return Err(refused(
            PACK,
            8,
            kpak::ErrorKind::TooManyEntries {
                count: size(files.len()),
                limit: limits.max_entries,
            },
        ));
    }

    let mut pack = vec![0; kpak::HEADER_SIZE];
    let mut blobs: BTreeMap<&[u8], Blob> = BTreeMap::new();
    let mut entries: Vec<(u64, &str, Blob)> = Vec::with_capacity(files.len());
    for (path, raw) in files {
        if raw.len() > limits.max_decoded_bytes {
            return Err(refused(
                path,
                0,
                kpak::ErrorKind::EntryTooLarge {
                    size: size(raw.len()),
                    limit: limits.max_decoded_bytes,
                },
            ));
        }
        let blob = match blobs.get(raw.as_slice()) {
            Some(&blob) => blob,
            None => {
                align(&mut pack);
                let stream = klz::compress(raw);
                let (codec, stored) = if stream.len() < raw.len() {
                    (Codec::Klz, &stream[..])
                } else {
                    (Codec::Stored, &raw[..])
                };
                // Both sizes are within the limit, which is below 4 GiB.
                let blob = Blob {
                    offset: pack.len() as u64,
                    stored_size: size(stored.len()),
                    raw_size: size(raw.len()),
                    codec,
                    crc32: crc32(stored),
                };
                pack.extend_from_slice(stored);
                blobs.insert(raw, blob);
                blob
            }
        };
        entries.push((hash(path.as_bytes()), path, blob));
    }
    entries.sort_by_key(|&(hash, ..)| hash);
    if let Some(pair) = entries.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(CookError::OutputCollision {
            first: pair[0].1.to_string(),
            second: pair[1].1.to_string(),
        });
    }
    align(&mut pack);

    let toc_offset = pack.len() as u64;
    for &(hash, _, blob) in &entries {
        pack.extend_from_slice(&hash.to_le_bytes());
        pack.extend_from_slice(&blob.offset.to_le_bytes());
        pack.extend_from_slice(&blob.stored_size.to_le_bytes());
        pack.extend_from_slice(&blob.raw_size.to_le_bytes());
        let codec = match blob.codec {
            Codec::Stored => 0,
            Codec::Klz => 1,
        };
        pack.extend_from_slice(&[codec, 0, 0, 0]);
        pack.extend_from_slice(&blob.crc32.to_le_bytes());
    }
    for &(_, path, _) in &entries {
        let Ok(length) = u16::try_from(path.len()) else {
            return Err(CookError::BadPath {
                path: path.to_string(),
                reason: "it is longer than a pack's 65,535 bytes for a path",
            });
        };
        pack.extend_from_slice(&length.to_le_bytes());
        pack.extend_from_slice(path.as_bytes());
    }
    let toc = &pack[toc_offset as usize..];
    if toc.len() > limits.max_decoded_bytes {
        return Err(refused(
            PACK,
            20,
            kpak::ErrorKind::TocTooLarge {
                toc_size: size(toc.len()),
                limit: limits.max_decoded_bytes,
            },
        ));
    }
    let (toc_size, toc_crc) = (size(toc.len()), crc32(toc));

    let mut header = Vec::with_capacity(kpak::HEADER_SIZE);
    header.extend_from_slice(&kpak::MAGIC);
    header.extend_from_slice(&kpak::VERSION.to_le_bytes());
    header.extend_from_slice(&kpak::NAMES.to_le_bytes());
    header.extend_from_slice(&size(entries.len()).to_le_bytes());
    header.extend_from_slice(&toc_offset.to_le_bytes());
    header.extend_from_slice(&toc_size.to_le_bytes());
    header.extend_from_slice(&toc_crc.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    pack[..kpak::HEADER_SIZE].copy_from_slice(&header);

    // What the game will read: every file, found by its path.
    let checked = kpak::parse(&pack, limits).map_err(|error| CookError::Encode {
        path: PACK.to_string(),
        error,
    })?;
    for (path, raw) in files {
        let index = checked
            .find(path)
            .expect("the reader finds each path of a pack it reads, which hashed apart");
        let bytes = checked
            .read(&pack, index, limits)
            .map_err(|error| CookError::Encode {
                path: PACK.to_string(),
                error,
            })?;
        debug_assert!(bytes == *raw, "`{path}` reads back as other bytes");
    }
    Ok(pack)
}

/// Pads `pack` with zeros to a multiple of 16.
fn align(pack: &mut Vec<u8>) {
    let padded = pack.len().next_multiple_of(kpak::ALIGN as usize);
    pack.resize(padded, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
        list.iter()
            .map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
            .collect()
    }

    #[test]
    fn identical_files_are_stored_once_and_klz_only_where_it_is_smaller() {
        let run = vec![7; 1000];
        let files = files(&[
            ("data/a.kbj", b"abc"),
            ("data/b.kbj", &run),
            ("data/c.kbj", b"abc"),
            ("data/d.kbj", &run),
            ("data/empty.kbj", b""),
            ("data/other.kbj", b""),
        ]);
        let pack = write(&files).unwrap();
        let toc = kpak::parse(&pack, Limits::default()).unwrap();
        assert_eq!(toc.len(), 6);
        let entry = |path: &str| toc.entries()[toc.find(path).unwrap()];
        assert_eq!(entry("data/a.kbj").offset, entry("data/c.kbj").offset);
        assert_eq!(entry("data/b.kbj").offset, entry("data/d.kbj").offset);
        assert_eq!(entry("data/a.kbj").codec, Codec::Stored);
        assert_eq!(entry("data/b.kbj").codec, Codec::Klz);
        assert!(entry("data/b.kbj").stored_size < 100);
        assert_eq!(
            entry("data/empty.kbj").offset,
            entry("data/other.kbj").offset
        );
        for (path, bytes) in &files {
            let index = toc.find(path).unwrap();
            assert_eq!(toc.name(index), Some(path.as_str()));
            assert_eq!(&toc.read(&pack, index, Limits::default()).unwrap(), bytes);
            assert_eq!(toc.entries()[index].offset % kpak::ALIGN, 0);
        }
        // In path order: `abc` first, after the header, then the run.
        assert_eq!(entry("data/a.kbj").offset, kpak::HEADER_SIZE as u64);
        assert!(entry("data/b.kbj").offset > entry("data/a.kbj").offset);
    }

    #[test]
    fn the_same_files_make_the_same_pack() {
        let files = files(&[("atlas/a.qoi", b"qoif"), ("levels/b.klvl", b"KBJ1")]);
        assert_eq!(write(&files).unwrap(), write(&files).unwrap());
        let empty = write(&[]).unwrap();
        assert_eq!(empty.len(), kpak::HEADER_SIZE);
        assert!(kpak::parse(&empty, Limits::default()).unwrap().is_empty());
    }

    #[test]
    fn two_paths_of_one_hash_fail_the_cook() {
        let files = files(&[("data/a.kbj", b"a"), ("data/b.kbj", b"b")]);
        let error = write_with(&files, |_| 7).unwrap_err();
        assert_eq!(
            error,
            CookError::OutputCollision {
                first: "data/a.kbj".to_string(),
                second: "data/b.kbj".to_string(),
            }
        );
        assert_eq!(
            error.to_string(),
            "`data/a.kbj` and `data/b.kbj` have one path hash, so would be one entry of the pack: rename one"
        );
    }

    #[test]
    fn a_path_too_long_for_the_string_table_is_refused() {
        let long = format!("data/{}.kbj", "a".repeat(65_535));
        let files = vec![(long.clone(), b"a".to_vec())];
        assert!(matches!(
            write(&files),
            Err(CookError::BadPath { path, .. }) if path == long
        ));
    }
}
