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

//! Where assets are read from.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use nadir_codec::{Limits, kpak};

use crate::{AssetError, AssetPath};

/// Where an [`AssetServer`](crate::AssetServer) reads its files: loose
/// files under a directory ([`LooseSource`]), or a pack file
/// ([`PackSource`]).
///
/// The server's workers call it, from several threads at once.
pub trait Source: Send + Sync {
    /// The bytes of the file at `path`.
    ///
    /// # Errors
    ///
    /// [`AssetError::NotFound`] if the source has no such file,
    /// [`AssetError::Read`] if it cannot be read, and
    /// [`AssetError::Decode`] if its bytes are damaged, as a pack's entry
    /// can be ([`PackSource`]).
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError>;

    /// When the file at `path` last changed, if the source can tell, for
    /// hot reload: the server's watcher asks it
    /// ([`AssetServer::start_watcher`](crate::AssetServer::start_watcher)).
    /// Loose files can; a pack cannot. `None` by default.
    fn modified(&self, path: &AssetPath) -> Option<SystemTime> {
        let _ = path;
        None
    }
}

/// Reads loose files under a root directory: the cooked assets in
/// `target/cooked/` during development, say.
///
/// An asset path is lowercase, so on a system whose file names keep their
/// case (Linux), only a lowercase file name is found.
#[derive(Clone, Debug)]
pub struct LooseSource {
    root: PathBuf,
}

impl LooseSource {
    /// A source of the files under `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory it reads under.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// [`AssetError::NotFound`] for a file that is not there, and
/// [`AssetError::Read`] for any other failure.
fn read_error(error: io::Error) -> AssetError {
    match error.kind() {
        io::ErrorKind::NotFound => AssetError::NotFound,
        kind => AssetError::Read(kind),
    }
}

impl Source for LooseSource {
    /// Reads `root/path`. A directory there is [`AssetError::Read`].
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
        fs::read(self.root.join(path.as_str())).map_err(read_error)
    }

    fn modified(&self, path: &AssetPath) -> Option<SystemTime> {
        fs::metadata(self.root.join(path.as_str()))
            .and_then(|metadata| metadata.modified())
            .ok()
    }
}

/// Reads a pack file, `game.kpak`, as the cook writes it (ADR-0041): each
/// file found by a binary search of the pack's table of contents
/// ([`nadir_codec::kpak`]), and its bytes checked against their CRC-32
/// each time it is read, before they are decompressed.
///
/// Opening it reads the header and the table of contents, within the
/// game's [`Limits`], and keeps the file open. A read seeks to its entry
/// and reads its bytes alone: the workers take turns at the file, and
/// check and decompress what they read at once. A pack cannot say when a
/// file changed, so [`Source::modified`] is `None`.
#[derive(Debug)]
pub struct PackSource {
    path: PathBuf,
    file: Mutex<File>,
    toc: kpak::Toc,
}

impl PackSource {
    /// The pack's name among a game's cooked files, which the cook writes.
    pub const FILE_NAME: &'static str = "game.kpak";

    /// Opens the pack at `path`, and reads its header and its table of
    /// contents.
    ///
    /// # Errors
    ///
    /// [`AssetError::NotFound`] if there is no file at `path`;
    /// [`AssetError::Read`] if it cannot be read; [`AssetError::Decode`],
    /// with a [`CodecError::Kpak`](nadir_codec::CodecError::Kpak), if it is
    /// not a pack the game reads.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, AssetError> {
        let path = path.into();
        let limits = Limits::default();
        let mut file = File::open(&path).map_err(read_error)?;
        let size = file.metadata().map_err(read_error)?.len();
        // A file shorter than a header is the header's error.
        let mut start = Vec::with_capacity(kpak::HEADER_SIZE);
        (&mut file)
            .take(kpak::HEADER_SIZE as u64)
            .read_to_end(&mut start)
            .map_err(read_error)?;
        let header = kpak::header(&start, size, limits).map_err(AssetError::Decode)?;
        // The header checked the TOC's size against the limits.
        let mut bytes = vec![0; header.toc_size as usize];
        file.seek(SeekFrom::Start(header.toc_offset))
            .and_then(|_| file.read_exact(&mut bytes))
            .map_err(read_error)?;
        let toc = kpak::toc(&header, &bytes, limits).map_err(AssetError::Decode)?;
        Ok(Self {
            path,
            file: Mutex::new(file),
            toc,
        })
    }

    /// The pack's file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many files it holds.
    pub fn len(&self) -> usize {
        self.toc.len()
    }

    /// Whether it holds no files.
    pub fn is_empty(&self) -> bool {
        self.toc.is_empty()
    }
}

