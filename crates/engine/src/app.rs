//! [`App`]: the game's configuration, its world and systems, and the main
//! loop.

use std::any::type_name;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use nadir_assets::{AssetServer, FinalizeCtx, OnFailure};
use nadir_core::UVec2;
use nadir_core::log::{debug, error};
use nadir_core::profile::{self, ScopeRecord};
use nadir_core::time::{FixedClock, Stopwatch};
use nadir_ecs::{Component, FixedStage, FixedSystem, FrameStage, FrameSystem, Schedule, World};
use nadir_gfx::{Device, PassTarget};
use nadir_input::{ActionKind, ActionMap, ActionSet, ActionState, Input};
use nadir_platform::{
    Event, GpuSurface, ModalFrame, Platform, WindowConfig, precise_sleep, show_error_dialog,
};
use nadir_render::{FrameStats, Output, RenderConfig, RenderFrame, Renderer, TileLayers};
use nadir_scene::{FromValue, TypeRegistry};

use crate::limiter::FrameLimiter;
use crate::{
    Collider, Collisions, Error, Parent, Player, PrevTransform, SaveId, Sprite, TickStats,
    Transform, TriggerEnter, TriggerExit, Velocity, glue, log_file,
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
    /// The folder the game's assets are loaded from.
    pub assets: PathBuf,
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
        }
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
type LatchFn = Box<dyn FnMut(&mut Input, &World)>;

/// The game's action set: latches the input into its state once a tick.
struct Actions {
    /// The set's type, for messages, or `None` before the game adds one.
    set: Option<&'static str>,
    latch: LatchFn,
}

