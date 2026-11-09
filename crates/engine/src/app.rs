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

//! [`App`]: the game's configuration, its world and systems, and the main
//! loop.

use std::any::type_name;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use nadir_assets::{AssetServer, FinalizeCtx, OnFailure};
use nadir_audio::{AudioConfig, Bus, Mixer};
use nadir_core::UVec2;
use nadir_core::log::{debug, error, info, warn};
use nadir_core::profile::{self, ScopeRecord};
use nadir_core::time::{FixedClock, Stopwatch};
use nadir_ecs::{Component, FixedStage, FixedSystem, FrameStage, FrameSystem, Schedule, World};
use nadir_gfx::{Device, PassTarget, RenderTargetId, TextureFormat};
use nadir_input::{ActionKind, ActionMap, ActionSet, ActionState, Input, InputPlayer};
use nadir_platform::{
    AudioOutputConfig, AudioStream, Event, GpuSurface, ModalFrame, Platform, WindowConfig,
    open_audio, precise_sleep, show_error_dialog,
};
use nadir_render::{FrameStats, Output, RenderConfig, RenderFrame, Renderer, TileLayers};
use nadir_scene::{FromValue, PrefabLibrary, TypeRegistry};

use crate::animation::AnimationClock;
use crate::limiter::FrameLimiter;
use crate::replay::{PlayReplay, ReplayInput};
use crate::sound::{AudioState, MixerOutput, Tracked};
use crate::{
    AnimationFinished, Animator, AudioEmitter, AudioListener, Clips, Collider, CollisionLayers,
    Collisions, CurrentLevel, Error, Facing, LevelError, MusicArea, Parent, Player, PrevTransform,
    Replay, SaveId, Sprite, TickStats, Transform, TriggerEnter, TriggerExit, Velocity, glue,
    log_file,
};

/// How many times a second the simulation steps.
pub const TICK_HZ: u32 = 60;

/// The longest that one frame counts for. After a stall, the simulation
/// falls behind real time by the rest, which is better than a long burst of
/// ticks to catch up. See [`FixedClock`].
const MAX_FRAME: Duration = Duration::from_millis(250);

/// How many profile scope records each frame's buffer holds before it
/// must grow: a frame of 15 ticks of 60 systems each.
const PROFILE_RECORDS: usize = 1_024;

/// What a game asks of the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The text in the window's title bar.
    pub title: String,
    /// Who makes the game. On Windows the game's folders for settings, saves
    /// and logs are inside a folder of this name.
    pub studio: String,
    /// The name of the game's folders for settings, saves and logs. Keep it
    /// the same from one release to the next, or players lose their
    /// settings and saves. Two games with the same name and studio share
    /// their folders.
    ///
    /// This and [`studio`](Self::studio) must each be a valid folder name
    /// on every system (the rules are at [`nadir_platform::user_dirs`]). If
    /// one is not, the game runs without a log file, and a warning says why.
    pub game: String,
    /// The size of the inside of the window, in logical pixels.
    pub window_size: UVec2,
    /// The smallest size the user can give the window, in logical pixels.
    /// A zero means no limit in that direction.
    pub min_window_size: UVec2,
    /// Whether the user can resize the window.
    pub resizable: bool,
    /// Whether the window starts in borderless fullscreen.
    pub fullscreen: bool,
    /// The most frames to run in a second. `None` and `Some(0)` mean no
    /// limit. With [`vsync`](Self::vsync) the display paces the loop too,
    /// but some drivers accept vsync and do not wait (llvmpipe, XWayland),
    /// so the limit stays as a cap. Without either, the loop runs as fast as
    /// it can and keeps a processor core busy.
    pub frame_rate_limit: Option<u32>,
    /// Whether each frame waits for the display's refresh, so that it does
    /// not tear (a swap interval of 1).
    pub vsync: bool,
    /// The size of the image the world is drawn into, in pixels. It is
    /// scaled up into the window by the largest whole factor that fits
    /// (pixel-perfect mode, ADR-0018).
    pub virtual_resolution: UVec2,
    /// The folder the game's assets are loaded from: the cook's output,
    /// whose pack `game.kpak` builds without `dev` read when it is there
    /// ([`App::new`]).
    pub assets: PathBuf,
    /// The volume of each of the mixer's buses as the game starts: full,
    /// by default.
    pub volumes: Volumes,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            title: "Nadir".to_string(),
            studio: "Nadir".to_string(),
            game: "Nadir".to_string(),
            window_size: UVec2::new(1280, 720),
            min_window_size: UVec2::new(320, 180),
            resizable: true,
            fullscreen: false,
            frame_rate_limit: Some(60),
            vsync: true,
            virtual_resolution: UVec2::new(480, 270),
            assets: PathBuf::from("assets"),
            volumes: Volumes::default(),
        }
    }
}

/// The volume of each of the mixer's buses, in percent of full: 0 to 100,
/// and more counts as 100. A percent is heard as its square
/// ([`Volumes::gain`]), so 50 is −12 dB, nearer half as loud than the
/// −6 dB of half the gain.
///
/// [`App::new`] sets the buses to [`Config::volumes`]. A game changes one
/// while it runs, from its settings say, with
/// `audio.set_bus_volume(bus, volumes.gain(bus), fade)`
/// ([`Audio::set_bus_volume`](nadir_audio::Audio::set_bus_volume)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volumes {
    /// The music's: [`Bus::Music`].
    pub music: u8,
    /// The sound effects': [`Bus::Sfx`].
    pub sfx: u8,
    /// The interface's sounds': [`Bus::Ui`].
    pub ui: u8,
    /// The ambient loops': [`Bus::Ambience`].
    pub ambience: u8,
}

impl Default for Volumes {
    /// Every bus at 100: as its sounds are.
    fn default() -> Self {
        Self {
            music: 100,
            sfx: 100,
            ui: 100,
            ambience: 100,
        }
    }
}

impl Volumes {
    /// The percent of `bus`.
    pub const fn get(&self, bus: Bus) -> u8 {
        match bus {
            Bus::Music => self.music,
            Bus::Sfx => self.sfx,
            Bus::Ui => self.ui,
            Bus::Ambience => self.ambience,
        }
    }

    /// Sets the percent of `bus`: for a menu that goes through
    /// [`Bus::ALL`].
    pub fn set(&mut self, bus: Bus, percent: u8) {
        let volume = match bus {
            Bus::Music => &mut self.music,
            Bus::Sfx => &mut self.sfx,
            Bus::Ui => &mut self.ui,
            Bus::Ambience => &mut self.ambience,
        };
        *volume = percent;
    }

    /// The gain of `bus`, for
    /// [`Audio::set_bus_volume`](nadir_audio::Audio::set_bus_volume): its
    /// percent, at most 100, as a part of 100, squared. 50 is 0.25.
    pub fn gain(&self, bus: Bus) -> f32 {
        let part = f32::from(self.get(bus).min(100)) / 100.0;
        part * part
    }
}

impl Config {
    /// The window this configuration asks for.
    fn window(&self) -> WindowConfig {
        WindowConfig {
            title: self.title.clone(),
            size: self.window_size,
            min_size: self.min_window_size,
            resizable: self.resizable,
            fullscreen: self.fullscreen,
            vsync: self.vsync,
        }
    }
}

/// Latches the input into the game's action state.
pub(crate) type LatchFn = Box<dyn FnMut(&mut Input, &World)>;

/// The game's action set: latches the input into its state once a tick, or
/// plays a replay into it.
struct Actions {
    /// The set's type, for messages, or `None` before the game adds one.
    set: Option<&'static str>,
    latch: LatchFn,
    /// The replay that [`App::play_replay`] plays, until it ends.
    replay: Option<Box<dyn PlayReplay>>,
}

impl Actions {
    /// Before the game adds its actions: a latch into a set of its own, so
    /// that each tick still ends, and [`Input::wheel`] moves on.
    fn none() -> Self {
        Self {
            set: None,
            latch: latch_into_nothing(),
            replay: None,
        }
    }
}

/// A latch of the input into a set of its own, which nothing reads.
fn latch_into_nothing() -> LatchFn {
    let map = ActionMap::<NoActions>::new().expect("a set of one action is valid");
    let mut state = ActionState::new();
    Box::new(move |input, _| input.latch(&map, &mut state))
}

/// The action set the input is latched into before the game adds its own:
/// one action, bound to nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoActions {
    Nothing,
}

impl ActionSet for NoActions {
    const ALL: &'static [Self] = &[NoActions::Nothing];

    fn index(self) -> usize {
        0
    }

    fn name(self) -> &'static str {
        "nothing"
    }

    fn kind(self) -> ActionKind {
        ActionKind::Button
    }
}

/// Where the loop gets the window's events from, and the surface it draws
/// on: the platform in a game, and a script in tests.
trait EventSource {
    /// Appends the events that arrived since the last call. Does not wait.
    fn pump_events(&mut self, out: &mut Vec<Event>);

    /// Sets what to call while the system's own event loop keeps
    /// [`EventSource::pump_events`] from returning.
    fn set_modal_frame_callback(&mut self, callback: Box<dyn FnMut(ModalFrame)>);

    /// The surface to draw on, if there is one.
    fn gpu_surface(&self) -> Option<GpuSurface<'_>> {
        None
    }

    /// The size of the inside of the window, in physical pixels.
    fn window_size(&self) -> UVec2 {
        UVec2::ZERO
    }
}

impl EventSource for Platform {
    fn pump_events(&mut self, out: &mut Vec<Event>) {
        Platform::pump_events(self, out);
    }

    fn set_modal_frame_callback(&mut self, callback: Box<dyn FnMut(ModalFrame)>) {
        Platform::set_modal_frame_callback(self, callback);
    }

    fn gpu_surface(&self) -> Option<GpuSurface<'_>> {
        Some(Platform::gpu_surface(self))
    }

    fn window_size(&self) -> UVec2 {
        Platform::window_size(self)
    }
}

/// The GPU side of the loop, where there is a device.
struct Graphics {
    // The renderer's resources are the device's, so it goes first.
    renderer: Renderer,
    device: Device,
}

impl Graphics {
    /// Draws the frame the frame systems made, shows it, and then makes the
    /// assets the workers have decoded, within the asset server's budget.
    fn draw(&mut self, world: &World, surface: GpuSurface<'_>, size: UVec2) -> Result<(), Error> {
        self.device.begin_frame(surface)?;
        self.render(world, PassTarget::Surface, size)?;
        self.device.end_frame(surface)?;
        self.finish_loads(world, false);
        Ok(())
    }

    /// Draws the frame the frame systems made into `target`, of `size`, on
    /// the device's offscreen context, as [`Graphics::draw`] does on the
    /// window, and reads it back. It then makes every asset loaded so far,
    /// waiting for the workers, so that a recording shows the same frames
    /// each time.
    fn draw_offscreen(
        &mut self,
        world: &World,
        target: RenderTargetId,
        size: UVec2,
    ) -> Result<Vec<u8>, Error> {
        self.device.begin_offscreen_frame()?;
        self.render(world, PassTarget::RenderTarget(target), size)?;
        self.device.end_offscreen_frame()?;
        self.finish_loads(world, true);
        Ok(self.device.read_pixels(target)?)
    }

    /// Renders the frame into `target`, and sets [`FrameStats`].
    fn render(&mut self, world: &World, target: PassTarget, size: UVec2) -> Result<(), Error> {
        let stats = {
            let assets = world.resource::<AssetServer>();
            let tiles = world.resource::<TileLayers>();
            let frame = world.resource::<RenderFrame>();
            let output = Output { target, size };
            self.renderer
                .render(&mut self.device, &assets, &tiles, &frame, output)?
        };
        *world.resource_mut::<FrameStats>() = stats;
        Ok(())
    }

    /// Makes the assets the workers have decoded, and frees those of
    /// unloaded scopes. With `wait`, it waits for every load first.
    fn finish_loads(&mut self, world: &World, wait: bool) {
        let pending = world.resource::<AssetServer>().has_pending();
        if pending {
            let mut server = world.resource_mut::<AssetServer>();
            let mut ctx = FinalizeCtx::new().with(&mut self.device);
            if wait {
                server.finish_all_loads(&mut ctx);
            } else {
                server.finish_completed_loads(&mut ctx);
            }
        }
    }
}

/// Everything a frame needs, shared by the main loop and the modal frame
/// callback.
struct EngineCore {
    app: App,
    stopwatch: Stopwatch,
    graphics: Option<Graphics>,
    /// An error that ends the loop, from a frame the loop did not run
    /// itself (a modal one).
    error: Option<Error>,
    /// The main thread's heap use at the end of the last frame.
    #[cfg(feature = "dev")]
    allocs_at_frame_end: nadir_platform::AllocCounts,
    /// The hot reload, where there is a device to draw what it reloads.
    #[cfg(feature = "dev")]
    reload: Option<crate::hot_reload::HotReload>,
}