impl Source for PackSource {
    /// Reads the file of `path` from the pack.
    ///
    /// # Errors
    ///
    /// [`AssetError::NotFound`] if the pack has no file of `path`;
    /// [`AssetError::Read`] if the pack cannot be read, as when it was cut
    /// short since it was opened; [`AssetError::Decode`] with a
    /// [`CodecError::Kpak`](nadir_codec::CodecError::Kpak) if the file's
    /// bytes are damaged (`kpak::ErrorKind::Damaged`, which names its path),
    /// or cannot be decompressed.
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
        let index = self.toc.find(path.as_str()).ok_or(AssetError::NotFound)?;
        let entry = self.toc.entries()[index];
        // The TOC checked the stored size against the limits.
        let mut stored = vec![0; entry.stored_size as usize];
        {
            // A worker that panicked holding the file left it whole: each
            // read seeks first.
            let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
            file.seek(SeekFrom::Start(entry.offset))
                .and_then(|_| file.read_exact(&mut stored))
                .map_err(read_error)?;
        }
        self.toc
            .decode(index, &stored, Limits::default())
            .map_err(AssetError::Decode)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nadir_codec::CodecError;
    use nadir_core::hash::{crc32, fnv1a64};

    use super::*;

    fn root(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nadir-assets-source-{test}-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).expect("remove the previous run's directory");
        }
        fs::create_dir_all(dir.join("notes")).expect("create the test directory");
        dir
    }

    fn path(path: &str) -> AssetPath {
        AssetPath::new(path).expect("an asset path")
    }

    #[test]
    fn a_loose_source_reads_files_below_its_root() {
        let dir = root("read");
        fs::write(dir.join("notes/hello.txt"), "hello").expect("write a file");
        let source = LooseSource::new(&dir);
        assert_eq!(source.root(), dir);
        assert_eq!(source.read(&path("notes/hello.txt")), Ok(b"hello".to_vec()));
        assert_eq!(
            source.read(&path("notes/missing.txt")),
            Err(AssetError::NotFound)
        );
        assert!(matches!(
            source.read(&path("notes")),
            Err(AssetError::Read(_))
        ));
        fs::remove_dir_all(&dir).expect("remove the test directory");
    }

    #[test]
    fn a_loose_source_says_when_a_file_changed() {
        let dir = root("modified");
        fs::write(dir.join("notes/hello.txt"), "hello").expect("write a file");
        let source = LooseSource::new(&dir);
        let written = fs::metadata(dir.join("notes/hello.txt"))
            .and_then(|metadata| metadata.modified())
            .expect("this system keeps modification times");
        assert_eq!(source.modified(&path("notes/hello.txt")), Some(written));
        assert_eq!(source.modified(&path("notes/missing.txt")), None);
        fs::remove_dir_all(&dir).expect("remove the test directory");
    }

    /// A pack of `files`, each stored as it is, with its path, as the cook
    /// lays one out (ADR-0041).
    fn pack(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = vec![0; kpak::HEADER_SIZE];
        let mut entries = Vec::new();
        for (path, data) in files {
            bytes.resize(bytes.len().next_multiple_of(16), 0);
            entries.push((fnv1a64(path.as_bytes()), *path, bytes.len(), *data));
            bytes.extend_from_slice(data);
        }
        bytes.resize(bytes.len().next_multiple_of(16), 0);
        entries.sort_by_key(|entry| entry.0);
        let toc_offset = bytes.len();
        for (hash, _, offset, data) in &entries {
            bytes.extend_from_slice(&hash.to_le_bytes());
            bytes.extend_from_slice(&(*offset as u64).to_le_bytes());
            bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&[0; 4]);
            bytes.extend_from_slice(&crc32(data).to_le_bytes());
        }
        for (_, path, ..) in &entries {
            bytes.extend_from_slice(&(path.len() as u16).to_le_bytes());
            bytes.extend_from_slice(path.as_bytes());
        }
        let toc = bytes[toc_offset..].to_vec();
        let mut header = b"KPAK\x01\x00\x01\x00".to_vec();
        header.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        header.extend_from_slice(&(toc_offset as u64).to_le_bytes());
        header.extend_from_slice(&(toc.len() as u32).to_le_bytes());
        header.extend_from_slice(&crc32(&toc).to_le_bytes());
        header.extend_from_slice(&[0; 4]);
        bytes[..kpak::HEADER_SIZE].copy_from_slice(&header);
        bytes
    }

    const FILES: [(&str, &[u8]); 3] = [
        ("atlas/sprites_0.qoi", b"a page"),
        ("data/items.kbj", b"items"),
        ("levels/field.klvl", b"a level"),
    ];

    fn written(test: &str, bytes: &[u8]) -> PathBuf {
        let path = root(test).join(PackSource::FILE_NAME);
        fs::write(&path, bytes).expect("write the pack");
        path
    }

    #[test]
    fn a_pack_source_reads_the_files_of_its_pack_and_no_others() {
        let file = written("pack", &pack(&FILES));
        let source = PackSource::open(&file).expect("a pack");
        assert_eq!(source.path(), file);
        assert_eq!((source.len(), source.is_empty()), (3, false));
        for (name, bytes) in FILES {
            assert_eq!(source.read(&path(name)), Ok(bytes.to_vec()), "{name}");
        }
        assert_eq!(
            source.read(&path("data/missing.kbj")),
            Err(AssetError::NotFound)
        );
        assert_eq!(source.modified(&path("data/items.kbj")), None);
        let empty = written("pack_empty", &pack(&[]));
        assert!(PackSource::open(&empty).expect("a pack").is_empty());
        fs::remove_dir_all(file.parent().expect("a folder")).expect("remove the test directory");
        fs::remove_dir_all(empty.parent().expect("a folder")).expect("remove the test directory");
    }

    #[test]
    fn a_damaged_file_of_a_pack_is_an_error_naming_its_path() {
        let mut bytes = pack(&FILES);
        // `data/items.kbj`, the second file, is at 48.
        bytes[49] ^= 1;
        let file = written("pack_damaged", &bytes);
        let source = PackSource::open(&file).expect("a pack whose TOC is whole");
        let error = source.read(&path("data/items.kbj")).unwrap_err();
        assert!(
            matches!(
                &error,
                AssetError::Decode(CodecError::Kpak {
                    offset: 48,
                    kind: kpak::ErrorKind::Damaged { name: Some(name), .. },
                }) if name == "data/items.kbj"
            ),
            "{error:?}"
        );
        assert!(
            error.to_string().contains("`data/items.kbj` is damaged"),
            "{error}"
        );
        // The others are whole.
        assert_eq!(
            source.read(&path("levels/field.klvl")),
            Ok(b"a level".to_vec())
        );
        fs::remove_dir_all(file.parent().expect("a folder")).expect("remove the test directory");
    }

    #[test]
    fn opening_what_is_not_a_pack_says_why() {
        let dir = root("pack_not");
        assert_eq!(
            PackSource::open(dir.join("missing.kpak")).map(|_| ()),
            Err(AssetError::NotFound)
        );
        assert!(matches!(
            PackSource::open(dir.join("notes")),
            Err(AssetError::Read(_))
        ));
        fs::write(
            dir.join("text.kpak"),
            "not a pack, but long enough for a header",
        )
        .expect("write a file");
        assert!(matches!(
            PackSource::open(dir.join("text.kpak")),
            Err(AssetError::Decode(CodecError::Kpak {
                offset: 0,
                kind: kpak::ErrorKind::NotKpak,
            }))
        ));
        fs::write(dir.join("short.kpak"), "KPAK").expect("write a file");
        assert!(matches!(
            PackSource::open(dir.join("short.kpak")),
            Err(AssetError::Decode(CodecError::Kpak {
                offset: 4,
                kind: kpak::ErrorKind::UnexpectedEnd { .. },
            }))
        ));
        let mut bytes = pack(&FILES);
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(dir.join("toc.kpak"), &bytes).expect("write a file");
        assert!(matches!(
            PackSource::open(dir.join("toc.kpak")),
            Err(AssetError::Decode(CodecError::Kpak {
                kind: kpak::ErrorKind::TocChecksum { .. },
                ..
            }))
        ));
        fs::remove_dir_all(&dir).expect("remove the test directory");
    }

    #[test]
    fn a_pack_cut_short_after_it_was_opened_cannot_be_read() {
        let file = written("pack_cut", &pack(&FILES));
        let source = PackSource::open(&file).expect("a pack");
        let file_handle = fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .expect("open the pack to cut it");
        file_handle.set_len(40).expect("cut the pack short");
        assert_eq!(
            source.read(&path("data/items.kbj")),
            Err(AssetError::Read(io::ErrorKind::UnexpectedEof))
        );
        fs::remove_dir_all(file.parent().expect("a folder")).expect("remove the test directory");
    }

    #[test]
    fn workers_read_a_pack_at_once() {
        let file = written("pack_threads", &pack(&FILES));
        let source = Arc::new(PackSource::open(&file).expect("a pack"));
        let threads: Vec<_> = (0..4)
            .map(|worker| {
                let source = Arc::clone(&source);
                std::thread::spawn(move || {
                    for turn in 0..200 {
                        let (name, bytes) = FILES[(worker + turn) % FILES.len()];
                        assert_eq!(source.read(&path(name)), Ok(bytes.to_vec()));
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("a worker that read every file");
        }
        fs::remove_dir_all(file.parent().expect("a folder")).expect("remove the test directory");
    }

    #[test]
    fn a_source_that_cannot_tell_says_nothing_changed() {
        struct Empty;
        impl Source for Empty {
            fn read(&self, _: &AssetPath) -> Result<Vec<u8>, AssetError> {
                Err(AssetError::NotFound)
            }
        }
        assert_eq!(Empty.modified(&path("hero.qoi")), None);
    }
}
