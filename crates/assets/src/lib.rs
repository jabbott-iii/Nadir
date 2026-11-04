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

//! Asset loading for the Nadir engine: an [`AssetServer`] that reads and
//! decodes files on worker threads, makes the assets on the main thread,
//! and keeps them, by [`Handle`], until each [`Scope`] they were loaded in
//! is unloaded.
//!
//! # Where it fits
//!
//! A game reaches this crate through nadir_engine, as `nadir::assets`; its
//! prelude has [`AssetServer`], [`FinalizeCtx`], [`Handle`] and
//! [`LoadState`]. The facade's `App` keeps an [`AssetServer`] as a resource
//! of its world, reading `Config::assets`: in builds without the facade's
//! `dev` feature, the cook's pack there through a [`PackSource`] when
//! there is one, and otherwise its loose files (ADR-0041). It finishes
//! loads within the budget after each frame it draws in the window, and
//! waits for every load after each frame it draws offscreen. With the
//! facade's `dev` feature a failed asset shows its placeholder
//! ([`OnFailure::Placeholder`]); without it, nothing ([`OnFailure::Nothing`]).
//!
//! The asset types live beside what uses them: nadir-render's `Texture`, a
//! QOI file, and `Atlas`, the cook's atlas file, and nadir-scene's `Level`
//! and `PrefabAsset`, the cook's level and prefab files, each decoded with
//! nadir-codec.
//! The files are the cook's outputs, in `target/cooked/` during
//! development, loose or in its pack, `game.kpak`. Any type can be an
//! asset: implement [`Asset`] for it.
//!
//! The design is in
//! [`docs/subsystems/assets-and-codecs.md`](https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/assets-and-codecs.md)
//! (ADR-0030).
//!
//! # The parts
//!
//! - **Loading:** [`AssetServer::load`], or [`AssetServer::load_in`] with a
//!   [`Scope`], returns a [`Handle`] at once and queues the file for a
//!   worker. [`AssetServer::finish_completed_loads`], on the main thread
//!   once a frame, makes the assets the workers have decoded, within a
//!   budget ([`AssetServer::set_finish_budget`], 2 ms unless set).
//!   [`AssetServer::finish_all_loads`] waits for every load instead, for
//!   tools, tests and recordings. [`AssetServer::load_now`] and
//!   [`AssetServer::load_now_in`] make one asset before they return, for
//!   what a game needs before its first tick, such as an atlas's regions.
//! - **Looking up:** [`AssetServer::get`] gives the asset once it is
//!   loaded, [`AssetServer::state`] its [`LoadState`],
//!   [`AssetServer::error`] why it failed, and [`AssetServer::path`] where
//!   it came from. [`AssetServer::assets`] gives the [`Assets`] of one type,
//!   which can also find a handle by path ([`Assets::handle`]).
//! - **Asset types:** the [`Asset`] trait. [`Asset::decode`] turns bytes
//!   into data on a worker, with a [`DecodeCtx`] that holds the path and
//!   the codecs' limits; [`Asset::finalize`] makes the asset on the main
//!   thread, with the resources in a [`FinalizeCtx`], such as the GPU
//!   device. [`Asset::placeholder`] and [`Asset::release`] are optional.
//! - **Paths:** [`AssetPath`], a path below the source's root, such as
//!   `sprites/hero.qoi`: lowercased, then checked, the same on every system.
//! - **Sources:** the [`Source`] trait, which the workers read through;
//!   [`LooseSource`], which reads files under a folder; and [`PackSource`],
//!   which reads the cook's pack, each file checked against its CRC-32 as
//!   it is read (ADR-0041). [`AssetServer::new`] reads through a
//!   [`LooseSource`], and [`AssetServer::with_source`] through any
//!   [`Source`].
//! - **Freeing:** [`AssetServer::unload_scope`].
//!
//! Not built yet: hot reload (M5.16). [`Source::modified`] already tells
//! when a loose file changed, but nothing watches it.
//!
//! # Example
//!
//! An asset type, read from files held in memory:
//!
//! ```
//! use std::collections::HashMap;
//!
//! use nadir_assets::{
//!     Asset, AssetError, AssetPath, AssetServer, DecodeCtx, FinalizeCtx, LoadState, OnFailure,
//!     Source,
//! };
//!
//! /// A text file.
//! struct Note(String);
//!
//! impl Asset for Note {
//!     type Decoded = String;
//!     const EXTENSIONS: &'static [&'static str] = &["txt"];
//!
//!     // On a worker thread.
//!     fn decode(bytes: &[u8], _: &DecodeCtx<'_>) -> Result<String, AssetError> {
//!         String::from_utf8(bytes.to_vec()).map_err(|_| AssetError::Invalid("not UTF-8".into()))
//!     }
//!
//!     // On the main thread.
//!     fn finalize(text: String, _: &mut FinalizeCtx<'_>) -> Result<Note, AssetError> {
//!         Ok(Note(text))
//!     }
//! }
//!
//! /// Files in memory, by path.
//! struct Memory(HashMap<String, Vec<u8>>);
//!
//! impl Source for Memory {
//!     fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
//!         self.0.get(path.as_str()).cloned().ok_or(AssetError::NotFound)
//!     }
//! }
//!
//! let files = HashMap::from([("notes/hello.txt".to_string(), b"hello".to_vec())]);
//! let mut server = AssetServer::with_source(Memory(files), OnFailure::Nothing);
//!
//! // Both return at once; the workers read and decode the files.
//! let hello = server.load::<Note>("Notes/Hello.txt");
//! let missing = server.load::<Note>("notes/missing.txt");
//! assert_eq!(server.state(hello), Some(LoadState::Loading));
//!
//! // A game calls `finish_completed_loads` once a frame; this waits.
//! server.finish_all_loads(&mut FinalizeCtx::new());
//! assert_eq!(server.get(hello).map(|note| note.0.as_str()), Some("hello"));
//! assert_eq!(server.path(hello), Some("notes/hello.txt"));
//! assert_eq!(server.state(missing), Some(LoadState::Failed));
//! assert_eq!(server.error(missing), Some(&AssetError::NotFound));
//! ```
//!
//! # Scopes and handles
//!
//! A [`Handle`] is a plain id: nothing counts it, and dropping it does
//! nothing. An asset belongs to each [`Scope`] it was loaded in, and stays
//! until every one of them is unloaded. Loading a path again, in any scope,
//! gives the same handle, and adds the scope.
//! [`AssetServer::unload_scope`] removes the assets it leaves in no scope:
//! their handles name nothing from then on, and the next finish frees each
//! with [`Asset::release`]. A handle from one server can name an asset of
//! another, so keep each handle with its server.
//!
//! ```
//! # use nadir_assets::{Asset, AssetError, DecodeCtx, FinalizeCtx};
//! # struct Note(String);
//! # impl Asset for Note {
//! #     type Decoded = String;
//! #     const EXTENSIONS: &'static [&'static str] = &["txt"];
//! #     fn decode(bytes: &[u8], _: &DecodeCtx<'_>) -> Result<String, AssetError> {
//! #         Ok(String::from_utf8_lossy(bytes).into_owned())
//! #     }
//! #     fn finalize(text: String, _: &mut FinalizeCtx<'_>) -> Result<Note, AssetError> {
//! #         Ok(Note(text))
//! #     }
//! # }
//! use nadir_assets::{AssetServer, OnFailure, Scope};
//!
//! let mut server = AssetServer::new("no/such/folder", OnFailure::Nothing);
//! let menu = server.load::<Note>("menu.txt");
//! let intro = server.load_in::<Note>("intro.txt", Scope::Level);
//! assert_eq!(server.load_in::<Note>("menu.txt", Scope::Level), menu);
//!
//! // `menu.txt` is in the global scope too, so it stays.
//! assert_eq!(server.unload_scope(Scope::Level), 1);
//! assert!(server.state(intro).is_none());
//! assert!(server.state(menu).is_some());
//! ```
//!
//! # Errors
//!
//! A load that fails is [`LoadState::Failed`] once it is finished, and
//! [`AssetServer::error`] gives the [`AssetError`]: [`AssetError::NotFound`]
//! for a missing file, [`AssetError::BadPath`] or [`AssetError::Extension`]
//! for a path the type cannot have, [`AssetError::Decode`] with the
//! [`CodecError`](nadir_codec::CodecError) for a file a codec refused, and
//! [`AssetError::Invalid`] for a reason of the asset type's own. The error
//! holds no path: the server logs each failure, with its path, through
//! `nadir_core::log`. With [`OnFailure::Placeholder`], [`AssetServer::get`]
//! then gives the type's placeholder, such as nadir-render's magenta and
//! black checker for a texture; its state stays `Failed`.
//! [`AssetServer::load_now`] returns the error as well.
//!
//! # Threads
//!
//! - A server starts 1 to 4 worker threads, `nadir-assets-0` and on: two
//!   fewer than the cores, which leaves the main thread a core and one to
//!   spare. If none can start, the files are read and decoded on the thread
//!   that asks for the load.
//! - A [`Source`] is `Send + Sync`: the workers call it from several
//!   threads at once.
//! - A panic while a file is read or decoded fails that asset alone.
//! - Dropping the server drops the jobs still queued, and waits for those
//!   running.
//!
//! # Determinism
//!
//! [`AssetServer::finish_completed_loads`] makes an asset some frames after
//! its load, never at a frame that can be known in advance, so tick systems
//! must not depend on whether an asset is loaded (ADR-0025).
//! [`AssetServer::finish_all_loads`] makes every asset, type by type in the
//! order of each type's first load, and each type's in the order they were
//! loaded, so the same loads are made in the same order every run.
//!
//! # Allocation
//!
//! A [`FinalizeCtx`] holds up to [`FinalizeCtx::CAPACITY`] resources and
//! allocates nothing, so a game can make one every frame; when
//! [`AssetServer::has_pending`] is false, it need not make one at all.

mod asset;
mod assets;
mod error;
mod path;
mod server;
mod source;
mod workers;

pub use asset::{Asset, DecodeCtx, FinalizeCtx};
pub use assets::{Assets, LoadState};
pub use error::AssetError;
pub use nadir_core::Handle;
pub use path::AssetPath;
pub use server::{AssetServer, OnFailure, Scope};
pub use source::{LooseSource, PackSource, Source};

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