impl EngineCore {
    /// Runs one frame, for the time since the last, with the events that
    /// came before it. Draws it on `window`'s surface, if there is one.
    fn frame(&mut self, events: &[Event], window: Option<(GpuSurface<'_>, UVec2)>) {
        let elapsed = self.stopwatch.lap();
        self.app.update(elapsed, events);
        if let (Some(graphics), Some((surface, size))) = (&mut self.graphics, window) {
            let drawn = graphics.draw(&self.app.world, surface, size);
            #[cfg(feature = "dev")]
            if drawn.is_ok()
                && let Some(reload) = &mut self.reload
            {
                reload.frame(
                    &mut self.app.world,
                    &mut graphics.renderer,
                    &mut graphics.device,
                );
            }
            if let Err(error) = drawn {
                self.error.get_or_insert(error);
            }
        }
        #[cfg(feature = "dev")]
        {
            let now = nadir_platform::thread_alloc_counts();
            let allocs = crate::FrameAllocs(now.since(self.allocs_at_frame_end));
            *self.app.world.resource_mut::<crate::FrameAllocs>() = allocs;
            self.allocs_at_frame_end = now;
        }
    }
}

/// A game: its configuration, its [`World`] of entities and resources, and
/// its systems in stages. [`App::run`] opens the log file and the window,
/// and runs the main loop; [`App::update`] runs one frame without them.
///
/// Systems are functions of the world and the tick, or the frame, added to
/// a stage with a name. They run in stage order, then in the order they
/// were added (`docs/subsystems/ecs.md`). The engine adds its own first in
/// their stages:
///
/// - `nadir::snapshot` (tick `Input`): copies each [`Transform`] into its
///   [`PrevTransform`], and gives one to every entity with a [`Sprite`]
///   and to the root of every drawn child;
/// - `nadir::movement` (tick `Movement`): rebuilds [`Collisions`] and
///   moves every entity with a [`Velocity`], sliding the ones with a
///   [`Collider`];
/// - `nadir::parent_offsets` (tick `PostMovement`): puts each entity with
///   a [`Parent`] where its parent is, plus its offset;
/// - `nadir::triggers` (tick `PostMovement`): sends [`TriggerEnter`] and
///   [`TriggerExit`];
/// - `nadir::animate` (frame `Animate`): advances every [`Animator`] and
///   writes its frame into its [`Sprite`], sends [`AnimationFinished`],
///   mirrors the Sprite of each entity that faces [`Facing::Left`], and
///   unmirrors the rest with a [`Facing`];
/// - `nadir::sound` (frame `Extract`): puts the listener at the
///   [`AudioListener`], or else the camera, plays each [`AudioEmitter`] at
///   its entity, sends the track of the [`MusicArea`] the listener is in,
///   and calls [`Audio::update`](nadir_audio::Audio::update);
/// - `nadir::extract_sprites` (frame `Extract`): draws every [`Sprite`]
///   into the [`RenderFrame`], between the last two ticks;
/// - `nadir::extract_level` (frame `Extract`): draws the tile layers of the
///   [`CurrentLevel`], with its background as the clear colour.
///
/// So a game sets velocities in `Gameplay`, and reacts to the moves after
/// the engine's, in `Movement` or `PostMovement`. It sets [`Facing`] and
/// plays clips in ticks too, and reacts to finished clips in frames.
///
/// The engine's resources:
///
/// - [`Input`], which latches the keys and buttons into the actions, and
///   holds the pointer and the wheel
/// - with [`App::add_actions`], the game's [`ActionMap`] and its
///   [`ActionState`], latched once a tick before the tick stages
/// - [`AssetServer`], loading from [`Config::assets`]
/// - [`RenderFrame`], emptied before the frame stages, which fill it; the
///   loop then draws it. Its `time`, which animated tiles show and
///   animators follow, is how long the simulation has run, ticks and
///   `alpha`.
/// - [`TileLayers`], the game's tile layers, which the frame's
///   `tile_layers` name and the loop draws from
/// - [`FrameStats`], what the last frame drew
/// - [`TickStats`], how long this frame's ticks took
/// - [`Collisions`], the colliders
/// - [`Clips`], the clips that animators play
/// - [`TypeRegistry`], the reflected component types
/// - [`PrefabLibrary`], the prefabs the game loads, none until it loads
///   them
/// - [`CollisionLayers`], the names of the game's collision layers, none
///   until [`App::collision_layers`] gives them
/// - [`CurrentLevel`], the level [`App::load_level`] loaded, none until it
///   loads one
/// - [`Audio`](nadir_audio::Audio), the game's handle on the mixer, whose
///   buses start at [`Config::volumes`], and [`AudioState`], what
///   `nadir::sound` plays
/// - with [`App::play_replay`], [`Replay`], how far the replay got
/// - the events [`TriggerEnter`], [`TriggerExit`] and [`AnimationFinished`]
/// - with the `dev` feature, `FrameAllocs`, what the last frame allocated
///
/// ```no_run
/// use nadir::prelude::*;
///
/// struct Spin(f32);
///
/// fn simulate(world: &World, tick: &Tick) {
///     // 60 times a second: move things by `tick.dt`.
///     world.resource_mut::<Spin>().0 += tick.dt;
/// }
///
/// fn main() -> Result<(), nadir::Error> {
///     App::new(Config {
///         title: "My game".into(),
///         ..Config::default()
///     })
///     .insert_resource(Spin(0.0))
///     .add_fixed_system(FixedStage::Gameplay, "simulate", simulate)
///     .add_frame_system(FrameStage::Ui, "show", |world, frame| {
///         // Once for each frame, after its ticks.
///         let _ = (world.resource::<Spin>().0, frame.alpha);
///     })
///     .run()
/// }
/// ```
pub struct App {
    config: Config,
    world: World,
    schedule: Schedule,
    actions: Actions,
    clock: FixedClock,
    /// The last frame's profile scopes (the `profile` feature of
    /// `nadir-core`), drained each frame so that the recorder keeps its
    /// buffer. Nothing reads them until the profiler (roadmap M7.3).
    profile: Vec<ScopeRecord>,
    /// The folder of the shader files the hot reload watches: render's
    /// `shaders/`, or a test's.
    #[cfg(feature = "dev")]
    shader_dir: PathBuf,
    /// The mixer that [`Audio`](nadir_audio::Audio) sends to, until
    /// [`App::run`] hands it to the audio output, [`App::take_mixer`] takes
    /// it, or [`App::update`] drops it.
    mixer: Option<Mixer>,
}

impl App {
    /// An app with the engine's resources, components and systems, and no
    /// entities.
    ///
    /// Its [`AssetServer`] loads from [`Config::assets`]. With the `dev`
    /// feature, it reads the loose files there, watches each file loaded
    /// from then on for the hot reload of [`App::run`], and a failed asset
    /// shows its placeholder. Without it, it reads the cook's pack there,
    /// `game.kpak`, when there is one, and the loose files otherwise; a
    /// pack that does not open is logged, and every load then fails with
    /// its error (ADR-0041); and a failed asset shows nothing.
    ///
    /// Of the engine's components, [`Transform`], [`Velocity`], [`Player`],
    /// [`SaveId`], [`Facing`], [`Sprite`], [`Collider`] and [`Animator`] are
    /// reflected, so that prefabs and levels can give them (ADR-0038).
    /// [`Parent`] is not yet (roadmap M9.4), and [`PrevTransform`] is the
    /// engine's own. Its resources include a [`PrefabLibrary`] with no
    /// prefabs, and [`CollisionLayers`] with no names
    /// ([`App::collision_layers`]).
    ///
    /// It makes the mixer, of [`AudioConfig::default`], and sets each bus
    /// to [`Config::volumes`]. [`AudioListener`], [`AudioEmitter`] and
    /// [`MusicArea`] are not reflected: a game gives them in code.
    pub fn new(config: Config) -> Self {
        let on_failure = if cfg!(feature = "dev") {
            OnFailure::Placeholder
        } else {
            OnFailure::Nothing
        };
        let mut world = World::new();
        world.insert_resource(Input::new());
        #[cfg_attr(not(feature = "dev"), expect(unused_mut))]
        let mut server =
            crate::pack::asset_server(&config.assets, on_failure, !cfg!(feature = "dev"));
        // Watched from before the game's setup reads a file, so that the
        // hot reload of `App::run` sees an edit made after the read.
        #[cfg(feature = "dev")]
        if let Err(cause) = server.start_watcher() {
            nadir_core::log::warn!(
                "the assets will not reload: their watcher did not start: {cause}"
            );
        }
        world.insert_resource(server);
        world.insert_resource(RenderFrame::new());
        world.insert_resource(TileLayers::new());
        world.insert_resource(FrameStats::default());
        world.insert_resource(TickStats::default());
        world.insert_resource(Collisions::new());
        world.insert_resource(Clips::new());
        world.insert_resource(CollisionLayers::default());
        world.insert_resource(PrefabLibrary::new());
        world.insert_resource(CurrentLevel::default());
        world.insert_resource(AnimationClock::default());
        let sound = AudioConfig::default();
        let (audio, mixer) = nadir_audio::new(sound).expect("the default audio config is valid");
        // Queued, for the mixer's first callback.
        for bus in Bus::ALL {
            audio.set_bus_volume(bus, config.volumes.gain(bus), 0);
        }
        world.insert_resource(audio);
        world.insert_resource(AudioState::new(&sound));
        #[cfg(feature = "dev")]
        world.insert_resource(crate::FrameAllocs::default());
        #[cfg(feature = "dev")]
        world.insert_resource(crate::hot_reload::Reloads::default());
        world.add_event::<TriggerEnter>();
        world.add_event::<TriggerExit>();
        world.add_event::<AnimationFinished>();

        let mut registry = TypeRegistry::new();
        registry.register_component::<Transform>();
        registry.register_component::<Velocity>();
        registry.register_component::<Player>();
        registry.register_component::<SaveId>();
        registry.register_component::<Facing>();
        registry.register_component::<Sprite>();
        registry.register_component::<Collider>();
        registry.register_component::<Animator>();
        registry.register_components(&mut world);
        world.insert_resource(registry);
        world.register::<PrevTransform>();
        world.register::<Sprite>();
        world.register::<Collider>();
        world.register::<Parent>();
        world.register::<Animator>();
        world.register::<AudioListener>();
        world.register::<AudioEmitter>();
        world.register::<MusicArea>();
        world.register::<Tracked>();

        let mut schedule = Schedule::new();
        schedule
            .add_fixed(FixedStage::Input, "nadir::snapshot", glue::snapshot)
            .add_fixed(FixedStage::Movement, "nadir::movement", glue::movement)
            .add_fixed(
                FixedStage::PostMovement,
                "nadir::parent_offsets",
                glue::parent_offsets,
            )
            .add_fixed(FixedStage::PostMovement, "nadir::triggers", glue::triggers)
            .add_frame(FrameStage::Animate, "nadir::animate", glue::animate)
            .add_frame(FrameStage::Extract, "nadir::sound", crate::sound::sound)
            .add_frame(
                FrameStage::Extract,
                "nadir::extract_sprites",
                glue::extract_sprites,
            )
            .add_frame(
                FrameStage::Extract,
                "nadir::extract_level",
                crate::level::extract_level,
            );
        Self {
            config,
            world,
            schedule,
            actions: Actions::none(),
            clock: FixedClock::new(TICK_HZ, MAX_FRAME),
            profile: Vec::new(),
            #[cfg(feature = "dev")]
            shader_dir: PathBuf::from(nadir_render::SHADER_DIR),
            mixer: Some(mixer),
        }
    }

    /// Takes the mixer, for a test or a tool that renders it itself
    /// ([`Mixer::render`]): [`App::run`] then opens no audio output. Take it
    /// before the first [`App::update`], which drops a mixer the app still
    /// holds: `None` then, and once it is taken.
    pub fn take_mixer(&mut self) -> Option<Mixer> {
        self.mixer.take()
    }

    /// Adds `resource`, or replaces the one of its type.
    #[must_use]
    pub fn insert_resource<R: 'static>(mut self, resource: R) -> Self {
        self.world.insert_resource(resource);
        self
    }

