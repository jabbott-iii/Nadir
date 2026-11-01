//! The assets of one type, by handle and by path.

use std::borrow::Cow;
use std::mem;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use nadir_core::log::{error, warn};
use nadir_core::{FastHashMap, GenArena, Handle};

use crate::{Asset, AssetError, FinalizeCtx, OnFailure, Scope};

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
    /// Being read and decoded on a worker, or waiting for
    /// [`AssetServer::finish_completed_loads`](crate::AssetServer::finish_completed_loads).
    Loading,
    /// Ready.
    Loaded,
    /// It failed. [`Assets::error`] says why, and with placeholders shown,
    /// [`Assets::get`] gives the placeholder.
    Failed,
}

/// What a worker sends back: the asset, and what reading and decoding its
/// file made.
pub(crate) type Decoded<T> = (Handle<T>, Result<<T as Asset>::Decoded, AssetError>);

pub(crate) enum Slot<T: Asset> {
    /// Being read and decoded on a worker.
    Decoding,
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

/// The scopes an asset was loaded in, one bit each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Scopes(u8);

impl Scopes {
    fn bit(scope: Scope) -> u8 {
        match scope {
            Scope::Global => 1,
            Scope::Level => 2,
            Scope::Transient => 4,
        }
    }
}

struct Entry<T: Asset> {
    path: String,
    scopes: Scopes,
    slot: Slot<T>,
}

/// How long one call may spend finalizing assets.
pub(crate) struct Budget {
    /// `None` for no limit.
    deadline: Option<Instant>,
    /// Whether the call has finalized one: it always finalizes one at least.
    started: bool,
}

impl Budget {
    /// A budget that ends at `deadline`, or never.
    pub(crate) fn until(deadline: Option<Instant>) -> Self {
        Self {
            deadline,
            started: false,
        }
    }

    /// Whether there is no time for one more, asked before each.
    fn spent(&mut self) -> bool {
        let spent = self.started
            && self
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline);
        self.started = true;
        spent
    }
}

/// The assets of type `T`: every one loaded, by handle and by path.
pub struct Assets<T: Asset> {
    arena: GenArena<Entry<T>>,
    by_path: FastHashMap<String, Handle<T>>,
    /// Those not yet finalized, decoded or not, in the order they were
    /// loaded.
    pending: Vec<Handle<T>>,
    /// The workers' end of `decoded`, which each job takes a copy of.
    done: Sender<Decoded<T>>,
    /// In a mutex only so that `Assets` stays `Sync`, as a `Receiver` is
    /// not: it is reached through `&mut self`, and never locked.
    decoded: Mutex<Receiver<Decoded<T>>>,
    /// How many jobs have not sent their result yet, unloaded assets'
    /// included.
    in_flight: usize,
    /// The assets of unloaded scopes, with their paths, to free at the next
    /// finish.
    released: Vec<(String, T)>,
}

impl<T: Asset> Assets<T> {
    pub(crate) fn new() -> Self {
        let (done, decoded) = mpsc::channel();
        Self {
            arena: GenArena::new(),
            by_path: FastHashMap::default(),
            pending: Vec::new(),
            done,
            decoded: Mutex::new(decoded),
            in_flight: 0,
            released: Vec::new(),
        }
    }

