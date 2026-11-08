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

//! Hot reload, under `dev` (roadmap M5.8, ADR-0042, ADR-0043): the game
//! reloads what changed while it runs. [`App::run`](crate::App::run)
//! starts it, and each drawn frame [`HotReload::frame`]:
//!
//! 1. finds again what the last finish swapped in: an atlas's clips by
//!    name ([`Clips::reload_atlas`]); a prefab, in the [`PrefabLibrary`];
//!    and whether the current level's file, or what it reads, changed;
//! 2. makes the current level again ([`reload_level`]) once no reload is
//!    pending, so that it reads every file of a cook at once;
//! 3. rebuilds the programs whose shader files changed;
//! 4. hands the files the asset watcher saw change to the asset server,
//!    which reads them on its workers for the next frames' finishes.
//!
//! A texture needs nothing: the renderer looks its handle up each frame.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use nadir_assets::{AssetServer, Handle, Scope};
use nadir_core::log::{error, info, warn};
use nadir_ecs::World;
use nadir_gfx::{Device, ShaderSource};
use nadir_render::{Atlas, Program, Renderer, Texture};
use nadir_scene::{Level, PrefabAsset, PrefabLibrary, TypeRegistry};

use crate::level::reload_level;
use crate::{Clips, CurrentLevel};

/// How often the shader watcher looks at the files' times of change: as
/// often as the asset watcher looks at the assets'.
const LOOK: Duration = Duration::from_millis(250);

// Each program has a bit of the shader watcher's changes.
const _: () = assert!(Program::ALL.len() <= 64);

/// What the hot reload did: a resource of every app under `dev`, for the
/// tests to read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Reloads {
    /// Whether the hot reload started.
    pub(crate) started: bool,
    /// Atlases whose clips were found again.
    pub(crate) atlases: u32,
    /// Prefabs replaced in the library.
    pub(crate) prefabs: u32,
    /// Times the current level was made again.
    pub(crate) levels: u32,
    /// Times it could not be, and stayed as it was.
    pub(crate) levels_refused: u32,
    /// Programs rebuilt from their files.
    pub(crate) shaders: u32,
    /// Times a program's files could not be read or built, and it stayed
    /// as it was.
    pub(crate) shaders_refused: u32,
}

/// The hot reload: the asset watcher, which the asset server keeps, the
/// shader watcher, and what is left to do.
pub(crate) struct HotReload {
    shaders: Option<ShaderWatch>,
    /// When the watcher handed over the changes not yet all made, and how
    /// many reloads it asked for since.
    since: Option<(Instant, usize)>,
    /// The current level must be made again, once no reload is pending.
    level_due: bool,
}

impl HotReload {
    /// Starts the asset server's watcher, unless it runs, as [`App::new`]
    /// starts it, and a watcher of the shader files in `shaders`, render's
    /// `shaders/` folder, if it is one. A watcher that cannot start is
    /// logged, and the rest goes on without it.
    ///
    /// [`App::new`]: crate::App::new
    pub(crate) fn start(world: &World, shaders: &Path) -> Self {
        if let Err(cause) = world.resource_mut::<AssetServer>().start_watcher() {
            warn!("the assets will not reload: their watcher did not start: {cause}");
        }
        let shaders = if shaders.is_dir() {
            match ShaderWatch::start(shaders) {
                Ok(watch) => Some(watch),
                Err(cause) => {
                    warn!("the shaders will not reload: their watcher did not start: {cause}");
                    None
                }
            }
        } else {
            warn!(
                "the shaders will not reload: `{}` is not a folder",
                shaders.display()
            );
            None
        };
        info!(
            "watching the loaded assets{}, to reload them",
            shaders.as_ref().map_or(String::new(), |watch| format!(
                " and the shaders in `{}`",
                watch.dir.display()
            ))
        );
        world.resource_mut::<Reloads>().started = true;
        Self {
            shaders,
            since: None,
            level_due: false,
        }
    }

    /// The hot reload's work for a frame that was drawn, after its finish:
    /// see the module's docs. It allocates nothing when nothing changed.
    pub(crate) fn frame(
        &mut self,
        world: &mut World,
        renderer: &mut Renderer,
        device: &mut Device,
    ) {
        let pending = self.assets(world);
        self.shaders(world, renderer, device);
        self.hand_over(world, pending);
    }

    /// Finds again what the last finish swapped in, and makes the current
    /// level again once it is due and no load or reload is pending, which
    /// it returns.
    pub(crate) fn assets(&mut self, world: &mut World) -> bool {
        self.find_swapped(world);
        let pending = world.resource::<AssetServer>().has_pending();
        if self.level_due && !pending {
            self.level_due = false;
            let name = world
                .resource::<CurrentLevel>()
                .name()
                .unwrap_or_default()
                .to_string();
            match reload_level(world) {
                Ok(true) => world.resource_mut::<Reloads>().levels += 1,
                Ok(false) => {}
                Err(cause) => {
                    error!("the level `{name}` was not reloaded, and stays as it was: {cause}");
                    world.resource_mut::<Reloads>().levels_refused += 1;
                }
            }
        }
        pending
    }

