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

//! Where the game's assets are read from: the cook's pack, `game.kpak`, in
//! builds without `dev`, or the loose files (ADR-0041).

use std::path::Path;

use nadir_assets::{AssetError, AssetPath, AssetServer, OnFailure, PackSource, Source};
use nadir_core::log::{error, info};

/// The game's asset server, reading from the folder `assets`.
///
/// With `read_pack`, as in builds without `dev`, it reads the pack
/// [`PackSource::FILE_NAME`] in `assets` when there is one, and the loose
/// files otherwise. A pack that is there but does not open is logged, and
/// every load then fails with the error that opening it gave, rather than
/// reading loose files that may not be its cook's. Without `read_pack`, as
/// with `dev`, whose hot reload watches them (`App::run`), it reads the
/// loose files.
pub(crate) fn asset_server(assets: &Path, on_failure: OnFailure, read_pack: bool) -> AssetServer {
    if !read_pack {
        return AssetServer::new(assets, on_failure);
    }
    let path = assets.join(PackSource::FILE_NAME);
    match PackSource::open(&path) {
        Ok(pack) => {
            info!(
                "the assets are the pack {}, of {} files",
                path.display(),
                pack.len()
            );
            AssetServer::with_source(pack, on_failure)
        }
        Err(AssetError::NotFound) => AssetServer::new(assets, on_failure),
        Err(cause) => {
            error!(
                "the pack {} cannot be opened, so no asset loads: {cause}",
                path.display()
            );
            AssetServer::with_source(Unopened(cause), on_failure)
        }
    }
}

/// A pack that did not open: every read fails as opening it did.
struct Unopened(AssetError);

impl Source for Unopened {
    fn read(&self, _: &AssetPath) -> Result<Vec<u8>, AssetError> {
        Err(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use nadir_assets::{Asset, DecodeCtx, FinalizeCtx};
    use nadir_codec::CodecError;
    use nadir_codec::kpak::{self, ErrorKind};
    use nadir_core::hash::{crc32, fnv1a64};

    use super::*;

    /// A file's bytes, as an asset.
    #[derive(Debug)]
    struct Text(Vec<u8>);

    impl Asset for Text {
        type Decoded = Vec<u8>;
        const EXTENSIONS: &'static [&'static str] = &["txt"];

        fn decode(bytes: &[u8], _: &DecodeCtx<'_>) -> Result<Vec<u8>, AssetError> {
            Ok(bytes.to_vec())
        }

        fn finalize(bytes: Vec<u8>, _: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
            Ok(Self(bytes))
        }
    }

    /// The bytes of `path`, read through `server`.
    fn read(server: &mut AssetServer, path: &str) -> Result<Vec<u8>, AssetError> {
        let handle = server.load_now::<Text>(path, &mut FinalizeCtx::new())?;
        Ok(server.get(handle).expect("a loaded asset").0.clone())
    }

    /// A pack of `files`, each stored as it is, as the cook lays one out.
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

    /// A new folder for one test, holding `data/a.txt` as `loose`.
    fn folder(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nadir-pack-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("data")).expect("make the folder");
        fs::write(dir.join("data/a.txt"), "loose").expect("write a loose file");
        dir
    }

    #[test]
    fn the_pack_is_read_in_place_of_the_loose_files_when_it_is_there() {
        let dir = folder("there");
        let packed = pack(&[("data/a.txt", b"packed"), ("data/b.txt", b"only packed")]);
        fs::write(dir.join(PackSource::FILE_NAME), packed).expect("write the pack");

        let mut server = asset_server(&dir, OnFailure::Nothing, true);
        assert_eq!(read(&mut server, "data/a.txt").unwrap(), b"packed");
        assert_eq!(read(&mut server, "data/b.txt").unwrap(), b"only packed");
        // What is loose alone is not read.
        fs::write(dir.join("data/c.txt"), "loose alone").expect("write a loose file");
        assert_eq!(read(&mut server, "data/c.txt"), Err(AssetError::NotFound));

        // As with `dev`: the loose files.
        let mut loose = asset_server(&dir, OnFailure::Nothing, false);
        assert_eq!(read(&mut loose, "data/a.txt").unwrap(), b"loose");
        assert_eq!(read(&mut loose, "data/b.txt"), Err(AssetError::NotFound));

        // With no pack, the loose files.
        fs::remove_file(dir.join(PackSource::FILE_NAME)).expect("remove the pack");
        let mut server = asset_server(&dir, OnFailure::Nothing, true);
        assert_eq!(read(&mut server, "data/a.txt").unwrap(), b"loose");
        fs::remove_dir_all(&dir).expect("remove the folder");
    }

    #[test]
    fn a_pack_that_does_not_open_fails_every_load_with_its_error() {
        let dir = folder("broken");
        let mut packed = pack(&[("data/a.txt", b"packed")]);
        packed[..4].copy_from_slice(b"KPAX");
        fs::write(dir.join(PackSource::FILE_NAME), &packed).expect("write the pack");

        let mut server = asset_server(&dir, OnFailure::Nothing, true);
        let refused = AssetError::Decode(CodecError::Kpak {
            offset: 0,
            kind: ErrorKind::NotKpak,
        });
        for path in ["data/a.txt", "data/b.txt"] {
            assert_eq!(read(&mut server, path), Err(refused.clone()), "{path}");
        }
        // As with `dev`, the loose files are read still.
        let mut loose = asset_server(&dir, OnFailure::Nothing, false);
        assert_eq!(read(&mut loose, "data/a.txt").unwrap(), b"loose");

        // A pack whose TOC is damaged.
        let mut packed = pack(&[("data/a.txt", b"packed")]);
        let last = packed.len() - 1;
        packed[last] ^= 1;
        fs::write(dir.join(PackSource::FILE_NAME), &packed).expect("write the pack");
        let mut server = asset_server(&dir, OnFailure::Nothing, true);
        assert!(
            matches!(
                read(&mut server, "data/a.txt"),
                Err(AssetError::Decode(CodecError::Kpak {
                    kind: ErrorKind::TocChecksum { .. },
                    ..
                }))
            ),
            "the TOC's CRC"
        );
        fs::remove_dir_all(&dir).expect("remove the folder");
    }

    #[test]
    fn the_app_reads_the_pack_without_dev_and_the_loose_files_with_it() {
        let dir = folder("app");
        fs::write(
            dir.join(PackSource::FILE_NAME),
            pack(&[("data/a.txt", b"packed")]),
        )
        .expect("write the pack");
        let app = crate::App::new(crate::Config {
            assets: dir.clone(),
            ..crate::Config::default()
        });
        let got = read(&mut app.world().resource_mut::<AssetServer>(), "data/a.txt");
        let expected: &[u8] = if cfg!(feature = "dev") {
            b"loose"
        } else {
            b"packed"
        };
        assert_eq!(got.unwrap(), expected);
        drop(app);
        fs::remove_dir_all(&dir).expect("remove the folder");
    }
}
