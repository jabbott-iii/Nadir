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

//! The debug UI in the game, in development builds (feature `dev`;
//! roadmap M7.10, ADR-0058): the windows a game adds, each shown and hidden
//! by its own key; nadir-debug's UI fed from the window's events, drawn
//! after the game's UI in the engine's font, and holding the game's input
//! off while it has the pointer or the keyboard; and its windows' places,
//! kept in `debug_ui.jsonc`. The engine's own windows are the stats
//! overlay on F1 and the profiler on F3, which writes its traces to the
//! logs folder (roadmap M7.3, ADR-0059), and shows the GPU's time over each
//! frame drawn, from the renderer's timers (roadmap M7.12, ADR-0061); and
//! the inspector on F2, which outlines the entity it shows in the world and
//! writes its prefabs to the logs folder's `prefabs` (roadmap M7.4,
//! ADR-0062); the console on the backtick, over the engine's log, with the
//! engine's commands and the game's (roadmap M7.5, ADR-0063); and the debug
//! drawing on F4, whose window ticks the overlays drawn into the world, and
//! the time controls, F5 to F7, which pause, step and scale the game's time,
//! a label in the corner saying so (roadmap M7.6, ADR-0064).

mod draw;
mod font;
#[cfg(test)]
mod golden;
mod places;
mod time;

use std::fmt::Write as _;
use std::fs;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

use nadir_assets::{
    AssetError, AssetPath, AssetServer, FinalizeCtx, Handle, LooseSource, Scope, Source,
};
use nadir_audio::Audio;
use nadir_core::civil::DateTime;
use nadir_core::log::{self, Level, error, info, warn};
use nadir_core::profile::ScopeRecord;
use nadir_core::{Color, Key, MouseButton, Rect, UVec2, Vec2};
use nadir_debug::{
    Args, Console, ConsoleError, ConsoleOut, DebugInput, DebugUi, FontMetrics, FrameTiming,
    GpuTiming, Inspector, Profiler, Stats, Style, Ui,
};
use nadir_ecs::{Entity, Schedule, World};
use nadir_input::{Capture, Input, UiEvent, UiEvents};
use nadir_platform::Event;
use nadir_render::{
    Font, FrameStats, GpuTimes, RenderFrame, SortMode, SpriteDraw, TextDraw, Texture, UiList,
    UiScale, UiSpace, UiView,
};
use nadir_scene::{FromValue, PrefabError, PrefabLibrary, TypeRegistry};

use crate::{
    Collider, FrameAllocs, LevelError, Parent, Player, PrevTransform, SaveId, Sprite, TickStats,
    Transform, WindowSize, loading_context, reflect_context,
};

pub(crate) use places::FILE_NAME;

/// A window of the debug UI, which fills it with widgets each frame it is
/// open: a function of the world and the window's [`Ui`], added with
/// [`App::add_debug_window`](crate::App::add_debug_window).
pub type DebugWindow = fn(&World, &mut Ui<'_>);

/// A command of the game's in the debug console: a function of the world,
/// the arguments typed, checked against the command's usage, and where to
/// print, added with
/// [`App::add_console_command`](crate::App::add_console_command). An error
/// it returns ([`ConsoleError::Failed`] with why) is printed in red.
pub type ConsoleCommand = fn(&World, &Args<'_>, &mut ConsoleOut<'_>) -> Result<(), ConsoleError>;

/// The stats overlay's key, title and first place.
const STATS: (Key, &str, Rect) = (
    Key::F1,
    "Stats##nadir::stats",
    Rect::new(Vec2::new(8.0, 8.0), Vec2::new(208.0, 172.0)),
);

/// The profiler's key, title and first place.
const PROFILER: (Key, &str, Rect) = (
    Key::F3,
    "Profiler##nadir::profiler",
    Rect::new(Vec2::new(216.0, 8.0), Vec2::new(576.0, 308.0)),
);

/// The inspector's key, title and first place.
const INSPECTOR: (Key, &str, Rect) = (
    Key::F2,
    "Inspector##nadir::inspector",
    Rect::new(Vec2::new(584.0, 8.0), Vec2::new(944.0, 508.0)),
);

/// The console's key, title and first place.
const CONSOLE: (Key, &str, Rect) = (
    Key::Grave,
    "Console##nadir::console",
    Rect::new(Vec2::new(8.0, 316.0), Vec2::new(944.0, 556.0)),
);

/// The debug drawing's key, title and first place.
const DEBUG_DRAW: (Key, &str, Rect) = (
    Key::F4,
    "Debug draw##nadir::debug_draw",
    Rect::new(Vec2::new(952.0, 8.0), Vec2::new(1092.0, 162.0)),
);

/// How far the time controls' label is from the screen's top-right
/// corner, in the debug UI's units.
const CORNER: f32 = 8.0;

/// The source the inspector's values are read and written under, which
/// its errors and warnings name.
const INSPECTED: &str = "the inspector";

/// The source the console's spawns are read under, which their errors
/// name.
const TYPED: &str = "the console";

/// The time scales `timescale` takes: F7's range (roadmap M7.6).
const TIME_SCALES: RangeInclusive<f32> = 0.25..=4.0;

/// The engine's own commands in the console.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Builtin {
    Schedule,
    Spawn,
    Tp,
    Timescale,
    Level,
    Reload,
    Profile,
    Log,
    Quit,
}

/// The engine's commands: each one's usage, help and command, in the order
/// `help` lists them, after its own.
const BUILTINS: [(&str, &str, Builtin); 9] = [
    (
        "schedule",
        "lists the systems in the order they run",
        Builtin::Schedule,
    ),
    (
        "spawn <prefab> [x] [y]",
        "spawns a prefab at (x, y), or where the camera looks",
        Builtin::Spawn,
    ),
    ("tp <x> <y>", "moves the player to (x, y)", Builtin::Tp),
    (
        "timescale <scale>",
        "runs the game's time at scale times real time, 0.25 to 4",
        Builtin::Timescale,
    ),
    (
        "level <name>",
        "loads the level levels/<name>.klvl",
        Builtin::Level,
    ),
    (
        "reload",
        "reads every loaded asset's file again",
        Builtin::Reload,
    ),
    (
        "profile dump [path]",
        "writes the profiler's trace to path, or beside the log",
        Builtin::Profile,
    ),
    (
        "log level <target> <error|warn|info|debug|trace>",
        "sets how much a module, and those in it, logs",
        Builtin::Log,
    ),
    (
        "quit",
        "ends the game, as closing its window does",
        Builtin::Quit,
    ),
];

/// A command of the console: the engine's or the game's.
#[derive(Clone, Copy)]
enum Command {
    Engine(Builtin),
    Game(ConsoleCommand),
}

/// The folder of the logs folder the inspector's prefabs are written to.
const PREFABS: &str = "prefabs";

/// The components a prefab the inspector saves leaves out: a spawn places
/// the entity, and gives it its own save number.
const NOT_IN_PREFABS: [&str; 2] = [Transform::TYPE_NAME, SaveId::TYPE_NAME];

/// The color of the outline around the entity the inspector shows.
const OUTLINE: Color = Color::rgb(255, 200, 0);

/// Half the side of the box outlined around an entity with a place but no
/// sprite or collider, in world units.
const MARK: f32 = 4.0;

/// The last renders whose frames are kept, to give each the GPU's time
/// over it when that is read back: twice the renderer's four renders in
/// flight. A time read back 8 renders or more after its render is
/// dropped.
const RENDERS: usize = 8;

/// What fills a window.
#[derive(Clone, Copy)]
enum Content {
    /// A game's window.
    Game(DebugWindow),
    /// The engine's stats overlay.
    Stats,
    /// The engine's profiler.
    Profiler,
    /// The engine's inspector.
    Inspector,
    /// The engine's console.
    Console,
    /// The engine's debug drawing: the overlays it ticks.
    DebugDraw,
}

/// A window of the debug UI: the engine's or a game's.
struct Window {
    toggle: Key,
    title: &'static str,
    first: Rect,
    content: Content,
    open: bool,
}

impl Window {
    fn closed(toggle: Key, title: &'static str, first: Rect, content: Content) -> Self {
        Self {
            toggle,
            title,
            first,
            content,
            open: false,
        }
    }
}

/// The asset server's source in development builds: the engine's own
/// files, the debug UI's font and white texel, made the first time one is
/// read, and the game's loose files.
pub(crate) struct EngineFiles {
    loose: LooseSource,
    files: OnceLock<Vec<(&'static str, Vec<u8>)>>,
}

impl EngineFiles {
    /// The engine's files, and the loose files under `root`.
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            loose: LooseSource::new(root),
            files: OnceLock::new(),
        }
    }
}

impl Source for EngineFiles {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
        if !font::is_file(path.as_str()) {
            return self.loose.read(path);
        }
        self.files
            .get_or_init(font::files)
            .iter()
            .find(|(name, _)| *name == path.as_str())
            .map(|(_, bytes)| bytes.clone())
            .ok_or(AssetError::NotFound)
    }

    /// `None` for the engine's files, which never change.
    fn modified(&self, path: &AssetPath) -> Option<SystemTime> {
        if font::is_file(path.as_str()) {
            None
        } else {
            self.loose.modified(path)
        }
    }
}

/// The debug UI's state in the app: the engine's windows and the game's,
/// the window's events for the UI, the UI itself once a window has been
/// open, where its places are kept, and the profiler.
pub(crate) struct DebugTools {
    windows: Vec<Window>,
    events: UiEvents,
    input: DebugInput,
    /// Made the first time a window is open, with the engine's font.
    ui: Option<DebugUi>,
    /// Whether making it failed, which the log said once.
    failed: bool,
    /// The free scale where [`UiSettings`](crate::UiSettings) has none: the
    /// window's scale factor, whole and at least 1.
    scale: f32,
    /// What the debug UI held back from the game when it last changed it.
    held: Capture,
    /// Who holds the left button down, from where its press was: the debug
    /// UI (`true`) or the game; `None` while it is up.
    left: Option<bool>,
    /// The places file, which [`App::run`](crate::App::run) names; and the
    /// places read from it, given to the UI when it is made.
    file: Option<PathBuf>,
    places: Vec<places::Place>,
    /// The UI's places revision when they were last read or written.
    saved: u64,
    /// The last frames' timings and scopes.
    profiler: Profiler,
    /// When the frame being run started, and how long the processor has
    /// worked on it so far; how long it worked on the last one.
    frame_start: Option<Instant>,
    work: Duration,
    last_work: Duration,
    /// The renders so far, and for the last [`RENDERS`] of them, by their
    /// number modulo that, the frame each drew and when it started.
    renders: u64,
    drawn: [Option<(u64, Instant)>; RENDERS],
    /// The logs folder, which [`App::run`](crate::App::run) names: the
    /// profiler's traces go into it, and the inspector's prefabs into its
    /// [`PREFABS`].
    logs: Option<PathBuf>,
    /// The entity list and the components of the entity it shows.
    inspector: Inspector,
    /// The white texel, loaded with the UI's font, which the inspector's
    /// outline and the debug drawing are drawn with; and the font.
    white: Option<Handle<Texture>>,
    font: Option<Handle<Font>>,
    /// The console, and how many of the log's records it has read.
    console: Console<Command>,
    log_read: u64,
    /// The game's time, as the time controls and `timescale` leave it, and
    /// the corner label's text, written each frame it shows.
    time: time::Time,
    label: String,
    /// The overlays the debug drawing ticks.
    overlays: draw::Overlays,
    /// The level `level` asked for, which the app loads after the frame.
    level: Option<String>,
    /// Whether `quit` asked [`App::run`](crate::App::run) to end.
    quit: bool,
}

impl DebugTools {
    pub(crate) fn new() -> Self {
        let engine = |(toggle, title, first): (Key, &'static str, Rect), content| {
            Window::closed(toggle, title, first, content)
        };
        let mut console = Console::new();
        for (usage, help, builtin) in BUILTINS {
            console
                .register(usage, help, Command::Engine(builtin))
                .expect("the engine's usages start with names");
        }
        Self {
            windows: vec![
                engine(STATS, Content::Stats),
                engine(INSPECTOR, Content::Inspector),
                engine(PROFILER, Content::Profiler),
                engine(CONSOLE, Content::Console),
                engine(DEBUG_DRAW, Content::DebugDraw),
            ],
            events: UiEvents::new(),
            input: DebugInput::new(),
            ui: None,
            failed: false,
            scale: 1.0,
            held: Capture::NONE,
            left: None,
            file: None,
            places: Vec::new(),
            saved: 0,
            profiler: Profiler::new(),
            frame_start: None,
            work: Duration::ZERO,
            last_work: Duration::ZERO,
            renders: 0,
            drawn: [None; RENDERS],
            logs: None,
            inspector: Inspector::new(),
            white: None,
            font: None,
            console,
            log_read: 0,
            time: time::Time::new(),
            label: String::new(),
            overlays: draw::Overlays::new(),
            level: None,
            quit: false,
        }
    }

    /// Adds a game's window, closed.
    pub(crate) fn add(&mut self, toggle: Key, title: &'static str, first: Rect, add: DebugWindow) {
        self.windows
            .push(Window::closed(toggle, title, first, Content::Game(add)));
    }

    /// Adds a game's command to the console, under `usage`, whose first
    /// word is its name, with `help`; or, if the usage starts with no name,
    /// logs why not.
    pub(crate) fn add_command(&mut self, usage: &str, help: &str, command: ConsoleCommand) {
        if let Err(cause) = self.console.register(usage, help, Command::Game(command)) {
            error!("the console command is not added: {cause}");
        }
    }

    /// The time the game's clock is given for a frame `elapsed` long, as
    /// the time controls and `timescale` leave it: none while paused but a
    /// tick for each step asked for since, otherwise `elapsed` at the
    /// scale.
    pub(crate) fn game_time(&mut self, elapsed: Duration) -> Duration {
        self.time.game_time(elapsed)
    }

