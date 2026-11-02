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

use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{fs, io};

use crate::{AssetError, AssetPath};

/// Where an [`AssetServer`](crate::AssetServer) reads its files: loose
/// files under a directory ([`LooseSource`]), or a pack file (roadmap M5.14).
///
/// The server's workers call it, from several threads at once.
pub trait Source: Send + Sync {
    /// The bytes of the file at `path`.
    ///
    /// # Errors
    ///
    /// [`AssetError::NotFound`] if the source has no such file, and
    /// [`AssetError::Read`] if it cannot be read.
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError>;

    /// When the file at `path` last changed, if the source can tell, for
    /// hot reload (roadmap M5.8). Loose files can; a pack cannot. `None` by
    /// default.
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

impl Source for LooseSource {
    /// Reads `root/path`. A directory there is [`AssetError::Read`].
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
        fs::read(self.root.join(path.as_str())).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => AssetError::NotFound,
            kind => AssetError::Read(kind),
        })
    }

    fn modified(&self, path: &AssetPath) -> Option<SystemTime> {
        fs::metadata(self.root.join(path.as_str()))
            .and_then(|metadata| metadata.modified())
            .ok()
    }
}

#[cfg(test)]
mod tests {
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