    /// Rebuilds the programs whose files changed.
    fn shaders(&self, world: &World, renderer: &mut Renderer, device: &mut Device) {
        let Some(watch) = &self.shaders else {
            return;
        };
        let changed = watch.take_changed();
        if changed == 0 {
            return;
        }
        for (index, &program) in Program::ALL.iter().enumerate() {
            if changed & (1 << index) != 0 {
                let rebuilt = rebuild(&watch.dir, program, renderer, device);
                let mut reloads = world.resource_mut::<Reloads>();
                if rebuilt {
                    reloads.shaders += 1;
                } else {
                    reloads.shaders_refused += 1;
                }
            }
        }
    }

    /// Hands the paths the watcher saw change to the asset server, and logs
    /// how long the reloads it handed over took, once they are all made and
    /// nothing was `pending` this frame.
    pub(crate) fn hand_over(&mut self, world: &World, pending: bool) {
        let mut server = world.resource_mut::<AssetServer>();
        let asked = server.reload_changed();
        if asked > 0 {
            let (_, count) = self.since.get_or_insert((Instant::now(), 0));
            *count += asked;
        } else if let Some((since, count)) = self.since
            && !pending
            && !server.has_pending()
            && !self.level_due
        {
            let ms = since.elapsed().as_secs_f64() * 1000.0;
            if count == 1 {
                info!("made 1 reload, {ms:.1} ms after the watcher handed its file over");
            } else {
                info!("made {count} reloads, {ms:.1} ms after the watcher handed their files over");
            }
            self.since = None;
        }
    }

    /// Finds again what the last finish swapped in: each atlas's clips,
    /// each prefab, and whether the current level must be made again.
    fn find_swapped(&mut self, world: &mut World) {
        let (atlases, prefabs, levels) = {
            let mut server = world.resource_mut::<AssetServer>();
            // Nothing is collected, nor allocated, when nothing was swapped.
            let atlases: Vec<Handle<Atlas>> = server.drain_swapped().collect();
            let prefabs: Vec<Handle<PrefabAsset>> = server.drain_swapped().collect();
            let levels: Vec<Handle<Level>> = server.drain_swapped().collect();
            (atlases, prefabs, levels)
        };
        let loaded = world.resource::<CurrentLevel>().name().is_some();
        for atlas in atlases {
            found_atlas(world, atlas);
            self.level_due |= loaded;
        }
        // A prefab the library has not read yet, as one a level could
        // not load, may be what the level needs now.
        for prefab in prefabs {
            found_prefab(world, prefab);
            self.level_due |= loaded;
        }
        if !levels.is_empty() {
            let path = world.resource::<CurrentLevel>().path();
            let server = world.resource::<AssetServer>();
            for level in levels {
                if path.is_some() && server.path(level) == path.as_deref() {
                    self.level_due = true;
                }
            }
        }
    }
}

/// Finds the clips of the atlas of `handle`, just reloaded, again by name
/// ([`Clips::reload_atlas`]), with the textures of its pages. A page that
/// is not loaded yet is loaded in [`Scope::Global`]: the game and a level
/// may share the atlas, and a page in the level's scope alone would go
/// with the level, under the game's clips.
fn found_atlas(world: &mut World, handle: Handle<Atlas>) {
    let mut server = world.resource_mut::<AssetServer>();
    let (Some(atlas), Some(path)) = (server.get(handle).cloned(), server.path(handle)) else {
        return;
    };
    let path = path.to_string();
    let loaded: Vec<Option<Handle<Texture>>> = {
        let textures = server.assets::<Texture>();
        atlas
            .pages()
            .iter()
            .map(|page| textures.and_then(|it| it.handle(page.path())))
            .collect()
    };
    let pages: Vec<Handle<Texture>> = atlas
        .pages()
        .iter()
        .zip(loaded)
        .map(|(page, loaded)| loaded.unwrap_or_else(|| server.load_in(page.path(), Scope::Global)))
        .collect();
    let Some(reload) = world.resource_mut::<Clips>().reload_atlas(&atlas, &pages) else {
        return;
    };
    world
        .resource_mut::<CurrentLevel>()
        .replace_atlas_pages(&reload.old_pages, &pages);
    info!(
        "found the clips of `{path}` again: {} replaced, {} added",
        reload.replaced, reload.added
    );
    for name in &reload.kept {
        warn!("`{path}` has no clip `{name}` now: it stays as it was");
    }
    for (name, cause) in &reload.refused {
        error!("the clip `{name}` of `{path}` was not reloaded: {cause}");
    }
    world.resource_mut::<Reloads>().atlases += 1;
}

