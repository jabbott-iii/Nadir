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

//! File-format codecs for the Nadir engine: Adler-32, inflate, PNG, QOI, QOA,
//! WAV, the sound file (KPCM) and a resampler, KLZ block compression,
//! JSON(C), binary JSON (KBJ), BMFont, Aseprite's sprite sheet data, LDtk's
//! projects, the cook's atlas, level and prefab files, and the pack file.
//!
//! # Where it fits
//!
//! Nadir's artists keep their own tools. Their formats, PNG, Aseprite and
//! LDtk exports, BMFont and WAV, are read only by the cook, `nadir-cook`,
//! which turns them into a few small formats of Nadir's own that the game
//! reads ([the design](https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/assets-and-codecs.md);
//! ADR-0008, ADR-0012). This crate holds both sides. It turns bytes and text into values, and values
//! into bytes, and does nothing else: it reads no files, starts no threads
//! and logs nothing.
//!
//! Games do not call it themselves. The asset types call its decoders on
//! the asset server's worker threads (nadir-assets), such as nadir-render's
//! `Texture`, a QOI file, and `Atlas`, an atlas file. The facade,
//! nadir_engine, does not re-export this crate, but nadir-scene re-exports
//! [`json::Value`], the value tree its reflection reads, as
//! `nadir::scene::Value`.
//!
//! # Formats
//!
//! Made for the game, which reads them at run time:
//!
//! | Module | What it reads | Functions |
//! | :- | :- | :- |
//! | [`qoi`] | QOI images (specification 1.0), RGBA8: the game's textures and the atlas pages the cook writes | [`qoi::decode`], [`qoi::encode`] |
//! | [`qoa`] | QOA sounds: 16-bit samples of 1 to 8 channels at 3.2 bits a sample, whole or a frame at a time, for music and long sounds | [`qoa::decode`], [`qoa::header`], [`qoa::decode_frame`], [`qoa::encode`] |
//! | [`kpcm`] | The sound file, `audio/<path>.kpcm`: 16-bit samples of 1 or 2 channels as they are, for short sounds | [`kpcm::decode`], [`kpcm::encode`] |
//! | [`kbj`] | KBJ, the binary form of a [`json::Value`] tree that cooked data is stored in | [`kbj::decode`], [`kbj::encode`] |
//! | [`atlas`] | The cook's atlas file, `atlas/<group>.kbj`: each sprite's frames on the pages, and its clips | [`atlas::parse`] |
//! | [`level`] | The level file, `levels/<name>.klvl`: tile layers, a collision grid, tall tiles, entities | [`level::parse`] |
//! | [`prefab`] | The prefab file, `data/prefabs/<name>.kbj`: a prefab's base and its components' values | [`prefab::parse`], [`prefab::read`] |
//! | [`kpak`] | The pack file, `game.kpak`: its header, its table of contents, and each entry checked against its CRC-32 | [`kpak::header`], [`kpak::toc`], [`kpak::parse`] |
//! | [`klz`] | KLZ, an LZ77 block compression of Nadir's own, for pack entries | [`klz::decompress`], [`klz::compress`] |
//!
//! Today the game reads QOI textures, atlas files, prefab files and level
//! files, which the engine's `load_level` loads into the world. The pack
//! file's reader is built (roadmap M5.13); the cook writes packs, and
//! nadir-assets' `PackSource` reads them (M5.14), as the engine's release
//! builds do (M5.7). QOA's encoder and decoder are built (M6.1), and the
//! sound file's (M6.8): the cook writes both, and the mixer plays QOA
//! (M6.2); the game loads them as assets from M6.9.
//!
//! [`pcm`] changes a sound's rate, with a windowed sinc that gives the same
//! bits on every system and keeps a loop's seam ([`pcm::resample`]), and
//! makes 16-bit samples of `f32` ones ([`pcm::to_i16`]): the cook resamples
//! sounds to 48 kHz with it.
//!
//! Read only by the cook:
//!
//! | Module | What it reads | Functions |
//! | :- | :- | :- |
//! | `png` (feature `png`) | PNG images of every color type and bit depth, to RGBA8 | `png::decode` |
//! | [`inflate`] | zlib (RFC 1950) and bare DEFLATE (RFC 1951) streams: PNG's image data | [`inflate::zlib`], [`inflate::raw`] |
//! | [`adler32`] | The Adler-32 checksum at the end of a zlib stream | [`adler32::checksum`] |
//! | [`aseprite`] | Aseprite's sprite sheet data (`--format json-array`), its tags expanded into clips | [`aseprite::parse`] |
//! | [`bmfont`] | BMFont fonts in the text format | [`bmfont::parse`] |
//! | [`ldtk`] | LDtk 1.5.3 projects (`.ldtk`) and level files (`.ldtkl`) | [`ldtk::parse_project`], [`ldtk::parse_level`] |
//! | [`wav`] | WAV sounds: PCM of 16 and 24 bits, float of 32, mono or stereo, to `f32` samples | [`wav::decode`] |
//!
//! [`json`] reads JSON (RFC 8259) with the JSONC extensions, `//` and
//! `/* */` comments and trailing commas, into a [`json::Value`]
//! ([`json::parse`]), and writes strict JSON back
//! ([`json::to_string_compact`], [`json::to_string_pretty`]). The cook
//! reads its data files and sidecars with it; KBJ, the atlas, level and
//! prefab files, and the Aseprite and LDtk readers all work on the same
//! value tree.
//!
//! # Example
//!
//! A data file's JSONC, as the cook reads it, and the same value as KBJ,
//! which the cook writes:
//!
//! ```
//! use nadir_codec::json::{self, Value};
//! use nadir_codec::{Limits, kbj};
//!
//! let text = r#"{
//!     // The player's starting stats.
//!     "name": "hero",
//!     "hp": 12,
//!     "speed": 1.5,
//! }"#;
//! let value = json::parse(text, Limits::default())?;
//! assert_eq!(value.get("name").and_then(Value::as_str), Some("hero"));
//! assert_eq!(value.get("hp").and_then(Value::as_i64), Some(12));
//! assert_eq!(value.get("speed").and_then(Value::as_f64), Some(1.5));
//!
//! let bytes = kbj::encode(&value, Limits::default())?;
//! assert!(bytes.starts_with(b"KBJ1"));
//! assert_eq!(kbj::decode(&bytes, Limits::default())?, value);
//! # Ok::<(), nadir_codec::CodecError>(())
//! ```
//!
//! A prefab, read from its source's JSONC and checked:
//!
//! ```
//! use nadir_codec::{Limits, json, prefab};
//!
//! let text = r#"{
//!     "name": "slime",
//!     "extends": "enemy_base",
//!     "components": { "Health": { "current": 12, "max": 12 } },
//! }"#;
//! let file = prefab::read(&json::parse(text, Limits::default())?)?;
//! assert_eq!(file.name, "slime");
//! assert_eq!(file.extends.as_deref(), Some("enemy_base"));
//! assert_eq!(file.components[0].0, "Health");
//! # Ok::<(), nadir_codec::CodecError>(())
//! ```
//!
//! # Errors
//!
//! Every function that can fail returns a [`CodecError`], which implements
//! [`Display`](std::fmt::Display) and [`std::error::Error`]. Its variant
//! names the format, and says where the problem is:
//!
//! - a byte offset in a binary format: [`CodecError::Qoi`],
//!   [`CodecError::Qoa`], [`CodecError::Kbj`], [`CodecError::Klz`], [`CodecError::Kpak`],
//!   [`CodecError::Kpcm`], [`CodecError::Inflate`], [`CodecError::Wav`]
//!   and, with `png`, `CodecError::Png`;
//! - a line and a column, counted in characters from 1, in text:
//!   [`CodecError::Json`] and [`CodecError::BmFont`];
//! - a member path, such as `sprites[2].frames[0].w`, in a file checked
//!   after its KBJ or JSON is read: [`CodecError::Atlas`],
//!   [`CodecError::Level`], [`CodecError::Prefab`],
//!   [`CodecError::Aseprite`] and [`CodecError::Ldtk`]. KBJ or JSON that
//!   cannot be read at all is the KBJ or JSON error.
//!
//! The `ErrorKind` of each module that can fail says what is wrong. An
//! encoder that refuses a value has a variant of its own,
//! [`CodecError::QoiEncode`], [`CodecError::QoaEncode`],
//! [`CodecError::KpcmEncode`] and [`CodecError::KbjEncode`]; samples
//! [`pcm::resample`] refuses are [`CodecError::Pcm`]. [`CodecError`]
//! is `#[non_exhaustive]`: which variants exist depends on the features, so
//! a `match` on it needs a `_` arm.
//!
//! ```
//! use nadir_codec::json::{self, ErrorKind};
//! use nadir_codec::{CodecError, Limits};
//!
//! let text = "{\n  \"hp\": 3,\n  \"hp\": 4\n}";
//! match json::parse(text, Limits::default()) {
//!     Err(CodecError::Json { line, kind: ErrorKind::DuplicateKey { key }, .. }) => {
//!         assert_eq!((line, key.as_str()), (3, "hp"));
//!     }
//!     other => panic!("expected the key twice, found {other:?}"),
//! }
//! ```
//!
//! # Untrusted input and limits
//!
//! Every decoder treats its input as untrusted, a damaged file or a hostile
//! one. It takes a [`Limits`], checks each length and offset against the
//! input before it uses it, checks each size against the limits before it
//! allocates, returns a [`CodecError`] for anything malformed, and never
//! panics. [`qoa::header`] and [`qoa::decode_frame`], which allocate
//! nothing, take no limits; [`qoa::decode`] does. Each has a fuzz target in the repository
//! (`cargo xtask fuzz-smoke`).
//!
//! [`Limits::default`] gives the game's limits: nesting 64 deep, strings of
//! 1 MiB, 1,000,000 entries in an array or object, images of 8192 pixels a
//! side, and 256 MiB decoded from one file. Tools may raise them: the cook
//! reads source images wider than a texture. Past a limit is an error:
//!
//! ```
//! use nadir_codec::json::{self, ErrorKind};
//! use nadir_codec::{CodecError, Limits};
//!
//! let limits = Limits { max_depth: 1, ..Limits::default() };
//! assert!(json::parse("[1, 2]", limits).is_ok());
//! assert!(matches!(
//!     json::parse("[[1, 2]]", limits),
//!     Err(CodecError::Json { kind: ErrorKind::TooDeep { limit: 1 }, .. })
//! ));
//! ```
//!
//! What each decoder accepts and rejects, beside its specification, is in
//! its module's documentation and in the design.
//!
//! # Threads, determinism and allocation
//!
//! - The functions hold no state between calls, so any thread may call
//!   them, as many at once as it likes: the asset server's workers do.
//! - The encoders are deterministic: the same value always gives the same
//!   bytes. [`kbj::decode`] accepts only the bytes [`kbj::encode`] writes,
//!   one encoding per value. [`qoi::encode`] writes a file byte for byte as
//!   the reference encoder does, and [`qoa::encode`] chooses each slice as
//!   its reference does (its module says where their bytes may differ).
//! - Decoders and encoders allocate what they return: they are for loading
//!   and cooking, not for each frame. [`qoa::decode_frame`] is the
//!   exception: it writes into the caller's buffer and allocates nothing,
//!   for music decoded as it plays.
//!
//! # Features
//!
//! - `png`: the `png` module, the PNG decoder, which only the cook needs.
//!   Off by default. With it on, [`CodecError`] has a `Png` variant too.

pub mod adler32;
pub mod aseprite;
pub mod atlas;
pub mod bmfont;
mod error;
pub mod inflate;
pub mod json;
pub mod kbj;
pub mod klz;
pub mod kpak;
pub mod kpcm;
pub mod ldtk;
pub mod level;
mod limits;
pub mod pcm;
#[cfg(feature = "png")]
pub mod png;
pub mod prefab;
pub mod qoa;
pub mod qoi;
pub mod wav;

pub use error::CodecError;
pub use limits::Limits;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