    /// The level `level` asked for in the frame, once.
    pub(crate) fn take_level(&mut self) -> Option<String> {
        self.level.take()
    }

    /// Says in the console why the level `name` that `level` asked for did
    /// not load; the log says when one did.
    pub(crate) fn level_loaded(&mut self, name: &str, result: Result<(), LevelError>) {
        if let Err(cause) = result {
            self.console
                .print_error(format_args!("the level `{name}` is not loaded: {cause}"));
        }
    }

    /// Whether `quit` asked the game to end.
    pub(crate) fn quit(&self) -> bool {
        self.quit
    }

    /// Writes the profiler's traces into `folder`, and the inspector's
    /// prefabs into its folder [`PREFABS`].
    pub(crate) fn write_traces_in(&mut self, folder: PathBuf) {
        self.logs = Some(folder);
    }

    /// As a frame starts, at `now`: ends the frame before, if there was
    /// one, and gives it to the profiler with `records`, the scopes
    /// collected since it started.
    pub(crate) fn start_profile(&mut self, now: Instant, records: &[ScopeRecord]) {
        if let Some(start) = self.frame_start {
            let length = now.saturating_duration_since(start);
            self.profiler
                .push(FrameTiming::new(start, length, self.work), records);
            self.last_work = self.work;
        }
        self.frame_start = Some(now);
        self.work = Duration::ZERO;
    }

    /// Counts `work` more of the processor's time on the frame being run.
    pub(crate) fn add_work(&mut self, work: Duration) {
        self.work += work;
    }

    /// After the frame being run was drawn, its drawing started at
    /// `submitted`: gives the profiler `times`, the GPU's times that this
    /// render read back ([`Renderer::gpu_times`](nadir_render::Renderer::gpu_times)),
    /// each for the frame it was timed in. Each says how many renders
    /// before this one it was, and a frame not drawn (a modal one) is no
    /// render: so the frames of the last renders are kept by render. Call
    /// it after each render of a renderer that times every render, and
    /// after no other.
    pub(crate) fn rendered(&mut self, submitted: Instant, times: &[GpuTimes]) {
        let render = self.renders;
        self.renders += 1;
        let slot = |render: u64| (render % RENDERS as u64) as usize;
        self.drawn[slot(render)] = Some((self.profiler.next_index(), submitted));
        for time in times {
            let ago = u64::from(time.renders_ago);
            if ago == 0 || ago >= RENDERS as u64 || ago > render {
                continue;
            }
            if let Some((frame, submitted)) = self.drawn[slot(render - ago)] {
                let gpu = GpuTiming::new(submitted, time.world, time.output);
                self.profiler.set_gpu(frame, gpu);
            }
        }
    }

    /// Keeps the windows' places in the file at `path`: reads them now, and
    /// writes them when they change.
    pub(crate) fn keep_places_in(&mut self, path: PathBuf) {
        self.places = places::read(&path);
        if let Some(ui) = &mut self.ui {
            restore(ui, &mut self.places);
            self.saved = ui.places_revision();
        }
        self.file = Some(path);
    }

    /// Before a frame's events reach `input`: holds the keys back if a
    /// debug text field had the keyboard when the last frame ended. The
    /// mouse is decided at each press and each turn of the wheel
    /// ([`DebugTools::handle`]), never by where the pointer rests, so that
    /// a button the game holds is not taken from it by the pointer passing
    /// over a window.
    pub(crate) fn start_frame(&mut self, input: &mut Input) {
        let keyboard = self.ui.as_ref().is_some_and(DebugUi::wants_keyboard);
        let mouse = self.held.mouse;
        self.hold(input, Capture { keyboard, mouse });
    }

    /// Takes in `event`, before `input` does. A press of a mouse button, or
    /// a turn of the wheel, is held back from the game while the debug UI
    /// holds the left button from a press on one of its windows, and, while
    /// nobody holds it, if the pointer is over a window the debug UI drew
    /// in the last frame; a press of the left button decides who holds it,
    /// until its release or a loss of the focus. The time controls take
    /// their keys here, before the frame's clock, so that they act in the
    /// frame they were pressed in.
    pub(crate) fn handle(&mut self, input: &mut Input, event: &Event) {
        match *event {
            Event::Key {
                key,
                pressed,
                repeat,
            } => self.time.key(key, pressed, repeat),
            Event::MouseButton {
                button,
                pressed: true,
            } => {
                let over = self.is_over(input.pointer());
                let mouse = self.left.unwrap_or(over);
                if button == MouseButton::Left && self.left.is_none() {
                    self.left = Some(over);
                }
                self.hold_mouse(input, mouse);
            }
            Event::MouseButton {
                button: MouseButton::Left,
                pressed: false,
            } => self.left = None,
            Event::Focused(false) => {
                self.left = None;
                self.time.focus_lost();
            }
            Event::MouseWheel(_) => {
                let mouse = self.left.unwrap_or_else(|| self.is_over(input.pointer()));
                self.hold_mouse(input, mouse);
            }
            Event::ScaleFactorChanged(factor) => self.scale = whole_scale(factor),
            _ => {}
        }
        self.events.handle(event);
    }

    /// Whether a window of the engine's whose content is `which` is open.
    fn is_open(&self, which: impl Fn(Content) -> bool) -> bool {
        self.windows
            .iter()
            .any(|window| window.open && which(window.content))
    }

    /// Whether `at` is over a window drawn in the last frame.
    fn is_over(&self, at: Vec2) -> bool {
        self.ui.as_ref().is_some_and(|ui| ui.is_over(at))
    }

    fn hold_mouse(&mut self, input: &mut Input, mouse: bool) {
        let keyboard = self.held.keyboard;
        self.hold(input, Capture { keyboard, mouse });
    }

    /// Holds `capture` back from the game's actions, if it changed. The
    /// debug UI then touches the capture only while it wants something,
    /// and once more to let go.
    fn hold(&mut self, input: &mut Input, capture: Capture) {
        if capture != self.held {
            input.set_capture(capture);
            self.held = capture;
        }
    }

    /// After the frame stages: toggles the windows whose keys were pressed,
    /// draws the open ones into the frame's UI, after the game's, and
    /// writes the places if they changed. With the inspector open, it
    /// outlines the entity it shows in the frame's sprites, drawn `alpha`
    /// of the way from the last tick's place to this one's. With the
    /// console open, it gives it the log's records since it last read
    /// them, and runs the commands typed, which reach `schedule`. With the
    /// debug drawing open, it draws the overlays ticked into the frame's
    /// sprites and texts, as the inspector's outline is; and while the
    /// game's time is not real, a label at the top-right says so.
    pub(crate) fn frame(
        &mut self,
        world: &World,
        schedule: &Schedule,
        virtual_resolution: UVec2,
        alpha: f32,
    ) {
        for event in self.events.events() {
            if let UiEvent::Key {
                key,
                pressed: true,
                repeat: false,
            } = *event
            {
                for window in &mut self.windows {
                    if window.toggle == key {
                        window.open = !window.open;
                    }
                }
            }
        }
        let wanted = self.windows.iter().any(|window| window.open) || !self.time.is_real();
        if self.ui.is_none() && !self.failed && wanted {
            match make_ui(world) {
                Ok((mut ui, font, white)) => {
                    restore(&mut ui, &mut self.places);
                    self.saved = ui.places_revision();
                    self.ui = Some(ui);
                    self.font = Some(font);
                    self.white = Some(white);
                }
                Err(cause) => {
                    error!("the debug UI cannot be shown: its font did not load: {cause}");
                    self.failed = true;
                }
            }
        }
        let windows = &self.windows;
        feed(&mut self.input, self.events.events(), |key| {
            windows.iter().any(|window| window.toggle == key)
        });
        self.events.clear();
        if self.is_open(|content| matches!(content, Content::Console)) {
            let console = &mut self.console;
            self.log_read = log::since(self.log_read, |record| console.log_record(record));
        }
        let Some(ui) = &mut self.ui else {
            // Nothing takes the events: keep only where they leave the
            // pointer and what they leave held.
            self.input.clear();
            return;
        };
        // The player's scale, from the settings, where there is one.
        let scale = world.resource::<crate::UiSettings>().scale;
        ui.style_mut().scale = UiScale::Free(scale.unwrap_or(self.scale));
        let output = world.resource::<WindowSize>().0;
        // The time controls' label goes first, so that a window over the
        // corner is drawn in front of it.
        if !self.time.is_real()
            && let (Some(white), Some(font)) = (self.white, self.font)
        {
            self.label.clear();
            // Writing to a `String` cannot fail.
            let _ = write!(self.label, "{}", self.time);
            let style = *ui.style();
            let size = UiSpace::new(style.scale, virtual_resolution, output).size;
            corner(
                &mut world.resource_mut::<RenderFrame>().ui,
                &self.label,
                (font, white),
                &style,
                size,
            );
        }
        let mut frame = ui.frame(&mut self.input, virtual_resolution, output);
        for window in self.windows.iter().filter(|window| window.open) {
            let (title, first) = (window.title, window.first);
            match window.content {
                Content::Game(add) => frame.window(title, first, |ui| add(world, ui)),
                Content::Stats => {
                    let stats = stats(world, &self.profiler, self.last_work);
                    frame.window(title, first, |ui| stats.show(ui))
                }
                Content::Profiler => frame.window(title, first, |ui| {
                    if self.profiler.show(ui).write_trace {
                        // The note and the log say how it went.
                        let _ = write_trace(&mut self.profiler, self.logs.as_deref(), None);
                    }
                }),
                Content::Inspector => frame.window(title, first, |ui| {
                    let response = loading_context(world, INSPECTED, Scope::Level, |mut cx| {
                        self.inspector.show(world, ui, &mut cx)
                    });
                    if response.save_prefab {
                        save_prefab(world, &mut self.inspector, self.logs.as_deref());
                    }
                }),
                Content::Console => frame.window(title, first, |ui| {
                    let mut reach = Reach {
                        world,
                        schedule,
                        profiler: &mut self.profiler,
                        logs: self.logs.as_deref(),
                        time: &mut self.time,
                        level: &mut self.level,
                        quit: &mut self.quit,
                    };
                    self.console
                        .show(ui, |command, args, out| run(&mut reach, command, args, out));
                }),
                Content::DebugDraw => frame.window(title, first, |ui| self.overlays.show(ui)),
            };
        }
        frame.finish(&mut world.resource_mut::<RenderFrame>().ui);
        let drawing = self
            .windows
            .iter()
            .any(|window| window.open && matches!(window.content, Content::DebugDraw));
        if drawing && let (Some(white), Some(font)) = (self.white, self.font) {
            self.overlays
                .draw(world, white, font, virtual_resolution, alpha);
        }
        let inspecting = self
            .windows
            .iter()
            .any(|window| window.open && matches!(window.content, Content::Inspector));
        if inspecting
            && let (Some(entity), Some(white)) = (self.inspector.selected(), self.white)
            && let Some(rect) = outlined(world, entity, alpha)
        {
            outline(
                &mut world.resource_mut::<RenderFrame>().sprites,
                white,
                rect,
            );
        }
        if let Some(path) = &self.file
            && ui.places_revision() != self.saved
        {
            self.saved = ui.places_revision();
            save(path, ui);
        }
    }
}

/// The stats overlay's numbers: the frame rate and the last frame's
/// processor time from the profiler, the rest from the world's resources:
/// this frame's ticks, and the last frame's drawing and allocations.
fn stats(world: &World, profiler: &Profiler, work: Duration) -> Stats {
    let ticks = *world.resource::<TickStats>();
    let drawn = *world.resource::<FrameStats>();
    let allocs = world.resource::<FrameAllocs>().0;
    let audio = world.resource::<Audio>().stats();
    let mut stats = Stats::default();
    stats.frame_rate = profiler.frame_rate();
    stats.frame_time = work;
    stats.tick_time = ticks.time;
    stats.ticks = ticks.ticks;
    stats.draw_calls = drawn.batches + drawn.chunks + drawn.ui_batches;
    stats.sprites = drawn.sprites;
    stats.entities = world.entity_count();
    stats.allocations = allocs.allocations;
    stats.allocated_bytes = allocs.bytes;
    stats.voices = audio.active_voices;
    stats.underruns = audio.underruns;
    stats
}

/// Writes the prefab the inspector made of the entity it shows into
/// `folder`'s [`PREFABS`], as `<name>.jsonc`, and says where in its note
/// and the log; or why not. It leaves out [`NOT_IN_PREFABS`]. A name whose
/// file in a game's assets, `data/prefabs/<name>.jsonc`, is no asset path,
/// such as a Windows device's (`nul`), is not written.
fn save_prefab(world: &World, inspector: &mut Inspector, folder: Option<&Path>) {
    let Some(folder) = folder else {
        inspector.set_note("Prefabs are written only by App::run.");
        return;
    };
    let asset = format!("data/prefabs/{}.jsonc", inspector.prefab_name());
    if let Err(cause) = AssetPath::new(&asset) {
        warn!("the inspector's prefab is not written: {asset}: {cause}");
        inspector.set_note(format_args!("Not written: {asset}: {cause}"));
        return;
    }
    let text = reflect_context(world, INSPECTED, |mut cx| {
        inspector.prefab(world, &NOT_IN_PREFABS, &mut cx)
    });
    let Some(text) = text else {
        return;
    };
    let folder = folder.join(PREFABS);
    let path = folder.join(format!("{}.jsonc", inspector.prefab_name()));
    match fs::create_dir_all(&folder).and_then(|()| fs::write(&path, text)) {
        Ok(()) => {
            info!("the inspector saved the prefab {}", path.display());
            inspector.set_note(format_args!("Wrote {}", path.display()));
        }
        Err(cause) => {
            warn!(
                "the inspector's prefab is not written: {} cannot be: {cause}",
                path.display()
            );
            inspector.set_note(format_args!("Not written: {cause}"));
        }
    }
}

