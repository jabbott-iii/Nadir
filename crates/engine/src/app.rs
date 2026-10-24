//! [`App`]: the game's configuration, its systems and resources, and the
//! main loop.

use std::any::type_name;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use nadir_assets::{AssetServer, FinalizeCtx, OnFailure};
use nadir_core::UVec2;
use nadir_core::log::{debug, error};
use nadir_core::time::{FixedClock, Frame, Stopwatch, Tick};
use nadir_gfx::{Device, PassTarget};
use nadir_input::{ActionKind, ActionMap, ActionSet, ActionState, Input};
use nadir_platform::{
    Event, GpuSurface, ModalFrame, Platform, WindowConfig, precise_sleep, show_error_dialog,
};
use nadir_render::{FrameStats, Output, RenderConfig, RenderFrame, Renderer};

use crate::limiter::FrameLimiter;
use crate::{Error, Resources, log_file};

/// How many times a second the simulation steps.
pub const TICK_HZ: u32 = 60;

/// The longest that one frame counts for. After a stall, the simulation
/// falls behind real time by the rest, which is better than a long burst of
/// ticks to catch up. See [`FixedClock`].
const MAX_FRAME: Duration = Duration::from_millis(250);

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

/// What a system is: a function of the resources and a tick or frame.
type SystemFn<T> = Box<dyn FnMut(&Resources, &T)>;

/// A function that the loop calls, with the name it was added under.
struct System<T> {
    name: &'static str,
    run: SystemFn<T>,
}

/// Latches the input into the game's action state.
type LatchFn = Box<dyn FnMut(&mut Input, &Resources)>;

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
    fn draw(
        &mut self,
        resources: &Resources,
        surface: GpuSurface<'_>,
        size: UVec2,
    ) -> Result<(), Error> {
        resources.set_system("nadir::draw");
        self.device.begin_frame(surface)?;
        let stats = {
            let assets = resources.resource::<AssetServer>();
            let frame = resources.resource::<RenderFrame>();
            let output = Output {
                target: PassTarget::Surface,
                size,
            };
            self.renderer
                .render(&mut self.device, &assets, &frame, output)?
        };
        *resources.resource_mut::<FrameStats>() = stats;
        self.device.end_frame(surface)?;
        let pending = resources.resource::<AssetServer>().has_pending();
        if pending {
            resources.set_system("nadir::finish_loads");
            resources
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
    clock: FixedClock,
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
        self.app.frame(&mut self.clock, elapsed, events);
        if let (Some(graphics), Some((surface, size))) = (&mut self.graphics, window)
            && let Err(error) = graphics.draw(&self.app.resources, surface, size)
        {
            self.error.get_or_insert(error);
        }
        #[cfg(feature = "dev")]
        {
            let now = nadir_platform::thread_alloc_counts();
            let allocs = crate::FrameAllocs(now.since(self.allocs_at_frame_end));
            *self.app.resources.resource_mut::<crate::FrameAllocs>() = allocs;
            self.allocs_at_frame_end = now;
        }
        self.app.resources.set_system("(none)");
    }
}

/// A game: its configuration, resources and systems. [`App::run`] opens the
/// log file and the window, and runs the main loop.
///
/// Systems share state through [`Resources`]. The engine provides:
///
/// - [`Input`], which latches the keys and buttons into the actions, and
///   holds the pointer and the wheel
/// - with [`App::add_actions`], the game's [`ActionMap`] and its
///   [`ActionState`], latched once a tick before the fixed systems
/// - [`AssetServer`], loading from [`Config::assets`]
/// - [`RenderFrame`], emptied before the frame systems, which fill it; the
///   loop then draws it
/// - [`FrameStats`], what the last frame drew
/// - with the `dev` feature, `FrameAllocs`, what the last frame allocated
///
/// ```no_run
/// use nadir::prelude::*;
///
/// struct Spin(f32);
///
/// fn main() -> Result<(), nadir::Error> {
///     App::new(Config {
///         title: "My game".into(),
///         ..Config::default()
///     })
///     .insert_resource(Spin(0.0))
///     .add_fixed_system("simulate", |resources, tick| {
///         // 60 times a second: move things by `tick.dt`.
///         resources.resource_mut::<Spin>().0 += tick.dt;
///     })
///     .add_frame_system("draw", |resources, frame| {
///         // Once for each frame: blend by `frame.alpha`, and fill the
///         // RenderFrame.
///         let _ = (resources.resource::<Spin>().0, frame.alpha);
///     })
///     .run()
/// }
/// ```
///
/// Systems take [`Resources`] until the entity world replaces it (roadmap
/// M4.4, ADR-0019).
pub struct App {
    config: Config,
    resources: Resources,
    fixed_systems: Vec<System<Tick>>,
    frame_systems: Vec<System<Frame>>,
    actions: Actions,
}

