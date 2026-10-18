//! [`App`]: the game's configuration, its systems, and the main loop.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use nadir_core::UVec2;
use nadir_core::log::{debug, error};
use nadir_core::time::{FixedClock, Frame, Stopwatch, Tick};
use nadir_platform::{Event, ModalFrame, Platform, WindowConfig, precise_sleep, show_error_dialog};

use crate::Error;
use crate::limiter::FrameLimiter;
use crate::log_file;

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
    /// limit: the loop then runs as fast as it can, and keeps a processor
    /// core busy. Until there is rendering to wait for the display with,
    /// this limit is all that paces the loop.
    pub frame_rate_limit: Option<u32>,
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
        }
    }
}

/// A function that the loop calls, with the name it was added under.
struct System<T> {
    name: &'static str,
    run: Box<dyn FnMut(&T)>,
}

/// Where the loop gets the window's events from: the platform in a game,
/// and a script in tests.
trait EventSource {
    /// Appends the events that arrived since the last call. Does not wait.
    fn pump_events(&mut self, out: &mut Vec<Event>);

    /// Sets what to call while the system's own event loop keeps
    /// [`EventSource::pump_events`] from returning.
    fn set_modal_frame_callback(&mut self, callback: Box<dyn FnMut(ModalFrame)>);
}

impl EventSource for Platform {
    fn pump_events(&mut self, out: &mut Vec<Event>) {
        Platform::pump_events(self, out);
    }

    fn set_modal_frame_callback(&mut self, callback: Box<dyn FnMut(ModalFrame)>) {
        Platform::set_modal_frame_callback(self, callback);
    }
}

/// Everything a frame needs, shared by the main loop and the modal frame
/// callback.
struct EngineCore {
    app: App,
    clock: FixedClock,
    stopwatch: Stopwatch,
}

impl EngineCore {
    /// Runs one frame, for the time since the last.
    fn frame(&mut self) {
        let elapsed = self.stopwatch.lap();
        self.app.step(&mut self.clock, elapsed);
    }
}

/// A game: its configuration and its systems. [`App::run`] opens the log
/// file and the window, and runs the main loop.
///
/// ```no_run
/// use nadir::prelude::*;
///
/// fn main() -> Result<(), nadir::Error> {
///     App::new(Config {
///         title: "My game".into(),
///         ..Config::default()
///     })
///     .add_fixed_system("simulate", |tick| {
///         // 60 times a second: move things by `tick.dt`.
///         let _ = tick;
///     })
///     .add_frame_system("animate", |frame| {
///         // Once for each frame: blend by `frame.alpha`.
///         let _ = frame;
///     })
///     .run()
/// }
/// ```
///
/// The systems are plain closures for now. Once the engine has an entity
/// world, a system will also receive that.
pub struct App {
    config: Config,
    fixed_systems: Vec<System<Tick>>,
    frame_systems: Vec<System<Frame>>,
}

