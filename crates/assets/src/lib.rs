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
