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

//! The asset server: loads by path on worker threads, and finalizes once a
//! frame.

use std::any::{Any, TypeId};
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use nadir_codec::Limits;
use nadir_core::{FastHashMap, Handle};

use crate::assets::{self, Assets, Budget, Decoded, Slot};
use crate::watch::{self, Watch};
use crate::workers::Workers;
use crate::{Asset, AssetError, AssetPath, DecodeCtx, FinalizeCtx, LoadState, LooseSource, Source};

/// What a failed load leaves behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OnFailure {
    /// The asset type's placeholder, such as a magenta and black checker
    /// for a texture, so that the failure shows: for development.
    Placeholder,
    /// Nothing: [`AssetServer::get`] gives `None`.
    Nothing,
}

/// How long an asset stays loaded: until every scope it was loaded in is
/// unloaded, with [`AssetServer::unload_scope`].
///
/// Loading the same path in another scope gives the same asset, which then
/// stays until both scopes are unloaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The whole game: the font, the player, the menus.
    /// [`AssetServer::load`] loads in it.
    Global,
    /// The level being played, unloaded when the game leaves it.
    Level,
    /// A while shorter than a level: a cutscene, a screen.
    Transient,
}

/// The assets of each type, as [`Assets`] boxed and typed away.
trait Storage: Any {
    fn finish(&mut self, ctx: &mut FinalizeCtx<'_>, on_failure: OnFailure, budget: &mut Budget);
    fn wait(&mut self);
    fn unload(&mut self, scope: Scope) -> usize;
    fn has_pending(&self) -> bool;
    /// Queues a read of the asset at `path`, which is lowercased, if this
    /// type has one there that can be read. Returns whether it did.
    fn reload(&mut self, path: &str, reader: &Reader<'_>) -> bool;
    /// Whether this type has an asset at `path`, which is lowercased.
    fn holds(&self, path: &str) -> bool;
    /// Calls `each` with the path of each of this type's assets.
    fn paths(&self, each: &mut dyn FnMut(&str));
}

/// The assets of each type, in the order their types' first loads came.
#[derive(Default)]
struct Storages {
    list: Vec<Box<dyn Storage>>,
    index: FastHashMap<TypeId, usize>,
    /// Which type goes first in the next budgeted finish: they take turns,
    /// so that one type's backlog cannot hold the others back for good.
    turn: usize,
}

impl Storages {
    fn get<T: Asset>(&self) -> Option<&Assets<T>> {
        let storage: &dyn Any = self
            .list
            .get(*self.index.get(&TypeId::of::<T>())?)?
            .as_ref();
        storage.downcast_ref()
    }

    /// The assets of type `T`, if any was loaded.
    fn existing_mut<T: Asset>(&mut self) -> Option<&mut Assets<T>> {
        let storage: &mut dyn Any = self
            .list
            .get_mut(*self.index.get(&TypeId::of::<T>())?)?
            .as_mut();
        storage.downcast_mut()
    }

    /// The assets of type `T`, made empty if there are none yet.
    fn get_mut<T: Asset>(&mut self) -> &mut Assets<T> {
        let list = &mut self.list;
        let index = *self.index.entry(TypeId::of::<T>()).or_insert_with(|| {
            list.push(Box::new(Assets::<T>::new()));
            list.len() - 1
        });
        let storage: &mut dyn Any = list[index].as_mut();
        storage
            .downcast_mut()
            .expect("the storage under a type's index holds assets of that type")
    }

    /// Every type's assets, from the one whose turn it is if `rotate`, and
    /// from the first otherwise; the next call with `rotate` starts one
    /// later.
    fn each(&mut self, rotate: bool) -> impl Iterator<Item = &mut Box<dyn Storage>> {
        let first = if rotate && !self.list.is_empty() {
            let first = self.turn % self.list.len();
            self.turn = first + 1;
            first
        } else {
            0
        };
        let (before, after) = self.list.split_at_mut(first);
        after.iter_mut().chain(before.iter_mut())
    }
}

impl<T: Asset> Storage for Assets<T> {
    fn finish(&mut self, ctx: &mut FinalizeCtx<'_>, on_failure: OnFailure, budget: &mut Budget) {
        Assets::finish(self, ctx, on_failure, budget);
    }

    fn wait(&mut self) {
        Assets::wait(self);
    }

    fn unload(&mut self, scope: Scope) -> usize {
        Assets::unload(self, scope)
    }

    fn has_pending(&self) -> bool {
        Assets::has_pending(self)
    }

    fn reload(&mut self, path: &str, reader: &Reader<'_>) -> bool {
        // A path that failed its type's check has no file to read again.
        if !self.holds(path) {
            return false;
        }
        let Ok(checked) = checked::<T>(path) else {
            return false;
        };
        let Some((handle, done, read)) = Assets::reload(self, checked.as_str()) else {
            return false;
        };
        reader.queue(checked, handle, (done, read));
        true
    }

    fn holds(&self, path: &str) -> bool {
        Assets::holds(self, path)
    }

    fn paths(&self, each: &mut dyn FnMut(&str)) {
        for path in Assets::paths(self) {
            each(path);
        }
    }
}

/// What a job needs to read and decode a file: the server's workers,
/// source and limits, and its watcher's paths if it watches.
struct Reader<'a> {
    workers: &'a Workers,
    source: &'a Arc<dyn Source>,
    limits: Limits,
    watch: Option<&'a Arc<watch::Shared>>,
}

impl<'a> Reader<'a> {
    fn new(server: &'a AssetServer) -> Self {
        Self {
            workers: &server.workers,
            source: &server.source,
            limits: server.limits,
            watch: server.watch.as_ref().map(Watch::shared),
        }
    }