    /// The asset, once loaded. For a failed one, its placeholder if it has
    /// one.
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        match &self.arena.get(handle.id())?.slot {
            Slot::Loaded(asset) => Some(asset),
            Slot::Failed { placeholder, .. } => placeholder.as_ref(),
            Slot::Decoding | Slot::Pending(_) | Slot::Finalizing => None,
        }
    }

    /// Where the load is, or `None` if these assets hold nothing under the
    /// handle: never did, or no longer do, since its scopes were unloaded.
    pub fn state(&self, handle: Handle<T>) -> Option<LoadState> {
        Some(match self.arena.get(handle.id())?.slot {
            Slot::Decoding | Slot::Pending(_) | Slot::Finalizing => LoadState::Loading,
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

    /// Whether some loads, or some assets to free, wait for
    /// [`AssetServer::finish_completed_loads`](crate::AssetServer::finish_completed_loads).
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || !self.released.is_empty() || self.in_flight > 0
    }

    /// The asset at `path`, if there is one, now loaded in `scope` too.
    pub(crate) fn find(&mut self, path: &str, scope: Scope) -> Option<Handle<T>> {
        let handle = *self.by_path.get(path)?;
        if let Some(entry) = self.arena.get_mut(handle.id()) {
            entry.scopes.0 |= Scopes::bit(scope);
        }
        Some(handle)
    }

    /// Adds an asset at `path`, which no asset has, in `scope`.
    pub(crate) fn insert(&mut self, path: &str, scope: Scope, slot: Slot<T>) -> Handle<T> {
        let handle = Handle::new(self.arena.insert(Entry {
            path: path.to_string(),
            scopes: Scopes(Scopes::bit(scope)),
            slot,
        }));
        self.by_path.insert(path.to_string(), handle);
        self.pending.push(handle);
        handle
    }

    /// Where a job that reads and decodes an asset sends its result. It
    /// must send one.
    pub(crate) fn job_sender(&mut self) -> Sender<Decoded<T>> {
        self.in_flight += 1;
        self.done.clone()
    }

    /// The results the workers send.
    fn results(&mut self) -> &Receiver<Decoded<T>> {
        // Nothing panics while holding it, since it is never locked.
        self.decoded
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes the results the workers have sent.
    fn receive(&mut self) {
        while let Ok(decoded) = self.results().try_recv() {
            self.take(decoded);
        }
    }

    /// Waits for every result the workers owe.
    pub(crate) fn wait(&mut self) {
        while self.in_flight > 0 {
            // It cannot be disconnected: `done` is a sender.
            let Ok(decoded) = self.results().recv() else {
                break;
            };
            self.take(decoded);
        }
    }

    fn take(&mut self, (handle, result): Decoded<T>) {
        self.in_flight = self.in_flight.saturating_sub(1);
        // An asset unloaded while it was decoding is no longer there.
        if let Some(entry) = self.arena.get_mut(handle.id())
            && matches!(entry.slot, Slot::Decoding)
        {
            entry.slot = Slot::Pending(result);
        }
    }

    /// Makes the asset under `handle` now: waits for its worker if it is
    /// still decoding, and finalizes it if it waits to be. Returns how its
    /// load ended.
    pub(crate) fn finish_now(
        &mut self,
        handle: Handle<T>,
        ctx: &mut FinalizeCtx<'_>,
        on_failure: OnFailure,
    ) -> Result<(), AssetError> {
        let decoding = |assets: &Self| {
            assets
                .arena
                .get(handle.id())
                .is_some_and(|entry| matches!(entry.slot, Slot::Decoding))
        };
        while decoding(self) && self.in_flight > 0 {
            // It cannot be disconnected: `done` is a sender.
            let Ok(decoded) = self.results().recv() else {
                break;
            };
            self.take(decoded);
        }
        let Some(entry) = self.arena.get_mut(handle.id()) else {
            return Err(AssetError::Invalid("it was unloaded".to_string()));
        };
        if matches!(entry.slot, Slot::Pending(_)) {
            finalize(entry, ctx, on_failure);
            self.pending.retain(|&pending| pending != handle);
        }
        match &entry.slot {
            Slot::Loaded(_) => Ok(()),
            Slot::Failed { error, .. } => Err(error.clone()),
            Slot::Decoding | Slot::Pending(_) | Slot::Finalizing => {
                Err(AssetError::Invalid("it did not finish loading".to_string()))
            }
        }
    }

    /// Takes `scope` from every asset, and removes those it leaves in no
    /// scope. Returns how many it removed.
    ///
    /// Their handles name nothing from now on. A loaded asset or a
    /// placeholder is released by the next finish; one still decoding is
    /// dropped when its worker is done.
    pub(crate) fn unload(&mut self, scope: Scope) -> usize {
        let bit = Scopes::bit(scope);
        let mut gone = Vec::new();
        for (index, entry) in self.arena.iter_mut() {
            if entry.scopes.0 & bit != 0 {
                entry.scopes.0 &= !bit;
                if entry.scopes.0 == 0 {
                    gone.push(index);
                }
            }
        }
        for &index in &gone {
            let Some(entry) = self.arena.remove(index) else {
                continue;
            };
            self.by_path.remove(&entry.path);
            match entry.slot {
                Slot::Loaded(asset)
                | Slot::Failed {
                    placeholder: Some(asset),
                    ..
                } => self.released.push((entry.path, asset)),
                Slot::Decoding
                | Slot::Pending(_)
                | Slot::Failed {
                    placeholder: None, ..
                }
                | Slot::Finalizing => {}
            }
        }
        let arena = &self.arena;
        self.pending.retain(|handle| arena.contains(handle.id()));
        gone.len()
    }

    /// Takes the workers' results, frees the released assets, and
    /// finalizes the decoded ones in the order they were loaded, passing
    /// over those still decoding, while the budget lasts.
    pub(crate) fn finish(
        &mut self,
        ctx: &mut FinalizeCtx<'_>,
        on_failure: OnFailure,
        budget: &mut Budget,
    ) {
        self.receive();
        for (path, asset) in self.released.drain(..) {
            if let Err(cause) = asset.release(ctx) {
                warn!("asset `{path}`: it could not be freed: {cause}");
            }
        }
        let arena = &mut self.arena;
        self.pending.retain(|&handle| {
            let Some(entry) = arena.get_mut(handle.id()) else {
                return false;
            };
            match entry.slot {
                Slot::Decoding => true,
                Slot::Pending(_) if budget.spent() => true,
                Slot::Pending(_) => {
                    finalize(entry, ctx, on_failure);
                    false
                }
                Slot::Loaded(_) | Slot::Failed { .. } | Slot::Finalizing => false,
            }
        });
    }
}

/// Makes the asset in `entry`, which is decoded, or its placeholder if that
/// fails.
fn finalize<T: Asset>(entry: &mut Entry<T>, ctx: &mut FinalizeCtx<'_>, on_failure: OnFailure) {
    let result = match mem::replace(&mut entry.slot, Slot::Finalizing) {
        Slot::Pending(Ok(decoded)) => T::finalize(decoded, ctx),
        Slot::Pending(Err(error)) => Err(error),
        done => {
            entry.slot = done;
            return;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DecodeCtx;

    struct Nothing;

    impl Asset for Nothing {
        type Decoded = ();
        const EXTENSIONS: &'static [&'static str] = &["x"];

        fn decode(_: &[u8], _: &DecodeCtx<'_>) -> Result<(), AssetError> {
            Ok(())
        }

        fn finalize(_: (), _: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
            Ok(Self)
        }
    }

    #[test]
    fn the_assets_of_a_type_can_be_shared_between_threads() {
        fn shared<A: Send + Sync>() {}
        shared::<Assets<Nothing>>();
    }
}