    /// Adds the event type `T`: the resource `Events<T>`, which the
    /// schedule clears (`World::add_event`).
    #[must_use]
    pub fn add_event<T: Clone + 'static>(mut self) -> Self {
        self.world.add_event::<T>();
        self
    }

    /// Adds the reflected component type `T` to the world and to the
    /// [`TypeRegistry`], under its reflected name. A component that is not
    /// reflected is added with `app.world_mut().register::<T>()`.
    ///
    /// # Panics
    ///
    /// If another type has the same reflected name.
    #[must_use]
    pub fn register_component<T: Component + FromValue>(mut self) -> Self {
        self.world
            .resource_mut::<TypeRegistry>()
            .register_component::<T>();
        self.world.register::<T>();
        self
    }

    /// Loads the level `name`, `levels/<name>.klvl`, in place of the one the
    /// world holds, as [`load_level`](crate::load_level) says: its prefabs
    /// and tile sets are read before this returns, its entities spawned,
    /// and its tile layers drawn from the next frame.
    ///
    /// # Errors
    ///
    /// A [`LevelError`] if it cannot be loaded. The previous level is gone
    /// then, and nothing of the new one is in the world.
    pub fn load_level(&mut self, name: &str) -> Result<(), LevelError> {
        crate::load_level(&mut self.world, name)
    }

    /// Names the game's collision layers: bit 0 of a
    /// [`LayerMask`](nadir_collision::LayerMask) the first, bit 1 the next,
    /// and so on, as prefabs and levels write a collider's layers
    /// (`["World", "People"]`). It replaces the [`CollisionLayers`]
    /// resource, which has no names until then.
    ///
    /// # Panics
    ///
    /// If there are more than 32 names, one is empty, or two are the same.
    #[must_use]
    pub fn collision_layers(self, names: &[&str]) -> Self {
        *self.world.resource_mut::<CollisionLayers>() = CollisionLayers::new(names);
        self
    }

    /// Adds the game's actions, with their bindings: the resources
    /// [`ActionMap<A>`] and [`ActionState<A>`], whose state is latched from
    /// the input once a tick, before the tick stages.
    ///
    /// Read edges (`just_pressed`) in fixed systems. A frame system sees
    /// only the last tick's: it misses those of a frame's earlier ticks, and
    /// sees one again when a frame has no tick.
    ///
    /// # Panics
    ///
    /// An app has one action set: adding another type panics. Adding the
    /// same type again replaces its bindings. Adding actions while a replay
    /// plays panics: [`App::play_replay`] comes after `add_actions`.
    #[must_use]
    pub fn add_actions<A: ActionSet>(mut self, map: ActionMap<A>) -> Self {
        let set = type_name::<A>();
        if let Some(existing) = self.actions.set {
            assert!(
                existing == set,
                "an App has one action set: it has `{existing}`, so it cannot add `{set}`",
            );
        }
        assert!(
            self.actions.replay.is_none(),
            "a replay is playing: play_replay must come after add_actions"
        );
        self.world.insert_resource(map);
        self.world.insert_resource(ActionState::<A>::new());
        self.actions = Actions {
            set: Some(set),
            latch: Box::new(|input, world| {
                let map = world.resource::<ActionMap<A>>();
                let mut state = world.resource_mut::<ActionState<A>>();
                input.latch(&map, &mut state);
            }),
            replay: None,
        };
        self
    }

    /// Plays `replay` into the game's actions in place of the live input:
    /// each tick from the next on takes its [`ActionState<A>`] from the
    /// replay's next tick. It adds the resource [`Replay`], which says how
    /// far it got. The replay is read a tick ahead, so it is
    /// [`Replay::Ended`] as soon as its last tick has run, and the ticks
    /// after that are latched live.
    ///
    /// While it plays, the live input still reaches [`Input`], but not the
    /// actions. The pointer and the wheel stay live: replays do not hold
    /// them, so a fixed system that reads them does not replay. When the
    /// replay ends, the first live tick's edges are those of the live keys
    /// alone: an action held in the replay's last tick is not just
    /// released, and a key held all along is held but not just pressed.
    ///
    /// A damaged tick ends the replay as [`Replay::Failed`], after an error
    /// in the log; the ticks before it play. A replay with no ticks is
    /// [`Replay::Ended`] at once. Playing another replay replaces this one.
    ///
    /// # Panics
    ///
    /// If the game's action set, added with [`App::add_actions`], is not
    /// `A`.
    #[must_use]
    pub fn play_replay<A: ActionSet>(mut self, replay: InputPlayer<'static, A>) -> Self {
        let set = type_name::<A>();
        assert!(
            self.actions.set == Some(set),
            "a replay of `{set}` plays into those actions: add them with add_actions first"
        );
        let replay = ReplayInput::new(replay, latch_into_nothing());
        let status = match replay.ahead() {
            Ok(true) => Replay::Playing { ticks: 0 },
            Ok(false) => Replay::Ended { ticks: 0 },
            Err(error) => failed(0, error),
        };
        self.actions.replay = matches!(status, Replay::Playing { .. })
            .then(|| Box::new(replay) as Box<dyn PlayReplay>);
        self.world.insert_resource(status);
        self
    }

    /// The [`world_hash`](crate::world_hash) of the world: the same after
    /// the same ticks of the same replay, from the same start, whatever the
    /// frames were, under the conditions that function gives.
    pub fn world_hash(&self) -> u64 {
        crate::world_hash(&self.world)
    }

    /// Adds a system to a stage of each simulation tick, after the stage's
    /// others. Ticks come [`TICK_HZ`] times a second, however fast frames
    /// come.
    #[must_use]
    pub fn add_fixed_system(
        mut self,
        stage: FixedStage,
        name: &'static str,
        system: FixedSystem,
    ) -> Self {
        self.schedule.add_fixed(stage, name, system);
        self
    }

    /// Adds a system to a stage of each frame, after the stage's others.
    /// A frame's stages run after its ticks.
    #[must_use]
    pub fn add_frame_system(
        mut self,
        stage: FrameStage,
        name: &'static str,
        system: FrameSystem,
    ) -> Self {
        self.schedule.add_frame(stage, name, system);
        self
    }

    /// The world, to read it.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// The world, for setup: spawning entities, loading assets,
    /// registering components that are not reflected.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The systems, in stages: [`Schedule::describe`] lists them in the
    /// order they run.
    pub fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    /// Runs one frame that comes `elapsed` after the last, with the events
    /// that came before it, without drawing it:
    ///
    /// 1. feeds the events to [`Input`];
    /// 2. runs the ticks that are due: for each, latches the actions, or
    ///    takes them from the replay that [`App::play_replay`] plays, and
    ///    runs the tick stages, which apply their commands;
    /// 3. sets [`TickStats`];
    /// 4. empties the [`RenderFrame`], sets its `time` to how long the
    ///    simulation has run, and runs the frame stages;
    /// 5. collects the profile scopes the systems recorded, with the
    ///    `profile` feature of `nadir-core`, so that they do not pile up.
    ///
    /// [`App::run`] calls it once a frame, then draws the `RenderFrame`.
    /// Tests, benchmarks and headless runs call it themselves. A frame of
    /// `Duration::from_nanos(16_666_667)` runs one tick: the third of a
    /// nanosecond it holds past a tick adds up to another only after 50
    /// million frames. Asset loads finish only in a drawn frame, so a
    /// headless run finishes none: tick systems must not depend on whether
    /// an asset has loaded.
    ///
    /// It plays no sound. Its first call drops the mixer, unless
    /// [`App::take_mixer`] took it, and [`Audio`](nadir_audio::Audio) then
    /// does nothing: so tests, benchmarks, headless runs and
    /// [`App::offscreen`] are silent, and [`App::run`] after it opens no
    /// audio output.
    pub fn update(&mut self, elapsed: Duration, events: &[Event]) {
        // Nothing renders it here: dropped, it leaves no queue to fill.
        self.mixer = None;
        {
            let mut input = self.world.resource_mut::<Input>();
            for event in events {
                input.handle(event);
            }
        }
        let frame = self.clock.begin_frame(elapsed);
        let mut stats = TickStats::default();
        while let Some(tick) = self.clock.consume_tick() {
            let start = Instant::now();
            let replayed = {
                let mut input = self.world.resource_mut::<Input>();
                match &mut self.actions.replay {
                    Some(replay) => {
                        let ticks = replay.latch(&mut input, &self.world);
                        *self.world.resource_mut::<Replay>() = Replay::Playing { ticks };
                        true
                    }
                    None => {
                        (self.actions.latch)(&mut input, &self.world);
                        false
                    }
                }
            };
            self.schedule.run_fixed(&mut self.world, &tick);
            if replayed {
                self.end_replay_tick();
            }
            let took = start.elapsed();
            stats.ticks += 1;
            stats.time += took;
            stats.longest = stats.longest.max(took);
        }
        *self.world.resource_mut::<TickStats>() = stats;
        {
            let mut out = self.world.resource_mut::<RenderFrame>();
            out.clear();
            out.time = simulated_time(&self.clock);
        }
        self.schedule.run_frame(&mut self.world, &frame);
        profile::take_frame(&mut self.profile);
        // Records came back, so the feature is on. The recorder gets this
        // buffer back next frame: make it big enough that a frame of many
        // ticks does not grow it.
        if !self.profile.is_empty() {
            let more = PROFILE_RECORDS.saturating_sub(self.profile.len());
            self.profile.reserve(more);
        }
    }

    /// Opens the log file and the window, and runs the main loop until the
    /// user closes the window.
    ///
    /// The log file is `nadir.log`, in the game's logs folder: on Linux
    /// `~/.local/share/<game>`. Each run starts a new file and keeps the
    /// last three runs' files beside it, as `nadir.log.1` to `nadir.log.3`.
    /// If the file cannot be opened, the game goes on without it.
    ///
    /// Where the system has no OpenGL device yet (Windows and macOS, until
    /// roadmap M2.6 and M2.7), the error is logged and the game runs
    /// without drawing.
    ///
    /// It opens the default audio output, 48 kHz stereo, with the mixer,
    /// once the window is open, and closes it when the loop ends, before an
    /// error is shown. Where it does not open (Windows and macOS until
    /// roadmap M6.4 and M6.5, Linux without `libasound.so.2`), a warning
    /// says why and the game runs without sound; with no mixer left
    /// ([`App::take_mixer`], or an [`App::update`] before), the log says so.
    /// A missing device is no error: the output starts without one, and
    /// opens it once it comes.
    ///
    /// With the `dev` feature, it reloads what changes while it runs: the
    /// loaded assets' files and render's shaders, after each drawn frame
    /// (the crate's docs, "Hot reload").
    ///
    /// # Errors
    ///
    /// Fails if the window cannot be opened, the renderer cannot start, or
    /// a frame cannot be drawn. Before it returns the error, it logs it and
    /// shows it to the player with [`show_error_dialog`].
    pub fn run(mut self) -> Result<(), Error> {
        log_file::start(&self.config.studio, &self.config.game);
        let title = self.config.title.clone();
        let mut platform =
            Platform::new(&self.config.window()).map_err(|error| fatal(&title, error.into()))?;
        let graphics = match Device::new(platform.gpu_surface()) {
            Ok(mut device) => {
                let config = RenderConfig {
                    virtual_resolution: self.config.virtual_resolution,
                };
                let renderer = Renderer::new(&mut device, &config)
                    .map_err(|error| fatal(&title, error.into()))?;
                Some(Graphics { renderer, device })
            }
            Err(error) => {
                error!("no GPU device, so the game draws nothing: {error}");
                None
            }
        };
        // The stream stops as it drops, at the end of the block, so that the
        // sound stops before an error is shown.
        let result = {
            let _stream = self.start_audio();
            self.run_loop(&mut platform, graphics)
        };
        result.map_err(|error| fatal(&title, error))
    }

    /// Opens the audio output with the mixer, if the app still holds it.
    /// `None`, after the log says why, where the game plays no sound.
    fn start_audio(&mut self) -> Option<AudioStream> {
        let Some(mixer) = self.mixer.take() else {
            info!(
                "the game plays no sound: its mixer was taken (App::take_mixer), or dropped by an \
                 App::update before App::run"
            );
            return None;
        };
        let config = AudioOutputConfig::default();
        match open_audio(&config, Box::new(MixerOutput(mixer))) {
            Ok(stream) => Some(stream),
            Err(error) => {
                warn!("no audio output, so the game plays no sound: {error}");
                None
            }
        }
    }

    /// The main loop: events, then the frame and its ticks, then drawing.
    ///
    /// While the system runs an event loop of its own, as Windows does while
    /// the window is dragged and macOS while it is resized, the pump does not
    /// return. Frames then run from the modal frame callback, unpaced and
    /// undrawn: the system calls it every 10 to 16 ms on Windows, and once
    /// for each step of the resize on macOS. (Drawing there needs a surface
    /// in `ModalFrame`, roadmap M2.6 and M2.7.)
    fn run_loop(
        self,
        window: &mut dyn EventSource,
        graphics: Option<Graphics>,
    ) -> Result<(), Error> {
        debug!("the systems, in the order they run:");
        for system in self.schedule.describe() {
            debug!("  {system}");
        }
        #[cfg(feature = "dev")]
        let reload = graphics
            .is_some()
            .then(|| crate::hot_reload::HotReload::start(&self.world, &self.shader_dir));
        let mut limiter = FrameLimiter::new(self.config.frame_rate_limit, Instant::now());
        let core = Rc::new(RefCell::new(EngineCore {
            app: self,
            stopwatch: Stopwatch::start(),
            graphics,
            error: None,
            #[cfg(feature = "dev")]
            allocs_at_frame_end: nadir_platform::thread_alloc_counts(),
            #[cfg(feature = "dev")]
            reload,
        }));
        let modal_core = Rc::clone(&core);
        window.set_modal_frame_callback(Box::new(move |_frame| {
            // The loop borrows the core only between pumps, and this runs
            // during one, so the borrow always succeeds.
            if let Ok(mut core) = modal_core.try_borrow_mut() {
                core.frame(&[], None);
            }
        }));
        let mut events = Vec::with_capacity(256);
        loop {
            window.pump_events(&mut events);
            if events.contains(&Event::CloseRequested) {
                break;
            }
            {
                let mut core = core.borrow_mut();
                let surface = window
                    .gpu_surface()
                    .map(|surface| (surface, window.window_size()));
                core.frame(&events, surface);
                if let Some(error) = core.error.take() {
                    return Err(error);
                }
            }
            events.clear();

            if let Some(due) = limiter.end_of_frame(Instant::now()) {
                precise_sleep(due);
            }
        }
        Ok(())
    }

    /// Draws the game offscreen, with no window, in frames of `size`
    /// pixels: [`Offscreen::frame`] runs a frame, draws it as a window of
    /// that size would show it, and returns its pixels. For recordings of
    /// the game, such as the demo page's (ADR-0029). It opens no log file.
    ///
    /// # Errors
    ///
    /// [`Error::Render`] where there is no offscreen OpenGL context
    /// ([`GfxError::NoOffscreenContext`](nadir_gfx::GfxError) on Windows
    /// and macOS, until roadmap M2.6 and M2.7) or it cannot be made, or the
    /// renderer or a target of `size` cannot be.
    pub fn offscreen(self, size: UVec2) -> Result<Offscreen, Error> {
        let mut device = Device::new_headless()?;
        let config = RenderConfig {
            virtual_resolution: self.config.virtual_resolution,
        };
        let renderer = Renderer::new(&mut device, &config)?;
        let target = device.create_render_target(size, TextureFormat::Rgba8Srgb)?;
        Ok(Offscreen {
            app: self,
            graphics: Graphics { renderer, device },
            target,
            size,
        })
    }
}

/// A game drawn offscreen, a frame at a time, with no window. Made by
/// [`App::offscreen`].
pub struct Offscreen {
    // The app goes first, as in the main loop: its assets are the device's.
    app: App,
    graphics: Graphics,
    /// What each frame is drawn into.
    target: RenderTargetId,
    size: UVec2,
}

impl Offscreen {
    /// Runs one frame that comes `elapsed` after the last, with the events
    /// that came before it, as [`App::update`] does. Then draws it as
    /// [`App::run`] does, waits for every asset loaded so far and makes it,
    /// and returns the frame's pixels: RGBA8, sRGB-encoded, from the top
    /// row, 4 bytes a pixel.
    ///
    /// Unlike [`App::run`], it waits for the asset server's workers, so that
    /// each frame shows every asset loaded before it, and a recording is the
    /// same each time. The assets loaded before the first frame are made
    /// after it is drawn, so it shows none of them. A first frame of
    /// [`Duration::ZERO`] makes them without a tick.
    ///
    /// # Errors
    ///
    /// [`Error::Render`] if the frame cannot be drawn or read back.
    pub fn frame(&mut self, elapsed: Duration, events: &[Event]) -> Result<Vec<u8>, Error> {
        self.app.update(elapsed, events);
        self.graphics
            .draw_offscreen(&self.app.world, self.target, self.size)
    }

    /// The game.
    pub fn app(&self) -> &App {
        &self.app
    }

    /// The size of each frame, in pixels.
    pub fn size(&self) -> UVec2 {
        self.size
    }

    /// The renderer that draws the frames, as the driver names it: for
    /// instance `llvmpipe (LLVM 20.1.2, 256 bits)`.
    pub fn renderer(&self) -> &str {
        &self.graphics.device.caps().renderer
    }
}

impl App {
    /// After a replayed tick: if it was the replay's last, or the next
    /// cannot be read, says so in [`Replay`] and lets the live input latch
    /// from the next tick on.
    fn end_replay_tick(&mut self) {
        let Some(replay) = &self.actions.replay else {
            return;
        };
        let ticks = replay.ticks();
        let status = match replay.ahead() {
            Ok(true) => return,
            Ok(false) => Replay::Ended { ticks },
            Err(error) => failed(ticks, error),
        };
        *self.world.resource_mut::<Replay>() = status;
        self.actions.replay = None;
    }
}

/// [`Replay::Failed`] after `ticks` ticks, logged.
fn failed(ticks: u64, error: &nadir_input::InputError) -> Replay {
    error!(
        "the replay's tick {} cannot be read: {error}; the input is live from now on",
        ticks + 1
    );
    Replay::Failed {
        ticks,
        error: error.clone(),
    }
}

/// How long the simulation has run, as a frame shows it: the ticks so far
/// and the `alpha` of a tick more. Whatever slows or stops the clock slows
/// or stops what follows this, such as animated tiles.
fn simulated_time(clock: &FixedClock) -> Duration {
    let hz = u128::from(TICK_HZ);
    let ticks = u128::from(clock.ticks()) * 1_000_000_000 / hz;
    let part = (f64::from(clock.alpha()) * 1e9 / TICK_HZ as f64) as u128;
    Duration::from_nanos(u64::try_from(ticks + part).unwrap_or(u64::MAX))
}