    /// Queues the read of `path` for the asset under `handle`, the read
    /// [`Assets::job`] gave. With a watcher, the path's time of change is
    /// taken first, if no look and no read has taken it yet: a load's read
    /// or a reload's, whichever comes first, as either may be the one that
    /// lands.
    fn queue<T: Asset>(
        &self,
        path: AssetPath,
        handle: Handle<T>,
        (done, read): (Sender<Decoded<T>>, u64),
    ) {
        let (source, limits) = (Arc::clone(self.source), self.limits);
        let watch = self.watch.map(Arc::clone);
        self.workers.run(Box::new(move || {
            let result = read_and_decode::<T>(source.as_ref(), &path, limits, watch.as_deref());
            // Fails only once the server is gone, when nothing waits for it.
            let _ = done.send((handle, read, result));
        }));
    }
}

/// `path` as an [`AssetPath`] of an asset of type `T`: its last part ends
/// in one of [`Asset::EXTENSIONS`].
fn checked<T: Asset>(path: &str) -> Result<AssetPath, AssetError> {
    let path = AssetPath::new(path)?;
    match path.extension() {
        Some(extension) if T::EXTENSIONS.contains(&extension) => Ok(path),
        _ => Err(AssetError::Extension {
            expected: T::EXTENSIONS,
        }),
    }
}

/// Reads the file at `path` from `source` and decodes it, as a worker does.
/// With `watch`, the path's time of change is taken first, if the watcher
/// has not taken it yet. A panic in any of them fails the asset, and goes
/// no further.
fn read_and_decode<T: Asset>(
    source: &dyn Source,
    path: &AssetPath,
    limits: Limits,
    watch: Option<&watch::Shared>,
) -> Result<T::Decoded, AssetError> {
    panic::catch_unwind(AssertUnwindSafe(|| {
        if let Some(watch) = watch {
            watch.before_read(path, source);
        }
        let bytes = source.read(path)?;
        T::decode(&bytes, &DecodeCtx::new(path.as_str(), limits))
    }))
    .unwrap_or_else(|_| {
        Err(AssetError::Invalid(
            "reading or decoding it panicked".to_string(),
        ))
    })
}

/// How long [`AssetServer::finish_completed_loads`] spends making assets,
/// unless [`AssetServer::set_finish_budget`] changes it.
const FINISH_BUDGET: Duration = Duration::from_millis(2);

/// Loads assets from a [`Source`] on worker threads, and keeps them.
///
/// [`AssetServer::load`] returns at once, and a worker reads and decodes the
/// file. [`AssetServer::finish_completed_loads`] then makes the asset on the
/// main thread, once a frame, for at most a couple of milliseconds a frame.
/// A load finishes some frames later, never at a frame that can be known in
/// advance, so tick systems must not depend on load state (ADR-0025):
/// [`AssetServer::finish_all_loads`] waits for every load instead.
///
/// Handles are plain ids: nothing counts them. An asset stays until every
/// [`Scope`] it was loaded in is unloaded.
///
/// The server has 1 to 4 worker threads, two less than the cores, which
/// leaves the main thread a core and one to spare. If none can start, it
/// reads and decodes on the thread that asks for the load. Dropping it waits
/// for the workers' current files.
pub struct AssetServer {
    // Dropped first, so that the workers stop before the assets go.
    workers: Workers,
    /// The watcher, once [`AssetServer::start_watcher`] started it.
    watch: Option<Watch>,
    source: Arc<dyn Source>,
    on_failure: OnFailure,
    limits: Limits,
    finish_budget: Duration,
    storages: Storages,
}

impl AssetServer {
    /// A server that reads loose files under `root`, as a [`LooseSource`],
    /// with the codecs' default limits.
    pub fn new(root: impl Into<PathBuf>, on_failure: OnFailure) -> Self {
        Self::with_source(LooseSource::new(root), on_failure)
    }