impl Actions {
    /// Before the game adds its actions: a latch into a set of its own, so
    /// that each tick still ends, and [`Input::wheel`] moves on.
    fn none() -> Self {
        let map = ActionMap::<NoActions>::new().expect("a set of one action is valid");
        let mut state = ActionState::new();
        Self {
            set: None,
            latch: Box::new(move |input, _| input.latch(&map, &mut state)),
        }
    }
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
    /// assets loaded since the last frame.
    fn draw(&mut self, world: &World, surface: GpuSurface<'_>, size: UVec2) -> Result<(), Error> {
        self.device.begin_frame(surface)?;
        let stats = {
            let assets = world.resource::<AssetServer>();
            let tiles = world.resource::<TileLayers>();
            let frame = world.resource::<RenderFrame>();
            let output = Output {
                target: PassTarget::Surface,
                size,
            };
            self.renderer
                .render(&mut self.device, &assets, &tiles, &frame, output)?
        };
        *world.resource_mut::<FrameStats>() = stats;
        self.device.end_frame(surface)?;
        let pending = world.resource::<AssetServer>().has_pending();
        if pending {
            world
                .resource_mut::<AssetServer>()
                .finish_completed_loads(&mut FinalizeCtx::new().with(&mut self.device));
        }
        Ok(())
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
}

impl EngineCore {
    /// Runs one frame, for the time since the last, with the events that
    /// came before it. Draws it on `window`'s surface, if there is one.
    fn frame(&mut self, events: &[Event], window: Option<(GpuSurface<'_>, UVec2)>) {
        let elapsed = self.stopwatch.lap();
        self.app.update(elapsed, events);
        if let (Some(graphics), Some((surface, size))) = (&mut self.graphics, window)
            && let Err(error) = graphics.draw(&self.app.world, surface, size)
        {
            self.error.get_or_insert(error);
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
/// - `nadir::extract_sprites` (frame `Extract`): draws every [`Sprite`]
///   into the [`RenderFrame`], between the last two ticks.
///
/// So a game sets velocities in `Gameplay`, and reacts to the moves after
/// the engine's, in `Movement` or `PostMovement`.
///
/// The engine's resources:
///
/// - [`Input`], which latches the keys and buttons into the actions, and
///   holds the pointer and the wheel
/// - with [`App::add_actions`], the game's [`ActionMap`] and its
///   [`ActionState`], latched once a tick before the tick stages
/// - [`AssetServer`], loading from [`Config::assets`]
/// - [`RenderFrame`], emptied before the frame stages, which fill it; the
///   loop then draws it. Its `time`, which animated tiles show, is how long
///   the simulation has run, ticks and `alpha`.
/// - [`TileLayers`], the game's tile layers, which the frame's
///   `tile_layers` name and the loop draws from
/// - [`FrameStats`], what the last frame drew
/// - [`TickStats`], how long this frame's ticks took
/// - [`Collisions`], the colliders
/// - [`TypeRegistry`], the reflected component types
/// - the events [`TriggerEnter`] and [`TriggerExit`]
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
}

impl App {
    /// An app with the engine's resources, components and systems, and no
    /// entities.
    ///
    /// Its [`AssetServer`] loads from [`Config::assets`]. With the `dev`
    /// feature, a failed asset shows its placeholder; without it, nothing.
    ///
    /// Of the engine's components, [`Transform`], [`Velocity`], [`Player`]
    /// and [`SaveId`] are reflected. [`Sprite`], [`Collider`] and
    /// [`Parent`] are not yet (roadmap M5.6 and M9.4), and
    /// [`PrevTransform`] is the engine's own.
    pub fn new(config: Config) -> Self {
        let on_failure = if cfg!(feature = "dev") {
            OnFailure::Placeholder
        } else {
            OnFailure::Nothing
        };
        let mut world = World::new();
        world.insert_resource(Input::new());
        world.insert_resource(AssetServer::new(&config.assets, on_failure));
        world.insert_resource(RenderFrame::new());
        world.insert_resource(TileLayers::new());
        world.insert_resource(FrameStats::default());
        world.insert_resource(TickStats::default());
        world.insert_resource(Collisions::new());
        #[cfg(feature = "dev")]
        world.insert_resource(crate::FrameAllocs::default());
        world.add_event::<TriggerEnter>();
        world.add_event::<TriggerExit>();

        let mut registry = TypeRegistry::new();
        registry.register_component::<Transform>();
        registry.register_component::<Velocity>();
        registry.register_component::<Player>();
        registry.register_component::<SaveId>();
        registry.register_components(&mut world);
        world.insert_resource(registry);
        world.register::<PrevTransform>();
        world.register::<Sprite>();
        world.register::<Collider>();
        world.register::<Parent>();

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
            .add_frame(
                FrameStage::Extract,
                "nadir::extract_sprites",
                glue::extract_sprites,
            );
        Self {
            config,
            world,
            schedule,
            actions: Actions::none(),
            clock: FixedClock::new(TICK_HZ, MAX_FRAME),
            profile: Vec::new(),
        }
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
    /// same type again replaces its bindings.
    #[must_use]
    pub fn add_actions<A: ActionSet>(mut self, map: ActionMap<A>) -> Self {
        let set = type_name::<A>();
        if let Some(existing) = self.actions.set {
            assert!(
                existing == set,
                "an App has one action set: it has `{existing}`, so it cannot add `{set}`",
            );
        }
        self.world.insert_resource(map);
        self.world.insert_resource(ActionState::<A>::new());
        self.actions = Actions {
            set: Some(set),
            latch: Box::new(|input, world| {
                let map = world.resource::<ActionMap<A>>();
                let mut state = world.resource_mut::<ActionState<A>>();
                input.latch(&map, &mut state);
            }),
        };
        self
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
    /// 2. runs the ticks that are due: for each, latches the actions and
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
    /// million frames.
    pub fn update(&mut self, elapsed: Duration, events: &[Event]) {
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
            {
                let mut input = self.world.resource_mut::<Input>();
                (self.actions.latch)(&mut input, &self.world);
            }
            self.schedule.run_fixed(&mut self.world, &tick);
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
    /// # Errors
    ///
    /// Fails if the window cannot be opened, the renderer cannot start, or
    /// a frame cannot be drawn. Before it returns the error, it logs it and
    /// shows it to the player with [`show_error_dialog`].
    pub fn run(self) -> Result<(), Error> {
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
        self.run_loop(&mut platform, graphics)
            .map_err(|error| fatal(&title, error))
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
        let mut limiter = FrameLimiter::new(self.config.frame_rate_limit, Instant::now());
        let core = Rc::new(RefCell::new(EngineCore {
            app: self,
            stopwatch: Stopwatch::start(),
            graphics,
            error: None,
            #[cfg(feature = "dev")]
            allocs_at_frame_end: nadir_platform::thread_alloc_counts(),
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
    use nadir_input::Binding;

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
            .add_frame_system(FrameStage::Camera, "follow", |_, _| {});
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
                "frame Camera: follow",
                "frame Extract: nadir::extract_sprites",
                "frame Extract: draw_ground",
            ]
        );
    }

    #[test]
    fn the_engines_components_are_registered_and_four_are_reflected() {
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
        let registry = world.resource::<TypeRegistry>();
        let names: Vec<&str> = registry.iter().map(|it| it.name()).collect();
        assert_eq!(names, ["Transform", "Velocity", "Player", "SaveId"]);
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

    /// A steady frame of 10,000 sprites allocates nothing (roadmap M3.5),
    /// now drawn from entities.
    #[cfg(feature = "dev")]
    mod steady_frames {
        use std::env;
        use std::fs;
        use std::hint::black_box;
        use std::path::PathBuf;
        use std::process;

        use nadir_assets::Handle;
        use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
        use nadir_core::{Color, Vec2};
        use nadir_platform::{
            AllocCounts, HeadlessGl, PlatformError, create_headless_gl, thread_alloc_counts,
        };
        use nadir_render::{Tile, TileLayer, TileLayerDraw, TileSet};

        use super::*;
        use crate::FrameAllocs;

        /// How many sprites walk about.
        const CROWD: usize = 10_000;
        /// The ground under them: a tile layer of 64 × 64 cells of 16
        /// pixels, so chunks of 32 × 32 cells, a quarter of them animated.
        /// Its four chunks meet at the view's centre.
        const GROUND: u32 = 64;
        /// Frames before the counting starts: the textures load after the
        /// first draw, the buffers grow in the frames after it, and the
        /// camera moves from the ground's first chunk to the view, where
        /// four meet, packing three more.
        const WARM_UP: usize = 10;
        /// The frame of the warm-up from which the camera is where four
        /// chunks meet.
        const MOVED: usize = 6;
        /// Frames counted.
        const COUNTED: usize = 100;
        /// The view, which the walkers stay inside.
        const VIEW: Vec2 = Vec2::new(480.0, 270.0);

        /// The ground's layer, and the frames drawn so far.
        struct Ground(Handle<TileLayer>, usize);

        /// What each frame saw: the last frame's allocations and drawing,
        /// and the ticks so far. Room for every frame is made up front, and
        /// the test keeps it when the loop drops the app.
        struct Seen {
            frames: Vec<(AllocCounts, FrameStats, u64)>,
            ticks: u64,
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
        fn headless(expected: &str) -> Option<HeadlessGl> {
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
            let mut sprites = world.write::<Sprite>();
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
                if let Some(sprite) = sprites.get_mut(entity) {
                    sprite.flip.x = velocity.0.x < 0.0;
                }
            }
            world.resource::<SharedSeen>().borrow_mut().ticks += 1;
        }

        fn draw_ground(world: &World, _: &Frame) {
            let mut ground = world.resource_mut::<Ground>();
            let mut out = world.resource_mut::<RenderFrame>();
            // Over the first chunk only, then on the walkers, where four
            // chunks meet.
            out.camera.position = if ground.1 < MOVED {
                VIEW * 0.5 - Vec2::splat(256.0)
            } else {
                VIEW * 0.5
            };
            ground.1 += 1;
            out.tile_layers.push(TileLayerDraw::new(ground.0));
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
            .add_frame_system(FrameStage::Extract, "draw_ground", draw_ground)
            .add_frame_system(FrameStage::Ui, "probe", probe);
            let (walker, ground) = {
                let mut server = app.world().resource_mut::<AssetServer>();
                (server.load("walker.qoi"), server.load("ground.qoi"))
            };
            let set = TileSet {
                texture: ground,
                columns: 2,
                rows: 2,
            };
            // Cell (32, 32), the corner of four chunks, at the view's centre.
            let corner = VIEW * 0.5 - Vec2::splat(512.0);
            let mut layer = TileLayer::new(set, UVec2::splat(GROUND), 16.0, corner);
            for y in 0..GROUND as i32 {
                for x in 0..GROUND as i32 {
                    let tile = if (x + y) % 4 == 0 {
                        Tile::animated(1, 3, 100)
                    } else {
                        Tile::new(0)
                    };
                    layer.set(nadir_core::IVec2::new(x, y), Some(tile));
                }
            }
            let ground = app.world().resource_mut::<TileLayers>().add(layer);
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
                app.world_mut()
                    .spawn()
                    .with(Transform::at(position))
                    .with(Velocity(velocity))
                    .with(sprite);
            }
            app.insert_resource(Ground(ground, 0))
        }

        #[test]
        fn a_steady_frame_of_10k_sprites_and_a_tile_layer_allocates_nothing() {
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
                assert!(
                    seen[2].0.allocations > 0,
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
