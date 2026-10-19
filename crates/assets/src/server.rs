//! The asset server: loads by path, and finalizes once a frame.

use std::any::{Any, TypeId};
use std::path::PathBuf;
use std::{fs, io};

use nadir_codec::Limits;
use nadir_core::{FastHashMap, Handle};

use crate::assets::{self, Assets};
use crate::{Asset, AssetError, DecodeCtx, FinalizeCtx, LoadState, path};

/// What a failed load leaves behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OnFailure {
    /// The asset type's placeholder, such as a magenta and black checker
    /// for a texture, so that the failure shows: for development.
    Placeholder,
    /// Nothing: [`AssetServer::get`] gives `None`.
    Nothing,
}

/// The assets of each type, as [`Assets`] boxed and typed away.
trait Storage: Any {
    fn finish(&mut self, ctx: &mut FinalizeCtx<'_>, on_failure: OnFailure);
    fn has_pending(&self) -> bool;
}

impl<T: Asset> Storage for Assets<T> {
    fn finish(&mut self, ctx: &mut FinalizeCtx<'_>, on_failure: OnFailure) {
        Assets::finish(self, ctx, on_failure);
    }

    fn has_pending(&self) -> bool {
        Assets::has_pending(self)
    }
}

/// Loads assets from files under a root directory, and keeps them.
///
/// Loading is synchronous for now: [`AssetServer::load`] reads and decodes
/// the file at once. The asset is then made on the main thread by the next
/// [`AssetServer::finish_completed_loads`], as it will be once decoding moves
/// to worker threads (roadmap M5.3).
///
/// Handles are plain ids: nothing counts them, and assets stay until their
/// scope is unloaded (M5.3).
pub struct AssetServer {
    root: PathBuf,
    on_failure: OnFailure,
    limits: Limits,
    storages: FastHashMap<TypeId, Box<dyn Storage>>,
}

impl AssetServer {
    /// A server that reads files under `root`, with the codecs' default
    /// limits.
    pub fn new(root: impl Into<PathBuf>, on_failure: OnFailure) -> Self {
        Self {
            root: root.into(),
            on_failure,
            limits: Limits::default(),
            storages: FastHashMap::default(),
        }
    }

    /// Loads the asset at `path`, a path under the root such as
    /// `sprites/hero.qoi`, and returns its handle at once.
    ///
    /// The same path, once lowercased, gives the same handle, whatever happened to its load.
    /// Otherwise the file is read and decoded now, and the asset is made by
    /// the next [`AssetServer::finish_completed_loads`]. Until then it is
    /// [`LoadState::Loading`]. A failure, a missing file say, shows once it
    /// is finished: [`LoadState::Failed`], logged with the path.
    ///
    /// A path is lowercased (ASCII letters only), so `Sprites/Hero.qoi` is
    /// `sprites/hero.qoi`: the same asset, read from that file. It is then
    /// checked as the same string on every system: `/` between its parts,
    /// none of them empty, `.` or `..`, and the last ending in one of
    /// [`Asset::EXTENSIONS`].
    pub fn load<T: Asset>(&mut self, path: &str) -> Handle<T> {
        let path = assets::lowercase(path);
        let path = path.as_ref();
        let (root, limits) = (&self.root, self.limits);
        storage_mut::<T>(&mut self.storages).add(path, || {
            path::check(path).map_err(AssetError::BadPath)?;
            let extension = path
                .rsplit('/')
                .next()
                .and_then(|name| name.rsplit_once('.'))
                .map(|(_, extension)| extension);
            if !extension.is_some_and(|extension| T::EXTENSIONS.contains(&extension)) {
                return Err(AssetError::Extension {
                    expected: T::EXTENSIONS,
                });
            }
            let bytes = fs::read(root.join(path)).map_err(|error| match error.kind() {
                io::ErrorKind::NotFound => AssetError::NotFound,
                kind => AssetError::Read(kind),
            })?;
            T::decode(&bytes, &DecodeCtx::new(path, limits))
        })
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
        let storage: &dyn Any = self.storages.get(&TypeId::of::<T>())?.as_ref();
        storage.downcast_ref()
    }

    /// Whether some loads wait for [`AssetServer::finish_completed_loads`].
    /// When none do, a game need not make a [`FinalizeCtx`] at all.
    pub fn has_pending(&self) -> bool {
        self.storages.values().any(|storage| storage.has_pending())
    }

    /// Makes every asset loaded since the last call, with the resources in
    /// `ctx`: call it on the main thread, once a frame.
    ///
    /// A failure is logged with the asset's path, and with
    /// [`OnFailure::Placeholder`] the asset gets its type's placeholder.
    pub fn finish_completed_loads(&mut self, ctx: &mut FinalizeCtx<'_>) {
        for storage in self.storages.values_mut() {
            storage.finish(ctx, self.on_failure);
        }
    }
}

/// The assets of type `T`, made empty if there are none yet.
fn storage_mut<T: Asset>(storages: &mut FastHashMap<TypeId, Box<dyn Storage>>) -> &mut Assets<T> {
    let storage: &mut dyn Any = storages
        .entry(TypeId::of::<T>())
        .or_insert_with(|| Box::new(Assets::<T>::new()))
        .as_mut();
    storage
        .downcast_mut()
        .expect("the storage under a type's id holds assets of that type")
}
