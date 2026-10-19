//! Asset management for the Nadir engine: handles, typed storage, the VFS
//! (loose files or pack), loaders, hot reload and the worker pool.
//!
//! So far (roadmap M3.2): [`Handle`]s, the [`Assets`] of each type, the
//! [`Asset`] trait, and an [`AssetServer`] that loads loose files
//! synchronously. `docs/subsystems/assets-and-codecs.md` has the design.

mod asset;
mod assets;
mod error;
mod path;
mod server;

pub use asset::{Asset, DecodeCtx, FinalizeCtx};
pub use assets::{Assets, LoadState};
pub use error::AssetError;
pub use nadir_core::Handle;
pub use server::{AssetServer, OnFailure};
