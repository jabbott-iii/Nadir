# nadir-assets

Asset loading for the Nadir engine: an asset server that reads and decodes
files on worker threads, makes the assets on the main thread, and keeps
them by handle until the scope they were loaded in is unloaded.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Games use it through
[nadir_engine](https://crates.io/crates/nadir_engine), which re-exports it as
`nadir::assets`; depend on it directly to use it on its own.

## Status

Version 0.2: the API will change before 1.0.

Built:

- `AssetServer`: `load` returns a handle at once, and a worker thread reads
  and decodes the file. `finish_completed_loads`, called once a frame on
  the main thread, makes the decoded assets within a time budget (2 ms
  unless set). `finish_all_loads` waits for every load, and `load_now`
  makes one asset before it returns.
- The `Asset` trait, which any type implements to be loaded: `decode` runs
  on a worker, `finalize` on the main thread, with the resources it needs,
  such as the GPU device, in a `FinalizeCtx`.
- `Handle<T>`, a plain id with no reference count, and `Assets<T>`, the
  assets of one type.
- Scopes: an asset loaded in `Scope::Global`, `Scope::Level` or
  `Scope::Transient` stays until each scope it was loaded in is unloaded.
- `AssetPath`: the same path rules on every system.
- The `Source` trait; `LooseSource`, which reads files under a folder; and
  `PackSource`, which reads the pack file the cook writes, `game.kpak`,
  each file found by a binary search of its path's hash and checked
  against its CRC-32 as it is read, so that a damaged one is an error
  naming its path.
- Placeholders for failed loads, when the server is asked to show them.

Not built yet: hot reload (M5.8).

The engine's asset types are in nadir-render: `Texture` (`.qoi`) and `Atlas`
(`.kbj`).

## Usage

```toml
[dependencies]
nadir-assets = "0.2"
```

An asset type, and a server that loads it from loose files:

```rust,no_run
use nadir_assets::{Asset, AssetError, AssetServer, DecodeCtx, FinalizeCtx, OnFailure};

/// A text file.
struct Note(String);

impl Asset for Note {
    type Decoded = String;
    const EXTENSIONS: &'static [&'static str] = &["txt"];

    // On a worker thread.
    fn decode(bytes: &[u8], _: &DecodeCtx<'_>) -> Result<String, AssetError> {
        String::from_utf8(bytes.to_vec()).map_err(|_| AssetError::Invalid("not UTF-8".into()))
    }

    // On the main thread, where a texture is uploaded to the GPU.
    fn finalize(text: String, _: &mut FinalizeCtx<'_>) -> Result<Note, AssetError> {
        Ok(Note(text))
    }
}

fn main() {
    // Reads `target/cooked/notes/hello.txt`.
    let mut server = AssetServer::new("target/cooked", OnFailure::Nothing);
    let hello = server.load::<Note>("notes/hello.txt");

    // A game calls `finish_completed_loads` once a frame; this waits.
    server.finish_all_loads(&mut FinalizeCtx::new());
    match server.get(hello) {
        Some(note) => println!("{}", note.0),
        None => println!("{:?}: {:?}", server.state(hello), server.error(hello)),
    }
}
```

A failed load is logged with its path, and `AssetServer::error` says why:
`AssetError::NotFound` for a missing file, `AssetError::Decode` for a file
a codec refused.

## Platforms

It uses only the standard library's files and threads, and works the same
way on Windows, macOS and Linux. An asset path is lowercased, the same on
every system, so on a case-sensitive file system, as Linux's are, only
lowercase file names are found; Windows and macOS ignore case by default.
Keep file names lowercase, as the cook's outputs are.

## Documentation

- API reference: https://docs.rs/nadir-assets
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/assets-and-codecs.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