/// What the inspector outlines of `entity`, in the world: its sprite,
/// unrotated, where it is drawn, `alpha` of the way from the last tick's
/// place, by its parent's if it has one, as `nadir::extract_sprites` draws
/// it; or its collider's bounds; or a box of [`MARK`] around its place.
/// `None` for an entity with no place.
fn outlined(world: &World, entity: Entity, alpha: f32) -> Option<Rect> {
    let transforms = world.read::<Transform>();
    let previous = world.read::<PrevTransform>();
    let parents = world.read::<Parent>();
    let drawn_at = |entity| {
        let current = transforms.get(entity)?.position;
        Some(
            previous
                .get(entity)
                .map_or(current, |previous| previous.0.position.lerp(current, alpha)),
        )
    };
    if let Some(sprite) = world.read::<Sprite>().get(entity) {
        let parented = if parents.contains(entity) {
            crate::glue::root(&parents, entity)
                .and_then(|(root, offset)| Some(drawn_at(root)? + offset))
        } else {
            None
        };
        if let Some(position) = parented.or_else(|| drawn_at(entity)) {
            // Mirrored about its pivot, as it is drawn.
            let mut pivot = sprite.pivot;
            if sprite.flip.x {
                pivot.x = 1.0 - pivot.x;
            }
            if sprite.flip.y {
                pivot.y = 1.0 - pivot.y;
            }
            let min = position - pivot * sprite.size;
            return Some(Rect::new(min, min + sprite.size));
        }
    }
    let position = drawn_at(entity)?;
    if let Some(collider) = world.read::<Collider>().get(entity) {
        return Some(collider.0.bounds(position));
    }
    Some(Rect::new(
        position - Vec2::splat(MARK),
        position + Vec2::splat(MARK),
    ))
}

/// Pushes a line a world unit wide around `rect`, outside it, in front of
/// every sprite: four of the white texel, in [`OUTLINE`].
fn outline(sprites: &mut Vec<SpriteDraw>, white: Handle<Texture>, rect: Rect) {
    let (min, max) = (rect.min.min(rect.max), rect.min.max(rect.max));
    let edges = [
        Rect::new(min - Vec2::ONE, Vec2::new(max.x + 1.0, min.y)),
        Rect::new(Vec2::new(min.x - 1.0, max.y), max + Vec2::ONE),
        Rect::new(Vec2::new(min.x - 1.0, min.y), Vec2::new(min.x, max.y)),
        Rect::new(Vec2::new(max.x, min.y), Vec2::new(max.x + 1.0, max.y)),
    ];
    for edge in edges {
        let mut draw = SpriteDraw::new(white, edge.min, edge.max - edge.min);
        draw.pivot = Vec2::ZERO;
        draw.color = OUTLINE;
        draw.layer = u8::MAX;
        draw.sort = SortMode::Fixed(u32::MAX);
        sprites.push(draw);
    }
}

/// Writes the profiler's trace to `path`, its folder made if need be, or
/// with none into `folder`, as `trace-<UTC time>.json`, and says where in
/// its note and the log, and returns it; or why not, which it returns too.
fn write_trace(
    profiler: &mut Profiler,
    folder: Option<&Path>,
    path: Option<&Path>,
) -> Result<PathBuf, String> {
    let path = match (path, folder) {
        (Some(path), _) => path.to_path_buf(),
        (None, Some(folder)) => folder.join(format!("trace-{}.json", DateTime::now().file_stamp())),
        (None, None) => {
            let note = "Traces are written only by App::run.";
            profiler.set_note(note);
            return Err(note.to_string());
        }
    };
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let written = parent
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&path, profiler.trace_json()));
    match written {
        Ok(()) => {
            info!("the profiler's trace is in {}", path.display());
            profiler.set_note(format_args!("Wrote {}", path.display()));
            Ok(path)
        }
        Err(cause) => {
            warn!(
                "the profiler's trace is not written: {} cannot be: {cause}",
                path.display()
            );
            let note = format!("Not written: {cause}");
            profiler.set_note(&note);
            Err(note)
        }
    }
}

/// What the engine's console commands reach beside the world.
struct Reach<'a> {
    world: &'a World,
    schedule: &'a Schedule,
    profiler: &'a mut Profiler,
    logs: Option<&'a Path>,
    time: &'a mut time::Time,
    level: &'a mut Option<String>,
    quit: &'a mut bool,
}

/// Runs a command of the console, its arguments checked against its usage.
fn run(
    reach: &mut Reach<'_>,
    command: Command,
    args: &Args<'_>,
    out: &mut ConsoleOut<'_>,
) -> Result<(), ConsoleError> {
    let world = reach.world;
    let builtin = match command {
        Command::Game(command) => return command(world, args, out),
        Command::Engine(builtin) => builtin,
    };
    match builtin {
        Builtin::Schedule => {
            for system in reach.schedule.describe() {
                out.print(system);
            }
        }
        Builtin::Spawn => spawn(world, args, out)?,
        Builtin::Tp => teleport(world, args, out)?,
        Builtin::Timescale => {
            let scale = args.f32(0)?;
            if !TIME_SCALES.contains(&scale) {
                return Err(ConsoleError::InvalidArgument {
                    usage: args.usage().to_string(),
                    argument: "<scale>".to_string(),
                    found: args.str(0)?.to_string(),
                    expected: "from 0.25 to 4".to_string(),
                });
            }
            reach.time.scale = scale;
            out.print(reach.time.scale_note());
        }
        Builtin::Level => *reach.level = Some(args.str(0)?.to_string()),
        Builtin::Reload => {
            let reads = world.resource_mut::<AssetServer>().reload_all();
            out.print(format_args!(
                "reading {reads} assets' files again: each is swapped in once read"
            ));
        }
        Builtin::Profile => {
            let path = args.get(1).map(Path::new);
            let path =
                write_trace(reach.profiler, reach.logs, path).map_err(ConsoleError::Failed)?;
            out.print(format_args!("Wrote {}", path.display()));
        }
        Builtin::Log => {
            let target = args.str(1)?;
            let level = match args.str(2)? {
                "error" => Level::Error,
                "warn" => Level::Warn,
                "info" => Level::Info,
                "debug" => Level::Debug,
                "trace" => Level::Trace,
                other => return Err(ConsoleError::Failed(format!("no log level `{other}`"))),
            };
            log::set_target_level(target, Some(level));
            out.print(format_args!(
                "`{target}` logs {level} and more severe records"
            ));
        }
        Builtin::Quit => {
            *reach.quit = true;
            out.print("quitting");
        }
    }
    Ok(())
}

/// `spawn <prefab> [x] [y]`: loads the prefab into the library, if it is
/// not there, in the level's scope, and spawns it through the world's
/// commands at (x, y), or where the camera looks.
fn spawn(world: &World, args: &Args<'_>, out: &mut ConsoleOut<'_>) -> Result<(), ConsoleError> {
    let name = args.str(0)?;
    let at = match args.len() {
        3 => Vec2::new(args.f32(1)?, args.f32(2)?),
        1 => world.resource::<RenderFrame>().camera.position,
        _ => {
            return Err(ConsoleError::Failed(format!(
                "[x] and [y] go together: {}",
                args.usage()
            )));
        }
    };
    let failed =
        |cause: PrefabError| ConsoleError::Failed(format!("`{name}` is not spawned: {cause}"));
    let registry = world.resource::<TypeRegistry>();
    world
        .resource_mut::<PrefabLibrary>()
        .load(
            &mut world.resource_mut::<AssetServer>(),
            &registry,
            name,
            Scope::Level,
        )
        .map_err(failed)?;
    let library = world.resource::<PrefabLibrary>();
    let mut commands = world.commands();
    let entity = loading_context(world, TYPED, Scope::Level, |mut cx| {
        library.spawn(name, &[], &registry, &mut commands, &mut cx)
    })
    .map_err(failed)?;
    commands.insert(entity, Transform::at(at));
    out.print(format_args!(
        "spawned `{name}` as {}v{} at ({}, {}), from the next tick or frame",
        entity.index(),
        entity.generation(),
        at.x,
        at.y
    ));
    Ok(())
}

/// `tp <x> <y>`: moves each entity with [`Player`] and a [`Transform`] to
/// (x, y), with no step between: its last place too.
fn teleport(world: &World, args: &Args<'_>, out: &mut ConsoleOut<'_>) -> Result<(), ConsoleError> {
    let at = Vec2::new(args.f32(0)?, args.f32(1)?);
    let players = world.read::<Player>();
    let mut transforms = world.write::<Transform>();
    let mut previous = world.write::<PrevTransform>();
    let mut moved = 0;
    for (entity, _) in players.iter() {
        if let Some(transform) = transforms.get_mut(entity) {
            transform.position = at;
            if let Some(previous) = previous.get_mut(entity) {
                previous.0.position = at;
            }
            moved += 1;
        }
    }
    if moved == 0 {
        return Err(ConsoleError::Failed(
            "nothing moved: no entity has `Player` and a `Transform`".to_string(),
        ));
    }
    out.print(format_args!(
        "moved {moved} player(s) to ({}, {})",
        at.x, at.y
    ));
    Ok(())
}

/// Draws `text` on a panel at the top-right of a UI `size` units large, at
/// `style`'s scale, in the debug UI's `font` and `white` texel: the time
/// controls' label, pushed before the windows, which cover it. It puts the
/// list's view back.
fn corner(
    list: &mut UiList,
    text: &str,
    (font, white): (Handle<Font>, Handle<Texture>),
    style: &Style,
    size: Vec2,
) {
    let pad = style.padding;
    let width = text.chars().count() as f32 * font::ADVANCE as f32;
    let max = Vec2::new(
        size.x - CORNER,
        CORNER + font::LINE_HEIGHT as f32 + 2.0 * pad,
    );
    let min = Vec2::new(max.x - width - 2.0 * pad, CORNER);
    let before = list.view();
    list.set_view(UiView::new(style.scale));
    let mut panel = SpriteDraw::new(white, min, max - min);
    panel.pivot = Vec2::ZERO;
    panel.color = style.panel;
    panel.layer = u8::MAX;
    panel.sort = SortMode::Fixed(u32::MAX);
    list.sprite(panel);
    let mut draw = TextDraw::new(font, min + Vec2::splat(pad));
    draw.layout.markup = false;
    draw.color = style.text;
    draw.layer = u8::MAX;
    draw.sort = SortMode::Fixed(u32::MAX);
    list.text(draw, text);
    list.set_view(before);
}

/// The debug UI, in the engine's font, which it loads in the global scope,
/// the font, and the white texel it draws with.
fn make_ui(world: &World) -> Result<(DebugUi, Handle<Font>, Handle<Texture>), AssetError> {
    let mut server = world.resource_mut::<AssetServer>();
    let font = Font::load_now_in(
        &mut server,
        font::FONT,
        Scope::Global,
        &mut FinalizeCtx::new(),
    )?;
    let white = server.load_in::<Texture>(font::WHITE, Scope::Global);
    let metrics = server
        .get(font)
        .map(|loaded| FontMetrics::new(loaded.clone()))
        .ok_or_else(|| AssetError::Invalid("the engine's font is not loaded".to_string()))?;
    Ok((DebugUi::new(font, metrics, white), font, white))
}

/// Gives `ui` the places read from the file, by their names: each a
/// window's explicit ID or title, which `set_place` would cut at a `##` it
/// holds unless it comes after one.
fn restore(ui: &mut DebugUi, places: &mut Vec<places::Place>) {
    for place in places.drain(..) {
        ui.set_place(&format!("##{}", place.name), place.rect, place.collapsed);
    }
}

/// Feeds the debug UI's input with `events`, in their order, but the
/// character a key that `toggles` a window types as it is pressed: each
/// system sends a key's press before its text (X11's `XLookupString`, a
/// `WM_CHAR` after its `WM_KEYDOWN`, `insertText:` after `keyDown:`), so
/// that is the text after the press, before another key's. No debug field
/// then takes the backtick that shows or hides the console.
fn feed(input: &mut DebugInput, events: &[UiEvent], toggles: impl Fn(Key) -> bool) {
    // The character a window's toggle types as it is pressed (the
    // backtick's), which no debug field takes: the next text, unless
    // another key is pressed first.
    let mut toggled = false;
    for event in events {
        match *event {
            UiEvent::PointerMoved(position) => input.pointer_moved(position),
            UiEvent::Button { button, pressed } => input.button(button, pressed),
            UiEvent::Wheel(lines) => input.wheel(lines),
            UiEvent::Key {
                key,
                pressed,
                repeat,
            } => {
                if pressed {
                    toggled = toggles(key);
                }
                input.key(key, pressed, repeat);
            }
            UiEvent::Text(_) if toggled => toggled = false,
            UiEvent::Text(character) => input.text(character.encode_utf8(&mut [0; 4])),
            UiEvent::FocusLost => input.focus_lost(),
        }
    }
}

/// Writes the places of `ui` to `path`, or says why not.
fn save(path: &Path, ui: &DebugUi) {
    if let Err(cause) = places::write(path, &places::to_text(ui.places())) {
        warn!(
            "the debug UI's windows are not kept: {} cannot be written: {cause}",
            path.display()
        );
    }
}