/// Logs an error that ends the game and shows it to the player. Returns it,
/// for [`App::run`] to return.
fn fatal(title: &str, error: Error) -> Error {
    error!("{error}");
    show_error_dialog(title, &error.to_string());
    error
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::thread;

    use nadir_core::time::{Frame, Tick};
    use nadir_core::{Key, Vec2};
    use nadir_ecs::Entity;
    use nadir_input::{Binding, InputRecorder};

    use super::*;

    /// One tick's length, as a frame: it runs one tick.
    const TICK: Duration = Duration::from_nanos(16_666_667);
    const MS: Duration = Duration::from_millis(1);

    fn app() -> App {
        App::new(Config::default())
    }

    /// What the systems of a test saw, shared with the test through a
    /// resource, so that it outlives an app that the loop consumed.
    #[derive(Default)]
    struct Log {
        ticks: Cell<u64>,
        frames: Cell<u64>,
        seen: RefCell<Vec<String>>,
        values: RefCell<Vec<f32>>,
    }

    type Shared = Rc<Log>;

    fn shared() -> Shared {
        Rc::default()
    }

    fn count_ticks(world: &World, _: &Tick) {
        let log = world.resource::<Shared>();
        log.ticks.set(log.ticks.get() + 1);
    }

    fn count_frames(world: &World, _: &Frame) {
        let log = world.resource::<Shared>();
        log.frames.set(log.frames.get() + 1);
    }

    /// An app whose systems count ticks and frames into `log`.
    fn counting_app(log: &Shared) -> App {
        app()
            .insert_resource(Rc::clone(log))
            .add_fixed_system(FixedStage::Gameplay, "count_ticks", count_ticks)
            .add_frame_system(FrameStage::Ui, "count_frames", count_frames)
    }

    /// Runs frames of one length until `total` has passed.
    fn simulate(app: &mut App, frame_length: Duration, total: Duration) {
        let mut passed = Duration::ZERO;
        while passed < total {
            let step = frame_length.min(total - passed);
            app.update(step, &[]);
            passed += step;
        }
    }

    // -- configuration ------------------------------------------------------

    #[test]
    fn the_default_configuration_is_a_resizable_window_at_60_frames() {
        let config = Config::default();
        assert_eq!(config.title, "Nadir");
        assert_eq!(config.studio, "Nadir");
        assert_eq!(config.game, "Nadir");
        assert_eq!(config.window_size, UVec2::new(1280, 720));
        assert_eq!(config.min_window_size, UVec2::new(320, 180));
        assert!(config.resizable);
        assert!(!config.fullscreen);
        assert_eq!(config.frame_rate_limit, Some(60));
        assert!(config.vsync);
        assert_eq!(config.virtual_resolution, UVec2::new(480, 270));
        assert_eq!(config.assets, PathBuf::from("assets"));
        for bus in Bus::ALL {
            assert_eq!(config.volumes.get(bus), 100, "{bus:?}");
            assert_eq!(config.volumes.gain(bus), 1.0, "{bus:?}");
        }
    }

    #[test]
    fn a_volume_is_heard_as_its_square_and_held_to_100() {
        let mut volumes = Volumes::default();
        volumes.set(Bus::Music, 50);
        volumes.set(Bus::Ui, 0);
        volumes.set(Bus::Ambience, 250);
        assert_eq!(
            volumes,
            Volumes {
                music: 50,
                sfx: 100,
                ui: 0,
                ambience: 250
            }
        );
        // 50 is a quarter of the gain: −12 dB.
        assert_eq!(volumes.gain(Bus::Music), 0.25);
        assert_eq!(volumes.gain(Bus::Ui), 0.0);
        assert_eq!(volumes.gain(Bus::Ambience), 1.0);
        assert_eq!(volumes.get(Bus::Ambience), 250);
    }

    #[test]
    fn the_window_is_the_one_the_configuration_asks_for() {
        let config = Config {
            title: "Sandbox".to_string(),
            studio: "Studio".to_string(),
            game: "Game".to_string(),
            window_size: UVec2::new(960, 540),
            min_window_size: UVec2::new(480, 270),
            resizable: false,
            fullscreen: true,
            frame_rate_limit: None,
            vsync: false,
            virtual_resolution: UVec2::new(320, 180),
            assets: PathBuf::from("data"),
            volumes: Volumes {
                music: 60,
                sfx: 80,
                ui: 100,
                ambience: 40,
            },
        };
        let expected = WindowConfig {
            title: "Sandbox".to_string(),
            size: UVec2::new(960, 540),
            min_size: UVec2::new(480, 270),
            resizable: false,
            fullscreen: true,
            vsync: false,
        };
        assert_eq!(config.window(), expected);
    }

    // -- the schedule and the world -----------------------------------------

    #[test]
    fn the_schedule_prints_the_engines_systems_first_in_their_stages() {
        let app = app()
            .add_frame_system(FrameStage::Extract, "draw_ground", |_, _| {})
            .add_fixed_system(FixedStage::Movement, "bounce", |_, _| {})
            .add_fixed_system(FixedStage::Gameplay, "walk", |_, _| {})
            .add_frame_system(FrameStage::Camera, "follow", |_, _| {})
            .add_frame_system(FrameStage::Animate, "react", |_, _| {});
        let printed: Vec<String> = app
            .schedule()
            .describe()
            .map(|system| system.to_string())
            .collect();
        assert_eq!(
            printed,
            [
                "tick  Input: nadir::snapshot",
                "tick  Gameplay: walk",
                "tick  Movement: nadir::movement",
                "tick  Movement: bounce",
                "tick  PostMovement: nadir::parent_offsets",
                "tick  PostMovement: nadir::triggers",
                "frame Animate: nadir::animate",
                "frame Animate: react",
                "frame Camera: follow",
                "frame Extract: nadir::sound",
                "frame Extract: nadir::extract_sprites",
                "frame Extract: nadir::extract_level",
                "frame Extract: draw_ground",
            ]
        );
    }

    #[test]
    fn the_engines_components_are_registered_and_eight_are_reflected() {
        let app = app();
        let world = app.world();
        assert!(world.is_registered::<Transform>());
        assert!(world.is_registered::<PrevTransform>());
        assert!(world.is_registered::<Velocity>());
        assert!(world.is_registered::<Sprite>());
        assert!(world.is_registered::<Collider>());
        assert!(world.is_registered::<Player>());
        assert!(world.is_registered::<Parent>());
        assert!(world.is_registered::<SaveId>());
        assert!(world.is_registered::<Facing>());
        assert!(world.is_registered::<Animator>());
        assert!(world.is_registered::<AudioListener>());
        assert!(world.is_registered::<AudioEmitter>());
        assert!(world.is_registered::<MusicArea>());
        let registry = world.resource::<TypeRegistry>();
        let names: Vec<&str> = registry.iter().map(|it| it.name()).collect();
        assert_eq!(
            names,
            [
                "Transform",
                "Velocity",
                "Player",
                "SaveId",
                "Facing",
                "Sprite",
                "Collider",
                "Animator"
            ]
        );
        // The engine's own, the prefabs a game loads, no level yet, and
        // the mixer's handle, with no music sent.
        assert!(world.contains_resource::<CollisionLayers>());
        assert!(world.contains_resource::<PrefabLibrary>());
        assert_eq!(world.resource::<CurrentLevel>().name(), None);
        assert!(world.contains_resource::<nadir_audio::Audio>());
        assert_eq!(world.resource::<AudioState>().music_area(), None);
    }

    nadir_scene::reflect_newtype! {
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        struct Score(u32);
    }

    impl Component for Score {}

    #[test]
    fn a_game_registers_its_reflected_components_and_events() {
        #[derive(Clone)]
        struct Scored;
        let mut app = app()
            .register_component::<Score>()
            .add_event::<Scored>()
            .add_fixed_system(FixedStage::Gameplay, "score", |world, _| {
                world
                    .resource_mut::<nadir_ecs::Events<Scored>>()
                    .send(Scored);
            });
        assert!(
            app.world()
                .resource::<TypeRegistry>()
                .get("Score")
                .is_some()
        );
        let entity = app.world_mut().spawn().with(Score(3)).id();
        assert_eq!(app.world().read::<Score>().get(entity), Some(&Score(3)));
        app.update(TICK, &[]);
        let events = app.world().resource::<nadir_ecs::Events<Scored>>();
        assert_eq!(events.frame_events().len(), 0, "cleared after the frame");
    }

    // -- one frame ----------------------------------------------------------

    fn note(world: &World, text: String) {
        world.resource::<Shared>().seen.borrow_mut().push(text);
    }

    #[test]
    fn a_frame_runs_its_ticks_and_then_its_frame_stages_in_order() {
        let log = shared();
        let mut app = app()
            .insert_resource(Rc::clone(&log))
            .add_fixed_system(FixedStage::Gameplay, "a", |world, tick| {
                note(world, format!("a tick {}", tick.index));
            })
            .add_frame_system(FrameStage::Ui, "c", |world, _| note(world, "c".into()))
            .add_fixed_system(FixedStage::Input, "b", |world, tick| {
                note(world, format!("b tick {}", tick.index));
            })
            .add_frame_system(FrameStage::Camera, "d", |world, _| note(world, "d".into()));
        // 40 ms hold two ticks.
        app.update(40 * MS, &[]);
        assert_eq!(
            *log.seen.borrow(),
            ["b tick 0", "a tick 0", "b tick 1", "a tick 1", "d", "c"]
        );
    }

    #[test]
    fn a_frame_shorter_than_a_tick_still_runs_the_frame_stages() {
        let log = shared();
        let mut app = counting_app(&log);
        app.update(5 * MS, &[]);
        app.update(5 * MS, &[]);
        assert_eq!((log.ticks.get(), log.frames.get()), (0, 2));
        app.update(10 * MS, &[]);
        assert_eq!((log.ticks.get(), log.frames.get()), (1, 3));
    }

    #[test]
    fn systems_get_the_tick_length_and_the_frame_time() {
        let log = shared();
        let mut app = app()
            .insert_resource(Rc::clone(&log))
            .add_fixed_system(FixedStage::Gameplay, "tick", |world, tick| {
                let log = world.resource::<Shared>();
                log.values.borrow_mut().push(tick.dt);
            })
            .add_frame_system(FrameStage::Ui, "frame", |world, frame| {
                let log = world.resource::<Shared>();
                log.values.borrow_mut().extend([frame.dt, frame.alpha]);
            });
        app.update(20 * MS, &[]);
        let values = log.values.borrow();
        assert_eq!(values[..2], [1.0 / 60.0, 0.02]);
        // 20 ms is one tick of 16.67 ms and a fifth of the next.
        assert!((values[2] - 0.2).abs() < 1e-6, "{values:?}");
    }

    #[test]
    fn tick_stats_count_this_frames_ticks_for_its_frame_stages() {
        let log = shared();
        let mut app = app()
            .insert_resource(Rc::clone(&log))
            .add_fixed_system(FixedStage::Gameplay, "slow", |_, tick| {
                if tick.index == 1 {
                    thread::sleep(3 * MS);
                }
            })
            .add_frame_system(FrameStage::Ui, "read", |world, _| {
                let stats = *world.resource::<TickStats>();
                note(world, format!("{}", stats.ticks));
            });
        app.update(40 * MS, &[]);
        let two = *app.world().resource::<TickStats>();
        assert_eq!(two.ticks, 2);
        assert!(two.longest >= 3 * MS && two.time >= two.longest, "{two:?}");
        app.update(5 * MS, &[]);
        assert_eq!(*app.world().resource::<TickStats>(), TickStats::default());
        assert_eq!(*log.seen.borrow(), ["2", "0"]);
    }

    // -- the tick rate ------------------------------------------------------

    #[test]
    fn the_frame_time_is_how_long_the_simulation_has_run() {
        let mut app = app();
        // A quarter of a tick: no tick yet.
        app.update(TICK / 4, &[]);
        let time = |app: &App| app.world().resource::<RenderFrame>().time;
        let close = |found: Duration, expected: Duration| {
            assert!(
                found.abs_diff(expected) < Duration::from_micros(1),
                "{found:?}, not {expected:?}"
            );
        };
        close(time(&app), TICK / 4);
        for _ in 0..3 {
            app.update(TICK, &[]);
        }
        close(time(&app), TICK * 3 + TICK / 4);
        // A hitch counts for a quarter of a second, as the ticks do.
        app.update(Duration::from_secs(5), &[]);
        close(time(&app), TICK * 3 + TICK / 4 + Duration::from_millis(250));
        // Emptying the frame for the next leaves the time to the engine.
        app.world().resource_mut::<RenderFrame>().clear();
        assert!(time(&app) > Duration::ZERO);
    }

    #[test]
    fn ten_seconds_run_600_ticks_whatever_the_frame_rate() {
        let frame_lengths = [
            TICK,                            // 60 frames a second
            Duration::from_nanos(6_944_444), // 144
            Duration::from_micros(33_333),   // 30
            Duration::from_millis(3),
            Duration::from_millis(110),
        ];
        for frame_length in frame_lengths {
            let log = shared();
            let mut app = counting_app(&log);
            simulate(&mut app, frame_length, Duration::from_secs(10));
            // The task asks for 60 Hz within 1%. The clock is exact.
            assert_eq!(log.ticks.get(), 600, "frames of {frame_length:?}");
            assert!(log.frames.get() >= 90, "frames of {frame_length:?}");
        }
    }

    #[test]
    fn a_simulated_hitch_runs_a_quarter_second_of_ticks_and_no_more() {
        let log = shared();
        let mut app = counting_app(&log);
        simulate(&mut app, 10 * MS, Duration::from_secs(1));
        assert_eq!(log.ticks.get(), 60);

        // One frame takes a whole second.
        app.update(Duration::from_secs(1), &[]);
        assert_eq!(log.ticks.get(), 60 + 15);

        // Nothing is owed afterwards: the next second is an ordinary one.
        simulate(&mut app, 10 * MS, Duration::from_secs(1));
        assert_eq!(log.ticks.get(), 60 + 15 + 60);
    }

    // -- the loop -----------------------------------------------------------

    /// A window that reports a few events and asks to close when its script
    /// says so. During a pump, it can also hold the loop as a system's own
    /// event loop does, and run modal frames.
    struct Script {
        pumps: u32,
        close: Box<dyn FnMut(u32) -> bool>,
        asked_to_close: bool,
        /// How many modal frames to run during the pump of this number.
        modal_frames: Box<dyn FnMut(u32) -> u32>,
        modal_frame_callback: Option<Box<dyn FnMut(ModalFrame)>>,
    }

    impl Script {
        fn closing_when(close: impl FnMut(u32) -> bool + 'static) -> Self {
            Self {
                pumps: 0,
                close: Box::new(close),
                asked_to_close: false,
                modal_frames: Box::new(|_| 0),
                modal_frame_callback: None,
            }
        }

        fn with_modal_frames(mut self, modal_frames: impl FnMut(u32) -> u32 + 'static) -> Self {
            self.modal_frames = Box::new(modal_frames);
            self
        }
    }

    impl EventSource for Script {
        fn pump_events(&mut self, out: &mut Vec<Event>) {
            // A loop that missed the request would never end. Fail instead.
            assert!(!self.asked_to_close, "the loop went on after a close");
            self.pumps += 1;
            let frame = ModalFrame {
                size: UVec2::new(800, 600),
                scale_factor: 1.0,
            };
            for _ in 0..(self.modal_frames)(self.pumps) {
                let callback = self.modal_frame_callback.as_mut();
                (callback.expect("the loop set a modal frame callback"))(frame);
            }
            // Other events do not end the loop.
            out.push(Event::Focused(true));
            out.push(Event::Resized(UVec2::new(800, 600)));
            if (self.close)(self.pumps) {
                self.asked_to_close = true;
                out.push(Event::CloseRequested);
            }
        }

        fn set_modal_frame_callback(&mut self, callback: Box<dyn FnMut(ModalFrame)>) {
            self.modal_frame_callback = Some(callback);
        }
    }

    #[test]
    fn the_loop_runs_until_the_window_asks_to_close() {
        let log = shared();
        let mut app = counting_app(&log);
        app.config.frame_rate_limit = None;
        let mut window = Script::closing_when(|pumps| pumps == 8);
        app.run_loop(&mut window, None)
            .expect("no error without a device");
        // Seven frames ran, and the eighth pump ended the loop before its
        // frame.
        assert_eq!(window.pumps, 8);
        assert_eq!(log.frames.get(), 7);
    }

    #[test]
    fn a_close_request_on_the_first_pump_runs_no_frame() {
        let log = shared();
        let app = counting_app(&log);
        app.run_loop(&mut Script::closing_when(|_| true), None)
            .expect("no error without a device");
        assert_eq!((log.ticks.get(), log.frames.get()), (0, 0));
    }

    #[test]
    fn while_the_system_holds_the_loop_frames_run_from_the_callback() {
        let log = shared();
        let mut app = counting_app(&log);
        app.config.frame_rate_limit = None;
        // The second pump holds the loop for five modal frames, as a drag
        // of the window does on Windows.
        let mut window = Script::closing_when(|pumps| pumps == 3)
            .with_modal_frames(|pumps| if pumps == 2 { 5 } else { 0 });
        app.run_loop(&mut window, None)
            .expect("no error without a device");
        // One frame after the first pump, five during the second and one
        // after it, and none after the third.
        assert_eq!(log.frames.get(), 7);
    }

    #[test]
    fn modal_frames_keep_the_simulation_on_time() {
        let log = shared();
        let mut app = counting_app(&log)
            .add_frame_system(FrameStage::Ui, "wait", |_, _| thread::sleep(10 * MS));
        app.config.frame_rate_limit = None;
        // The first pump holds the loop for 300 ms or more, in modal frames
        // of at least 10 ms.
        let start = Instant::now();
        let mut window = Script::closing_when(|pumps| pumps == 2)
            .with_modal_frames(|pumps| if pumps == 1 { 30 } else { 0 });
        app.run_loop(&mut window, None)
            .expect("no error without a device");
        let elapsed = start.elapsed();
        assert_eq!(log.frames.get(), 30 + 1);
        // The ticks kept up with real time while the loop was held: 60 a
        // second for 300 ms is 18, less one for rounding. Without modal
        // frames, the frame after the hold would have counted only a
        // quarter of a second: 15.
        let ticks = log.ticks.get();
        assert!(ticks >= 17, "{ticks} ticks in {elapsed:?}");
    }

    #[test]
    fn the_limiter_paces_the_loop() {
        let log = shared();
        let mut app = counting_app(&log);
        app.config.frame_rate_limit = Some(200);
        let start = Instant::now();
        app.run_loop(&mut Script::closing_when(|pumps| pumps == 21), None)
            .expect("no error without a device");
        // Twenty frames of 5 ms each. A busy machine can make them longer,
        // but nothing makes them shorter.
        assert_eq!(log.frames.get(), 20);
        assert!(
            start.elapsed() >= Duration::from_millis(100),
            "{:?}",
            start.elapsed()
        );
    }

    /// What the hitch test's systems saw.
    #[derive(Default)]
    struct Hitch {
        /// Each frame's ticks and `dt`.
        frames: RefCell<Vec<(u32, f32)>>,
        ticks_this_frame: Cell<u32>,
        stalled: Cell<bool>,
        frames_since_stall: Cell<u32>,
    }

    #[test]
    fn a_real_hitch_of_one_second_is_clamped_and_does_not_spiral() {
        let hitch: Rc<Hitch> = Rc::default();
        let app = app()
            .insert_resource(Rc::clone(&hitch))
            .add_fixed_system(FixedStage::Gameplay, "stall_once", |world, tick| {
                let hitch = world.resource::<Rc<Hitch>>();
                hitch.ticks_this_frame.set(hitch.ticks_this_frame.get() + 1);
                // The hitch: the third tick takes a second.
                if tick.index == 2 {
                    thread::sleep(Duration::from_secs(1));
                    hitch.stalled.set(true);
                }
            })
            .add_frame_system(FrameStage::Ui, "record", |world, frame| {
                let hitch = world.resource::<Rc<Hitch>>();
                let ticks = hitch.ticks_this_frame.take();
                hitch.frames.borrow_mut().push((ticks, frame.dt));
                if hitch.stalled.get() {
                    hitch
                        .frames_since_stall
                        .set(hitch.frames_since_stall.get() + 1);
                }
            });

        // Run on for a few frames after the one that stalled. If the stall
        // never comes, give up after ten seconds of frames, so that a bug
        // fails this test and does not hang it.
        let start = Instant::now();
        let script = Rc::clone(&hitch);
        app.run_loop(
            &mut Script::closing_when(move |pumps| {
                script.frames_since_stall.get() > 6 || pumps > 600
            }),
            None,
        )
        .expect("no error without a device");
        let elapsed = start.elapsed();
        assert!(hitch.stalled.get(), "the stall never happened");

        let frames = hitch.frames.borrow();
        let most_ticks = frames.iter().map(|&(ticks, _)| ticks).max();
        let longest_dt = frames.iter().map(|&(_, dt)| dt).fold(0.0, f32::max);
        let total_ticks: u32 = frames.iter().map(|&(ticks, _)| ticks).sum();

        // The frame after the stall saw a second go by. It counted a
        // quarter of one: 15 ticks. Unclamped, it would have run 60.
        assert_eq!(most_ticks, Some(15), "{frames:?}");
        assert_eq!(longest_dt, 0.25, "{frames:?}");

        // And the loop did not try to win the lost time back later: over
        // the whole run, the ticks stay at least three quarters of a second
        // behind real time.
        let behind = elapsed.as_secs_f64() - f64::from(total_ticks) / 60.0;
        assert!(
            behind >= 0.74,
            "{behind} s behind after {elapsed:?}: {frames:?}"
        );
    }

    // -- actions, movement and drawing ----------------------------------------

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Act {
        Move,
    }

    impl ActionSet for Act {
        const ALL: &'static [Self] = &[Act::Move];

        fn index(self) -> usize {
            self as usize
        }

        fn name(self) -> &'static str {
            "move"
        }

        fn kind(self) -> ActionKind {
            ActionKind::Axis2
        }
    }

    /// Pixels a second: two a tick.
    const SPEED: f32 = 120.0;

    /// The entity that moves.
    struct Mover(Entity);

    /// A texture handle, which nothing loads.
    fn texture() -> nadir_assets::Handle<nadir_render::Texture> {
        nadir_assets::Handle::new(nadir_core::GenIndex::new(0, std::num::NonZeroU32::MIN))
    }

    /// An app that moves an entity with `Act::Move`, bound to WASD, and
    /// notes in `log` where each frame draws it.
    fn moving_app(log: &Shared) -> App {
        let mut map = ActionMap::<Act>::new().expect("a valid set");
        let wasd = Binding::KeyComposite2 {
            up: Key::W,
            down: Key::S,
            left: Key::A,
            right: Key::D,
        };
        map.bind(Act::Move, 0, wasd).expect("fits");
        let mut app = app()
            .add_actions(map)
            .insert_resource(Rc::clone(log))
            .add_fixed_system(FixedStage::Gameplay, "steer", |world, _| {
                let direction = world.resource::<ActionState<Act>>().axis2(Act::Move);
                let mover = world.resource::<Mover>().0;
                if let Some(velocity) = world.write::<Velocity>().get_mut(mover) {
                    velocity.0 = direction * SPEED;
                }
            })
            .add_frame_system(FrameStage::Extract, "record", |world, _| {
                let x = world.resource::<RenderFrame>().sprites[0].position.x;
                world.resource::<Shared>().values.borrow_mut().push(x);
            });
        let mover = app
            .world_mut()
            .spawn()
            .with(Transform::at(Vec2::ZERO))
            .with(Velocity::default())
            .with(Sprite::new(texture(), Vec2::splat(8.0)))
            .id();
        app.insert_resource(Mover(mover))
    }

    fn key(key: Key, pressed: bool) -> Event {
        Event::Key {
            key,
            pressed,
            repeat: false,
        }
    }

    fn x(app: &App) -> f32 {
        let mover = app.world().resource::<Mover>().0;
        app.world()
            .read::<Transform>()
            .get(mover)
            .expect("it is")
            .position
            .x
    }

    #[test]
    fn a_tap_between_ticks_moves_exactly_one_tick() {
        let mut app = moving_app(&shared());
        // D goes down and up within a frame too short for a tick.
        app.update(5 * MS, &[key(Key::D, true), key(Key::D, false)]);
        assert_eq!(x(&app), 0.0, "no tick yet");
        // The next frame's tick moves it once.
        app.update(12 * MS, &[]);
        assert_eq!(x(&app), SPEED / 60.0);
        for _ in 0..10 {
            app.update(16 * MS, &[]);
        }
        assert_eq!(x(&app), SPEED / 60.0, "and never again");
        // In a frame of two ticks, the second sees no edge either.
        app.update(34 * MS, &[key(Key::D, true), key(Key::D, false)]);
        assert_eq!(x(&app), 2.0 * SPEED / 60.0);
    }

    #[test]
    fn a_held_key_moves_every_tick() {
        let mut app = moving_app(&shared());
        app.update(Duration::ZERO, &[key(Key::D, true)]);
        // Exactly a second of 60 Hz frames: sixty ticks.
        for _ in 0..60 {
            app.update(TICK, &[]);
        }
        assert!((x(&app) - SPEED).abs() < 1e-3, "{}", x(&app));
        app.update(Duration::ZERO, &[key(Key::D, false)]);
        app.update(20 * MS, &[]);
        assert!((x(&app) - SPEED).abs() < 1e-3, "it stops: {}", x(&app));
    }

    #[test]
    fn the_wheel_moves_on_each_tick_without_an_action_set() {
        let log = shared();
        let mut app = app().insert_resource(Rc::clone(&log)).add_fixed_system(
            FixedStage::Input,
            "zoom",
            |world, _| {
                let wheel = world.resource::<Input>().wheel();
                world.resource::<Shared>().values.borrow_mut().push(wheel.y);
            },
        );
        // A tick each.
        app.update(20 * MS, &[Event::MouseWheel(Vec2::new(0.0, 3.0))]);
        app.update(20 * MS, &[]);
        assert_eq!(*log.values.borrow(), [3.0, 0.0]);
    }

    #[test]
    fn interpolation_draws_where_the_mover_was_one_tick_ago_at_any_frame_rate() {
        // Held from the start, the mover is drawn at SPEED · (t − 1/60): one
        // tick behind the clock, so every frame moves it by its own length,
        // whatever that is.
        let mut irregular = nadir_core::Pcg32::new(7, 1);
        let rates: [(&str, Vec<Duration>); 5] = [
            ("60", vec![TICK; 120]),
            ("59.94", vec![Duration::from_nanos(16_683_350); 120]),
            ("144", vec![Duration::from_nanos(6_944_444); 288]),
            ("1 ms", vec![MS; 2000]),
            (
                "irregular",
                (0..500)
                    .map(|_| Duration::from_micros(u64::from(irregular.range(1000..9000u32))))
                    .collect(),
            ),
        ];
        for (name, frames) in rates {
            let log = shared();
            let mut app = moving_app(&log);
            app.update(Duration::ZERO, &[key(Key::D, true)]);
            let mut t = Duration::ZERO;
            let mut expected = vec![0.0];
            for frame in frames {
                app.update(frame, &[]);
                t += frame;
                let x = SPEED as f64 * (t.as_secs_f64() - 1.0 / 60.0).max(0.0);
                expected.push(x);
            }
            let drawn = log.values.borrow();
            assert_eq!(drawn.len(), expected.len(), "{name}");
            for (i, (&drawn, &expected)) in drawn.iter().zip(&expected).enumerate() {
                assert!(
                    (f64::from(drawn) - expected).abs() < 2e-3,
                    "{name}: frame {i} drew {drawn}, not {expected}"
                );
            }
        }
    }

    #[test]
    #[should_panic(expected = "an App has one action set")]
    fn a_second_action_set_is_refused() {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Menu {
            Back,
        }

        impl ActionSet for Menu {
            const ALL: &'static [Self] = &[Menu::Back];

            fn index(self) -> usize {
                0
            }

            fn name(self) -> &'static str {
                "back"
            }

            fn kind(self) -> ActionKind {
                ActionKind::Button
            }
        }

        let map = ActionMap::<Menu>::new().expect("a valid set");
        let _ = moving_app(&shared()).add_actions(map);
    }

    #[test]
    fn the_same_action_set_again_replaces_its_bindings() {
        let mut map = ActionMap::<Act>::new().expect("a valid set");
        let arrows = Binding::KeyComposite2 {
            up: Key::Up,
            down: Key::Down,
            left: Key::Left,
            right: Key::Right,
        };
        map.bind(Act::Move, 0, arrows).expect("fits");
        let mut app = moving_app(&shared()).add_actions(map);
        app.update(20 * MS, &[key(Key::D, true)]);
        assert_eq!(x(&app), 0.0, "D is no longer bound");
        app.update(20 * MS, &[key(Key::Right, true)]);
        assert_eq!(x(&app), SPEED / 60.0, "one latch a tick, with the new map");
    }

    // -- replays ----------------------------------------------------------------

    /// The replay of a run of `moving_app` that gets `frames`' events, a
    /// frame of one tick each.
    fn recording(frames: &[&[Event]]) -> &'static [u8] {
        let mut app = moving_app(&shared())
            .insert_resource(InputRecorder::<Act>::new().expect("a valid set"))
            .add_fixed_system(FixedStage::Input, "record", |world, _| {
                let state = world.resource::<ActionState<Act>>();
                world.resource_mut::<InputRecorder<Act>>().record(&state);
            });
        for events in frames {
            app.update(TICK, events);
        }
        let bytes = app
            .world()
            .resource::<InputRecorder<Act>>()
            .bytes()
            .to_vec();
        Box::leak(bytes.into_boxed_slice())
    }

    fn replaying(bytes: &'static [u8]) -> App {
        let player = InputPlayer::<Act>::new(bytes).expect("a replay of Act");
        moving_app(&shared()).play_replay(player)
    }

    fn replay(app: &App) -> Replay {
        app.world().resource::<Replay>().clone()
    }

    /// `Act::Move`'s held, just pressed and just released, in the last tick.
    fn moving(app: &App) -> (bool, bool, bool) {
        let state = app.world().resource::<ActionState<Act>>();
        (
            state.pressed(Act::Move),
            state.just_pressed(Act::Move),
            state.just_released(Act::Move),
        )
    }

    /// One tick's move at [`SPEED`].
    const STEP: f32 = SPEED / 60.0;

    #[test]
    fn a_replay_moves_as_the_recording_did_whatever_the_live_input() {
        let bytes = recording(&[&[key(Key::D, true)], &[], &[]]);
        let mut app = replaying(bytes);
        assert_eq!(replay(&app), Replay::Playing { ticks: 0 });
        // Live input that the recording did not have. Losing the focus
        // releases every key, so A goes down after it.
        let noise = [
            Event::Focused(false),
            Event::MouseWheel(Vec2::new(0.0, 3.0)),
            Event::MouseMoved(Vec2::new(5.0, 5.0)),
            key(Key::A, true),
        ];
        app.update(TICK, &noise);
        assert_eq!(
            (x(&app), replay(&app)),
            (STEP, Replay::Playing { ticks: 1 })
        );
        assert_eq!(moving(&app), (true, true, false));
        app.update(TICK, &[]);
        assert_eq!(replay(&app), Replay::Playing { ticks: 2 });
        // The last tick: ended as soon as it has run.
        app.update(TICK, &[]);
        assert_eq!(
            (x(&app), replay(&app)),
            (3.0 * STEP, Replay::Ended { ticks: 3 })
        );
        // Then the live input: A, held since the first tick, with no edge.
        app.update(TICK, &[]);
        assert_eq!(x(&app), 2.0 * STEP);
        assert_eq!(moving(&app), (true, false, false));
        assert_eq!(replay(&app), Replay::Ended { ticks: 3 });
    }

    #[test]
    fn a_frame_of_two_ticks_plays_two_replayed_ticks_and_a_short_frame_none() {
        let bytes = recording(&[&[key(Key::D, true)], &[], &[], &[]]);
        let mut app = replaying(bytes);
        app.update(40 * MS, &[]);
        assert_eq!(
            (x(&app), replay(&app)),
            (2.0 * STEP, Replay::Playing { ticks: 2 })
        );
        app.update(5 * MS, &[]);
        assert_eq!(
            (x(&app), replay(&app)),
            (2.0 * STEP, Replay::Playing { ticks: 2 })
        );
        app.update(40 * MS, &[]);
        assert_eq!(
            (x(&app), replay(&app)),
            (4.0 * STEP, Replay::Ended { ticks: 4 })
        );
    }

    #[test]
    fn after_a_replay_the_live_ticks_have_only_the_live_keys_edges() {
        // Held at the replay's end: not just released after it.
        let mut app = replaying(recording(&[&[key(Key::D, true)], &[]]));
        app.update(TICK, &[]);
        app.update(TICK, &[]);
        assert_eq!(replay(&app), Replay::Ended { ticks: 2 });
        app.update(TICK, &[]);
        assert_eq!(moving(&app), (false, false, false));

        // A tap during the replay was latched into nothing: no edge after.
        let mut app = replaying(recording(&[&[], &[]]));
        app.update(TICK, &[key(Key::D, true), key(Key::D, false)]);
        app.update(TICK, &[]);
        app.update(TICK, &[]);
        assert_eq!(moving(&app), (false, false, false));
        assert_eq!(x(&app), 0.0);
    }

    /// What `Replay` said in each tick, as its systems saw it.
    #[derive(Default)]
    struct SeenReplays(Vec<Replay>);

    #[test]
    fn each_tick_sees_its_own_count_and_the_first_after_the_end_sees_it_ended() {
        let mut app = replaying(recording(&[&[], &[], &[], &[]]))
            .insert_resource(SeenReplays::default())
            .add_fixed_system(FixedStage::Gameplay, "see", |world, _| {
                let replay = world.resource::<Replay>().clone();
                world.resource_mut::<SeenReplays>().0.push(replay);
            });
        // Frames of two ticks and of three: the second runs past the end.
        app.update(TICK * 2, &[]);
        app.update(TICK * 3, &[]);
        let playing = |ticks| Replay::Playing { ticks };
        assert_eq!(
            app.world().resource::<SeenReplays>().0,
            [
                playing(1),
                playing(2),
                playing(3),
                playing(4),
                Replay::Ended { ticks: 4 }
            ]
        );
    }

    #[test]
    fn a_damaged_tick_fails_the_replay_and_the_input_goes_live() {
        let mut bytes = recording(&[&[key(Key::D, true)], &[], &[]]).to_vec();
        // The header is 16 bytes and a kind; each tick a flag byte and an
        // x and a y. Bit 7 of the second tick's flags belongs to no action.
        bytes[17 + 5] |= 0x80;
        let mut app = replaying(Box::leak(bytes.into_boxed_slice()));
        app.update(TICK, &[key(Key::A, true)]);
        assert_eq!(x(&app), STEP, "the first tick plays");
        let Replay::Failed { ticks: 1, error } = replay(&app) else {
            panic!("{:?}", replay(&app));
        };
        assert!(error.to_string().contains("22"), "{error}");
        // A, live from now on.
        app.update(TICK, &[]);
        assert_eq!(x(&app), 0.0);
    }

    #[test]
    fn a_replay_of_no_ticks_ends_at_once() {
        let mut app = replaying(recording(&[]));
        assert_eq!(replay(&app), Replay::Ended { ticks: 0 });
        app.update(TICK, &[key(Key::D, true)]);
        assert_eq!(x(&app), STEP, "live");
    }

    #[test]
    #[should_panic(expected = "add them with add_actions first")]
    fn a_replay_needs_its_actions_first() {
        let bytes = recording(&[&[]]);
        let _ = app().play_replay(InputPlayer::<Act>::new(bytes).expect("a replay"));
    }

    #[test]
    #[should_panic(expected = "play_replay must come after add_actions")]
    fn the_actions_cannot_be_added_while_a_replay_plays() {
        let map = ActionMap::<Act>::new().expect("a valid set");
        let _ = replaying(recording(&[&[]])).add_actions(map);
    }

    #[test]
    fn the_world_hash_follows_the_reflected_components_only() {
        let mut app = app();
        let walker = app
            .world_mut()
            .spawn()
            .with(Transform::at(Vec2::ZERO))
            .with(Sprite::new(texture(), Vec2::ONE))
            .id();
        let start = app.world_hash();
        assert_eq!(app.world_hash(), start);
        app.world()
            .write::<Sprite>()
            .get_mut(walker)
            .expect("drawn")
            .size = Vec2::splat(2.0);
        assert_eq!(
            app.world_hash(),
            start,
            "a Sprite's size is left out, as animators write it"
        );
        app.world()
            .write::<Sprite>()
            .get_mut(walker)
            .expect("drawn")
            .layer = 2;
        let layered = app.world_hash();
        assert_ne!(layered, start, "a Sprite's layer is the game's, and hashed");
        app.world()
            .write::<Transform>()
            .get_mut(walker)
            .expect("placed")
            .position
            .x = 1.0;
        assert_ne!(app.world_hash(), layered);
    }

    #[test]
    fn the_render_frame_is_emptied_before_each_frame() {
        let log = shared();
        let mut app = app().insert_resource(Rc::clone(&log)).add_frame_system(
            FrameStage::Camera,
            "draw",
            |world, _| {
                let mut frame = world.resource_mut::<RenderFrame>();
                let size = frame.sprites.len() as f32;
                world.resource::<Shared>().values.borrow_mut().push(size);
                frame.sprites.push(nadir_render::SpriteDraw::new(
                    texture(),
                    Vec2::ZERO,
                    Vec2::ONE,
                ));
            },
        );
        for _ in 0..3 {
            app.update(16 * MS, &[]);
        }
        assert_eq!(*log.values.borrow(), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_missing_resource_names_the_system_that_asked() {
        let mut app = app().add_fixed_system(FixedStage::Gameplay, "needs_a_score", |world, _| {
            let _ = world.resource::<u64>();
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            app.update(20 * MS, &[]);
        }));
        let payload = result.expect_err("a panic");
        let message = payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default();
        assert!(
            message.contains("`needs_a_score`") && message.contains("u64"),
            "{message}"
        );
    }

    /// A script whose first pump taps D.
    struct Tapping {
        pumps: u32,
    }

    impl EventSource for Tapping {
        fn pump_events(&mut self, out: &mut Vec<Event>) {
            self.pumps += 1;
            match self.pumps {
                1 => out.extend([key(Key::D, true), key(Key::D, false)]),
                // A few frames later, enough for a tick or more.
                40 => out.push(Event::CloseRequested),
                _ => thread::sleep(MS),
            }
        }

        fn set_modal_frame_callback(&mut self, _callback: Box<dyn FnMut(ModalFrame)>) {}
    }

    #[test]
    fn the_loop_feeds_pumped_keys_to_the_actions() {
        let log = shared();
        let mut app = moving_app(&log).add_frame_system(FrameStage::Ui, "report", |world, _| {
            let mover = world.resource::<Mover>().0;
            let x = world
                .read::<Transform>()
                .get(mover)
                .expect("it is")
                .position
                .x;
            world
                .resource::<Shared>()
                .seen
                .borrow_mut()
                .push(x.to_string());
        });
        app.config.frame_rate_limit = None;
        app.run_loop(&mut Tapping { pumps: 0 }, None)
            .expect("no error without a device");
        let seen = log.seen.borrow();
        let last: f32 = seen.last().expect("frames ran").parse().expect("a number");
        assert_eq!(last, SPEED / 60.0, "the tap moved it one tick");
    }

    /// A hitch's frame of 15 ticks, after frames of one, allocates nothing:
    /// with nadir-core's `profile` feature too, whose records it holds.
    #[cfg(feature = "dev")]
    #[test]
    fn a_frame_of_many_ticks_after_frames_of_one_allocates_nothing() {
        let mut app = app();
        app.world_mut()
            .spawn()
            .with(Transform::at(Vec2::ZERO))
            .with(Velocity(Vec2::ONE))
            .with(Sprite::new(texture(), Vec2::ONE));
        for _ in 0..5 {
            app.update(TICK, &[]);
        }
        let start = nadir_platform::thread_alloc_counts();
        app.update(Duration::from_secs(1), &[]);
        let allocated = nadir_platform::thread_alloc_counts().since(start);
        assert_eq!(app.world().resource::<TickStats>().ticks, 15);
        assert_eq!(allocated, nadir_platform::AllocCounts::default());
    }

    /// [`App::offscreen`]: frames drawn with no window, for recordings
    /// (ADR-0029). Behind `dev`, as `steady_frames` is, since they need an
    /// offscreen context.
    #[cfg(feature = "dev")]
    mod offscreen_frames {
        use std::{env, fs, process};

        use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
        use nadir_core::{Color, Vec2};
        use nadir_gfx::GfxError;
        use nadir_render::RenderError;

        use super::*;

        /// The frames' size: the virtual resolution, so a pixel of the game
        /// is a pixel of the frame.
        const SIZE: UVec2 = UVec2::new(32, 18);
        const CLEAR: [u8; 4] = [0, 0, 255, 255];
        const RED: [u8; 4] = [255, 0, 0, 255];

        /// An app of one red sprite, 8 pixels square, its feet 4 pixels
        /// right of the view's centre, on blue, whose systems count the ticks
        /// into `log`. It is off the centre so that a frame upside down or
        /// mirrored shows it elsewhere.
        fn app(test: &str, log: &Shared) -> App {
            let assets = env::temp_dir().join(format!("nadir-engine-{test}-{}", process::id()));
            fs::create_dir_all(&assets).expect("make the asset folder");
            let header = Header {
                width: 8,
                height: 8,
                channels: Channels::Rgba,
                color_space: ColorSpace::Srgb,
            };
            let bytes = qoi::encode(&header, &RED.repeat(64)).expect("a valid image");
            fs::write(assets.join("red.qoi"), bytes).expect("write the texture");
            let mut app = App::new(Config {
                virtual_resolution: SIZE,
                assets,
                ..Config::default()
            })
            .insert_resource(Rc::clone(log))
            .add_fixed_system(FixedStage::Gameplay, "count_ticks", count_ticks)
            .add_frame_system(FrameStage::Camera, "look", |world, _| {
                let mut frame = world.resource_mut::<RenderFrame>();
                frame.clear = Color::rgb(CLEAR[0], CLEAR[1], CLEAR[2]);
                frame.camera.position = Vec2::ZERO;
            });
            let red = app
                .world()
                .resource_mut::<AssetServer>()
                .load::<nadir_render::Texture>("red.qoi");
            app.world_mut()
                .spawn()
                .with(Transform::at(Vec2::new(4.0, 0.0)))
                .with(Sprite::new(red, Vec2::splat(8.0)));
            app
        }

        /// `app` drawn offscreen, or `None` where this system has no
        /// offscreen context yet (Windows and macOS, until M2.6 and M2.7)
        /// and `NADIR_EXPECT_GL_RENDERER` asks for none.
        fn offscreen(app: App, size: UVec2) -> Option<Offscreen> {
            let expected = env::var("NADIR_EXPECT_GL_RENDERER").unwrap_or_default();
            let assets = app.config.assets.clone();
            match app.offscreen(size) {
                Ok(offscreen) => {
                    let renderer = offscreen.renderer();
                    assert!(renderer.contains(&expected), "{renderer} is not {expected}");
                    Some(offscreen)
                }
                Err(Error::Render(RenderError::Gfx(GfxError::NoOffscreenContext)))
                    if expected.is_empty() =>
                {
                    fs::remove_dir_all(&assets).expect("remove the asset folder");
                    None
                }
                Err(error) => panic!("no offscreen OpenGL context: {error}"),
            }
        }

        /// Removes the app's asset folder, once a frame has waited for its
        /// texture to be read: later frames need no file.
        fn remove_assets(offscreen: &Offscreen) {
            fs::remove_dir_all(&offscreen.app().config.assets).expect("remove the asset folder");
        }

        fn pixel(frame: &[u8], x: u32, y: u32) -> [u8; 4] {
            let at = ((y * SIZE.x + x) * 4) as usize;
            frame[at..at + 4].try_into().expect("4 bytes")
        }

        #[test]
        fn each_frame_runs_its_ticks_and_draws_what_a_window_would_show() {
            let log = shared();
            let Some(mut offscreen) = offscreen(app("offscreen-frames", &log), SIZE) else {
                return;
            };
            assert_eq!(offscreen.size(), SIZE);

            // The texture is made after the first frame is drawn, so that
            // frame shows only the clear color, and a frame of no time runs
            // no tick.
            let first = offscreen.frame(Duration::ZERO, &[]).expect("a frame");
            remove_assets(&offscreen);
            assert_eq!(first.len(), (SIZE.x * SIZE.y * 4) as usize);
            assert!(first.as_chunks::<4>().0.iter().all(|&p| p == CLEAR));
            assert_eq!(log.ticks.get(), 0);
            let stats = *offscreen.app().world().resource::<FrameStats>();
            assert_eq!((stats.sprites, stats.skipped), (0, 1));

            // Then the sprite: 8 pixels square, its feet at the centre,
            // (16, 9), 4 pixels right, so rows 1 to 8 and columns 16 to 23.
            let second = offscreen.frame(TICK, &[]).expect("a frame");
            assert_eq!(log.ticks.get(), 1);
            assert_eq!(offscreen.app().world().resource::<FrameStats>().sprites, 1);
            for (x, y, expected) in [
                (16, 1, RED),
                (23, 8, RED),
                (20, 4, RED),
                (15, 4, CLEAR),
                (24, 4, CLEAR),
                (20, 0, CLEAR),
                (20, 9, CLEAR),
                (0, 0, CLEAR),
            ] {
                assert_eq!(pixel(&second, x, y), expected, "({x}, {y})");
            }
            offscreen.frame(TICK * 3, &[]).expect("a frame");
            assert_eq!(log.ticks.get(), 4);
        }

        #[test]
        fn a_frame_waits_for_the_textures_asked_for_before_it_ends() {
            let log = shared();
            let app = app("offscreen-wait", &log);
            // Noise, which takes a worker far longer to decode than the rest
            // of the frame takes after it asks.
            const SIDE: u32 = 512;
            let mut seed = 1u32;
            let noise: Vec<u8> = (0..SIDE * SIDE)
                .flat_map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let [r, g, b, _] = seed.to_le_bytes();
                    [r, g, b, 255]
                })
                .collect();
            let header = Header {
                width: SIDE,
                height: SIDE,
                channels: Channels::Rgba,
                color_space: ColorSpace::Srgb,
            };
            let bytes = qoi::encode(&header, &noise).expect("a valid image");
            fs::write(app.config.assets.join("noise.qoi"), bytes).expect("write the texture");
            // Each frame shows the noise, asked for just before the frame
            // draws and finishes its loads.
            let app = app.add_frame_system(FrameStage::Camera, "noise", |world, _| {
                let noise = world
                    .resource_mut::<AssetServer>()
                    .load::<nadir_render::Texture>("noise.qoi");
                for (_, sprite) in world.write::<Sprite>().iter_mut() {
                    sprite.texture = noise;
                }
            });
            let Some(mut offscreen) = offscreen(app, SIZE) else {
                return;
            };
            offscreen.frame(Duration::ZERO, &[]).expect("a frame");
            remove_assets(&offscreen);
            offscreen.frame(TICK, &[]).expect("a frame");
            let stats = *offscreen.app().world().resource::<FrameStats>();
            assert_eq!(
                (stats.sprites, stats.skipped),
                (1, 0),
                "the second frame did not draw the texture the first asked for"
            );
        }

        #[test]
        fn frames_bigger_than_the_view_are_scaled_up_by_whole_numbers() {
            let log = shared();
            let Some(mut offscreen) = offscreen(app("offscreen-scaled", &log), SIZE * 2) else {
                return;
            };
            offscreen.frame(Duration::ZERO, &[]).expect("a frame");
            remove_assets(&offscreen);
            let frame = offscreen.frame(TICK, &[]).expect("a frame");
            let width = SIZE.x * 2;
            let at = |x: u32, y: u32| {
                let i = ((y * width + x) * 4) as usize;
                <[u8; 4]>::try_from(&frame[i..i + 4]).expect("4 bytes")
            };
            assert_eq!(frame.len(), (width * SIZE.y * 2 * 4) as usize);
            assert_eq!(at(32, 2), RED);
            assert_eq!(at(47, 17), RED);
            assert_eq!(at(31, 2), CLEAR);
            assert_eq!(at(32, 1), CLEAR);
            assert_eq!(at(48, 17), CLEAR);
            assert_eq!(at(47, 18), CLEAR);
        }

        /// Writes the level `plot` into `assets`: a layer over the whole
        /// view of the tile `tile` of a tile set of two tiles of 8 pixels,
        /// green then yellow, the texture `tiles.qoi`.
        fn write_plot(assets: &std::path::Path, tile: u16) {
            use nadir_codec::{Limits, json, kbj};
            let write_kbj = |path: &str, text: &str| {
                let value = json::parse(text, Limits::default()).expect("the test's JSON");
                let bytes = kbj::encode(&value, Limits::default()).expect("KBJ");
                let path = assets.join(path);
                fs::create_dir_all(path.parent().expect("a folder")).expect("make the folder");
                fs::write(path, bytes).expect("write the file");
            };
            let header = Header {
                width: 16,
                height: 8,
                channels: Channels::Rgba,
                color_space: ColorSpace::Srgb,
            };
            let pixels: Vec<u8> = (0..16 * 8)
                .flat_map(|at| if at % 16 < 8 { GREEN } else { YELLOW })
                .collect();
            let texture = qoi::encode(&header, &pixels).expect("a valid image");
            fs::write(assets.join("tiles.qoi"), texture).expect("write the texture");
            write_kbj(
                "tiles.kbj",
                r#"{ "version": 2, "pages": [ { "path": "tiles.qoi", "w": 16, "h": 8 } ],
                     "sprites": [ { "name": "tiles", "tiles": [8, 8],
                       "frames": [ { "page": 0, "x": 0, "y": 0, "w": 16, "h": 8,
                                     "ox": 0, "oy": 0, "sw": 16, "sh": 8 } ] } ] }"#,
            );
            let cell = 1 + tile;
            write_kbj(
                "levels/plot.klvl",
                &format!(
                    r#"{{ "version": 1, "size": [32, 24], "background": [0, 0, 255],
                         "assets": ["tiles.kbj"],
                         "tilesets": [ {{ "atlas": "tiles.kbj", "sprite": "tiles", "columns": 2,
                                          "rows": 1, "animations": [] }} ],
                         "layers": [ {{ "tileset": 0, "size": [4, 3], "cell": 8, "origin": [-16, -9],
                                        "layer": 0, "depth": 0, "cells": [{}] }} ],
                         "tall": [], "entities": [] }}"#,
                    vec![cell.to_string(); 12].join(", ")
                ),
            );
        }

        const GREEN: [u8; 4] = [0, 255, 0, 255];
        const YELLOW: [u8; 4] = [255, 255, 0, 255];

        /// How many of a frame's pixels are `color`.
        fn count(frame: &[u8], color: [u8; 4]) -> usize {
            frame
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|&&p| p == color)
                .count()
        }

        #[test]
        fn a_level_loaded_again_draws_its_new_layer_at_once() {
            let log = shared();
            let mut app = app("offscreen-level", &log);
            let assets = app.config.assets.clone();
            write_plot(&assets, 0);
            // The tile set's texture is the game's own, so that loading the
            // level again replaces only its layer.
            {
                let world = app.world();
                let mut server = world.resource_mut::<AssetServer>();
                let atlas = server
                    .load_now::<nadir_render::Atlas>("tiles.kbj", &mut FinalizeCtx::new())
                    .expect("the atlas");
                let atlas = server.get(atlas).cloned().expect("the atlas");
                atlas.load_pages(&mut server, nadir_assets::Scope::Global);
            }
            app.load_level("plot").expect("the plot");
            let Some(mut offscreen) = offscreen(app, SIZE) else {
                return;
            };
            // The texture is made after the first frame.
            offscreen.frame(Duration::ZERO, &[]).expect("a frame");
            let first = offscreen.frame(TICK, &[]).expect("a frame");
            assert!(count(&first, GREEN) > 0, "the green tiles are drawn");
            assert_eq!(count(&first, YELLOW), 0);

            // The same level, its layer now yellow: the layer it replaces in
            // the tile layers is drawn anew in the very next frame.
            write_plot(&assets, 1);
            offscreen.app.load_level("plot").expect("the plot again");
            let second = offscreen.frame(TICK, &[]).expect("a frame");
            fs::remove_dir_all(&assets).expect("remove the asset folder");
            assert!(count(&second, YELLOW) > 0, "the yellow tiles are drawn");
            assert_eq!(count(&second, GREEN), 0, "no green tile is left");
            assert_eq!(count(&first, YELLOW) + count(&second, GREEN), 0);
        }
    }

    /// The hot reload, under `dev` (M5.8): files rewritten while the loop
    /// runs reach the game behind the handles it holds.
    #[cfg(feature = "dev")]
    mod hot_reload {
        use std::fs::{self, File};
        use std::path::{Path, PathBuf};
        use std::time::SystemTime;
        use std::{env, process};

        use nadir_assets::Handle;
        use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
        use nadir_core::Rect;
        use nadir_platform::HeadlessGl;
        use nadir_render::{Atlas, Program, SHADER_DIR, Texture, TileLayer};

        use super::steady_frames::{headless, write_kbj};
        use super::*;
        use crate::hot_reload::Reloads;
        use crate::{Clip, Clips};

        /// The atlas `atlas/sprites.kbj`: a hero of two frames, who walks
        /// in `walk`'s steps; a rock, its frame at `rock` pixels across;
        /// and a tile set of two tiles of 8 pixels, on a page of its own.
        fn atlas(walk: &str, rock: u32) -> String {
            format!(
                r#"{{ "version": 2,
                     "pages": [ {{ "path": "atlas/sprites_0.qoi", "w": 32, "h": 32 }},
                                {{ "path": "atlas/ground.qoi", "w": 16, "h": 8 }} ],
                     "sprites": [
                       {{ "name": "sprites/hero", "pivot": [4.0, 8.0],
                          "frames": [ {{ "page": 0, "x": 0, "y": 0, "w": 8, "h": 8, "ox": 0, "oy": 0, "sw": 8, "sh": 8 }},
                                      {{ "page": 0, "x": 8, "y": 0, "w": 8, "h": 8, "ox": 0, "oy": 0, "sw": 8, "sh": 8 }} ],
                          "clips": [ {{ "name": "walk", "loop": true, "frames": [{walk}] }} ] }},
                       {{ "name": "sprites/rock", "pivot": [4.0, 8.0],
                          "frames": [ {{ "page": 0, "x": {rock}, "y": 8, "w": 8, "h": 8, "ox": 0, "oy": 0, "sw": 8, "sh": 8 }} ] }},
                       {{ "name": "tiles/ground", "tiles": [8, 8],
                          "frames": [ {{ "page": 1, "x": 0, "y": 0, "w": 16, "h": 8, "ox": 0, "oy": 0, "sw": 16, "sh": 8 }} ] }} ] }}"#
            )
        }

        /// The prefab `name`: a sprite of the atlas's rock, in `layer`.
        fn rock(name: &str, layer: u8) -> String {
            format!(
                r#"{{ "name": "{name}", "components": {{ "Transform": {{}},
                     "Sprite": {{ "atlas": "atlas/sprites.kbj", "sprite": "sprites/rock", "frame": 0,
                                  "layer": {layer} }} }} }}"#
            )
        }

        /// The level `yard`: a layer of 4 × 2 cells of the tile `tile`, and
        /// an entity of the prefab `prop`.
        fn yard(tile: u16, prop: &str) -> String {
            let cells = vec![(tile + 1).to_string(); 8].join(", ");
            format!(
                r#"{{ "version": 1, "size": [32, 16], "background": [0, 0, 255],
                     "assets": ["atlas/sprites.kbj"],
                     "tilesets": [ {{ "atlas": "atlas/sprites.kbj", "sprite": "tiles/ground",
                                      "columns": 2, "rows": 1, "animations": [] }} ],
                     "layers": [ {{ "tileset": 0, "size": [4, 2], "cell": 8, "origin": [-16, -8],
                                    "layer": 0, "depth": 0, "cells": [{cells}] }} ],
                     "tall": [],
                     "entities": [ {{ "prefab": "{prop}", "position": [0, 0], "overrides": {{}} }} ] }}"#
            )
        }

        /// A QOI texture of `size`, of one color.
        fn texture(size: UVec2) -> Vec<u8> {
            let header = Header {
                width: size.x,
                height: size.y,
                channels: Channels::Rgba,
                color_space: ColorSpace::Srgb,
            };
            let pixels = [200, 80, 40, 255].repeat((size.x * size.y) as usize);
            qoi::encode(&header, &pixels).expect("a valid image")
        }

        /// What the last frame saw.
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        struct Seen {
            /// The size of the atlas's first page.
            page: Option<UVec2>,
            /// How long the walk lasts, under the handle the game holds.
            walk: Option<Duration>,
            /// The level's rock: its region, and its layer.
            rock: Option<(Rect, u8)>,
            /// The level's first layer, and the tile of its first cell.
            layer: Option<(Handle<TileLayer>, Option<u16>)>,
            reloads: Reloads,
        }

        type SharedSeen = Rc<RefCell<Seen>>;

        /// The game's handles, which `look` reads.
        struct Game {
            page: Handle<Texture>,
            walk: Handle<Clip>,
        }

        fn look(world: &World, _: &Frame) {
            let game = world.resource::<Game>();
            let server = world.resource::<AssetServer>();
            let current = world.resource::<CurrentLevel>();
            let rock = current.entities().first().and_then(|&rock| {
                let sprites = world.read::<Sprite>();
                sprites.get(rock).map(|sprite| (sprite.uv, sprite.layer))
            });
            let layer = current.tile_layers().first().map(|draw| {
                let layers = world.resource::<TileLayers>();
                let tile = layers
                    .get(draw.tiles)
                    .and_then(|layer| layer.get(nadir_core::IVec2::ZERO))
                    .map(|tile| tile.index);
                (draw.tiles, tile)
            });
            *world.resource::<SharedSeen>().borrow_mut() = Seen {
                page: server.get(game.page).map(Texture::size),
                walk: world.resource::<Clips>().get(game.walk).map(Clip::duration),
                rock,
                layer,
                reloads: *world.resource::<Reloads>(),
            };
        }

        /// An edit, and what shows it was reloaded.
        struct Step {
            name: &'static str,
            edit: fn(&Files),
            done: fn(&Seen, &Seen) -> bool,
        }

        /// Where the game's files are.
        struct Files {
            assets: PathBuf,
            shaders: PathBuf,
        }

        impl Files {
            /// Writes `bytes` at `path` as the cook does, through a file
            /// beside it and a rename, with a time of change `ahead`
            /// seconds from now, so that a system that keeps coarse times
            /// sees each write.
            fn write(path: &Path, bytes: &[u8], ahead: u64) {
                let temporary = path.with_extension("tmp");
                fs::write(&temporary, bytes).expect("write the file");
                File::options()
                    .write(true)
                    .open(&temporary)
                    .and_then(|file| {
                        file.set_modified(SystemTime::now() + Duration::from_secs(ahead))
                    })
                    .expect("set its time");
                fs::rename(&temporary, path).expect("rename it into place");
            }

            fn kbj(&self, path: &str, json: &str, ahead: u64) {
                let temporary = self.assets.join("edit.kbj");
                write_kbj(&temporary, json);
                let bytes = fs::read(&temporary).expect("the file just written");
                fs::remove_file(&temporary).expect("remove it");
                Self::write(&self.assets.join(path), &bytes, ahead);
            }

            fn sprite_shader(&self) -> PathBuf {
                self.shaders.join(Program::Sprite.fragment_file())
            }
        }

        /// The edits, one after another, each waited for.
        const STEPS: &[Step] = &[
            Step {
                name: "a texture of another size",
                edit: |files| {
                    let page = files.assets.join("atlas/sprites_0.qoi");
                    Files::write(&page, &texture(UVec2::new(64, 64)), 10);
                },
                done: |_, now| now.page == Some(UVec2::new(64, 64)),
            },
            Step {
                name: "an atlas whose walk is longer and whose rock moved",
                edit: |files| {
                    files.kbj(
                        "atlas/sprites.kbj",
                        &atlas("[0, 50], [1, 50], [0, 50]", 24),
                        20,
                    )
                },
                done: |first, now| {
                    now.walk == Some(Duration::from_millis(150))
                        && now.rock.map(|rock| rock.0) != first.rock.map(|rock| rock.0)
                },
            },
            Step {
                name: "a prefab in another layer",
                edit: |files| files.kbj("data/prefabs/rock.kbj", &rock("rock", 3), 30),
                done: |_, now| now.rock.map(|rock| rock.1) == Some(3),
            },
            Step {
                name: "a level of other tiles",
                edit: |files| files.kbj("levels/yard.klvl", &yard(1, "rock"), 40),
                done: |first, now| {
                    now.layer.map(|layer| layer.1) == Some(Some(1))
                        && now.layer.map(|layer| layer.0) == first.layer.map(|layer| layer.0)
                },
            },
            Step {
                name: "a level of a prefab not cooked yet",
                edit: |files| files.kbj("levels/yard.klvl", &yard(1, "boulder"), 50),
                done: |_, now| now.reloads.levels_refused == 1,
            },
            Step {
                name: "the prefab cooked after its level",
                edit: |files| files.kbj("data/prefabs/boulder.kbj", &rock("boulder", 5), 60),
                done: |_, now| now.rock.map(|rock| rock.1) == Some(5),
            },
            Step {
                name: "a shader edited",
                edit: |files| {
                    let path = files.sprite_shader();
                    let mut text = fs::read_to_string(&path).expect("the shader");
                    text.push_str("\n// Edited.\n");
                    Files::write(&path, text.as_bytes(), 70);
                },
                done: |_, now| now.reloads.shaders == 1,
            },
            Step {
                name: "a shader that does not compile",
                edit: |files| {
                    let broken =
                        "#version 330 core\nout vec4 color;\nvoid main() { color = oops; }\n";
                    Files::write(&files.sprite_shader(), broken.as_bytes(), 80);
                },
                done: |_, now| now.reloads.shaders_refused == 1,
            },
        ];

        /// The window: an offscreen context, which makes each edit once the
        /// last is seen, and closes after the last, or once one is not seen
        /// in [`PATIENCE`].
        struct Editing {
            gl: Option<HeadlessGl>,
            seen: SharedSeen,
            files: Files,
            /// What the first frame that drew the page saw.
            first: Option<Seen>,
            /// The step being waited for, and when its edit was made.
            step: usize,
            since: Option<Instant>,
            /// How long each step took to be seen.
            took: Vec<Duration>,
            started: Instant,
            /// When the last step was seen.
            quiet: Option<Instant>,
        }

        /// How long the loop runs on after the last edit is seen: more than
        /// two looks of the watchers.
        const QUIET: Duration = Duration::from_millis(600);

        /// How long an edit may take to be seen, which a loaded machine
        /// running the tests stretches: the watchers look every 250 ms.
        const PATIENCE: Duration = Duration::from_secs(5);

        impl EventSource for Editing {
            fn pump_events(&mut self, out: &mut Vec<Event>) {
                let seen = *self.seen.borrow();
                let Some(first) = self.first else {
                    // The first frames make the assets loaded at setup.
                    if seen.page.is_some() {
                        self.first = Some(seen);
                    } else if self.started.elapsed() > PATIENCE {
                        out.push(Event::CloseRequested);
                    }
                    return;
                };
                let Some(step) = STEPS.get(self.step) else {
                    // A while with no edit, in which nothing more reloads.
                    let quiet = self.quiet.get_or_insert_with(Instant::now);
                    if quiet.elapsed() > QUIET {
                        out.push(Event::CloseRequested);
                    }
                    return;
                };
                match self.since {
                    None => {
                        (step.edit)(&self.files);
                        self.since = Some(Instant::now());
                    }
                    Some(since) if (step.done)(&first, &seen) => {
                        self.took.push(since.elapsed());
                        self.step += 1;
                        self.since = None;
                    }
                    Some(since) if since.elapsed() > PATIENCE => out.push(Event::CloseRequested),
                    Some(_) => {}
                }
            }

            fn set_modal_frame_callback(&mut self, _callback: Box<dyn FnMut(ModalFrame)>) {}

            fn gpu_surface(&self) -> Option<GpuSurface<'_>> {
                self.gl.as_ref().map(|gl| GpuSurface::Gl(gl))
            }

            fn window_size(&self) -> UVec2 {
                UVec2::new(64, 36)
            }
        }

        #[test]
        fn files_rewritten_while_the_game_runs_reach_it_behind_its_handles() {
            let expected = env::var("NADIR_EXPECT_GL_RENDERER").unwrap_or_default();
            let Some(gl) = headless(&expected) else {
                return;
            };
            let root = env::temp_dir().join(format!("nadir-engine-hot-reload-{}", process::id()));
            let files = Files {
                assets: root.join("assets"),
                shaders: root.join("shaders"),
            };
            for &program in Program::ALL {
                for file in [program.vertex_file(), program.fragment_file()] {
                    let to = files.shaders.join(file);
                    fs::create_dir_all(to.parent().expect("a folder")).expect("make the folder");
                    fs::copy(Path::new(SHADER_DIR).join(file), to).expect("copy the shader");
                }
            }
            let assets = &files.assets;
            fs::create_dir_all(assets.join("atlas")).expect("make the folder");
            fs::write(
                assets.join("atlas/sprites_0.qoi"),
                texture(UVec2::splat(32)),
            )
            .expect("write the page");
            fs::write(assets.join("atlas/ground.qoi"), texture(UVec2::new(16, 8)))
                .expect("write the page");
            write_kbj(
                &assets.join("atlas/sprites.kbj"),
                &atlas("[0, 100], [1, 100]", 16),
            );
            write_kbj(&assets.join("data/prefabs/rock.kbj"), &rock("rock", 1));
            write_kbj(&assets.join("levels/yard.klvl"), &yard(0, "rock"));

            let seen = Rc::new(RefCell::new(Seen::default()));
            let mut app = App::new(Config {
                assets: assets.clone(),
                virtual_resolution: UVec2::new(32, 18),
                ..Config::default()
            })
            .insert_resource(Rc::clone(&seen))
            .add_frame_system(FrameStage::Ui, "look", look);
            app.shader_dir = files.shaders.clone();
            // The game adds the atlas's clips, as the sandbox does, and
            // holds the walk's handle and the first page's.
            let game = {
                let world = app.world();
                let mut server = world.resource_mut::<AssetServer>();
                let handle = server
                    .load_now::<Atlas>("atlas/sprites.kbj", &mut FinalizeCtx::new())
                    .expect("the atlas");
                let atlas = server.get(handle).cloned().expect("the atlas");
                let pages = atlas.load_pages(&mut server, nadir_assets::Scope::Global);
                world
                    .resource_mut::<Clips>()
                    .add_atlas(&atlas, &pages)
                    .expect("the clips");
                let walk = world
                    .resource::<Clips>()
                    .find("sprites/hero/walk")
                    .expect("walk");
                Game {
                    page: pages[0],
                    walk,
                }
            };
            app = app.insert_resource(game);
            app.load_level("yard").expect("the yard");

            let mut device = Device::new(GpuSurface::Gl(&gl)).expect("a device");
            let config = RenderConfig {
                virtual_resolution: UVec2::new(32, 18),
            };
            let renderer = Renderer::new(&mut device, &config).expect("a renderer");
            let mut window = Editing {
                gl: Some(gl),
                seen: Rc::clone(&seen),
                files,
                first: None,
                step: 0,
                since: None,
                took: Vec::new(),
                started: Instant::now(),
                quiet: None,
            };
            app.run_loop(&mut window, Some(Graphics { renderer, device }))
                .expect("the frames draw");
            fs::remove_dir_all(&root).expect("remove the test's folder");

            let first = window.first.expect("the page loaded");
            assert!(first.reloads.started, "the hot reload started");
            for (step, took) in STEPS.iter().zip(&window.took) {
                nadir_core::log::info!("{}: seen after {} ms", step.name, took.as_millis());
            }
            let missed = STEPS.get(window.took.len()).map(|step| step.name);
            assert_eq!(missed, None, "an edit was not seen within {PATIENCE:?}");
            let reloads = seen.borrow().reloads;
            assert_eq!(reloads.atlases, 1, "{reloads:?}");
            assert_eq!(reloads.prefabs, 1, "{reloads:?}");
            assert_eq!(
                (reloads.levels, reloads.levels_refused),
                (4, 1),
                "{reloads:?}"
            );
            assert_eq!(
                (reloads.shaders, reloads.shaders_refused),
                (1, 1),
                "{reloads:?}"
            );
        }
    }

    /// A steady frame of 10,000 sprites allocates nothing (roadmap M3.5),
    /// now drawn from entities, over a level's tile layer, which the
    /// engine's `nadir::extract_level` draws (M5.6), with the hot reload
    /// watching its files and nothing changing (M5.8).
    #[cfg(feature = "dev")]
    mod steady_frames {
        use std::env;
        use std::fs;
        use std::hint::black_box;
        use std::path::PathBuf;
        use std::process;

        use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
        use nadir_codec::{Limits, json, kbj};
        use nadir_core::{Color, Rect, Vec2};
        use nadir_platform::{
            AllocCounts, HeadlessGl, PlatformError, create_headless_gl, thread_alloc_counts,
        };

        use super::*;
        use crate::{Clip, ClipFrame, FrameAllocs, Playback};

        /// How many sprites walk about.
        const CROWD: usize = 10_000;
        /// The ground under them: a tile layer of 64 × 64 cells of 16
        /// pixels, so chunks of 32 × 32 cells, a quarter of them animated.
        /// Its four chunks meet at the view's centre.
        const GROUND: u32 = 64;
        /// Frames before the counting starts: the textures are made at the
        /// end of the first frame after the workers decode them (the first
        /// frame, unless they are slow), the buffers grow in the frames
        /// after it, and the camera moves from the ground's first chunk to
        /// the view, where four meet, packing three more.
        const WARM_UP: usize = 10;
        /// The frame of the warm-up from which the camera is where four
        /// chunks meet.
        const MOVED: usize = 6;
        /// Frames counted.
        const COUNTED: usize = 100;
        /// The view, which the walkers stay inside.
        const VIEW: Vec2 = Vec2::new(480.0, 270.0);

        /// The frames drawn so far, by which the camera moves.
        struct Ground(usize);

        /// What each frame saw: the last frame's allocations and drawing,
        /// and the ticks so far. Room for every frame is made up front, and
        /// the test keeps it when the loop drops the app.
        struct Seen {
            frames: Vec<(AllocCounts, FrameStats, u64)>,
            ticks: u64,
            /// Whether the hot reload ran.
            reloading: bool,
        }

        type SharedSeen = Rc<RefCell<Seen>>;

        /// The window: an offscreen context, if there is one. It notes the
        /// thread's heap use as each pump starts, and closes after the
        /// counted frames.
        struct Offscreen {
            gl: Option<HeadlessGl>,
            at_pumps: Vec<AllocCounts>,
        }

        impl EventSource for Offscreen {
            fn pump_events(&mut self, out: &mut Vec<Event>) {
                self.at_pumps.push(thread_alloc_counts());
                if self.at_pumps.len() > WARM_UP + COUNTED {
                    out.push(Event::CloseRequested);
                }
            }

            fn set_modal_frame_callback(&mut self, _callback: Box<dyn FnMut(ModalFrame)>) {}

            fn gpu_surface(&self) -> Option<GpuSurface<'_>> {
                self.gl.as_ref().map(|gl| GpuSurface::Gl(gl))
            }

            fn window_size(&self) -> UVec2 {
                UVec2::new(960, 540)
            }
        }

        /// An offscreen context, or `None` where `nadir-platform` cannot
        /// make one yet (Windows and macOS, until M2.6 and M2.7) and no
        /// renderer is asked for in `NADIR_EXPECT_GL_RENDERER`, as for the
        /// gfx and render tests. Any other failure fails the test.
        pub(super) fn headless(expected: &str) -> Option<HeadlessGl> {
            match create_headless_gl() {
                Ok(gl) => Some(gl),
                Err(PlatformError::Unsupported) if expected.is_empty() => None,
                Err(error) => panic!(
                    "no offscreen OpenGL context: {error}. With libglvnd, Mesa can be chosen \
                     with __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json"
                ),
            }
        }

        /// Writes a texture of one color, opaque.
        fn write_texture(path: &PathBuf, size: u32, color: [u8; 4]) {
            let header = Header {
                width: size,
                height: size,
                channels: Channels::Rgba,
                color_space: ColorSpace::Srgb,
            };
            let pixels = color.repeat((size * size) as usize);
            let bytes = qoi::encode(&header, &pixels).expect("a valid image");
            fs::write(path, bytes).expect("write a texture");
        }

        /// Turns each walker back at the edges of the view, and faces it
        /// the way it goes.
        fn walk(world: &World, tick: &Tick) {
            let transforms = world.read::<Transform>();
            let mut velocities = world.write::<Velocity>();
            let mut facings = world.write::<Facing>();
            for (entity, velocity) in velocities.iter_mut() {
                let Some(transform) = transforms.get(entity) else {
                    continue;
                };
                let next = transform.position + velocity.0 * tick.dt;
                if !(8.0..=VIEW.x - 8.0).contains(&next.x) {
                    velocity.0.x = -velocity.0.x;
                }
                if !(16.0..=VIEW.y).contains(&next.y) {
                    velocity.0.y = -velocity.0.y;
                }
                if let Some(facing) = facings.get_mut(entity) {
                    *facing = facing_of(velocity.0);
                }
            }
            world.resource::<SharedSeen>().borrow_mut().ticks += 1;
        }

        fn facing_of(velocity: Vec2) -> Facing {
            if velocity.x < 0.0 {
                Facing::Left
            } else {
                Facing::Right
            }
        }

        fn move_camera(world: &World, _: &Frame) {
            let mut ground = world.resource_mut::<Ground>();
            let mut out = world.resource_mut::<RenderFrame>();
            // Over the first chunk only, then on the walkers, where four
            // chunks meet.
            out.camera.position = if ground.0 < MOVED {
                VIEW * 0.5 - Vec2::splat(256.0)
            } else {
                VIEW * 0.5
            };
            ground.0 += 1;
        }

        /// Writes `json` as KBJ at `path`.
        pub(super) fn write_kbj(path: &std::path::Path, json: &str) {
            let value = json::parse(json, Limits::default()).expect("the test's JSON");
            let bytes = kbj::encode(&value, Limits::default()).expect("KBJ");
            fs::create_dir_all(path.parent().expect("a folder")).expect("make the folder");
            fs::write(path, bytes).expect("write the file");
        }

        /// Writes the ground as the level `ground`: a tile set of 2 × 2
        /// tiles of 16 pixels, the texture `ground.qoi`, and a layer of it,
        /// a quarter of its tiles animated, with its corner at `corner`.
        fn write_ground(assets: &std::path::Path, corner: Vec2) {
            write_kbj(
                &assets.join("ground.kbj"),
                r#"{ "version": 2, "pages": [ { "path": "ground.qoi", "w": 32, "h": 32 } ],
                     "sprites": [ { "name": "ground", "tiles": [16, 16],
                       "frames": [ { "page": 0, "x": 0, "y": 0, "w": 32, "h": 32,
                                     "ox": 0, "oy": 0, "sw": 32, "sh": 32 } ] } ] }"#,
            );
            let mut cells = Vec::new();
            for y in 0..GROUND {
                for x in 0..GROUND {
                    // Tile 1, animated, or tile 0.
                    cells.push(if (x + y) % 4 == 0 { "2" } else { "1" });
                }
            }
            let side = GROUND * 16;
            let level = format!(
                r#"{{ "version": 1, "size": [{side}, {side}], "background": [0, 0, 0],
                     "assets": ["ground.kbj"],
                     "tilesets": [ {{ "atlas": "ground.kbj", "sprite": "ground", "columns": 2, "rows": 2,
                                      "animations": [ {{ "tile": 1, "frames": 3, "ms": 100 }} ] }} ],
                     "layers": [ {{ "tileset": 0, "size": [{GROUND}, {GROUND}], "cell": 16,
                                    "origin": [{}, {}], "layer": 0, "depth": 0, "cells": [{}] }} ],
                     "tall": [], "entities": [] }}"#,
                corner.x as i32,
                corner.y as i32,
                cells.join(", ")
            );
            write_kbj(&assets.join("levels/ground.klvl"), &level);
        }

        fn probe(world: &World, _: &Frame) {
            let seen = world.resource::<SharedSeen>();
            let mut seen = seen.borrow_mut();
            let entry = (
                world.resource::<FrameAllocs>().0,
                *world.resource::<FrameStats>(),
                seen.ticks,
            );
            seen.frames.push(entry);
            seen.reloading = world.resource::<crate::hot_reload::Reloads>().started;
            // In a warm-up frame, a known allocation, which that frame's
            // count must show.
            if seen.frames.len() == 5 {
                drop(black_box(Box::new(5u64)));
            }
        }

        /// The scene's app, loading its textures from `assets`, and noting
        /// in `seen` what each frame sees.
        fn scene(assets: PathBuf, seen: &SharedSeen) -> App {
            let mut app = App::new(Config {
                assets,
                // Frames then sleep in the limiter, which the count covers.
                frame_rate_limit: Some(500),
                ..Config::default()
            })
            .insert_resource(Rc::clone(seen))
            .add_fixed_system(FixedStage::Gameplay, "walk", walk)
            .add_frame_system(FrameStage::Camera, "move_camera", move_camera)
            .add_frame_system(FrameStage::Ui, "probe", probe);
            let walker = app.world().resource_mut::<AssetServer>().load("walker.qoi");
            // Cell (32, 32), the corner of four chunks, at the view's centre.
            write_ground(&app.config.assets, VIEW * 0.5 - Vec2::splat(512.0));
            app.load_level("ground").expect("the ground's level");
            // A walk of 4 frames, the quarters of the walker's texture. Clips
            // that play once, and the events they send, are in
            // `glue::tests::steady_animation`, whose frames are not timed by
            // the clock on the wall.
            let quarter = |index: u32| {
                let left = index as f32 * 0.25;
                let uv = Rect::new(Vec2::new(left, 0.0), Vec2::new(left + 0.25, 1.0));
                ClipFrame::new(walker, uv, 100 * MS)
            };
            let walk = app
                .world()
                .resource_mut::<Clips>()
                .add(Clip::new((0..4).map(quarter).collect(), Playback::Loop));
            let mut seed = 1u32;
            let mut random = move || {
                // A xorshift: the same walkers every run.
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed % 10_000) as f32 / 10_000.0
            };
            for index in 0..CROWD {
                let position = Vec2::new(
                    8.0 + random() * (VIEW.x - 16.0),
                    16.0 + random() * (VIEW.y - 16.0),
                );
                let velocity = Vec2::new(random() * 120.0 - 60.0, random() * 120.0 - 60.0);
                let mut sprite = Sprite::new(walker, Vec2::new(8.0, 16.0));
                sprite.layer = 1;
                if index % 4 == 0 {
                    sprite.color = Color::rgb(255, 200, 200);
                }
                let phase = 400 * MS * (index % 4) as u32 / 4;
                app.world_mut()
                    .spawn()
                    .with(Transform::at(position))
                    .with(Velocity(velocity))
                    .with(sprite)
                    .with(Animator::starting_at(walk, phase))
                    .with(facing_of(velocity));
            }
            app.insert_resource(Ground(0))
        }

        #[test]
        fn a_steady_frame_of_10k_animated_sprites_and_a_tile_layer_allocates_nothing() {
            // The counter is the global allocator, so a known allocation
            // shows: a count of zero below means something.
            let start = thread_alloc_counts();
            drop(black_box(Box::new(0u64)));
            assert_eq!(thread_alloc_counts().since(start).allocations, 1);

            let assets = env::temp_dir().join(format!("nadir-engine-steady-{}", process::id()));
            fs::create_dir_all(&assets).expect("make the asset folder");
            write_texture(&assets.join("walker.qoi"), 8, [200, 120, 60, 255]);
            write_texture(&assets.join("ground.qoi"), 32, [90, 140, 70, 255]);
            let seen = Rc::new(RefCell::new(Seen {
                frames: Vec::with_capacity(WARM_UP + COUNTED),
                ticks: 0,
                reloading: false,
            }));
            let app = scene(assets.clone(), &seen);

            let expected = env::var("NADIR_EXPECT_GL_RENDERER").unwrap_or_default();
            let mut window = Offscreen {
                gl: headless(&expected),
                at_pumps: Vec::with_capacity(WARM_UP + COUNTED + 2),
            };
            let graphics = window.gl.as_ref().map(|gl| {
                let mut device = Device::new(GpuSurface::Gl(gl)).expect("a device");
                let renderer = device.caps().renderer.clone();
                assert!(renderer.contains(&expected), "{renderer} is not {expected}");
                let config = RenderConfig {
                    virtual_resolution: VIEW.as_uvec2(),
                };
                let renderer = Renderer::new(&mut device, &config).expect("a renderer");
                Graphics { renderer, device }
            });
            let drawn = graphics.is_some();
            app.run_loop(&mut window, graphics)
                .expect("the frames draw");
            fs::remove_dir_all(&assets).expect("remove the asset folder");
            assert_eq!(
                seen.borrow().reloading,
                drawn,
                "the hot reload runs where it draws"
            );
            let seen = &seen.borrow().frames;

            // Frame f runs after pump f; `seen[f - 1]` is what frame f saw,
            // so `seen[f].0` is frame f's own count.
            let pumps = &window.at_pumps;
            assert_eq!(pumps.len(), WARM_UP + COUNTED + 1);
            assert_eq!(seen.len(), WARM_UP + COUNTED);

            // The counted frames: the loop between two pumps, from frame
            // WARM_UP + 1 to the last.
            let counted = pumps[WARM_UP + COUNTED].since(pumps[WARM_UP]);
            assert_eq!(
                counted,
                AllocCounts::default(),
                "the steady frames allocated"
            );
            let ticks = seen[WARM_UP + COUNTED - 1].2 - seen[WARM_UP].2;
            assert!(ticks > 0, "no tick ran in the counted frames");

            // `FrameAllocs` counts the same as the pumps, frame by frame,
            // warm-up included: the probe's allocation in frame 5, and with
            // a GPU, the first frame that draws the textures.
            for frame in 2..WARM_UP + COUNTED {
                let between_pumps = pumps[frame].since(pumps[frame - 1]);
                assert_eq!(seen[frame].0, between_pumps, "frame {frame}");
            }
            assert!(seen[5].0.allocations >= 1, "the probe's allocation");

            if drawn {
                // A frame makes the textures after it draws, so the first
                // to draw them all is the second at the soonest, in the
                // frames checked against the pumps above.
                let full = (1..WARM_UP)
                    .find(|&frame| seen[frame].1.sprites == CROWD as u32)
                    .expect("the sprites are drawn in the warm-up");
                assert!(full >= 2, "frame {full} drew the sprites");
                assert!(
                    seen[full].0.allocations > 0,
                    "the first full draw grows its lists"
                );
                let all = FrameStats {
                    sprites: CROWD as u32,
                    skipped: 0,
                    batches: 1,
                    tiles: 4 * 32 * 32,
                    chunks: 4,
                };
                assert_eq!(seen[WARM_UP].1, all, "drawn before the counting starts");
                assert_eq!(seen[WARM_UP + COUNTED - 1].1, all);
            }
        }
    }
}
