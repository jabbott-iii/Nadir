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

//! The assets of one type, by handle and by path.

use std::borrow::Cow;
use std::mem;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use nadir_core::log::{error, info, warn};
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

/// What a worker sends back: the asset, which of its reads this was, and
/// what reading and decoding its file made.
pub(crate) type Decoded<T> = (Handle<T>, u64, Result<<T as Asset>::Decoded, AssetError>);

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
    /// How many reads of its file were asked of the workers: the number of
    /// the latest.
    reads: u64,
    /// The number of the latest read whose result came back, 0 for none. An
    /// earlier read that comes back after it is dropped.
    landed: u64,
    /// A reload's result, waiting to be made in place of the value it has:
    /// the asset stays as it was until then.
    reloaded: Option<Result<T::Decoded, AssetError>>,
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
    /// Those with a reload out: being read, decoded, or waiting to be made.
    reloading: Vec<Handle<T>>,
    /// Those a reload was made for, once each, until they are drained.
    swapped: Vec<Handle<T>>,
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
            reloading: Vec::new(),
            swapped: Vec::new(),
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

    /// Whether some loads, reloads or assets to free wait for
    /// [`AssetServer::finish_completed_loads`](crate::AssetServer::finish_completed_loads).
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
            || !self.released.is_empty()
            || !self.reloading.is_empty()
            || self.in_flight > 0
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
            reads: 0,
            landed: 0,
            reloaded: None,
        }));
        self.by_path.insert(path.to_string(), handle);
        self.pending.push(handle);
        handle
    }

    /// A read of the file of the asset under `handle`, which is there: where
    /// the job that reads and decodes it sends its result, and the read's
    /// number, which the result carries. The job must send one.
    pub(crate) fn job(&mut self, handle: Handle<T>) -> (Sender<Decoded<T>>, u64) {
        let entry = self
            .arena
            .get_mut(handle.id())
            .expect("a job is asked only for an asset that is there");
        entry.reads += 1;
        self.in_flight += 1;
        (self.done.clone(), entry.reads)
    }

    /// Asks for the asset at `path`, which is lowercased, to be read again,
    /// if there is one: its handle, and the read's ends, as
    /// [`Assets::job`] gives them. Until the read is made, by a finish, the
    /// asset keeps the value it has.
    pub(crate) fn reload(&mut self, path: &str) -> Option<(Handle<T>, Sender<Decoded<T>>, u64)> {
        let handle = *self.by_path.get(path)?;
        let (done, read) = self.job(handle);
        if !self.reloading.contains(&handle) {
            self.reloading.push(handle);
        }
        Some((handle, done, read))
    }

    /// Whether an asset of this type is at `path`, which is lowercased.
    pub(crate) fn holds(&self, path: &str) -> bool {
        self.by_path.contains_key(path)
    }

    /// The paths of the assets of this type.
    pub(crate) fn paths(&self) -> impl Iterator<Item = &str> {
        self.by_path.keys().map(String::as_str)
    }

    /// Takes the handles of the assets a reload was made for since the last
    /// call, each once, in the order they were made.
    pub(crate) fn drain_swapped(&mut self) -> std::vec::Drain<'_, Handle<T>> {
        self.swapped.drain(..)
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

    fn take(&mut self, (handle, read, result): Decoded<T>) {
        self.in_flight = self.in_flight.saturating_sub(1);
        // An asset unloaded while it was decoding is no longer there.
        let Some(entry) = self.arena.get_mut(handle.id()) else {
            return;
        };
        // Up to four workers read at once, so two reads of a file can come
        // back out of order: only the later read may land.
        if read <= entry.landed {
            return;
        }
        entry.landed = read;
        match entry.slot {
            // Not made yet: made from the latest read.
            Slot::Decoding | Slot::Pending(_) => entry.slot = Slot::Pending(result),
            Slot::Loaded(_) | Slot::Failed { .. } => entry.reloaded = Some(result),
            // Only while a finish makes it, which holds the assets.
            Slot::Finalizing => {}
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
        self.reloading.retain(|handle| arena.contains(handle.id()));
        self.swapped.retain(|handle| arena.contains(handle.id()));
        gone.len()
    }

    /// Takes the workers' results, frees the released assets, and
    /// finalizes the decoded ones in the order they were loaded, passing
    /// over those still decoding, while the budget lasts; then makes the
    /// reloads read, in the order they were asked for, while it still
    /// lasts.
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
        let swapped = &mut self.swapped;
        self.reloading.retain(|&handle| {
            let Some(entry) = arena.get_mut(handle.id()) else {
                return false;
            };
            if entry.reloaded.is_some() {
                if budget.spent() {
                    return true;
                }
                if swap(entry, ctx) && !swapped.contains(&handle) {
                    swapped.push(handle);
                }
            }
            // A later read is still out, and may change it again.
            entry.reads > entry.landed
        });
    }
}