/// Replaces the prefab of the file of `handle`, just reloaded, in the
/// [`PrefabLibrary`], and loads and checks the bases it now extends, in
/// the level's scope if a level is loaded. A file the library never read
/// is left for a level to read, and one that names another prefab now is
/// logged, and leaves the prefab as it was.
fn found_prefab(world: &mut World, handle: Handle<PrefabAsset>) {
    let mut server = world.resource_mut::<AssetServer>();
    let (Some(asset), Some(path)) = (server.get(handle), server.path(handle)) else {
        return;
    };
    let (file, path) = (asset.file().clone(), path.to_string());
    let mut library = world.resource_mut::<PrefabLibrary>();
    let Some(name) = library
        .iter()
        .find(|prefab| prefab.source() == path)
        .map(|prefab| prefab.name().to_string())
    else {
        return;
    };
    if file.name != name {
        error!(
            "`{path}` is the prefab `{}` now, not `{name}`: `{name}` stays as it was",
            file.name
        );
        return;
    }
    library.insert(path, file);
    let scope = if world.resource::<CurrentLevel>().name().is_some() {
        Scope::Level
    } else {
        Scope::Global
    };
    let registry = world.resource::<TypeRegistry>();
    match library.load(&mut server, &registry, &name, scope) {
        Ok(()) => info!("reloaded the prefab `{name}`"),
        Err(cause) => error!("reloaded the prefab `{name}`, which spawns nothing now: {cause}"),
    }
    world.resource_mut::<Reloads>().prefabs += 1;
}

/// Rebuilds `program` from its files in `dir`, and returns whether it did.
/// Files that cannot be read, or that the renderer refuses, are logged,
/// and the renderer draws on with the program it had.
fn rebuild(dir: &Path, program: Program, renderer: &mut Renderer, device: &mut Device) -> bool {
    let start = Instant::now();
    let read = |file: &str| {
        let path = dir.join(file);
        fs::read_to_string(&path).map_err(|cause| format!("`{}`: {cause}", path.display()))
    };
    let built = read(program.vertex_file()).and_then(|vertex| {
        let fragment = read(program.fragment_file())?;
        let source = ShaderSource {
            glsl_vs: &vertex,
            glsl_fs: &fragment,
            msl: "",
        };
        renderer
            .reload_program(device, program, source)
            .map_err(|cause| cause.to_string())
    });
    let name = program.name();
    match built {
        Ok(()) => {
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            info!("rebuilt the `{name}` shaders in {ms:.1} ms");
            true
        }
        Err(cause) => {
            error!("the `{name}` shaders were not rebuilt, and the last ones draw on: {cause}");
            false
        }
    }
}

/// A thread, `nadir-shader-watch`, that looks at the times of change of
/// each program's shader files every [`LOOK`], and marks the programs
/// whose files changed. Dropping it stops it.
struct ShaderWatch {
    dir: PathBuf,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

/// What the shader watcher shares with the main thread.
#[derive(Default)]
struct Shared {
    /// A bit for each program of [`Program::ALL`] whose files changed.
    changed: AtomicU64,
    /// Whether the watcher is to stop, and its wake-up.
    stop: Mutex<bool>,
    wake: Condvar,
}

/// The vertex and fragment files of a program.
type Files = [PathBuf; 2];

/// When a program's files last changed: `None` for one that is not there.
type Seen = [Option<SystemTime>; 2];

impl ShaderWatch {
    /// Starts watching the shader files in `dir`, from their times of
    /// change now: a change from now on marks its program.
    fn start(dir: &Path) -> io::Result<Self> {
        let files: Vec<Files> = Program::ALL
            .iter()
            .map(|program| {
                [
                    dir.join(program.vertex_file()),
                    dir.join(program.fragment_file()),
                ]
            })
            .collect();
        let seen: Vec<Seen> = files.iter().map(times).collect();
        let shared = Arc::new(Shared::default());
        let thread = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("nadir-shader-watch".to_string())
                .spawn(move || watch(&files, seen, &shared))?
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            shared,
            thread: Some(thread),
        })
    }

    /// The programs whose files changed since the last call, a bit each
    /// in [`Program::ALL`]'s order.
    fn take_changed(&self) -> u64 {
        self.shared.changed.swap(0, Ordering::AcqRel)
    }
}

impl Drop for ShaderWatch {
    fn drop(&mut self) {
        *self
            .shared
            .stop
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = true;
        self.shared.wake.notify_all();
        if let Some(thread) = self.thread.take() {
            // A panic on the thread was printed when it happened.
            let _ = thread.join();
        }
    }
}

/// The times of change of `files`.
fn times(files: &Files) -> Seen {
    files
        .each_ref()
        .map(|path| fs::metadata(path).and_then(|it| it.modified()).ok())
}

/// The shader watcher's thread: a look every [`LOOK`], until it is asked
/// to stop.
fn watch(files: &[Files], mut seen: Vec<Seen>, shared: &Shared) {
    loop {
        {
            let stop = shared.stop.lock().unwrap_or_else(PoisonError::into_inner);
            let (stop, _) = shared
                .wake
                .wait_timeout_while(stop, LOOK, |stop| !*stop)
                .unwrap_or_else(PoisonError::into_inner);
            if *stop {
                return;
            }
        }
        for (index, (files, seen)) in files.iter().zip(&mut seen).enumerate() {
            let now = times(files);
            if now != *seen {
                *seen = now;
                shared.changed.fetch_or(1 << index, Ordering::AcqRel);
            }
        }
    }
}