    /// A server that reads from `source`, with the codecs' default limits.
    pub fn with_source(source: impl Source + 'static, on_failure: OnFailure) -> Self {
        Self {
            workers: Workers::new(Workers::count()),
            watch: None,
            source: Arc::new(source),
            on_failure,
            limits: Limits::default(),
            finish_budget: FINISH_BUDGET,
            storages: Storages::default(),
        }
    }

    /// Loads the asset at `path`, a path under the source's root such as
    /// `sprites/hero.qoi`, in [`Scope::Global`], and returns its handle at
    /// once. [`AssetServer::load_in`] says how.
    pub fn load<T: Asset>(&mut self, path: &str) -> Handle<T> {
        self.load_in(path, Scope::Global)
    }

    /// Loads the asset at `path` in `scope`, and returns its handle at once.
    ///
    /// The same path, once lowercased, gives the same handle, whatever
    /// happened to its load, until every scope it was loaded in is
    /// unloaded; it is then in `scope` too. Otherwise a worker reads and
    /// decodes the file, and a later
    /// [`AssetServer::finish_completed_loads`] makes the asset. Until then
    /// it is [`LoadState::Loading`]. A failure, a missing file say, shows
    /// once it is finished: [`LoadState::Failed`], logged with the path.
    ///
    /// The path is lowercased and checked as an [`AssetPath`], and its last
    /// part must end in one of [`Asset::EXTENSIONS`].
    pub fn load_in<T: Asset>(&mut self, path: &str, scope: Scope) -> Handle<T> {
        let path = assets::lowercase(path);
        let path = path.as_ref();
        let assets = self.storages.get_mut::<T>();
        if let Some(handle) = assets.find(path, scope) {
            return handle;
        }
        let path = match checked::<T>(path) {
            Ok(path) => path,
            Err(error) => return assets.insert(path, scope, Slot::Pending(Err(error))),
        };
        let handle = assets.insert(path.as_str(), scope, Slot::Decoding);
        let job = assets.job(handle);
        if let Some(watch) = &self.watch {
            watch.shared().watch(&path);
        }
        Reader::new(self).queue(path, handle, job);
        handle
    }

    /// Loads the asset at `path` in [`Scope::Global`], and makes it before
    /// returning. [`AssetServer::load_now_in`] says how.
    ///
    /// # Errors
    ///
    /// Why the asset failed to load, as [`AssetServer::error`] gives it.
    pub fn load_now<T: Asset>(
        &mut self,
        path: &str,
        ctx: &mut FinalizeCtx<'_>,
    ) -> Result<Handle<T>, AssetError> {
        self.load_now_in(path, Scope::Global, ctx)
    }

    /// Loads the asset at `path` in `scope`, and makes it with the
    /// resources in `ctx` before returning: for the data a game needs to
    /// set its world up before the first tick, such as an atlas's regions,
    /// which needs no resource the game does not have yet.
    ///
    /// `ctx` must hold every resource [`Asset::finalize`] needs: an asset
    /// made without one fails, as a finish without it would, and a failure
    /// is final. While a game sets up, before its window opens, there is no
    /// GPU device to give, so only assets that need none, such as an atlas,
    /// can be made then.
    ///
    /// It gives the handle [`AssetServer::load_in`] would. A new path is
    /// read and decoded on this thread, and made at once. A path loaded
    /// before keeps its load: one a worker is still decoding is waited for,
    /// and one waiting to be made is made now; one already made, or
    /// failed, is as it was. No other asset is made or freed, and no finish
    /// budget is spent.
    ///
    /// # Errors
    ///
    /// Why the asset failed to load, as [`AssetServer::error`] gives it. It
    /// is then [`LoadState::Failed`] under its handle, with its placeholder
    /// if placeholders are shown, and logged with its path, as any failure
    /// is. [`AssetServer::load`] of the path gives that handle.
    pub fn load_now_in<T: Asset>(
        &mut self,
        path: &str,
        scope: Scope,
        ctx: &mut FinalizeCtx<'_>,
    ) -> Result<Handle<T>, AssetError> {
        let path = assets::lowercase(path);
        let path = path.as_ref();
        let assets = self.storages.get_mut::<T>();
        let handle = match assets.find(path, scope) {
            Some(handle) => handle,
            None => {
                let watch = self.watch.as_ref().map(Watch::shared);
                let result = checked::<T>(path).and_then(|checked| {
                    if let Some(watch) = watch {
                        watch.watch(&checked);
                    }
                    read_and_decode::<T>(
                        self.source.as_ref(),
                        &checked,
                        self.limits,
                        watch.map(Arc::as_ref),
                    )
                });
                assets.insert(path, scope, Slot::Pending(result))
            }
        };
        assets.finish_now(handle, ctx, self.on_failure)?;
        Ok(handle)
    }