/// Makes the value a reload read for the asset in `entry`, and swaps it in
/// for the one the asset has, which is then released, a failed asset's
/// placeholder too. A reload that fails is logged with the path, and the
/// asset keeps its value. Returns whether it swapped.
fn swap<T: Asset>(entry: &mut Entry<T>, ctx: &mut FinalizeCtx<'_>) -> bool {
    let Some(result) = entry.reloaded.take() else {
        return false;
    };
    match result.and_then(|decoded| T::finalize(decoded, ctx)) {
        Ok(asset) => {
            let replaced = match mem::replace(&mut entry.slot, Slot::Loaded(asset)) {
                Slot::Loaded(old)
                | Slot::Failed {
                    placeholder: Some(old),
                    ..
                } => Some(old),
                // A reload is kept only for a made asset, so these are not
                // replaced.
                Slot::Decoding
                | Slot::Pending(_)
                | Slot::Failed {
                    placeholder: None, ..
                }
                | Slot::Finalizing => None,
            };
            if let Some(old) = replaced
                && let Err(cause) = old.release(ctx)
            {
                warn!(
                    "asset `{}`: the value it replaced could not be freed: {cause}",
                    entry.path
                );
            }
            info!("asset `{}`: reloaded", entry.path);
            true
        }
        Err(failure) => {
            error!(
                "asset `{}`: its reload failed, so it keeps the value it had: {failure}",
                entry.path
            );
            false
        }
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

    /// A number, whose releases are counted in the context's `u32`.
    #[derive(Debug, PartialEq)]
    struct Number(u32);

    impl Asset for Number {
        type Decoded = u32;
        const EXTENSIONS: &'static [&'static str] = &["n"];

        fn decode(_: &[u8], _: &DecodeCtx<'_>) -> Result<u32, AssetError> {
            Ok(0)
        }

        fn finalize(n: u32, _: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
            Ok(Self(n))
        }

        fn release(self, ctx: &mut FinalizeCtx<'_>) -> Result<(), AssetError> {
            *ctx.get_mut::<u32>()? += 1;
            Ok(())
        }
    }

    /// Finishes with no budget, and returns how many were released.
    fn finish(assets: &mut Assets<Number>) -> u32 {
        let mut released = 0u32;
        assets.finish(
            &mut FinalizeCtx::new().with(&mut released),
            OnFailure::Nothing,
            &mut Budget::until(None),
        );
        released
    }

    fn value(assets: &Assets<Number>, handle: Handle<Number>) -> Option<u32> {
        assets.get(handle).map(|number| number.0)
    }

    /// An asset at `a.n` made with the value 1.
    fn loaded() -> (Assets<Number>, Handle<Number>) {
        let mut assets = Assets::new();
        let handle = assets.insert("a.n", Scope::Global, Slot::Pending(Ok(1)));
        assert_eq!(finish(&mut assets), 0);
        assert_eq!(value(&assets, handle), Some(1));
        (assets, handle)
    }

    #[test]
    fn of_two_reloads_finished_out_of_order_the_later_read_stays() {
        let (mut assets, handle) = loaded();
        let (_, second, read_2) = assets.reload("a.n").expect("an asset at the path");
        let (_, third, read_3) = assets.reload("a.n").expect("an asset at the path");
        assert!(read_2 < read_3);
        third
            .send((handle, read_3, Ok(3)))
            .expect("the assets hold the receiver");
        second
            .send((handle, read_2, Ok(2)))
            .expect("the assets hold the receiver");
        assert_eq!(finish(&mut assets), 1, "the value it had, released once");
        assert_eq!(value(&assets, handle), Some(3));
        assert_eq!(assets.drain_swapped().collect::<Vec<_>>(), [handle]);
        assert!(!assets.has_pending());
    }

    #[test]
    fn a_read_that_comes_back_after_a_later_one_was_made_is_dropped() {
        let (mut assets, handle) = loaded();
        let (_, second, read_2) = assets.reload("a.n").expect("an asset at the path");
        let (_, third, read_3) = assets.reload("a.n").expect("an asset at the path");
        third
            .send((handle, read_3, Ok(3)))
            .expect("the assets hold the receiver");
        assert_eq!(finish(&mut assets), 1);
        assert_eq!(value(&assets, handle), Some(3));
        // The earlier read is still out, so the asset waits for it.
        assert!(assets.has_pending());
        second
            .send((handle, read_2, Ok(2)))
            .expect("the assets hold the receiver");
        assert_eq!(finish(&mut assets), 0, "nothing was swapped");
        assert_eq!(value(&assets, handle), Some(3));
        assert_eq!(assets.drain_swapped().collect::<Vec<_>>(), [handle]);
        assert!(!assets.has_pending());
    }

    #[test]
    fn a_reload_of_an_asset_still_loading_lands_as_its_first_value() {
        let mut assets = Assets::<Number>::new();
        let handle = assets.insert("a.n", Scope::Global, Slot::Decoding);
        let (first, read_1) = assets.job(handle);
        let (_, second, read_2) = assets.reload("a.n").expect("an asset at the path");
        second
            .send((handle, read_2, Ok(2)))
            .expect("the assets hold the receiver");
        first
            .send((handle, read_1, Ok(1)))
            .expect("the assets hold the receiver");
        assert_eq!(finish(&mut assets), 0, "nothing was made before it");
        assert_eq!(value(&assets, handle), Some(2));
        assert_eq!(
            assets.drain_swapped().count(),
            0,
            "a first value swaps nothing"
        );
        assert!(!assets.has_pending());
    }

    #[test]
    fn a_reload_waits_for_the_budget_and_keeps_the_value_until_then() {
        let (mut assets, handle) = loaded();
        let (_, done, read) = assets.reload("a.n").expect("an asset at the path");
        done.send((handle, read, Ok(2)))
            .expect("the assets hold the receiver");
        // A budget spent at once still makes one, so make two loads wait
        // in front of it.
        let other = assets.insert("b.n", Scope::Global, Slot::Pending(Ok(5)));
        let mut released = 0u32;
        let past = Instant::now().checked_sub(std::time::Duration::from_secs(1));
        assets.finish(
            &mut FinalizeCtx::new().with(&mut released),
            OnFailure::Nothing,
            &mut Budget::until(past),
        );
        assert_eq!(value(&assets, other), Some(5));
        assert_eq!(
            value(&assets, handle),
            Some(1),
            "it waits for the next finish"
        );
        assert_eq!(assets.state(handle), Some(LoadState::Loaded));
        assert!(assets.has_pending());
        assert_eq!(finish(&mut assets), 1);
        assert_eq!(value(&assets, handle), Some(2));
    }

    #[test]
    fn a_handle_swapped_twice_before_a_drain_is_drained_once() {
        let (mut assets, handle) = loaded();
        for n in [2, 3] {
            let (_, done, read) = assets.reload("a.n").expect("an asset at the path");
            done.send((handle, read, Ok(n)))
                .expect("the assets hold the receiver");
            assert_eq!(finish(&mut assets), 1);
        }
        assert_eq!(assets.drain_swapped().collect::<Vec<_>>(), [handle]);
        assert_eq!(assets.drain_swapped().count(), 0);
    }

    #[test]
    fn an_earlier_read_made_while_a_later_one_is_out_is_replaced_by_it() {
        let (mut assets, handle) = loaded();
        let (_, second, read_2) = assets.reload("a.n").expect("an asset at the path");
        let (_, third, read_3) = assets.reload("a.n").expect("an asset at the path");
        second
            .send((handle, read_2, Ok(2)))
            .expect("the assets hold the receiver");
        assert_eq!(finish(&mut assets), 1);
        assert_eq!(value(&assets, handle), Some(2));
        assert!(assets.has_pending(), "the later read is still out");
        third
            .send((handle, read_3, Ok(3)))
            .expect("the assets hold the receiver");
        assert_eq!(finish(&mut assets), 1);
        assert_eq!(value(&assets, handle), Some(3));
        assert!(!assets.has_pending());
    }

    #[test]
    fn a_reload_out_when_its_asset_is_made_now_stays_out() {
        let mut assets = Assets::<Number>::new();
        let handle = assets.insert("a.n", Scope::Global, Slot::Decoding);
        let (first, read_1) = assets.job(handle);
        let (_, second, read_2) = assets.reload("a.n").expect("an asset at the path");
        first
            .send((handle, read_1, Ok(1)))
            .expect("the assets hold the receiver");
        let mut released = 0u32;
        assert_eq!(
            assets.finish_now(
                handle,
                &mut FinalizeCtx::new().with(&mut released),
                OnFailure::Nothing
            ),
            Ok(())
        );
        assert_eq!(value(&assets, handle), Some(1));
        assert!(assets.has_pending(), "the reload is still out");
        second
            .send((handle, read_2, Ok(2)))
            .expect("the assets hold the receiver");
        assert_eq!(finish(&mut assets), 1);
        assert_eq!(value(&assets, handle), Some(2));
        assert_eq!(assets.drain_swapped().collect::<Vec<_>>(), [handle]);
        assert!(!assets.has_pending());
    }

    #[test]
    fn nothing_is_reloaded_at_a_path_no_asset_is_at() {
        let (mut assets, _) = loaded();
        assert!(assets.reload("b.n").is_none());
        assert!(!assets.has_pending());
    }
}