impl App {
    /// An app with no systems.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            fixed_systems: Vec::new(),
            frame_systems: Vec::new(),
        }
    }

    /// Adds a system that runs once for every simulation tick: [`TICK_HZ`]
    /// times a second, however fast frames come. Systems run in the order
    /// they were added.
    #[must_use]
    pub fn add_fixed_system(
        mut self,
        name: &'static str,
        system: impl FnMut(&Tick) + 'static,
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
        system: impl FnMut(&Frame) + 'static,
    ) -> Self {
        self.frame_systems.push(System {
            name,
            run: Box::new(system),
        });
        self
    }

    /// Opens the log file and the window, and runs the main loop until the
    /// user closes the window.
    ///
    /// The log file is `nadir.log`, in the game's logs folder: on Linux
    /// `~/.local/share/<game>`. Each run starts a new file and keeps the
    /// last three runs' files beside it, as `nadir.log.1` to `nadir.log.3`.
    /// If the file cannot be opened, the game goes on without it.
    ///
    /// # Errors
    ///
    /// Fails if the window cannot be opened. Before it returns the error,
    /// it logs it and shows it to the player with [`show_error_dialog`].
    pub fn run(self) -> Result<(), Error> {
        log_file::start(&self.config.studio, &self.config.game);
        let mut platform = Platform::new(&self.config.window())
            .map_err(|error| fatal(&self.config.title, error.into()))?;
        self.run_loop(&mut platform);
        Ok(())
    }

    /// The main loop: events, then the ticks that are due, then the frame.
    ///
    /// While the system runs an event loop of its own, as Windows does while
    /// the window is dragged and macOS while it is resized, the pump does not
    /// return. Frames then run from the modal frame callback, unpaced: the
    /// system calls it every 10 to 16 ms on Windows, and once for each step
    /// of the resize on macOS.
    fn run_loop(self, window: &mut dyn EventSource) {
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
        }));
        let modal_core = Rc::clone(&core);
        window.set_modal_frame_callback(Box::new(move |_frame| {
            // The loop borrows the core only between pumps, and this runs
            // during one, so the borrow always succeeds.
            if let Ok(mut core) = modal_core.try_borrow_mut() {
                core.frame();
            }
        }));
        let mut events = Vec::with_capacity(256);
        loop {
            window.pump_events(&mut events);
            let close = events.contains(&Event::CloseRequested);
            events.clear();
            if close {
                break;
            }

            core.borrow_mut().frame();

            if let Some(due) = limiter.end_of_frame(Instant::now()) {
                precise_sleep(due);
            }
        }
    }

    /// Runs the systems for one frame that comes `elapsed` after the last.
    fn step(&mut self, clock: &mut FixedClock, elapsed: Duration) {
        let frame = clock.begin_frame(elapsed);
        while let Some(tick) = clock.consume_tick() {
            for system in &mut self.fixed_systems {
                (system.run)(&tick);
            }
        }
        for system in &mut self.frame_systems {
            (system.run)(&frame);
        }
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
            .add_fixed_system("count_ticks", move |_| ticks.set(ticks.get() + 1))
            .add_frame_system("count_frames", move |_| frames.set(frames.get() + 1))
    }

    /// Runs frames of one length until `total` has passed.
    fn simulate(app: &mut App, clock: &mut FixedClock, frame_length: Duration, total: Duration) {
        let mut passed = Duration::ZERO;
        while passed < total {
            let step = frame_length.min(total - passed);
            app.step(clock, step);
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
        };
        let expected = WindowConfig {
            title: "Sandbox".to_string(),
            size: UVec2::new(960, 540),
            min_size: UVec2::new(480, 270),
            resizable: false,
            fullscreen: true,
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
            .add_fixed_system("a", move |tick: &Tick| a(format!("tick {}", tick.index)))
            .add_frame_system("c", move |_: &Frame| c("frame".to_string()))
            .add_fixed_system("b", move |tick: &Tick| b(format!("tick {}", tick.index)))
            .add_frame_system("d", move |_: &Frame| d("frame".to_string()));
        assert_eq!(names(&app.fixed_systems), ["a", "b"]);
        assert_eq!(names(&app.frame_systems), ["c", "d"]);

        // 40 ms hold two ticks.
        app.step(&mut new_clock(), Duration::from_millis(40));
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
        app.step(&mut clock, Duration::from_millis(5));
        app.step(&mut clock, Duration::from_millis(5));
        assert_eq!((ticks.get(), frames.get()), (0, 2));
        app.step(&mut clock, Duration::from_millis(10));
        assert_eq!((ticks.get(), frames.get()), (1, 3));
    }

    #[test]
    fn systems_get_the_tick_length_and_the_frame_time() {
        let dts: Rc<RefCell<Vec<f32>>> = Rc::default();
        let (tick_dts, frame_dts) = (Rc::clone(&dts), Rc::clone(&dts));
        let alphas: Rc<RefCell<Vec<f32>>> = Rc::default();
        let frame_alphas = Rc::clone(&alphas);
        let mut app = app()
            .add_fixed_system("tick", move |tick| tick_dts.borrow_mut().push(tick.dt))
            .add_frame_system("frame", move |frame| {
                frame_dts.borrow_mut().push(frame.dt);
                frame_alphas.borrow_mut().push(frame.alpha);
            });
        app.step(&mut new_clock(), Duration::from_millis(20));
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
        app.step(&mut clock, Duration::from_secs(1));
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
        app.run_loop(&mut window);
        // Seven frames ran, and the eighth pump ended the loop before its
        // frame.
        assert_eq!(window.pumps, 8);
        assert_eq!(frames.get(), 7);
    }

    #[test]
    fn a_close_request_on_the_first_pump_runs_no_frame() {
        let (ticks, frames) = (Rc::default(), Rc::default());
        let app = counting_app(&ticks, &frames);
        app.run_loop(&mut Script::closing_when(|_| true));
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
        app.run_loop(&mut window);
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
        let wait = move |_: &Frame| thread::sleep(Duration::from_millis(10));
        app = app.add_frame_system("wait", wait);
        app.run_loop(&mut window);
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
        app.run_loop(&mut Script::closing_when(|pumps| pumps == 21));
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
            .add_fixed_system("stall_once", move |tick| {
                tick_counter.set(tick_counter.get() + 1);
                // The hitch: the third tick takes a second.
                if tick.index == 2 {
                    thread::sleep(Duration::from_secs(1));
                    tick_stalled.set(true);
                }
            })
            .add_frame_system("record", move |frame| {
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
        app.run_loop(&mut Script::closing_when(move |pumps| {
            script_since.get() > 6 || pumps > 600
        }));
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
}
