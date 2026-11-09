# nadir-codec

Decoders and encoders for the file formats of the Nadir engine: the formats
artists export, which the cook reads, and the small formats of Nadir's own
that the cook writes for the game.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Games do not use it directly: the engine's asset types
call its decoders when they load a file, and the cook, `nadir-cook`, reads
its sources and writes its outputs with it.
[nadir_engine](https://crates.io/crates/nadir_engine) does not re-export it;
depend on it directly to read or write these formats on their own.

## Status

Version 0.3: the API will change before 1.0.

Formats made for the game, which reads them at run time:

- **QOI** images (specification 1.0), decoded and encoded: the game's
  textures.
- **KBJ**, Nadir's binary form of a JSON value tree, decoded and encoded:
  cooked data.
- **The cook's atlas file**, which says where each sprite's frames are on
  the atlas pages and which clips they make.
- **The level file and the prefab file**, which nadir-scene loads as
  assets (`Level`, and prefabs through `PrefabLibrary::load`), and the
  engine's `load_level` loads into the world.
- **The pack file**, `game.kpak`: its header and table of contents read
  and checked, an entry found by its path, and its bytes checked against
  their CRC-32 and decompressed. The cook writes it, and nadir-assets'
  `PackSource` reads it.
- **KLZ**, Nadir's LZ77 block compression, for the pack's entries.
- **QOA** sounds, encoded and decoded, whole or a frame at a time into a
  buffer of your own: 16-bit audio at 3.2 bits a sample, for music and long
  sounds. The cook writes them.
- **The sound file**, `.kpcm`: 16-bit samples of 1 or 2 channels as they
  are, for short sounds, decoded and encoded. The cook writes it.
- **A resampler** (`pcm`), a windowed sinc that gives the same bits on every
  system and keeps a looped sound's seam: the cook resamples sounds to 48
  kHz with it, and nadir-audio's `Sound` converts a sound cooked at another
  rate that ends in quiet, its last 2 ms within −48 dBFS, to 48 kHz with it
  as it loads.

Formats only the cook reads:

- **PNG**, every color type and bit depth, behind the `png` feature.
- **zlib and DEFLATE** streams (inflate), and the **Adler-32** checksum,
  which PNG's image data needs.
- **Aseprite** sprite sheet data (`--format json-array`), its tags expanded
  into clips.
- **BMFont** fonts, in the text format.
- **LDtk 1.5.3** projects and level files, which the cook reads.
- **WAV** sounds (PCM of 16 and 24 bits, float of 32, mono or stereo),
  which the cook cooks to the sound file or QOA.

**JSON** (RFC 8259) with the JSONC extensions, comments and trailing
commas, is read and written: the cook reads its data files and sidecars
with it, and KBJ holds the same value tree.

Every decoder takes its input as untrusted. It takes a `Limits`, checks
each size against it before it allocates, returns a `CodecError` for
anything malformed, and never panics, whatever the bytes. Each has a fuzz
target in the Nadir repository. QOA's `header` and `decode_frame`, which
allocate nothing, take no `Limits`; its `decode` does.

## Usage

```toml
[dependencies]
nadir-codec = "0.3"
```

Read JSONC text, then store the same value as KBJ and read it back:

```rust
use nadir_codec::json::{self, Value};
use nadir_codec::{CodecError, Limits, kbj};

fn main() -> Result<(), CodecError> {
    let text = r#"{
        // The player's starting stats.
        "name": "hero",
        "hp": 12,
        "speed": 1.5,
    }"#;
    let value = json::parse(text, Limits::default())?;
    assert_eq!(value.get("hp").and_then(Value::as_i64), Some(12));

    let bytes = kbj::encode(&value, Limits::default())?;
    assert_eq!(kbj::decode(&bytes, Limits::default())?, value);
    Ok(())
}
```

Encode a QOI image and decode it again:

```rust
use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
use nadir_codec::{CodecError, Limits};

fn main() -> Result<(), CodecError> {
    let header = Header {
        width: 2,
        height: 1,
        channels: Channels::Rgba,
        color_space: ColorSpace::Srgb,
    };
    // RGBA8, 4 bytes a pixel, rows from the top.
    let pixels = [255, 0, 0, 255, 0, 0, 255, 128];
    let file = qoi::encode(&header, &pixels)?;
    let image = qoi::decode(&file, Limits::default())?;
    assert_eq!(image.pixels, pixels);
    Ok(())
}
```

A file that is not right is an error that says where: a byte offset for a
binary format, a line and column for text, and a member path, such as
`sprites[2].frames[0].w`, for a file checked after it is decoded.

## Features

- `png`: the PNG decoder, `nadir_codec::png`, which only the cook needs. It
  is off by default, so a game that does not ask for it does not build it.
  With it on, `CodecError` has a `Png` variant too.

## Documentation

- API reference: https://docs.rs/nadir-codec
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/assets-and-codecs.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
