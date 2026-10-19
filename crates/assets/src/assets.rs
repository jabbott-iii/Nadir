//! The assets of one type, by handle and by path.

use std::borrow::Cow;
use std::mem;

use nadir_core::log::{error, warn};
use nadir_core::{FastHashMap, GenArena, Handle};

use crate::{Asset, AssetError, FinalizeCtx, OnFailure};

/// `path` with its ASCII letters lowercased, allocating only if it has
/// uppercase ones.
pub(crate) fn lowercase(path: &str) -> Cow<'_, str> {
    if path.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(path.to_ascii_lowercase())
    } else {
        Cow::Borrowed(path)
    }
}

/// Where a load is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoadState {
    /// Waiting for [`AssetServer::finish_completed_loads`](crate::AssetServer::finish_completed_loads).
    Loading,
    /// Ready.
    Loaded,
    /// It failed. [`Assets::error`] says why, and with placeholders shown,
    /// [`Assets::get`] gives the placeholder.
    Failed,
}

enum Slot<T: Asset> {
    /// Read and decoded, or failed on the way, and waiting to be finalized.
    Pending(Result<T::Decoded, AssetError>),
    Loaded(T),
    Failed {
        error: AssetError,
        placeholder: Option<T>,
    },
    /// Only while it is being finalized.
    Finalizing,
}

struct Entry<T: Asset> {
    path: String,
    slot: Slot<T>,
}

/// The assets of type `T`: every one loaded, by handle and by path.
pub struct Assets<T: Asset> {
    arena: GenArena<Entry<T>>,
    by_path: FastHashMap<String, Handle<T>>,
    /// In the order they were loaded.
    pending: Vec<Handle<T>>,
}

impl<T: Asset> Assets<T> {
    pub(crate) fn new() -> Self {
        Self {
            arena: GenArena::new(),
            by_path: FastHashMap::default(),
            pending: Vec::new(),
        }
    }

    /// The asset, once loaded. For a failed one, its placeholder if it has
    /// one.
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        match &self.arena.get(handle.id())?.slot {
            Slot::Loaded(asset) => Some(asset),
            Slot::Failed { placeholder, .. } => placeholder.as_ref(),
            Slot::Pending(_) | Slot::Finalizing => None,
        }
    }

    /// Where the load is, or `None` if these assets hold nothing under the
    /// handle.
    pub fn state(&self, handle: Handle<T>) -> Option<LoadState> {
        Some(match self.arena.get(handle.id())?.slot {
            Slot::Pending(_) | Slot::Finalizing => LoadState::Loading,
            Slot::Loaded(_) => LoadState::Loaded,
            Slot::Failed { .. } => LoadState::Failed,
        })
    }

    /// Why the load failed, once it has.
    pub fn error(&self, handle: Handle<T>) -> Option<&AssetError> {
        match &self.arena.get(handle.id())?.slot {
            Slot::Failed { error, .. } => Some(error),
            _ => None,
        }
    }

    /// The path the asset was loaded from.
    pub fn path(&self, handle: Handle<T>) -> Option<&str> {
        Some(&self.arena.get(handle.id())?.path)
    }

    /// The handle of the asset loaded from `path`, if there is one. As for
    /// [`AssetServer::load`](crate::AssetServer::load), ASCII letters are
    /// lowercased first.
    pub fn handle(&self, path: &str) -> Option<Handle<T>> {
        self.by_path.get(lowercase(path).as_ref()).copied()
    }

    /// How many assets there are, in every state.
    pub fn len(&self) -> usize {
        self.arena.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.arena.is_empty()
    }

    /// Whether some wait for
    /// [`AssetServer::finish_completed_loads`](crate::AssetServer::finish_completed_loads).
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Adds the asset at `path`, read and decoded by `load`, unless there
    /// is one: the same path gives the same handle, whatever happened to
    /// it.
    pub(crate) fn add(
        &mut self,
        path: &str,
        load: impl FnOnce() -> Result<T::Decoded, AssetError>,
    ) -> Handle<T> {
        if let Some(&handle) = self.by_path.get(path) {
            return handle;
        }
        let handle = Handle::new(self.arena.insert(Entry {
            path: path.to_string(),
            slot: Slot::Pending(load()),
        }));
        self.by_path.insert(path.to_string(), handle);
        self.pending.push(handle);
        handle
    }

    /// Finalizes every pending asset, in the order they were loaded.
    pub(crate) fn finish(&mut self, ctx: &mut FinalizeCtx<'_>, on_failure: OnFailure) {
        // Taken out and put back, so that its capacity is kept.
        let mut pending = mem::take(&mut self.pending);
        for handle in pending.drain(..) {
            let Some(entry) = self.arena.get_mut(handle.id()) else {
                continue;
            };
            let result = match mem::replace(&mut entry.slot, Slot::Finalizing) {
                Slot::Pending(Ok(decoded)) => T::finalize(decoded, ctx),
                Slot::Pending(Err(error)) => Err(error),
                done => {
                    entry.slot = done;
                    continue;
                }
            };
            entry.slot = match result {
                Ok(asset) => Slot::Loaded(asset),
                Err(failure) => {
                    error!("asset `{}`: {failure}", entry.path);
                    let placeholder = match on_failure {
                        OnFailure::Nothing => None,
                        OnFailure::Placeholder => match T::placeholder(ctx) {
                            Ok(placeholder) => Some(placeholder),
                            Err(AssetError::NoPlaceholder) => None,
                            Err(cause) => {
                                warn!("asset `{}`: no placeholder either: {cause}", entry.path);
                                None
                            }
                        },
                    };
                    Slot::Failed {
                        error: failure,
                        placeholder,
                    }
                }
            };
        }
        self.pending = pending;
    }
}