    /// Unloads `scope`: every asset loaded in it and in no other scope goes,
    /// and returns how many did.
    ///
    /// Their handles name nothing from now on: [`AssetServer::state`] gives
    /// `None`, and loading the path again loads it anew. The next
    /// [`AssetServer::finish_completed_loads`] frees them with
    /// [`Asset::release`], their placeholders too; one still being read is
    /// dropped once its worker is done with it.
    pub fn unload_scope(&mut self, scope: Scope) -> usize {
        let gone = self
            .storages
            .each(false)
            .map(|storage| storage.unload(scope))
            .sum();
        // A path no asset of any type is at is no longer watched.
        if gone > 0
            && let Some(watch) = &self.watch
        {
            let list = &self.storages.list;
            watch
                .shared()
                .retain(|path| list.iter().any(|storage| storage.holds(path)));
        }
        gone
    }

    /// The asset, once loaded. For a failed one, with
    /// [`OnFailure::Placeholder`], its placeholder.
    pub fn get<T: Asset>(&self, handle: Handle<T>) -> Option<&T> {
        self.assets::<T>()?.get(handle)
    }

    /// Where the load is, or `None` if this server holds nothing under the
    /// handle.
    ///
    /// A handle is a plain id. A handle from another server can name an
    /// asset of this one, so keep each handle with its server.
    pub fn state<T: Asset>(&self, handle: Handle<T>) -> Option<LoadState> {
        self.assets::<T>()?.state(handle)
    }

    /// Why the load failed, once it has.
    pub fn error<T: Asset>(&self, handle: Handle<T>) -> Option<&AssetError> {
        self.assets::<T>()?.error(handle)
    }

    /// The path the asset was loaded from.
    pub fn path<T: Asset>(&self, handle: Handle<T>) -> Option<&str> {
        self.assets::<T>()?.path(handle)
    }

    /// The assets of type `T`, if any was loaded.
    pub fn assets<T: Asset>(&self) -> Option<&Assets<T>> {
        self.storages.get()
    }

    /// Whether some loads or reloads, or some assets of unloaded scopes,
    /// wait for [`AssetServer::finish_completed_loads`]. When none do, a
    /// game need not make a [`FinalizeCtx`] at all.
    pub fn has_pending(&self) -> bool {
        self.storages
            .list
            .iter()
            .any(|storage| storage.has_pending())
    }

    /// Sets how long [`AssetServer::finish_completed_loads`] may spend
    /// making assets: 2 ms unless set. Each call makes one asset at least,
    /// so a single slow one can take longer.
    pub fn set_finish_budget(&mut self, budget: Duration) {
        self.finish_budget = budget;
    }

    /// Makes the assets the workers have decoded since the last call, with
    /// the resources in `ctx`, until the finish budget is spent; the rest
    /// wait for the next call. It also frees the assets of unloaded scopes,
    /// all of them, outside the budget. Call it on the main thread, once a
    /// frame. It never waits for a worker.
    ///
    /// The assets of a type are made in the order they were loaded, passing
    /// over those still decoding, so a later load can be made first; then
    /// its reloads, in the order they were asked for, each releasing the
    /// value it replaces ([`AssetServer::reload`]). The types share the
    /// budget and take turns at going first, one call after another.
    ///
    /// A failure is logged with the asset's path, and with
    /// [`OnFailure::Placeholder`] the asset gets its type's placeholder.
    pub fn finish_completed_loads(&mut self, ctx: &mut FinalizeCtx<'_>) {
        let mut budget = Budget::until(Instant::now().checked_add(self.finish_budget));
        for storage in self.storages.each(true) {
            storage.finish(ctx, self.on_failure, &mut budget);
        }
    }