impl App {
    /// An app with no systems, and the engine's resources.
    ///
    /// Its [`AssetServer`] loads from [`Config::assets`]. With the `dev`
    /// feature, a failed asset shows its placeholder; without it, nothing.
    pub fn new(config: Config) -> Self {
        let on_failure = if cfg!(feature = "dev") {
            OnFailure::Placeholder
        } else {
            OnFailure::Nothing
        };
        let mut resources = Resources::new();
        resources.insert_resource(Input::new());
        resources.insert_resource(AssetServer::new(&config.assets, on_failure));
        resources.insert_resource(RenderFrame::new());
        resources.insert_resource(FrameStats::default());
        #[cfg(feature = "dev")]
        resources.insert_resource(crate::FrameAllocs::default());
        Self {
            config,
            resources,
            fixed_systems: Vec::new(),
            frame_systems: Vec::new(),
            actions: Actions::none(),
        }
    }

    /// Adds `resource`, or replaces the one of its type.
    #[must_use]
    pub fn insert_resource<R: 'static>(mut self, resource: R) -> Self {
        self.resources.insert_resource(resource);
        self
    }

    /// Adds the game's actions, with their bindings: the resources
    /// [`ActionMap<A>`] and [`ActionState<A>`], whose state is latched from
    /// the input once a tick, before the fixed systems.
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
        self.resources.insert_resource(map);
        self.resources.insert_resource(ActionState::<A>::new());
        self.actions = Actions {
            set: Some(set),
            latch: Box::new(|input, resources| {
                let map = resources.resource::<ActionMap<A>>();
                let mut state = resources.resource_mut::<ActionState<A>>();
                input.latch(&map, &mut state);
            }),
        };
        self
    }

    /// Adds a system that runs once for every simulation tick: [`TICK_HZ`]
    /// times a second, however fast frames come. Systems run in the order
    /// they were added.
    #[must_use]
    pub fn add_fixed_system(
        mut self,
        name: &'static str,
        system: impl FnMut(&Resources, &Tick) + 'static,
    ) -> Self {
        self.fixed_systems.push(System {
            name,
            run: Box::new(system),
        });
        self
    }

    /// Adds a system that runs once for every frame, after the ticks of
    /// that frame. Systems run in the order they were added.
    #[must_use]
    pub fn add_frame_system(
        mut self,
        name: &'static str,
        system: impl FnMut(&Resources, &Frame) + 'static,
    ) -> Self {
        self.frame_systems.push(System {
            name,
            run: Box::new(system),
        });
        self
    }

    /// The resources, for setup: loading assets, say.
    pub fn resources(&self) -> &Resources {
        &self.resources
    }

    /// The resources, to change them before the game runs.
    pub fn resources_mut(&mut self) -> &mut Resources {
        &mut self.resources
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

    /// The main loop: events, then the ticks that are due, then the frame.
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
        debug!(
            "main loop: fixed systems {:?}, frame systems {:?}",
            names(&self.fixed_systems),
            names(&self.frame_systems)
        );
        let mut limiter = FrameLimiter::new(self.config.frame_rate_limit, Instant::now());
        let core = Rc::new(RefCell::new(EngineCore {
            app: self,
            clock: FixedClock::new(TICK_HZ, MAX_FRAME),
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

    /// Runs the systems for one frame that comes `elapsed` after the last,
    /// with the events that came before it.
    fn frame(&mut self, clock: &mut FixedClock, elapsed: Duration, events: &[Event]) {
        self.resources.set_system("nadir::input");
        {
            let mut input = self.resources.resource_mut::<Input>();
            for event in events {
                input.handle(event);
            }
        }
        let frame = clock.begin_frame(elapsed);
        while let Some(tick) = clock.consume_tick() {
            self.resources.set_system("nadir::latch");
            {
                let mut input = self.resources.resource_mut::<Input>();
                (self.actions.latch)(&mut input, &self.resources);
            }
            for system in &mut self.fixed_systems {
                self.resources.set_system(system.name);
                (system.run)(&self.resources, &tick);
            }
        }
        self.resources.set_system("nadir::frame");
        self.resources.resource_mut::<RenderFrame>().clear();
        for system in &mut self.frame_systems {
            self.resources.set_system(system.name);
            (system.run)(&self.resources, &frame);
        }
        self.resources.set_system("(none)");
    }
}

/// Logs an error that ends the game and shows it to the player. Returns it,
/// for [`App::run`] to return.
fn fatal(title: &str, error: Error) -> Error {
    error!("{error}");
    show_error_dialog(title, &error.to_string());
    error
}

/// The names of some systems, in the order they run.
fn names<T>(systems: &[System<T>]) -> Vec<&'static str> {
    systems.iter().map(|system| system.name).collect()
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::thread;

    use super::*;

    /// What the systems of a test saw, in order.
    type Seen = Rc<RefCell<Vec<String>>>;

    fn app() -> App {
        App::new(Config::default())
    }

    /// An app whose systems count ticks and frames.
    fn counting_app(ticks: &Rc<Cell<u64>>, frames: &Rc<Cell<u64>>) -> App {
        let (ticks, frames) = (Rc::clone(ticks), Rc::clone(frames));
        app()
            .add_fixed_system("count_ticks", move |_, _| ticks.set(ticks.get() + 1))
            .add_frame_system("count_frames", move |_, _| frames.set(frames.get() + 1))
    }

    /// Runs frames of one length until `total` has passed.
    fn simulate(app: &mut App, clock: &mut FixedClock, frame_length: Duration, total: Duration) {
        let mut passed = Duration::ZERO;
        while passed < total {
            let step = frame_length.min(total - passed);
            app.frame(clock, step, &[]);
            passed += step;
        }
    }

    fn new_clock() -> FixedClock {
        FixedClock::new(TICK_HZ, MAX_FRAME)
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

    // -- one frame ----------------------------------------------------------

    #[test]
    fn a_frame_runs_its_ticks_and_then_its_frame_systems_in_the_order_added() {
        let seen: Seen = Rc::default();
        let log = |seen: &Seen, name: &'static str| {
            let seen = Rc::clone(seen);
            move |text: String| seen.borrow_mut().push(format!("{name} {text}"))
        };
        let (a, b, c, d) = (
            log(&seen, "a"),
            log(&seen, "b"),
            log(&seen, "c"),
            log(&seen, "d"),
        );
        let mut app = app()
            .add_fixed_system("a", move |_: &Resources, tick: &Tick| {
                a(format!("tick {}", tick.index))
            })
            .add_frame_system("c", move |_: &Resources, _: &Frame| c("frame".to_string()))
            .add_fixed_system("b", move |_: &Resources, tick: &Tick| {
                b(format!("tick {}", tick.index))
            })
            .add_frame_system("d", move |_: &Resources, _: &Frame| d("frame".to_string()));
        assert_eq!(names(&app.fixed_systems), ["a", "b"]);
        assert_eq!(names(&app.frame_systems), ["c", "d"]);

        // 40 ms hold two ticks.
        app.frame(&mut new_clock(), Duration::from_millis(40), &[]);
        assert_eq!(
            *seen.borrow(),
            [
                "a tick 0", "b tick 0", "a tick 1", "b tick 1", "c frame", "d frame"
            ]
        );
    }

    #[test]
    fn a_frame_shorter_than_a_tick_still_runs_the_frame_systems() {
        let (ticks, frames) = (Rc::default(), Rc::default());
        let mut app = counting_app(&ticks, &frames);
        let mut clock = new_clock();
        app.frame(&mut clock, Duration::from_millis(5), &[]);
        app.frame(&mut clock, Duration::from_millis(5), &[]);
        assert_eq!((ticks.get(), frames.get()), (0, 2));
        app.frame(&mut clock, Duration::from_millis(10), &[]);
        assert_eq!((ticks.get(), frames.get()), (1, 3));
    }

    #[test]
    fn systems_get_the_tick_length_and_the_frame_time() {
        let dts: Rc<RefCell<Vec<f32>>> = Rc::default();
        let (tick_dts, frame_dts) = (Rc::clone(&dts), Rc::clone(&dts));
        let alphas: Rc<RefCell<Vec<f32>>> = Rc::default();
        let frame_alphas = Rc::clone(&alphas);
        let mut app = app()
            .add_fixed_system("tick", move |_, tick| tick_dts.borrow_mut().push(tick.dt))
            .add_frame_system("frame", move |_, frame| {
                frame_dts.borrow_mut().push(frame.dt);
                frame_alphas.borrow_mut().push(frame.alpha);
            });
        app.frame(&mut new_clock(), Duration::from_millis(20), &[]);
        assert_eq!(*dts.borrow(), [1.0 / 60.0, 0.02]);
        // 20 ms is one tick of 16.67 ms and a fifth of the next.
        assert!((alphas.borrow()[0] - 0.2).abs() < 1e-6, "{alphas:?}");
    }

    // -- the tick rate ------------------------------------------------------

    #[test]
    fn ten_seconds_run_600_ticks_whatever_the_frame_rate() {
        let frame_lengths = [
            Duration::from_nanos(16_666_667), // 60 frames a second
            Duration::from_nanos(6_944_444),  // 144
            Duration::from_micros(33_333),    // 30
            Duration::from_millis(3),
            Duration::from_millis(110),
        ];
        for frame_length in frame_lengths {
            let (ticks, frames) = (Rc::default(), Rc::default());
            let mut app = counting_app(&ticks, &frames);
            simulate(
                &mut app,
                &mut new_clock(),
                frame_length,
                Duration::from_secs(10),
            );
            // The task asks for 60 Hz within 1%. The clock is exact.
            assert_eq!(ticks.get(), 600, "frames of {frame_length:?}");
            assert!(frames.get() >= 90, "frames of {frame_length:?}");
        }
    }

    #[test]
    fn a_simulated_hitch_runs_a_quarter_second_of_ticks_and_no_more() {
        let (ticks, frames) = (Rc::default(), Rc::default());
        let mut app = counting_app(&ticks, &frames);
        let mut clock = new_clock();
        simulate(
            &mut app,
            &mut clock,
            Duration::from_millis(10),
            Duration::from_secs(1),
        );
        assert_eq!(ticks.get(), 60);

        // One frame takes a whole second.
        app.frame(&mut clock, Duration::from_secs(1), &[]);
        assert_eq!(ticks.get(), 60 + 15);

        // Nothing is owed afterwards: the next second is an ordinary one.
        simulate(
            &mut app,
            &mut clock,
            Duration::from_millis(10),
            Duration::from_secs(1),
        );
        assert_eq!(ticks.get(), 60 + 15 + 60);
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
        let (ticks, frames) = (Rc::default(), Rc::default());
        let mut app = counting_app(&ticks, &frames);
        app.config.frame_rate_limit = None;
        let mut window = Script::closing_when(|pumps| pumps == 8);
        app.run_loop(&mut window, None)
            .expect("no error without a device");
        // Seven frames ran, and the eighth pump ended the loop before its
        // frame.
        assert_eq!(window.pumps, 8);
        assert_eq!(frames.get(), 7);
    }

    #[test]
    fn a_close_request_on_the_first_pump_runs_no_frame() {
        let (ticks, frames) = (Rc::default(), Rc::default());
        let app = counting_app(&ticks, &frames);
        app.run_loop(&mut Script::closing_when(|_| true), None)
            .expect("no error without a device");
        assert_eq!((ticks.get(), frames.get()), (0, 0));
    }

    #[test]
    fn while_the_system_holds_the_loop_frames_run_from_the_callback() {
        let (ticks, frames) = (Rc::default(), Rc::default());
        let mut app = counting_app(&ticks, &frames);
        app.config.frame_rate_limit = None;
        // The second pump holds the loop for five modal frames, as a drag
        // of the window does on Windows.
        let mut window = Script::closing_when(|pumps| pumps == 3)
            .with_modal_frames(|pumps| if pumps == 2 { 5 } else { 0 });
        app.run_loop(&mut window, None)
            .expect("no error without a device");
        // One frame after the first pump, five during the second and one
        // after it, and none after the third.
        assert_eq!(frames.get(), 7);
    }

    #[test]
    fn modal_frames_keep_the_simulation_on_time() {
        let (ticks, frames) = (Rc::default(), Rc::default());
        let mut app = counting_app(&ticks, &frames);
        app.config.frame_rate_limit = None;
        // The first pump holds the loop for 300 ms or more, in modal frames
        // of at least 10 ms.
        let start = Instant::now();
        let mut window = Script::closing_when(|pumps| pumps == 2)
            .with_modal_frames(|pumps| if pumps == 1 { 30 } else { 0 });
        let wait = move |_: &Resources, _: &Frame| thread::sleep(Duration::from_millis(10));
        app = app.add_frame_system("wait", wait);
        app.run_loop(&mut window, None)
            .expect("no error without a device");
        let elapsed = start.elapsed();
        assert_eq!(frames.get(), 30 + 1);
        // The ticks kept up with real time while the loop was held: 60 a
        // second for 300 ms is 18, less one for rounding. Without modal
        // frames, the frame after the hold would have counted only a
        // quarter of a second: 15.
        assert!(ticks.get() >= 17, "{} ticks in {elapsed:?}", ticks.get());
    }

    #[test]
    fn the_limiter_paces_the_loop() {
        let (ticks, frames) = (Rc::default(), Rc::default());
        let mut app = counting_app(&ticks, &frames);
        app.config.frame_rate_limit = Some(200);
        let start = Instant::now();
        app.run_loop(&mut Script::closing_when(|pumps| pumps == 21), None)
            .expect("no error without a device");
        // Twenty frames of 5 ms each. A busy machine can make them longer,
        // but nothing makes them shorter.
        assert_eq!(frames.get(), 20);
        assert!(
            start.elapsed() >= Duration::from_millis(100),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn a_real_hitch_of_one_second_is_clamped_and_does_not_spiral() {
        // What each frame saw: how many ticks it ran, and its `dt`.
        let frames: Rc<RefCell<Vec<(u32, f32)>>> = Rc::default();
        let ticks_this_frame = Rc::new(Cell::new(0u32));
        let stalled = Rc::new(Cell::new(false));
        let frames_since_stall = Rc::new(Cell::new(0u32));

        let tick_counter = Rc::clone(&ticks_this_frame);
        let tick_stalled = Rc::clone(&stalled);
        let frame_log = Rc::clone(&frames);
        let frame_counter = Rc::clone(&ticks_this_frame);
        let frame_stalled = Rc::clone(&stalled);
        let frame_since = Rc::clone(&frames_since_stall);
        let script_since = Rc::clone(&frames_since_stall);

        let app = app()
            .add_fixed_system("stall_once", move |_, tick| {
                tick_counter.set(tick_counter.get() + 1);
                // The hitch: the third tick takes a second.
                if tick.index == 2 {
                    thread::sleep(Duration::from_secs(1));
                    tick_stalled.set(true);
                }
            })
            .add_frame_system("record", move |_, frame| {
                frame_log
                    .borrow_mut()
                    .push((frame_counter.take(), frame.dt));
                if frame_stalled.get() {
                    frame_since.set(frame_since.get() + 1);
                }
            });

        // Run on for a few frames after the one that stalled. If the stall
        // never comes, give up after ten seconds of frames, so that a bug
        // fails this test and does not hang it.
        let start = Instant::now();
        app.run_loop(
            &mut Script::closing_when(move |pumps| script_since.get() > 6 || pumps > 600),
            None,
        )
        .expect("no error without a device");
        let elapsed = start.elapsed();
        assert!(stalled.get(), "the stall never happened");

        let frames = frames.borrow();
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

    // -- actions and resources ------------------------------------------------

    use nadir_core::{Key, Vec2};
    use nadir_input::{ActionKind, Binding};

    use crate::Interpolated;

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

    /// Where the mover is, between ticks.
    struct Mover(Interpolated<Vec2>);

    /// Where each frame drew the mover.
    struct Drawn(Vec<f32>);

    /// An app that moves a `Mover` with `Act::Move`, bound to WASD, and
    /// records where each frame draws it.
    fn moving_app() -> App {
        let mut map = ActionMap::<Act>::new().expect("a valid set");
        let wasd = Binding::KeyComposite2 {
            up: Key::W,
            down: Key::S,
            left: Key::A,
            right: Key::D,
        };
        map.bind(Act::Move, 0, wasd).expect("fits");
        app()
            .add_actions(map)
            .insert_resource(Mover(Interpolated::new(Vec2::ZERO)))
            .insert_resource(Drawn(Vec::new()))
            .add_fixed_system("move", |resources, tick| {
                let direction = resources.resource::<ActionState<Act>>().axis2(Act::Move);
                let mut mover = resources.resource_mut::<Mover>();
                mover.0.snapshot();
                mover.0.current += direction * SPEED * tick.dt;
            })
            .add_frame_system("draw", |resources, frame| {
                let x = resources.resource::<Mover>().0.lerp(frame.alpha).x;
                resources.resource_mut::<Drawn>().0.push(x);
            })
    }

    fn key(key: Key, pressed: bool) -> Event {
        Event::Key {
            key,
            pressed,
            repeat: false,
        }
    }

    fn x(app: &App) -> f32 {
        app.resources().resource::<Mover>().0.current.x
    }

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_tap_between_ticks_moves_exactly_one_tick() {
        let mut app = moving_app();
        let mut clock = new_clock();
        // D goes down and up within a frame too short for a tick.
        app.frame(&mut clock, 5 * MS, &[key(Key::D, true), key(Key::D, false)]);
        assert_eq!(x(&app), 0.0, "no tick yet");
        // The next frame's tick moves it once.
        app.frame(&mut clock, 12 * MS, &[]);
        assert_eq!(x(&app), SPEED / 60.0);
        for _ in 0..10 {
            app.frame(&mut clock, 16 * MS, &[]);
        }
        assert_eq!(x(&app), SPEED / 60.0, "and never again");
        // In a frame of two ticks, the second sees no edge either.
        app.frame(
            &mut clock,
            34 * MS,
            &[key(Key::D, true), key(Key::D, false)],
        );
        assert_eq!(x(&app), 2.0 * SPEED / 60.0);
    }

    #[test]
    fn a_held_key_moves_every_tick() {
        let mut app = moving_app();
        let mut clock = new_clock();
        app.frame(&mut clock, Duration::ZERO, &[key(Key::D, true)]);
        // Exactly a second of 60 Hz frames: sixty ticks.
        for _ in 0..60 {
            app.frame(&mut clock, Duration::from_nanos(16_666_667), &[]);
        }
        assert!((x(&app) - SPEED).abs() < 1e-3, "{}", x(&app));
        app.frame(&mut clock, Duration::ZERO, &[key(Key::D, false)]);
        app.frame(&mut clock, 20 * MS, &[]);
        assert!((x(&app) - SPEED).abs() < 1e-3, "it stops: {}", x(&app));
    }

    #[test]
    fn the_wheel_moves_on_each_tick_without_an_action_set() {
        let seen: Rc<RefCell<Vec<Vec2>>> = Rc::default();
        let record = Rc::clone(&seen);
        let mut app = app().add_fixed_system("zoom", move |resources, _| {
            record
                .borrow_mut()
                .push(resources.resource::<Input>().wheel());
        });
        let mut clock = new_clock();
        let up = Vec2::new(0.0, 3.0);
        // A tick each.
        app.frame(&mut clock, 20 * MS, &[Event::MouseWheel(up)]);
        app.frame(&mut clock, 20 * MS, &[]);
        assert_eq!(*seen.borrow(), [up, Vec2::ZERO]);
    }

    #[test]
    fn interpolation_draws_where_the_mover_was_one_tick_ago_at_any_frame_rate() {
        // Held from the start, the mover is drawn at SPEED · (t − 1/60): one
        // tick behind the clock, so every frame moves it by its own length,
        // whatever that is.
        let mut irregular = nadir_core::Pcg32::new(7, 1);
        let rates: [(&str, Vec<Duration>); 5] = [
            ("60", vec![Duration::from_nanos(16_666_667); 120]),
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
            let mut app = moving_app();
            let mut clock = new_clock();
            app.frame(&mut clock, Duration::ZERO, &[key(Key::D, true)]);
            let mut t = Duration::ZERO;
            let mut expected = vec![0.0];
            for frame in frames {
                app.frame(&mut clock, frame, &[]);
                t += frame;
                let x = SPEED as f64 * (t.as_secs_f64() - 1.0 / 60.0).max(0.0);
                expected.push(x);
            }
            let drawn = &app.resources().resource::<Drawn>().0;
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
        let _ = moving_app().add_actions(map);
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
        let mut app = moving_app().add_actions(map);
        let mut clock = new_clock();
        app.frame(&mut clock, 20 * MS, &[key(Key::D, true)]);
        assert_eq!(x(&app), 0.0, "D is no longer bound");
        app.frame(&mut clock, 20 * MS, &[key(Key::Right, true)]);
        assert_eq!(x(&app), SPEED / 60.0, "one latch a tick, with the new map");
    }

    #[test]
    fn the_render_frame_is_emptied_before_each_frame() {
        let sizes: Rc<RefCell<Vec<usize>>> = Rc::default();
        let seen = Rc::clone(&sizes);
        let mut app = app().add_frame_system("draw", move |resources, _| {
            let mut frame = resources.resource_mut::<RenderFrame>();
            seen.borrow_mut().push(frame.sprites.len());
            let texture =
                nadir_assets::Handle::new(nadir_core::GenIndex::new(0, std::num::NonZeroU32::MIN));
            frame.sprites.push(nadir_render::SpriteDraw::new(
                texture,
                Vec2::ZERO,
                Vec2::ONE,
            ));
        });
        let mut clock = new_clock();
        for _ in 0..3 {
            app.frame(&mut clock, 16 * MS, &[]);
        }
        assert_eq!(*sizes.borrow(), [0, 0, 0]);
    }

    #[test]
    fn a_missing_resource_names_the_system_that_asked() {
        let mut app = app().add_fixed_system("needs_a_score", |resources, _| {
            let _ = resources.resource::<u64>();
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            app.frame(&mut new_clock(), 20 * MS, &[]);
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
        let moved: Rc<RefCell<f32>> = Rc::default();
        let report = Rc::clone(&moved);
        let mut app = moving_app().add_frame_system("report", move |resources, _| {
            *report.borrow_mut() = resources.resource::<Mover>().0.current.x;
        });
        app.config.frame_rate_limit = None;
        app.run_loop(&mut Tapping { pumps: 0 }, None)
            .expect("no error without a device");
        assert_eq!(*moved.borrow(), SPEED / 60.0, "the tap moved it one tick");
    }

    /// A steady frame of 10,000 sprites allocates nothing (roadmap M3.5).
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
        use nadir_render::{SortMode, SpriteDraw, Texture};

        use super::*;
        use crate::{FrameAllocs, Interpolated};

        /// How many sprites walk about.
        const CROWD: usize = 10_000;
        /// Ground tiles under them: 16 × 12 of 32 pixels.
        const TILES: usize = 192;
        /// Frames before the counting starts: the textures load after the
        /// first draw, and the buffers grow in the frames after it.
        const WARM_UP: usize = 10;
        /// Frames counted.
        const COUNTED: usize = 100;
        /// The view, which the walkers stay inside.
        const VIEW: Vec2 = Vec2::new(480.0, 270.0);

        struct Walker {
            position: Interpolated<Vec2>,
            velocity: Vec2,
            tint: Color,
        }

        /// The scene: its textures and the walkers.
        struct Scene {
            walker: Handle<Texture>,
            ground: Handle<Texture>,
            walkers: Vec<Walker>,
        }

        /// What each frame saw: the last frame's allocations and drawing,
        /// and the ticks so far. Room for every frame is made up front, and
        /// the test keeps it when the loop drops the app.
        struct Seen {
            frames: Vec<(AllocCounts, FrameStats, u64)>,
            ticks: u64,
        }

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

        /// The scene's app, loading its textures from `assets`, and noting
        /// in `seen` what each frame sees.
        fn scene(assets: PathBuf, seen: &Rc<RefCell<Seen>>) -> App {
            let mut app = App::new(Config {
                assets,
                // Frames then sleep in the limiter, which the count covers.
                frame_rate_limit: Some(500),
                ..Config::default()
            });
            let (walker, ground) = {
                let mut server = app.resources_mut().resource_mut::<AssetServer>();
                (server.load("walker.qoi"), server.load("ground.qoi"))
            };
            let mut seed = 1u32;
            let mut random = move || {
                // A xorshift: the same walkers every run.
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed % 10_000) as f32 / 10_000.0
            };
            let walkers = (0..CROWD)
                .map(|index| Walker {
                    position: Interpolated::new(Vec2::new(
                        8.0 + random() * (VIEW.x - 16.0),
                        16.0 + random() * (VIEW.y - 16.0),
                    )),
                    velocity: Vec2::new(random() * 120.0 - 60.0, random() * 120.0 - 60.0),
                    tint: if index % 4 == 0 {
                        Color::rgb(255, 200, 200)
                    } else {
                        Color::WHITE
                    },
                })
                .collect();
            app.insert_resource(Scene {
                walker,
                ground,
                walkers,
            })
            .add_fixed_system("walk", {
                let seen = Rc::clone(seen);
                move |resources, tick| {
                    for walker in &mut resources.resource_mut::<Scene>().walkers {
                        walker.position.snapshot();
                        let mut next = walker.position.current + walker.velocity * tick.dt;
                        if !(8.0..=VIEW.x - 8.0).contains(&next.x) {
                            walker.velocity.x = -walker.velocity.x;
                            next.x = walker.position.current.x;
                        }
                        if !(16.0..=VIEW.y).contains(&next.y) {
                            walker.velocity.y = -walker.velocity.y;
                            next.y = walker.position.current.y;
                        }
                        walker.position.current = next;
                    }
                    seen.borrow_mut().ticks += 1;
                }
            })
            .add_frame_system("draw", |resources, frame| {
                let scene = resources.resource::<Scene>();
                let mut out = resources.resource_mut::<RenderFrame>();
                out.camera.position = VIEW * 0.5;
                for tile in 0..TILES {
                    let corner = Vec2::new((tile % 16) as f32, (tile / 16) as f32) * 32.0;
                    let mut sprite = SpriteDraw::new(scene.ground, corner, Vec2::splat(32.0));
                    sprite.pivot = Vec2::ZERO;
                    sprite.sort = SortMode::Fixed(0);
                    out.sprites.push(sprite);
                }
                for walker in &scene.walkers {
                    let position = walker.position.lerp(frame.alpha);
                    let mut sprite = SpriteDraw::new(scene.walker, position, Vec2::new(8.0, 16.0));
                    sprite.layer = 1;
                    sprite.color = walker.tint;
                    sprite.flip.x = walker.velocity.x < 0.0;
                    out.sprites.push(sprite);
                }
            })
            .add_frame_system("probe", {
                let seen = Rc::clone(seen);
                move |resources, _| {
                    let mut seen = seen.borrow_mut();
                    let entry = (
                        resources.resource::<FrameAllocs>().0,
                        *resources.resource::<FrameStats>(),
                        seen.ticks,
                    );
                    seen.frames.push(entry);
                    // In a warm-up frame, a known allocation, which that
                    // frame's count must show.
                    if seen.frames.len() == 5 {
                        drop(black_box(Box::new(5u64)));
                    }
                }
            })
        }

        #[test]
        fn a_steady_frame_of_10k_sprites_allocates_nothing() {
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
                    sprites: (CROWD + TILES) as u32,
                    skipped: 0,
                    batches: 2,
                };
                assert_eq!(seen[WARM_UP].1, all, "drawn before the counting starts");
                assert_eq!(seen[WARM_UP + COUNTED - 1].1, all);
            }
        }
    }
}
