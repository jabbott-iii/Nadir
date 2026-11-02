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

//! Asset management for the Nadir engine: handles, typed storage, the VFS
//! (loose/pack), loaders, hot reload and the worker pool.
//!
//! So far (roadmap M5.3): [`Handle`]s, the [`Assets`] of each type, the
//! [`Asset`] trait, [`AssetPath`]s, the [`Source`] trait with its
//! [`LooseSource`], and an [`AssetServer`] that reads and decodes on worker
//! threads, finalizes on the main thread, and frees assets by [`Scope`].
//! `docs/subsystems/assets-and-codecs.md` has the design.

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
pub use source::{LooseSource, Source};