    /// Waits for the workers to read and decode every file asked for, then
    /// makes every asset, as [`AssetServer::finish_completed_loads`] does
    /// but with no budget: for tools, tests, and frames that must show
    /// every asset loaded before them. It makes them type by type, in the
    /// order of each type's first load, and each type's in the order they
    /// were loaded, so the same loads are made in the same order every time.
    pub fn finish_all_loads(&mut self, ctx: &mut FinalizeCtx<'_>) {
        let mut budget = Budget::until(None);
        for storage in self.storages.each(false) {
            storage.wait();
            storage.finish(ctx, self.on_failure, &mut budget);
        }
    }

    /// Reads the file at `path` again, for every asset at that path, of any
    /// type, and returns how many reads it queued: an asset whose path
    /// failed its type's check has no file to read again. The path is
    /// lowercased first, as for [`AssetServer::load`].
    ///
    /// A worker reads and decodes the file, and a later finish makes the
    /// new value, within the finish budget, and swaps it in behind the same
    /// handle; the value it replaces, or a failed asset's placeholder, is
    /// released with [`Asset::release`] then.
    /// [`AssetServer::drain_swapped`] gives the handles swapped. Until then
    /// the asset keeps the value and the [`LoadState`] it has, and
    /// [`AssetServer::has_pending`] is true.
    ///
    /// A reload that fails, to read, decode or make, is logged with its
    /// path, and the asset keeps the value it had: a failed asset stays
    /// failed. Of two reloads of a path, the later read's value is the one
    /// that stays, whichever finishes first. An asset still loading is made
    /// from the latest read, its reload's, which is then its first value,
    /// or its failure. An asset unloaded while it reloads is dropped, as a
    /// load is.
    pub fn reload(&mut self, path: &str) -> usize {
        let path = assets::lowercase(path);
        let reader = Reader {
            workers: &self.workers,
            source: &self.source,
            limits: self.limits,
            watch: self.watch.as_ref().map(Watch::shared),
        };
        let mut reloads = 0;
        for storage in &mut self.storages.list {
            if storage.reload(&path, &reader) {
                reloads += 1;
            }
        }
        reloads
    }

    /// Starts the watcher, a thread, `nadir-assets-watch`, that asks the
    /// source when the file of each loaded path last changed
    /// ([`Source::modified`]) every 250 ms, and hands the paths that changed
    /// to [`AssetServer::reload_changed`]. Starting it again does nothing.
    ///
    /// A path is watched from just before its file is first read, so a
    /// change after the read is seen if it gives the file a new time of
    /// change, and until no asset of any type is at it. A path loaded
    /// before the watcher started is watched from the watcher's first look
    /// at it. On a system that keeps coarse times, two writes within one
    /// tick of its clock look like one. A source that cannot tell, such as
    /// a [`PackSource`](crate::PackSource), shows no change. Dropping the
    /// server stops the thread.
    ///
    /// # Errors
    ///
    /// Why the thread could not start.
    pub fn start_watcher(&mut self) -> io::Result<()> {
        if self.watch.is_some() {
            return Ok(());
        }
        let watch = Watch::start(Arc::clone(&self.source))?;
        for storage in &self.storages.list {
            storage.paths(&mut |path| {
                // A path that is not an asset path has no file to watch.
                if let Ok(path) = AssetPath::new(path) {
                    watch.shared().watch(&path);
                }
            });
        }
        self.watch = Some(watch);
        Ok(())
    }

    /// Reloads the assets at each path the watcher saw change since the
    /// last call, as [`AssetServer::reload`] does, and returns how many it
    /// reloaded: none without a watcher. Call it on the main thread, once a
    /// frame, whether loads are pending or not. It never waits, and when
    /// nothing changed, it allocates nothing.
    pub fn reload_changed(&mut self) -> usize {
        let mut reloads = 0;
        while let Some(path) = self.watch.as_mut().and_then(Watch::next_changed) {
            reloads += self.reload(path.as_str());
        }
        reloads
    }

    /// Takes the handles of the assets of type `T` that a reload was swapped
    /// in for since the last call, each once, in the order they were
    /// swapped: for what a game copied out of them, such as an atlas's
    /// regions, to be found again. A handle unloaded since is left out.
    pub fn drain_swapped<T: Asset>(&mut self) -> impl Iterator<Item = Handle<T>> + '_ {
        self.storages
            .existing_mut::<T>()
            .into_iter()
            .flat_map(Assets::drain_swapped)
    }
}