/// The free scale for a window's scale factor: the nearest whole number,
/// so that the pixel font stays sharp, and at least 1.
fn whole_scale(factor: f32) -> f32 {
    if factor.is_finite() {
        factor.round().max(1.0)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::{env, fs, process};

    use nadir_codec::qoi;
    use nadir_core::time::Tick;
    use nadir_core::{GenIndex, Handle, MouseButton, Vec2};
    use nadir_ecs::{FixedStage, FrameStage};
    use nadir_input::{
        ActionKind, ActionMap, ActionSet, ActionState, Binding, InputPlayer, InputRecorder,
    };
    use nadir_render::{SpriteDraw, UiDraw};

    use super::*;
    use crate::{App, Config};

    /// One tick's length, as a frame.
    const TICK: Duration = Duration::from_nanos(16_666_667);
    const OUTPUT: UVec2 = UVec2::new(640, 360);
    /// The window: its title bar from y 10 to 24, its first row's middle
    /// at y 33 and its second's at y 49, both from x 12 (the engine's font
    /// is 10 a line, so a row is 14 high, 16 apart).
    const WINDOW: Rect = Rect::new(Vec2::new(10.0, 10.0), Vec2::new(210.0, 130.0));
    const FIELD: Vec2 = Vec2::new(50.0, 33.0);
    const BUTTON: Vec2 = Vec2::new(20.0, 49.0);
    const TITLE: Vec2 = Vec2::new(60.0, 17.0);
    const OUTSIDE: Vec2 = Vec2::new(400.0, 300.0);

    /// What the debug window keeps.
    #[derive(Default)]
    struct Notes {
        text: String,
        clicks: u32,
    }

    fn notes(world: &World, ui: &mut Ui<'_>) {
        let mut notes = world.resource_mut::<Notes>();
        ui.text_input("Name", &mut notes.text);
        if ui.button("Click").clicked {
            notes.clicks += 1;
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Act {
        Fire,
        Jump,
        Aim,
    }

    impl ActionSet for Act {
        const ALL: &'static [Self] = &[Act::Fire, Act::Jump, Act::Aim];

        fn index(self) -> usize {
            self as usize
        }

        fn name(self) -> &'static str {
            match self {
                Act::Fire => "fire",
                Act::Jump => "jump",
                Act::Aim => "aim",
            }
        }

        fn kind(self) -> ActionKind {
            ActionKind::Button
        }
    }

    /// The presses of each action the ticks saw, the ticks Jump was held,
    /// Aim's releases, and whether Fire and Aim are held now.
    #[derive(Default)]
    struct Seen {
        fire: u32,
        jump: u32,
        jump_held: u32,
        aim: u32,
        aim_released: u32,
        fire_down: bool,
        aim_down: bool,
    }

    fn count(world: &World, _: &Tick) {
        let state = world.resource::<ActionState<Act>>();
        let mut seen = world.resource_mut::<Seen>();
        seen.fire += u32::from(state.just_pressed(Act::Fire));
        seen.jump += u32::from(state.just_pressed(Act::Jump));
        seen.jump_held += u32::from(state.pressed(Act::Jump));
        seen.aim += u32::from(state.just_pressed(Act::Aim));
        seen.aim_released += u32::from(state.just_released(Act::Aim));
        seen.fire_down = state.pressed(Act::Fire);
        seen.aim_down = state.pressed(Act::Aim);
    }

    fn actions() -> ActionMap<Act> {
        let mut map = ActionMap::<Act>::new().expect("a valid set");
        map.bind(Act::Fire, 0, Binding::MouseButton(MouseButton::Left))
            .expect("a button");
        map.bind(Act::Jump, 0, Binding::Key(Key::Space))
            .expect("a key");
        map.bind(Act::Aim, 0, Binding::MouseButton(MouseButton::Right))
            .expect("a button");
        map
    }

    /// A game with the window `Tools` on F8, which also draws a box of its
    /// own UI each frame.
    fn app() -> App {
        let mut app = App::new(Config::default())
            .insert_resource(Notes::default())
            .insert_resource(Seen::default())
            .add_actions(actions())
            .add_fixed_system(FixedStage::Gameplay, "count", count)
            .add_frame_system(FrameStage::Ui, "game_ui", |world, _| {
                let mut sprite = SpriteDraw::new(game_box(), Vec2::ZERO, Vec2::ONE);
                sprite.pivot = Vec2::ZERO;
                world.resource_mut::<RenderFrame>().ui.sprite(sprite);
            })
            .add_debug_window(Key::F8, "Tools", WINDOW, notes);
        *app.world_mut().resource_mut::<WindowSize>() = WindowSize(OUTPUT);
        app
    }

    fn game_box() -> Handle<Texture> {
        Handle::new(GenIndex::new(99, std::num::NonZeroU32::MIN))
    }

    fn key(key: Key, pressed: bool) -> Event {
        Event::Key {
            key,
            pressed,
            repeat: false,
        }
    }

    fn tap(key: Key) -> [Event; 2] {
        [self::key(key, true), self::key(key, false)]
    }

    fn click(at: Vec2) -> [Event; 3] {
        [
            Event::MouseMoved(at),
            Event::MouseButton {
                button: MouseButton::Left,
                pressed: true,
            },
            Event::MouseButton {
                button: MouseButton::Left,
                pressed: false,
            },
        ]
    }

    /// The texts the frame's UI holds.
    fn texts(app: &App) -> Vec<String> {
        let frame = app.world().resource::<RenderFrame>();
        frame
            .ui
            .iter()
            .filter_map(|(draw, _)| match draw {
                UiDraw::Text(_, text) => Some(text.to_string()),
                _ => None,
            })
            .collect()
    }

    fn seen(app: &App) -> (u32, u32) {
        let seen = app.world().resource::<Seen>();
        (seen.fire, seen.jump)
    }

    #[test]
    fn a_window_shows_on_its_key_after_the_games_ui_and_hides_on_it_again() {
        let mut app = app();
        app.update(TICK, &[]);
        assert_eq!(app.world().resource::<RenderFrame>().ui.len(), 1);
        assert!(texts(&app).is_empty());
        app.update(TICK, &tap(Key::F8));
        {
            let frame = app.world().resource::<RenderFrame>();
            let mut items = frame.ui.iter();
            // The game's box first, then the debug window, at its own scale.
            assert!(matches!(
                items.next(),
                Some((UiDraw::Sprite(sprite), _)) if sprite.texture == game_box()
            ));
            assert!(items.len() > 4);
            assert!(items.all(|(_, view)| view.scale() == UiScale::Free(1.0)));
        }
        // The collapse box, the title, the field's text and its label, and
        // the button.
        assert_eq!(texts(&app), ["-", "Tools", "", "Name", "Click"]);
        // A repeat of the key does not toggle it.
        app.update(
            TICK,
            &[Event::Key {
                key: Key::F8,
                pressed: true,
                repeat: true,
            }],
        );
        assert_eq!(texts(&app), ["-", "Tools", "", "Name", "Click"]);
        app.update(TICK, &[key(Key::F8, false)]);
        app.update(TICK, &tap(Key::F8));
        assert!(texts(&app).is_empty());
        assert_eq!(app.world().resource::<RenderFrame>().ui.len(), 1);
    }

    #[test]
    fn the_games_actions_miss_what_the_debug_window_takes() {
        let mut app = app();
        app.update(TICK, &tap(Key::F8));
        // A click outside every window is the game's.
        app.update(TICK, &click(OUTSIDE));
        assert_eq!(seen(&app), (1, 0));
        // On the button: the window's.
        app.update(TICK, &click(BUTTON));
        assert_eq!(seen(&app), (1, 0));
        assert_eq!(app.world().resource::<Notes>().clicks, 1);
        // The pointer comes over the window and clicks in one frame: the
        // window's, as the press was over it.
        app.update(TICK, &[Event::MouseMoved(OUTSIDE)]);
        app.update(TICK, &click(BUTTON));
        assert_eq!(seen(&app), (1, 0));
        assert_eq!(app.world().resource::<Notes>().clicks, 2);
        // It leaves the window and clicks in one frame: the game's.
        app.update(TICK, &click(OUTSIDE));
        assert_eq!(seen(&app), (2, 0));
        // The wheel over the window is the window's.
        app.update(
            TICK,
            &[
                Event::MouseMoved(BUTTON),
                Event::MouseWheel(Vec2::new(0.0, 1.0)),
            ],
        );
        assert_eq!(app.world().resource::<Input>().wheel(), Vec2::ZERO);

        // Typed into the field, the keys are the window's, and so is the
        // Escape that leaves it.
        app.update(TICK, &click(FIELD));
        let mut typing = vec![key(Key::A, true), Event::Text('a'), key(Key::A, false)];
        typing.extend([
            key(Key::Space, true),
            Event::Text(' '),
            key(Key::Space, false),
        ]);
        typing.push(Event::Text('é'));
        app.update(TICK, &typing);
        assert_eq!(app.world().resource::<Notes>().text, "a é");
        assert_eq!(seen(&app), (2, 0));
        app.update(TICK, &tap(Key::Escape));
        app.update(TICK, &tap(Key::Space));
        assert_eq!(seen(&app), (2, 1));
        assert_eq!(app.world().resource::<Notes>().text, "a é");

        // Hidden, it holds nothing back from the next frame on.
        app.update(TICK, &[Event::MouseMoved(BUTTON)]);
        assert!(app.world().resource::<Input>().capture().mouse);
        app.update(TICK, &tap(Key::F8));
        app.update(TICK, &click(BUTTON));
        assert_eq!(app.world().resource::<Input>().capture(), Capture::NONE);
        assert_eq!(seen(&app), (3, 1));
        assert_eq!(app.world().resource::<Notes>().clicks, 2);
    }

    fn button(button: MouseButton, pressed: bool) -> Event {
        Event::MouseButton { button, pressed }
    }

    #[test]
    fn a_release_and_a_press_in_one_frame_go_where_each_happened() {
        let mut app = app();
        app.update(TICK, &tap(Key::F8));
        // The game holds Left; in one frame it is released, and pressed and
        // released again over the window's button: the second press is
        // the window's alone.
        app.update(
            TICK,
            &[Event::MouseMoved(OUTSIDE), button(MouseButton::Left, true)],
        );
        assert_eq!(seen(&app), (1, 0));
        app.update(
            TICK,
            &[
                button(MouseButton::Left, false),
                Event::MouseMoved(BUTTON),
                button(MouseButton::Left, true),
                button(MouseButton::Left, false),
            ],
        );
        for _ in 0..2 {
            app.update(TICK, &[]);
        }
        assert_eq!(seen(&app), (1, 0));
        assert_eq!(app.world().resource::<Notes>().clicks, 1);
        // The window holds Left by its title bar; in one frame it is
        // released, and pressed again outside: that press is the game's.
        app.update(
            TICK,
            &[Event::MouseMoved(TITLE), button(MouseButton::Left, true)],
        );
        app.update(
            TICK,
            &[
                button(MouseButton::Left, false),
                Event::MouseMoved(OUTSIDE),
                button(MouseButton::Left, true),
                button(MouseButton::Left, false),
            ],
        );
        assert_eq!(seen(&app), (2, 0));
    }

    #[test]
    fn a_button_the_game_holds_stays_its_as_the_pointer_passes_a_window() {
        let mut app = app();
        app.update(TICK, &tap(Key::F8));
        // Right held from outside, then over the window and away again.
        app.update(
            TICK,
            &[Event::MouseMoved(OUTSIDE), button(MouseButton::Right, true)],
        );
        for at in [BUTTON, FIELD, OUTSIDE] {
            app.update(TICK, &[Event::MouseMoved(at)]);
            app.update(TICK, &[]);
        }
        let seen = app.world().resource::<Seen>();
        assert_eq!((seen.aim, seen.aim_released, seen.aim_down), (1, 0, true));
    }

    #[test]
    fn while_the_window_holds_left_another_button_is_its_too() {
        let mut app = app();
        app.update(TICK, &tap(Key::F8));
        // Left pressed on the window, then Right outside in the same frame.
        app.update(
            TICK,
            &[
                Event::MouseMoved(TITLE),
                button(MouseButton::Left, true),
                Event::MouseMoved(OUTSIDE),
                button(MouseButton::Right, true),
            ],
        );
        for _ in 0..3 {
            app.update(TICK, &[]);
        }
        app.update(
            TICK,
            &[
                button(MouseButton::Right, false),
                button(MouseButton::Left, false),
            ],
        );
        let seen = app.world().resource::<Seen>();
        assert_eq!((seen.aim, seen.aim_down), (0, false));
        assert_eq!(seen.fire, 0);
    }

    #[test]
    fn opening_a_window_over_a_button_the_game_holds_leaves_it_the_game_s() {
        let mut app = app();
        // Left held by the game where the window will open.
        app.update(
            TICK,
            &[Event::MouseMoved(BUTTON), button(MouseButton::Left, true)],
        );
        app.update(TICK, &tap(Key::F8));
        for _ in 0..3 {
            app.update(TICK, &[Event::MouseMoved(FIELD)]);
        }
        let seen = app.world().resource::<Seen>();
        assert_eq!((seen.fire, seen.fire_down), (1, true));
    }

    #[test]
    fn a_replay_plays_while_a_debug_field_has_the_keyboard() {
        // A recording of Jump held for 12 ticks.
        let mut recorder = InputRecorder::<Act>::new().expect("a valid set");
        let map = actions();
        let mut input = Input::new();
        let mut state = ActionState::new();
        input.handle(&key(Key::Space, true));
        for _ in 0..12 {
            input.latch(&map, &mut state);
            recorder.record(&state);
        }
        let bytes: &'static [u8] = Box::leak(recorder.into_bytes().into_boxed_slice());
        let player = InputPlayer::<Act>::new(bytes).expect("a replay of Act");
        let mut app = app().play_replay(player);
        app.update(TICK, &tap(Key::F8));
        app.update(TICK, &click(FIELD));
        for _ in 0..4 {
            app.update(TICK, &[]);
        }
        // The field has the keyboard, and the replay's Jump went on, tick
        // after tick.
        assert!(app.world().resource::<Input>().capture().keyboard);
        assert_eq!(app.world().resource::<Seen>().jump_held, 6);
    }

    #[test]
    fn a_steady_frame_of_an_open_debug_window_allocates_nothing() {
        let mut app = app();
        app.world_mut().resource_mut::<Notes>().text = "hero".to_string();
        app.update(TICK, &tap(Key::F8));
        let moves = [Event::MouseMoved(BUTTON), Event::MouseMoved(OUTSIDE)];
        for index in 0..10 {
            app.update(TICK, &moves[index % 2..index % 2 + 1]);
        }
        let start = nadir_platform::thread_alloc_counts();
        for index in 0..20 {
            app.update(TICK, &moves[index % 2..index % 2 + 1]);
        }
        let allocated = nadir_platform::thread_alloc_counts().since(start);
        assert_eq!(texts(&app), ["-", "Tools", "hero", "Name", "Click"]);
        assert_eq!(allocated, nadir_platform::AllocCounts::default());
    }

    /// A debug UI driven by hand over an app's world, so that its places
    /// can be read.
    struct Rig {
        app: App,
        tools: DebugTools,
    }

    impl Rig {
        fn new() -> Self {
            let mut tools = DebugTools::new();
            tools.add(Key::F8, "Tools##tools", WINDOW, |_, ui| ui.label("Hello"));
            let mut app = App::new(Config::default());
            *app.world_mut().resource_mut::<WindowSize>() = WindowSize(OUTPUT);
            Self { app, tools }
        }

        fn frame(&mut self, events: &[Event]) {
            let world = self.app.world();
            {
                let mut input = world.resource_mut::<Input>();
                self.tools.start_frame(&mut input);
                for event in events {
                    self.tools.handle(&mut input, event);
                    input.handle(event);
                }
            }
            world.resource_mut::<RenderFrame>().ui.clear();
            let schedule = self.app.schedule();
            self.tools.frame(world, schedule, UVec2::new(480, 270), 0.0);
        }

        fn place(&self) -> Rect {
            let ui = self.tools.ui.as_ref().expect("made");
            let mut places = ui.places();
            places.next().expect("a window").rect
        }
    }

    #[test]
    fn a_window_s_place_is_written_when_it_moves_and_read_on_the_next_run() {
        let dir = env::temp_dir().join(format!("nadir-engine-debug-places-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join(FILE_NAME);
        let mut rig = Rig::new();
        rig.tools.keep_places_in(path.clone());
        rig.frame(&tap(Key::F8));
        assert_eq!(rig.place(), WINDOW);
        assert!(!path.exists(), "nothing moved yet");
        // Dragged by its title bar 100 to the right and 100 down.
        let press = Event::MouseButton {
            button: MouseButton::Left,
            pressed: true,
        };
        let release = Event::MouseButton {
            button: MouseButton::Left,
            pressed: false,
        };
        rig.frame(&[Event::MouseMoved(TITLE), press]);
        rig.frame(&[Event::MouseMoved(TITLE + Vec2::splat(100.0))]);
        rig.frame(&[release]);
        let moved = Rect::new(
            WINDOW.min + Vec2::splat(100.0),
            WINDOW.max + Vec2::splat(100.0),
        );
        assert_eq!(rig.place(), moved);
        let text = fs::read_to_string(&path).expect("the places were written");
        assert!(text.contains("\"tools\""), "{text}");

        // The next run puts it there, the first time it shows.
        let mut next = Rig::new();
        next.tools.keep_places_in(path.clone());
        next.frame(&tap(Key::F8));
        assert_eq!(next.place(), moved);
        let panel = next
            .app
            .world()
            .resource::<RenderFrame>()
            .ui
            .iter()
            .find_map(|(draw, _)| match draw {
                UiDraw::Sprite(sprite) => Some(sprite.position),
                _ => None,
            });
        assert_eq!(panel, Some(moved.min));
        fs::remove_dir_all(&dir).expect("remove the folder");
    }

    #[test]
    fn an_explicit_id_with_hashes_in_it_gets_its_place_back() {
        let dir = env::temp_dir().join(format!("nadir-engine-debug-hashes-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("the folder");
        let path = dir.join(FILE_NAME);
        fs::write(
            &path,
            r#"{ "version": 1, "windows": [
                { "name": "a##b", "x": 300, "y": 40, "w": 100, "h": 60, "collapsed": false } ] }"#,
        )
        .expect("a places file");
        let mut tools = DebugTools::new();
        // The window `a##b`: its title is `Odd`.
        tools.add(Key::F8, "Odd##a##b", WINDOW, |_, ui| ui.label("odd"));
        let mut rig = Rig {
            app: Rig::new().app,
            tools,
        };
        rig.tools.keep_places_in(path);
        rig.frame(&tap(Key::F8));
        let ui = rig.tools.ui.as_ref().expect("made");
        let places: Vec<_> = ui.places().map(|place| (place.title, place.rect)).collect();
        assert_eq!(
            places,
            [(
                "a##b",
                Rect::from_min_size(Vec2::new(300.0, 40.0), Vec2::new(100.0, 60.0))
            )]
        );
        fs::remove_dir_all(&dir).expect("remove the folder");
    }

    #[test]
    fn the_scale_is_the_window_s_whole_and_at_least_one() {
        assert_eq!(
            [0.5, 1.0, 1.25, 1.5, 2.0, 2.6, f32::NAN, f32::INFINITY].map(whole_scale),
            [1.0, 1.0, 1.0, 2.0, 2.0, 3.0, 1.0, 1.0]
        );
        let mut rig = Rig::new();
        rig.frame(&[Event::ScaleFactorChanged(2.0), tap(Key::F8)[0]]);
        let frame = rig.app.world().resource::<RenderFrame>();
        let (_, view) = frame.ui.iter().next().expect("the window");
        assert_eq!(view.scale(), UiScale::Free(2.0));
    }

    #[test]
    fn the_players_ui_scale_from_the_settings_comes_before_the_windows() {
        let mut rig = Rig::new();
        rig.app.world().resource_mut::<crate::UiSettings>().scale = Some(1.5);
        rig.frame(&[Event::ScaleFactorChanged(2.0), tap(Key::F8)[0]]);
        {
            let frame = rig.app.world().resource::<RenderFrame>();
            let (_, view) = frame.ui.iter().next().expect("the window");
            assert_eq!(view.scale(), UiScale::Free(1.5));
        }
        // With none, the window's again.
        rig.app.world().resource_mut::<crate::UiSettings>().scale = None;
        rig.frame(&[]);
        let frame = rig.app.world().resource::<RenderFrame>();
        let (_, view) = frame.ui.iter().next().expect("the window");
        assert_eq!(view.scale(), UiScale::Free(2.0));
    }

    #[test]
    fn the_engine_s_files_come_before_the_game_s_and_never_change() {
        let root = env::temp_dir().join(format!("nadir-engine-files-{}", process::id()));
        fs::create_dir_all(root.join("nadir/debug")).expect("the folder");
        fs::write(root.join("nadir/debug/white.qoi"), b"the game's").expect("a file");
        fs::write(root.join("nadir/debug/other.qoi"), b"the game's too").expect("a file");
        let source = EngineFiles::new(&root);
        let path = |path: &str| AssetPath::new(path).expect("a path");
        let white = source.read(&path(font::WHITE)).expect("the white texel");
        let image = qoi::decode(&white, nadir_codec::Limits::default()).expect("an image");
        assert_eq!((image.header.width, image.header.height), (1, 1));
        assert_eq!(image.pixels, [255; 4]);
        assert_eq!(source.modified(&path(font::WHITE)), None);
        assert_eq!(
            source.read(&path("nadir/debug/other.qoi")),
            Ok(b"the game's too".to_vec())
        );
        assert!(source.modified(&path("nadir/debug/other.qoi")).is_some());
        assert_eq!(source.read(&path("missing.qoi")), Err(AssetError::NotFound));
        fs::remove_dir_all(&root).expect("remove the folder");
    }

    // -- the engine's windows ---------------------------------------------

    /// Where the frame's UI drew `text`, in the UI's units: pixels here.
    fn text_at(app: &App, text: &str) -> Option<Vec2> {
        let frame = app.world().resource::<RenderFrame>();
        frame.ui.iter().find_map(|(draw, _)| match draw {
            UiDraw::Text(draw, shown) if shown == text => Some(draw.position),
            _ => None,
        })
    }

    fn shows_starting(app: &App, start: &str) -> bool {
        texts(app).iter().any(|text| text.starts_with(start))
    }

    #[test]
    fn f1_shows_the_frame_s_stats_and_f3_the_profiler_of_its_scopes() {
        let mut app = app();
        for _ in 0..3 {
            app.world_mut()
                .spawn()
                .with(crate::Transform::at(Vec2::ZERO));
        }
        for _ in 0..4 {
            app.update(TICK, &[]);
        }
        app.update(TICK, &tap(Key::F1));
        let shown = texts(&app);
        assert!(shown.iter().any(|text| text == "Stats"), "{shown:?}");
        assert!(shown.iter().any(|text| text == "entities 3"), "{shown:?}");
        assert!(shows_starting(&app, "ticks 1 in "), "{shown:?}");
        assert!(shown.iter().any(|text| text == "voices 0"), "{shown:?}");
        assert!(shown.iter().any(|text| text == "underruns 0"), "{shown:?}");
        app.update(TICK, &tap(Key::F1));
        assert!(texts(&app).is_empty());

        // Each frame before this one, its scopes collected as the next
        // started: the first frame has none before it.
        app.update(TICK, &tap(Key::F3));
        let shown = texts(&app);
        assert!(shown.iter().any(|text| text == "Profiler"), "{shown:?}");
        assert!(shows_starting(&app, "6 frames, "), "{shown:?}");
        assert!(
            shown
                .iter()
                .any(|text| text == "Click a frame for its scopes.")
        );

        // A click on the graph, from (218, 40) to (431.6, 100), selects a
        // frame: its scopes, the engine's and the systems', show.
        app.update(TICK, &click(Vec2::new(400.0, 70.0)));
        app.update(TICK, &[]);
        let shown = texts(&app);
        assert!(shows_starting(&app, "frame "), "{shown:?}");
        assert!(shows_starting(&app, "nadir::tick "), "{shown:?}");
        assert!(shows_starting(&app, "nadir::frame_stages "), "{shown:?}");
        assert!(shows_starting(&app, "  nadir::debug_ui "), "{shown:?}");
        let tick = shown
            .iter()
            .find(|text| text.starts_with("nadir::tick "))
            .expect("the tick");
        let at = text_at(&app, tick).expect("drawn");
        app.update(TICK, &click(at + Vec2::new(2.0, 2.0)));
        app.update(TICK, &[]);
        // The tick opened: the engine's systems and the game's in it.
        assert!(
            shows_starting(&app, "  nadir::snapshot "),
            "{:?}",
            texts(&app)
        );
        assert!(shows_starting(&app, "  count "), "{:?}", texts(&app));

        // Without App::run there is no folder to write a trace to.
        let button = text_at(&app, "Write trace").expect("the button");
        app.update(TICK, &click(button + Vec2::new(2.0, 2.0)));
        app.update(TICK, &[]);
        let shown = texts(&app);
        assert!(
            shown
                .iter()
                .any(|text| text == "Traces are written only by App::run."),
            "{shown:?}"
        );
    }

    #[test]
    fn a_steady_frame_of_the_stats_and_a_profiled_frame_s_tree_allocates_nothing() {
        let mut app = app();
        app.update(TICK, &tap(Key::F1));
        app.update(TICK, &tap(Key::F3));
        // Until the ring is full, the graph's lines grow the buffers that
        // hold them.
        for _ in 0..nadir_debug::PROFILER_FRAMES + 10 {
            app.update(TICK, &[]);
        }
        app.update(TICK, &click(Vec2::new(400.0, 70.0)));
        for _ in 0..10 {
            app.update(TICK, &[]);
        }
        assert!(shows_starting(&app, "nadir::tick "), "{:?}", texts(&app));
        let start = nadir_platform::thread_alloc_counts();
        for _ in 0..20 {
            app.update(TICK, &[]);
        }
        let allocated = nadir_platform::thread_alloc_counts().since(start);
        assert!(shows_starting(&app, "nadir::tick "), "{:?}", texts(&app));
        assert!(shows_starting(&app, "entities 0"), "{:?}", texts(&app));
        assert_eq!(allocated, nadir_platform::AllocCounts::default());
    }

    /// The GPU's times over a render `renders_ago` renders back.
    fn gpu_times(renders_ago: u32, world_us: u64, output_us: u64) -> GpuTimes {
        let mut times = GpuTimes::default();
        times.renders_ago = renders_ago;
        times.world = Duration::from_micros(world_us);
        times.output = Duration::from_micros(output_us);
        times
    }

    #[test]
    fn each_gpu_time_read_back_goes_to_the_frame_its_render_drew() {
        let mut tools = DebugTools::new();
        let base = Instant::now();
        let at = |frame: u32| base + frame * TICK;
        let drawn = |frame: u32| at(frame) + Duration::from_micros(500);
        // Each frame starts, then is drawn, but frame 2 (a modal one): so
        // render 2 is frame 3's, and so on. Frame 4 reads back render 1,
        // two renders back, frame 1's (two frames back is frame 2); frame 6
        // render 2, three back; frame 11, the eleventh render, render 3,
        // seven back, and one nine back, past what is kept: render 1, whose
        // slot now holds render 9, frame 10's. (Eight back would be the
        // slot of the render being drawn, whose frame is not held yet.)
        let mut read: [&[GpuTimes]; 12] = [&[]; 12];
        let (first, second, behind) = (
            [gpu_times(2, 100, 200)],
            [gpu_times(2, 300, 400)],
            [gpu_times(3, 500, 600)],
        );
        let late = [gpu_times(9, 1, 1), gpu_times(7, 700, 800)];
        read[3] = &first;
        read[4] = &second;
        read[6] = &behind;
        read[11] = &late;
        for (frame, times) in (0..).zip(read) {
            tools.start_profile(at(frame), &[]);
            if frame != 2 {
                tools.rendered(drawn(frame), times);
            }
        }
        tools.start_profile(at(12), &[]);
        let held: Vec<(u64, Option<GpuTiming>)> = tools
            .profiler
            .frames()
            .map(|frame| (frame.index, frame.gpu))
            .collect();
        let gpu = |frame: u32, world: u64, output: u64| {
            Some(GpuTiming::new(
                drawn(frame),
                Duration::from_micros(world),
                Duration::from_micros(output),
            ))
        };
        let mut expected: Vec<(u64, Option<GpuTiming>)> =
            (0..12).map(|index| (index, None)).collect();
        expected[0].1 = gpu(0, 100, 200);
        expected[1].1 = gpu(1, 300, 400);
        expected[3].1 = gpu(3, 500, 600);
        expected[4].1 = gpu(4, 700, 800);
        assert_eq!(held, expected);
    }

    #[test]
    fn a_trace_written_holds_each_frame_and_its_scopes_and_parses() {
        use nadir_codec::json::{self, Value};
        use nadir_core::profile::{self, ScopeRecord};

        let mut tools = DebugTools::new();
        let mut records: Vec<ScopeRecord> = Vec::new();
        for _ in 0..4 {
            {
                nadir_core::profile_scope!("outer");
                nadir_core::profile_scope!("inner");
            }
            profile::take_frame(&mut records);
            tools.start_profile(Instant::now(), &records);
        }
        let folder = env::temp_dir().join(format!("nadir-engine-traces-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        write_trace(&mut tools.profiler, Some(&folder), None).expect("the trace is written");
        let files: Vec<PathBuf> = fs::read_dir(&folder)
            .expect("the folder was made")
            .map(|entry| entry.expect("an entry").path())
            .collect();
        let [file] = &files[..] else {
            panic!("one trace: {files:?}");
        };
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a name");
        assert!(
            name.starts_with("trace-") && name.ends_with(".json"),
            "{name}"
        );
        let text = fs::read_to_string(file).expect("the trace");
        let value = json::parse(&text, nadir_codec::Limits::default()).expect("it parses");
        let events = value
            .get("traceEvents")
            .and_then(Value::as_array)
            .expect("its events");
        let names: Vec<&str> = events
            .iter()
            .filter(|event| event.get("ph").and_then(Value::as_str) == Some("X"))
            .filter_map(|event| event.get("name").and_then(Value::as_str))
            .collect();
        // Three frames: the first push only starts one. Each holds the
        // scopes collected as it ended, in the order they started.
        assert_eq!(names, ["frame", "outer", "inner"].repeat(3));
        fs::remove_dir_all(&folder).expect("remove the folder");
    }

    /// The outline's sprites the frame holds, as one rect around them all.
    fn outline_of(app: &App) -> Option<Rect> {
        let frame = app.world().resource::<RenderFrame>();
        let edges: Vec<Rect> = frame
            .sprites
            .iter()
            .filter(|sprite| sprite.color == OUTLINE)
            .map(|sprite| Rect::new(sprite.position, sprite.position + sprite.size))
            .collect();
        if edges.is_empty() {
            return None;
        }
        assert_eq!(edges.len(), 4, "four edges: {edges:?}");
        let min = edges
            .iter()
            .fold(Vec2::splat(f32::MAX), |at, edge| at.min(edge.min));
        let max = edges
            .iter()
            .fold(Vec2::splat(f32::MIN), |at, edge| at.max(edge.max));
        Some(Rect::new(min, max))
    }

    #[test]
    fn f2_inspects_an_entity_outlines_it_edits_it_and_saves_it_as_a_prefab() {
        use nadir_codec::json::{self, Value};

        use crate::{Player, Velocity};

        let mut app = app();
        // Wide enough for the inspector's first place.
        let screen = UVec2::new(1280, 720);
        *app.world_mut().resource_mut::<WindowSize>() = WindowSize(screen);
        let hero = app
            .world_mut()
            .spawn()
            .with(Transform::at(Vec2::new(100.0, 50.0)))
            .with(Velocity(Vec2::ZERO))
            .with(Player)
            .with(SaveId(7))
            .with(Sprite::new(game_box(), Vec2::new(16.0, 24.0)))
            .id();
        let folder = env::temp_dir().join(format!("nadir-engine-prefabs-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        app.debug_tools().write_traces_in(folder.clone());

        app.update(TICK, &tap(Key::F2));
        assert!(texts(&app).iter().any(|text| text == "1 entities"));
        let row = "  0v1 Transform Velocity Player SaveId Sprite";
        let at = text_at(&app, row).expect("the hero's row");
        app.update(TICK, &click(at + Vec2::new(1.0, 2.0)));
        assert_eq!(app.debug_tools().inspector.selected(), Some(hero));

        // Outlined in the world, a unit outside its sprite, whose feet are
        // at its place.
        app.update(TICK, &[]);
        let expected = Rect::new(Vec2::new(91.0, 25.0), Vec2::new(109.0, 51.0));
        assert_eq!(outline_of(&app), Some(expected));

        // Its place, dragged in the window through the engine's context: 100
        // units right is 10 world units.
        let at = text_at(&app, "Transform").expect("its node");
        app.update(TICK, &click(at + Vec2::new(1.0, 2.0)));
        app.update(TICK, &[]);
        let position = text_at(&app, "position").expect("its position");
        let x = position + Vec2::new(-126.0, 2.0);
        let press = Event::MouseButton {
            button: MouseButton::Left,
            pressed: true,
        };
        let release = Event::MouseButton {
            button: MouseButton::Left,
            pressed: false,
        };
        app.update(TICK, &[Event::MouseMoved(x), press]);
        app.update(TICK, &[Event::MouseMoved(x + Vec2::new(100.0, 0.0))]);
        app.update(TICK, &[release]);
        let moved = app
            .world()
            .read::<Transform>()
            .get(hero)
            .map(|at| at.position);
        assert_eq!(moved, Some(Vec2::new(110.0, 50.0)));

        // Saved as a prefab named in its field, the button's text 48 after
        // the field's middle: without its place or its save number.
        let button = text_at(&app, "Save as prefab").expect("the button");
        let typed = |text: &str| -> Vec<Event> {
            let mut events = vec![key(Key::LeftControl, true)];
            events.extend(tap(Key::A));
            events.push(key(Key::LeftControl, false));
            events.extend(text.chars().map(Event::Text));
            events
        };
        // A Windows device's name is no file: not written.
        app.update(TICK, &click(button + Vec2::new(-48.0, 2.0)));
        app.update(TICK, &typed("nul"));
        app.update(TICK, &click(button + Vec2::new(1.0, 2.0)));
        app.update(TICK, &[]);
        assert!(
            shows_starting(&app, "Not written: data/prefabs/nul.jsonc: "),
            "{:?}",
            texts(&app)
        );
        assert!(!folder.join(PREFABS).join("nul.jsonc").exists());
        app.update(TICK, &click(button + Vec2::new(-48.0, 2.0)));
        app.update(TICK, &typed("hero"));
        app.update(TICK, &click(button + Vec2::new(1.0, 2.0)));
        app.update(TICK, &[]);
        let path = folder.join(PREFABS).join("hero.jsonc");
        let text = fs::read_to_string(&path).expect("the prefab was written");
        assert!(texts(&app).contains(&format!("Wrote {}", path.display())));
        let prefab = json::parse(&text, nadir_codec::Limits::default()).expect("JSONC");
        assert_eq!(prefab.get("name").and_then(Value::as_str), Some("hero"));
        let names: Vec<&str> = prefab
            .get("components")
            .and_then(Value::as_object)
            .expect("components")
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["Velocity", "Player", "Sprite"]);

        // Closed, the inspector outlines nothing.
        app.update(TICK, &tap(Key::F2));
        assert_eq!(outline_of(&app), None);
        fs::remove_dir_all(&folder).expect("remove the folder");
    }

    #[test]
    fn an_entity_with_no_sprite_is_outlined_by_its_collider_or_a_box_at_its_place() {
        use nadir_collision::Shape;

        let mut app = app();
        let crate_box = app
            .world_mut()
            .spawn()
            .with(Transform::at(Vec2::new(40.0, 40.0)))
            .with(Collider::solid(Shape::Aabb {
                half: Vec2::new(5.0, 3.0),
            }))
            .id();
        let marker = app
            .world_mut()
            .spawn()
            .with(Transform::at(Vec2::new(-10.0, 0.0)))
            .id();
        let nowhere = app.world_mut().spawn().id();
        app.update(TICK, &tap(Key::F2));
        app.debug_tools().inspector.select(Some(crate_box));
        app.update(TICK, &[]);
        let expected = Rect::new(Vec2::new(34.0, 36.0), Vec2::new(46.0, 44.0));
        assert_eq!(outline_of(&app), Some(expected));
        app.debug_tools().inspector.select(Some(marker));
        app.update(TICK, &[]);
        let expected = Rect::new(Vec2::new(-15.0, -5.0), Vec2::new(-5.0, 5.0));
        assert_eq!(outline_of(&app), Some(expected));
        app.debug_tools().inspector.select(Some(nowhere));
        app.update(TICK, &[]);
        assert_eq!(outline_of(&app), None);
    }
    /// What the game's `god` command flips.
    #[derive(Default)]
    struct God(bool);

    fn god(world: &World, args: &Args<'_>, out: &mut ConsoleOut<'_>) -> Result<(), ConsoleError> {
        let mut god = world.resource_mut::<God>();
        god.0 = match args.get(0) {
            Some("on") => true,
            Some(_) => false,
            None => !god.0,
        };
        out.print(format_args!("god {}", if god.0 { "on" } else { "off" }));
        Ok(())
    }

    /// The backtick, as a window gives it: a key, and the text it types.
    fn backtick() -> [Event; 3] {
        [
            key(Key::Grave, true),
            Event::Text('`'),
            key(Key::Grave, false),
        ]
    }

    /// Types `line` in a frame and presses Enter, then runs a frame more,
    /// for the commands queued to apply.
    fn enter(app: &mut App, line: &str) {
        let mut events: Vec<Event> = line.chars().map(Event::Text).collect();
        events.extend(tap(Key::Enter));
        app.update(TICK, &events);
        app.update(TICK, &[]);
    }

    /// Whether `line` is a record of the log, which the tests running beside
    /// this one may add at any time: `<time> <LEVEL> <target>: <message>`,
    /// its time `2026-11-14T10:00:00.000Z`.
    fn is_record(line: &str) -> bool {
        line.len() > 25 && line.as_bytes()[10] == b'T' && line.as_bytes()[23] == b'Z'
    }

    /// The console's lines from the last `> line` echoed on, but the log's
    /// records.
    fn after(app: &mut App, line: &str) -> Vec<String> {
        let lines: Vec<String> = app
            .debug_tools()
            .console
            .lines()
            .map(String::from)
            .collect();
        let echo = format!("> {line}");
        let start = lines
            .iter()
            .rposition(|shown| *shown == echo)
            .unwrap_or_else(|| panic!("`{echo}` is not in the console: {lines:?}"));
        lines[start + 1..]
            .iter()
            .filter(|line| !is_record(line))
            .cloned()
            .collect()
    }

    /// A console app: the `god` command, a player at (1, 2), and the
    /// prefab `rock` in the library.
    fn console_app() -> (App, Entity) {
        let mut app = app()
            .insert_resource(God::default())
            .add_console_command("god [on|off]", "makes the player invulnerable", god)
            // Starts with no name: logged, not added.
            .add_console_command("<god>", "nothing", god);
        *app.world_mut().resource_mut::<WindowSize>() = WindowSize(UVec2::new(1280, 720));
        let player = app
            .world_mut()
            .spawn()
            .with(Transform::at(Vec2::new(1.0, 2.0)))
            .with(Player)
            .id();
        let rock = r#"{"name": "rock", "components": {"Velocity": [3, 4]}}"#;
        let value = nadir_codec::json::parse(rock, nadir_codec::Limits::default()).expect("JSON");
        let file = nadir_codec::prefab::read(&value).expect("a prefab");
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .insert("data/prefabs/rock.kbj", file);
        (app, player)
    }

    #[test]
    fn the_backtick_shows_the_console_whose_line_runs_the_engine_s_commands() {
        use crate::Velocity;

        let (mut app, player) = console_app();
        app.update(TICK, &backtick());
        assert!(texts(&app).iter().any(|text| text == "Console"));
        assert_eq!(
            app.debug_tools().console.line(),
            "",
            "the backtick is not typed"
        );
        // The game's keys are held back from the next frame on.
        assert!(
            app.debug_tools()
                .ui
                .as_ref()
                .is_some_and(DebugUi::wants_keyboard)
        );
        app.update(TICK, &[]);
        assert!(app.world().resource::<Input>().capture().keyboard);

        enter(&mut app, "tp 10 -20");
        let at = app
            .world()
            .read::<Transform>()
            .get(player)
            .map(|at| at.position);
        assert_eq!(at, Some(Vec2::new(10.0, -20.0)));
        assert_eq!(
            after(&mut app, "tp 10 -20"),
            ["moved 1 player(s) to (10, -20)"]
        );

        enter(&mut app, "spawn rock 5 6");
        let rocks: Vec<(Vec2, Vec2)> = {
            let world = app.world();
            let velocities = world.read::<Velocity>();
            let transforms = world.read::<Transform>();
            velocities
                .iter()
                .filter_map(|(entity, velocity)| {
                    Some((transforms.get(entity)?.position, velocity.0))
                })
                .collect()
        };
        // Spawned, then moved a tick at its velocity.
        assert_eq!(rocks.len(), 1, "{rocks:?}");
        assert_eq!(rocks[0].1, Vec2::new(3.0, 4.0));
        assert!(
            (rocks[0].0 - Vec2::new(5.0, 6.0)).length() < 0.2,
            "{rocks:?}"
        );
        assert!(after(&mut app, "spawn rock 5 6")[0].starts_with("spawned `rock` as "));
        // Both coordinates, or neither.
        enter(&mut app, "spawn rock 5");
        assert_eq!(
            after(&mut app, "spawn rock 5"),
            ["[x] and [y] go together: spawn <prefab> [x] [y]"]
        );
        enter(&mut app, "spawn boulder");
        let refused = after(&mut app, "spawn boulder");
        assert!(
            refused[0].starts_with("`boulder` is not spawned: "),
            "{refused:?}"
        );

        // Twice the time: two ticks a frame.
        enter(&mut app, "timescale 2");
        app.update(TICK, &[]);
        assert_eq!(app.world().resource::<TickStats>().ticks, 2);
        enter(&mut app, "timescale 9");
        assert_eq!(
            after(&mut app, "timescale 9"),
            ["<scale>: `9` is not from 0.25 to 4: timescale <scale>"]
        );
        enter(&mut app, "timescale 1");
        app.update(TICK, &[]);
        assert_eq!(app.world().resource::<TickStats>().ticks, 1);

        enter(&mut app, "schedule");
        let systems = after(&mut app, "schedule");
        assert!(
            // Still while paused (ADR-0067).
            systems
                .iter()
                .any(|line| line.ends_with("nadir::movement (conditional)")),
            "{systems:?}"
        );
        assert!(
            systems.iter().any(|line| line.ends_with(" count")),
            "{systems:?}"
        );

        enter(&mut app, "level nowhere");
        let refused = after(&mut app, "level nowhere");
        assert!(
            refused
                .iter()
                .any(|line| line.starts_with("the level `nowhere` is not loaded: ")),
            "{refused:?}"
        );

        enter(&mut app, "reload");
        assert!(after(&mut app, "reload")[0].starts_with("reading "));

        let folder = env::temp_dir().join(format!("nadir-engine-console-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        let trace = folder.join("deep").join("t.json");
        let line = format!("profile dump \"{}\"", trace.display());
        enter(&mut app, &line);
        assert_eq!(
            after(&mut app, &line)[0],
            format!("Wrote {}", trace.display())
        );
        assert!(
            fs::read_to_string(&trace)
                .expect("the trace")
                .contains("traceEvents")
        );
        fs::remove_dir_all(&folder).expect("remove the folder");
        // Without App::run, no folder for a trace without a path.
        enter(&mut app, "profile dump");
        assert_eq!(
            after(&mut app, "profile dump"),
            ["Traces are written only by App::run."]
        );

        let target = "nadir_engine::debug_ui::tests::console";
        assert!(!log::enabled(Level::Debug, target));
        enter(&mut app, &format!("log level {target} DEBUG"));
        assert!(log::enabled(Level::Debug, target));
        log::set_target_level(target, None);

        // The game's command, with its own usage.
        enter(&mut app, "god");
        assert!(app.world().resource::<God>().0);
        enter(&mut app, "god off");
        assert!(!app.world().resource::<God>().0);
        enter(&mut app, "god maybe");
        assert_eq!(
            after(&mut app, "god maybe"),
            ["[on|off]: `maybe` is not one of on, off: god [on|off]"]
        );

        assert!(!app.debug_tools().quit());
        enter(&mut app, "quit");
        assert!(app.debug_tools().quit());
    }

    #[test]
    fn the_console_shows_the_log_s_records_and_its_help_lists_every_command() {
        let (mut app, _) = console_app();
        warn!("a warning from before the console showed");
        app.update(TICK, &backtick());
        warn!("a warning while it shows");
        app.update(TICK, &[]);
        let lines: Vec<String> = app
            .debug_tools()
            .console
            .lines()
            .map(String::from)
            .collect();
        let before = lines
            .iter()
            .position(|line| line.ends_with("a warning from before the console showed"));
        let while_shown = lines
            .iter()
            .position(|line| line.ends_with("a warning while it shows"));
        assert!(before.is_some() && before < while_shown, "{lines:?}");
        assert!(
            lines.iter().any(|line| line.contains(" ERROR ")
                && line.contains("the console command is not added: `<god>`")),
            "{lines:?}"
        );

        enter(&mut app, "help");
        let help = after(&mut app, "help");
        let names: Vec<&str> = help
            .iter()
            .filter_map(|line| line.split_whitespace().next())
            .collect();
        assert_eq!(
            names,
            [
                "help",
                "schedule",
                "spawn",
                "tp",
                "timescale",
                "level",
                "reload",
                "profile",
                "log",
                "quit",
                "god"
            ]
        );
        // Tab completes from the engine's commands and the game's.
        app.update(
            TICK,
            &[Event::Text('g'), key(Key::Tab, true), key(Key::Tab, false)],
        );
        assert_eq!(app.debug_tools().console.line(), "god ");
        // Closed by the backtick, which it does not type.
        app.update(TICK, &backtick());
        assert_eq!(app.debug_tools().console.line(), "god ");
        assert!(!texts(&app).iter().any(|text| text == "Console"));
    }

    #[test]
    fn the_backtick_is_typed_into_no_debug_field_as_it_shows_or_hides_the_console() {
        let (mut app, _) = console_app();
        // The game's window, its field with the keyboard.
        app.update(TICK, &tap(Key::F8));
        app.update(TICK, &click(FIELD));
        app.update(TICK, &[Event::Text('a')]);
        assert_eq!(app.world().resource::<Notes>().text, "a");
        // The backtick shows the console, which takes the keyboard.
        app.update(TICK, &backtick());
        app.update(TICK, &[Event::Text('b')]);
        assert_eq!(app.world().resource::<Notes>().text, "a");
        assert_eq!(app.debug_tools().console.line(), "b");
        // Back in the game's field, the backtick hides the console.
        app.update(TICK, &click(FIELD));
        app.update(TICK, &backtick());
        app.update(TICK, &[Event::Text('c')]);
        assert_eq!(app.world().resource::<Notes>().text, "ac");
        assert!(!texts(&app).iter().any(|text| text == "Console"));
        // Text that comes after another key's press is typed: a key
        // pressed after the toggle, or a dead key's accent with it.
        app.update(TICK, &backtick());
        app.update(TICK, &click(FIELD));
        app.update(
            TICK,
            &[
                key(Key::Grave, true),
                key(Key::Grave, false),
                key(Key::E, true),
                Event::Text('e'),
                key(Key::E, false),
            ],
        );
        assert_eq!(app.world().resource::<Notes>().text, "ace");
        assert!(!texts(&app).iter().any(|text| text == "Console"));
    }

    /// The frame's sprites of `color`: where each is, how large, and how
    /// turned.
    fn colored(app: &App, color: Color) -> Vec<(Vec2, Vec2, f32)> {
        let frame = app.world().resource::<RenderFrame>();
        frame
            .sprites
            .iter()
            .filter(|sprite| sprite.color == color)
            .map(|sprite| (sprite.position, sprite.size, sprite.rotation))
            .collect()
    }

    /// The rect around unturned sprites with their pivots at their
    /// top-left corners.
    fn around(sprites: &[(Vec2, Vec2, f32)]) -> Option<Rect> {
        let mut rects = sprites
            .iter()
            .map(|&(at, size, _)| Rect::new(at, at + size));
        let first = rects.next()?;
        Some(rects.fold(first, |all, rect| {
            Rect::new(all.min.min(rect.min), all.max.max(rect.max))
        }))
    }

    /// The tile layer a frame system of the test draws.
    struct Chunked(Handle<nadir_render::TileLayer>);

    fn draw_chunked(world: &World, _: &nadir_core::time::Frame) {
        let tiles = world.resource::<Chunked>().0;
        let mut frame = world.resource_mut::<RenderFrame>();
        frame
            .tile_layers
            .push(nadir_render::TileLayerDraw::new(tiles));
    }

    #[test]
    fn f4_draws_the_overlays_ticked_into_the_world_and_nothing_once_closed() {
        use nadir_collision::{Shape, TileGrid};
        use nadir_core::IVec2;
        use nadir_render::{TileLayer, TileLayers, TileSet};

        use crate::Collisions;

        let mut app = app().add_frame_system(FrameStage::Extract, "chunked", draw_chunked);
        *app.world_mut().resource_mut::<WindowSize>() = WindowSize(UVec2::new(1280, 720));
        let world = app.world_mut();
        world
            .spawn()
            .with(Transform::at(Vec2::new(40.0, 40.0)))
            .with(Collider::solid(Shape::Aabb {
                half: Vec2::new(5.0, 3.0),
            }));
        world
            .spawn()
            .with(Transform::at(Vec2::new(-100.0, 50.0)))
            .with(Collider::trigger(Shape::Circle { radius: 8.0 }));
        // Out of the view, (-240, -135) to (240, 135): not drawn.
        world
            .spawn()
            .with(Transform::at(Vec2::new(1000.0, 0.0)))
            .with(Collider::solid(Shape::Aabb {
                half: Vec2::splat(4.0),
            }));
        let mut grid = TileGrid::new(4, 4, 16.0, Vec2::new(-64.0, -64.0));
        grid.set_solid(IVec2::new(1, 2), true);
        world.resource_mut::<Collisions>().set_grid(grid);
        let tileset = TileSet {
            texture: game_box(),
            columns: 1,
            rows: 1,
        };
        let layer = TileLayer::new(tileset, UVec2::new(40, 10), 16.0, Vec2::new(-200.0, -100.0));
        let tiles = world.resource_mut::<TileLayers>().add(layer);
        world.insert_resource(Chunked(tiles));

        app.update(TICK, &[]);
        assert!(colored(&app, draw::SOLID).is_empty(), "closed");
        app.update(TICK, &tap(Key::F4));
        app.update(TICK, &[]);
        // The colliders, the triggers and the grid, at first: a box's edges
        // inside it, a line wide; a circle's sides; a solid cell, filled.
        let solid = colored(&app, draw::SOLID);
        assert_eq!(solid.len(), 4, "the box out of view is not drawn");
        let expected = Rect::new(Vec2::new(35.0, 37.0), Vec2::new(45.0, 43.0));
        assert_eq!(around(&solid), Some(expected));
        assert!(
            solid
                .iter()
                .all(|&(_, size, _)| size.x == 1.0 || size.y == 1.0)
        );
        assert_eq!(colored(&app, draw::TRIGGER).len(), 24);
        let grid = colored(&app, draw::GRID);
        let cell = Rect::new(Vec2::new(-48.0, -32.0), Vec2::new(-32.0, -16.0));
        assert_eq!(around(&grid), Some(cell));
        assert_eq!(grid.len(), 1);
        for color in [draw::HASH, draw::CHUNK, draw::CAMERA] {
            assert!(colored(&app, color).is_empty());
        }

        // A click on the colliders' box, the first row's, unticks them.
        app.update(TICK, &click(Vec2::new(960.0, 31.0)));
        app.update(TICK, &[]);
        assert!(colored(&app, draw::SOLID).is_empty());
        assert_eq!(colored(&app, draw::TRIGGER).len(), 24);

        let overlays = &mut app.debug_tools().overlays;
        overlays.hash = true;
        overlays.chunks = true;
        overlays.camera = true;
        app.update(TICK, &[]);
        // The hash's cells of 64 that hold a collider in the view, and how
        // many each holds.
        let hash = colored(&app, draw::HASH);
        assert_eq!(hash.len(), 8);
        let counts: Vec<(Vec2, String)> = app
            .world()
            .resource::<RenderFrame>()
            .texts
            .iter()
            .filter(|(draw, _)| draw.color == draw::HASH)
            .map(|(draw, text)| (draw.position, text.to_string()))
            .collect();
        assert_eq!(
            counts,
            [
                (Vec2::new(-126.0, 2.0), "1".to_string()),
                (Vec2::new(2.0, 2.0), "1".to_string())
            ]
        );
        // The layer's first chunk of 32 tiles; the second is out of view.
        let chunk = colored(&app, draw::CHUNK);
        let expected = Rect::new(Vec2::new(-200.0, -100.0), Vec2::new(312.0, 60.0));
        assert_eq!((chunk.len(), around(&chunk)), (4, Some(expected)));
        // The camera's cross, two lines turned, and its view.
        let camera = colored(&app, draw::CAMERA);
        let (cross, view): (Vec<_>, Vec<_>) = camera
            .iter()
            .partition(|&&(_, size, _)| size == Vec2::new(12.0, 1.0));
        assert_eq!(cross.len(), 2);
        let expected = Rect::new(Vec2::new(-240.0, -135.0), Vec2::new(240.0, 135.0));
        assert_eq!((view.len(), around(&view)), (4, Some(expected)));

        // Closed, it draws nothing.
        app.update(TICK, &tap(Key::F4));
        for color in [
            draw::TRIGGER,
            draw::GRID,
            draw::HASH,
            draw::CHUNK,
            draw::CAMERA,
        ] {
            assert!(colored(&app, color).is_empty());
        }
    }

    #[test]
    fn f4_dots_the_open_cells_and_draws_the_paths() {
        use nadir_collision::{PathRequest, TileGrid};
        use nadir_core::IVec2;

        use crate::{Collisions, Navigation};

        let mut app = app();
        *app.world_mut().resource_mut::<WindowSize>() = WindowSize(UVec2::new(1280, 720));
        let world = app.world_mut();
        // 4 × 3 cells of 16 from (-32, -24), all in view, one solid.
        let mut grid = TileGrid::new(4, 3, 16.0, Vec2::new(-32.0, -24.0));
        grid.set_solid(IVec2::new(1, 1), true);
        world.resource_mut::<Collisions>().set_grid(grid);
        let walker = world.spawn().id();
        let request = PathRequest::new(Vec2::new(-24.0, -16.0), Vec2::new(-8.0, 16.0));
        world.resource_mut::<Navigation>().request(walker, request);
        app.update(TICK, &tap(Key::F4));
        app.update(TICK, &[]);
        // The paths at first, not the nav grid. Round the solid cell: down
        // column 0, then right, two legs, and a mark at each leg's end.
        let path = colored(&app, draw::PATH);
        let (legs, marks): (Vec<_>, Vec<_>) = path
            .iter()
            .partition(|&&(_, size, _)| size.y == 1.0 && size.x > 3.0);
        assert_eq!((legs.len(), marks.len()), (2, 2), "{path:?}");
        // The corner at (-24, 16), on whole pixels.
        assert!(marks.contains(&&(Vec2::new(-25.0, 15.0), Vec2::splat(3.0), 0.0)));
        assert!(colored(&app, draw::NAV).is_empty());

        let overlays = &mut app.debug_tools().overlays;
        overlays.nav_grid = true;
        overlays.paths = false;
        // Another solid cell, off the diagonal: (2, 1).
        app.world()
            .resource_mut::<Collisions>()
            .grid_mut()
            .set_solid(IVec2::new(2, 1), true);
        app.update(TICK, &[]);
        // A dot two pixels wide in the middle of each of the 10 open cells:
        // none in (1, 1) or (2, 1), one in (1, 2).
        let dots = colored(&app, draw::NAV);
        let dot = |x: i32, y: i32| {
            (
                Vec2::new(-25.0 + 16.0 * x as f32, -17.0 + 16.0 * y as f32),
                Vec2::splat(2.0),
                0.0,
            )
        };
        assert_eq!(dots.len(), 10);
        assert!(dots.contains(&dot(0, 0)));
        assert!(!dots.contains(&dot(1, 1)) && !dots.contains(&dot(2, 1)));
        assert!(dots.contains(&dot(1, 2)));
        assert!(colored(&app, draw::PATH).is_empty());
        // Navigation's own grid is the one drawn, when it has one.
        let mut own = TileGrid::new(4, 3, 16.0, Vec2::new(-32.0, -24.0));
        own.set_solid(IVec2::new(0, 0), true);
        app.world().resource_mut::<Navigation>().set_grid(Some(own));
        app.update(TICK, &[]);
        let dots = colored(&app, draw::NAV);
        assert_eq!(dots.len(), 11);
        assert!(!dots.contains(&dot(0, 0)) && dots.contains(&dot(1, 1)));
    }

    #[test]
    fn f4_draws_only_the_cells_in_view_and_every_leg_that_reaches_it() {
        use nadir_collision::{PathRequest, PathResult, TileGrid};

        use crate::{Collisions, Navigation};

        let mut app = app();
        *app.world_mut().resource_mut::<WindowSize>() = WindowSize(UVec2::new(1280, 720));
        let world = app.world_mut();
        // 40 × 3 cells from (-320, -24), wider than the view, (-240, -135)
        // to (240, 135).
        world.resource_mut::<Collisions>().set_grid(TileGrid::new(
            40,
            3,
            16.0,
            Vec2::new(-320.0, -24.0),
        ));
        let walker = world.spawn().id();
        // From the view's middle west, out past its left edge.
        let request = PathRequest::new(Vec2::new(100.0, -16.0), Vec2::new(-300.0, -16.0));
        world.resource_mut::<Navigation>().request(walker, request);
        app.update(TICK, &tap(Key::F4));
        app.debug_tools().overlays.nav_grid = true;
        app.update(TICK, &[]);
        let legs = match app.world().resource::<Navigation>().path(walker) {
            Some(PathResult::Found(points)) => points.len() - 1,
            other => panic!("{other:?}"),
        };
        // Every leg reaches the view, the last one leaving it to the left.
        let path = colored(&app, draw::PATH);
        let drawn = path.iter().filter(|&&(_, size, _)| size.x > 3.0).count();
        assert_eq!(drawn, legs, "{path:?}");
        // The dots of the cells the view touches, columns 5 to 35, not the
        // rest of the grid's 40.
        let dots = colored(&app, draw::NAV);
        assert_eq!(dots.len(), 31 * 3);
        assert!(dots.iter().all(|&(at, _, _)| at.x > -240.0 && at.x < 256.0));
    }

    #[test]
    fn a_box_off_the_pixel_grid_is_a_closed_ring_on_it_and_a_thin_one_a_fill() {
        use nadir_collision::Shape;

        let mut app = app();
        app.world_mut()
            .spawn()
            .with(Transform::at(Vec2::new(40.2, 40.2)))
            .with(Collider::solid(Shape::Aabb {
                half: Vec2::new(5.74, 3.74),
            }));
        app.world_mut()
            .spawn()
            .with(Transform::at(Vec2::new(-20.0, 0.0)))
            .with(Collider::trigger(Shape::Aabb {
                half: Vec2::new(0.4, 5.0),
            }));
        app.update(TICK, &tap(Key::F4));
        app.update(TICK, &[]);
        // Its corners on whole pixels, its four edges meeting and never
        // overlapping: the renderer, which puts each sprite's corner on a
        // whole pixel, draws them as they are.
        let ring = colored(&app, draw::SOLID);
        assert_eq!(ring.len(), 4);
        let whole = |v: Vec2| v.x.fract() == 0.0 && v.y.fract() == 0.0;
        assert!(ring.iter().all(|&(at, size, _)| whole(at) && whole(size)));
        let expected = Rect::new(Vec2::new(34.0, 36.0), Vec2::new(46.0, 44.0));
        assert_eq!(around(&ring), Some(expected));
        let area: f32 = ring.iter().map(|&(_, size, _)| size.x * size.y).sum();
        assert_eq!(area, 12.0 * 8.0 - 10.0 * 6.0);
        // Too thin for two edges: one sprite, a pixel wide at least.
        let thin = colored(&app, draw::TRIGGER);
        let expected = Rect::new(Vec2::new(-20.0, -5.0), Vec2::new(-19.0, 5.0));
        assert_eq!((thin.len(), around(&thin)), (1, Some(expected)));
    }

    #[test]
    fn a_child_s_collider_is_drawn_where_its_root_is_drawn_with_its_offset() {
        use nadir_collision::Shape;

        use crate::{Parent, Velocity};

        let mut app = app();
        let root = app
            .world_mut()
            .spawn()
            .with(Transform::at(Vec2::ZERO))
            .with(Velocity(Vec2::new(120.0, 0.0)))
            .with(Sprite::new(game_box(), Vec2::new(8.0, 8.0)))
            .id();
        app.world_mut()
            .spawn()
            .with(Transform::at(Vec2::ZERO))
            .with(Parent {
                entity: root,
                offset: Vec2::new(20.0, 0.0),
            })
            .with(Collider::trigger(Shape::Circle { radius: 4.0 }));
        app.update(TICK, &tap(Key::F4));
        // Three frames a tick: the root is drawn between its last two
        // ticks, and so is its child's collider.
        for _ in 0..7 {
            app.update(TICK / 3, &[]);
            let drawn = app
                .world()
                .resource::<RenderFrame>()
                .sprites
                .iter()
                .find(|sprite| sprite.texture == game_box())
                .map(|sprite| sprite.position)
                .expect("the root's sprite");
            let sides = colored(&app, draw::TRIGGER);
            assert_eq!(sides.len(), 24);
            let center = sides.iter().fold(Vec2::ZERO, |sum, &(at, _, _)| sum + at) / 24.0;
            let expected = drawn + Vec2::new(20.0, 0.0);
            assert!(
                (center - expected).length() < 1e-3,
                "{center:?} {expected:?}"
            );
        }
    }

    #[test]
    fn f5_pauses_f6_steps_and_f7_scales_the_game_s_time_saying_so_in_a_corner() {
        let mut app = app();
        let ticks = |app: &App| app.world().resource::<TickStats>().ticks;
        let time = |app: &App| app.world().resource::<RenderFrame>().time;
        let label = |app: &App| -> Vec<String> {
            texts(app)
                .into_iter()
                .filter(|text| text.starts_with("paused") || text.starts_with("time x"))
                .collect()
        };
        app.update(TICK, &[]);
        app.update(TICK, &[]);
        assert_eq!(ticks(&app), 1);
        assert!(label(&app).is_empty());
        // F5 pauses in the frame it is pressed: no tick, the frame's time
        // held, which holds the tiles and the clips.
        let held = time(&app);
        app.update(TICK, &tap(Key::F5));
        assert_eq!((ticks(&app), time(&app)), (0, held));
        assert_eq!(label(&app), ["paused"]);
        // Under the windows, which cover the corner if they reach it.
        app.update(TICK, &tap(Key::F1));
        let shown = texts(&app);
        let at = |text: &str| {
            shown
                .iter()
                .position(|shown| shown == text)
                .unwrap_or_else(|| panic!("no {text:?} in {shown:?}"))
        };
        assert!(at("paused") < at("Stats"), "{shown:?}");
        app.update(TICK, &tap(Key::F1));
        app.update(TICK, &[]);
        assert_eq!((ticks(&app), time(&app)), (0, held));
        // F6 steps a tick, and a held F6's repeats step on.
        app.update(TICK, &tap(Key::F6));
        assert_eq!(ticks(&app), 1);
        assert!(time(&app) > held);
        let repeat = Event::Key {
            key: Key::F6,
            pressed: true,
            repeat: true,
        };
        app.update(
            TICK,
            &[key(Key::F6, true), repeat, repeat, key(Key::F6, false)],
        );
        assert_eq!(ticks(&app), 3);
        // F5 runs it again, and the label goes.
        app.update(TICK, &tap(Key::F5));
        assert_eq!(ticks(&app), 1);
        assert!(label(&app).is_empty());
        // F7 doubles it, and Shift+F7 halves it again.
        app.update(TICK, &tap(Key::F7));
        assert_eq!(ticks(&app), 2);
        assert_eq!(label(&app), ["time x2"]);
        app.update(
            TICK,
            &[
                key(Key::LeftShift, true),
                key(Key::F7, true),
                key(Key::F7, false),
                key(Key::LeftShift, false),
            ],
        );
        assert_eq!(ticks(&app), 1);
        assert!(label(&app).is_empty());
        // The console's `timescale` sets the same scale.
        app.debug_tools().time.scale = 0.5;
        app.update(TICK, &[]);
        assert_eq!(label(&app), ["time x0.5"]);
    }

    #[test]
    fn a_step_takes_the_replay_s_next_tick() {
        use crate::Replay;

        // A recording of Jump held for 12 ticks.
        let mut recorder = InputRecorder::<Act>::new().expect("a valid set");
        let map = actions();
        let mut input = Input::new();
        let mut state = ActionState::new();
        input.handle(&key(Key::Space, true));
        for _ in 0..12 {
            input.latch(&map, &mut state);
            recorder.record(&state);
        }
        let bytes: &'static [u8] = Box::leak(recorder.into_bytes().into_boxed_slice());
        let player = InputPlayer::<Act>::new(bytes).expect("a replay of Act");
        let mut app = app().play_replay(player);
        let played = |app: &App| match *app.world().resource::<Replay>() {
            Replay::Playing { ticks } => ticks,
            ref other => panic!("{other:?}"),
        };
        app.update(TICK, &[]);
        app.update(TICK, &tap(Key::F5));
        let at = played(&app);
        let held = app.world().resource::<Seen>().jump_held;
        for step in 1..=3 {
            app.update(TICK, &tap(Key::F6));
            app.update(TICK, &[]);
            assert_eq!(played(&app), at + step);
            assert_eq!(app.world().resource::<Seen>().jump_held, held + step as u32);
        }
    }

    #[test]
    fn a_steady_frame_of_every_overlay_and_the_label_allocates_nothing() {
        use nadir_collision::Shape;

        let mut app = app();
        for at in [Vec2::new(40.0, 40.0), Vec2::new(-60.0, 10.0)] {
            app.world_mut()
                .spawn()
                .with(Transform::at(at))
                .with(crate::Velocity(Vec2::new(5.0, 0.0)))
                .with(Collider::solid(Shape::Circle { radius: 6.0 }));
        }
        // A grid with a wall, and a path round it.
        let mut grid = nadir_collision::TileGrid::new(8, 8, 16.0, Vec2::splat(-64.0));
        for y in 0..6 {
            grid.set_solid(nadir_core::IVec2::new(4, y), true);
        }
        let world = app.world_mut();
        world.resource_mut::<crate::Collisions>().set_grid(grid);
        let walker = world.spawn().id();
        let request =
            nadir_collision::PathRequest::new(Vec2::new(-40.0, -40.0), Vec2::new(40.0, -40.0));
        world
            .resource_mut::<crate::Navigation>()
            .request(walker, request);
        app.update(TICK, &tap(Key::F4));
        let overlays = &mut app.debug_tools().overlays;
        overlays.nav_grid = true;
        overlays.hash = true;
        overlays.chunks = true;
        overlays.camera = true;
        app.update(TICK, &tap(Key::F7));
        for _ in 0..10 {
            app.update(TICK, &[]);
        }
        let start = nadir_platform::thread_alloc_counts();
        for _ in 0..20 {
            app.update(TICK, &[]);
        }
        let allocated = nadir_platform::thread_alloc_counts().since(start);
        assert!(!colored(&app, draw::SOLID).is_empty());
        assert!(!colored(&app, draw::NAV).is_empty());
        assert!(!colored(&app, draw::PATH).is_empty());
        assert!(texts(&app).iter().any(|text| text == "time x2"));
        assert_eq!(allocated, nadir_platform::AllocCounts::default());
    }

    #[test]
    fn the_time_scale_scales_each_frame_s_length() {
        let mut tools = DebugTools::new();
        assert_eq!(tools.game_time(TICK), TICK);
        tools.time.scale = 0.5;
        assert_eq!(
            tools.game_time(Duration::from_millis(20)),
            Duration::from_millis(10)
        );
        tools.time.scale = 4.0;
        assert_eq!(tools.game_time(Duration::MAX), Duration::MAX);
    }
}
