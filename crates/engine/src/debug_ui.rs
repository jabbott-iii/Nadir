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
//! logs folder (roadmap M7.3, ADR-0059).

mod font;
#[cfg(test)]
mod golden;
mod places;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

use nadir_assets::{AssetError, AssetPath, AssetServer, FinalizeCtx, LooseSource, Scope, Source};
use nadir_audio::Audio;
use nadir_core::civil::DateTime;
use nadir_core::log::{error, info, warn};
use nadir_core::profile::ScopeRecord;
use nadir_core::{Key, MouseButton, Rect, UVec2, Vec2};
use nadir_debug::{DebugInput, DebugUi, FontMetrics, FrameTiming, Profiler, Stats, Ui};
use nadir_ecs::World;
use nadir_input::{Capture, Input, UiEvent, UiEvents};
use nadir_platform::Event;
use nadir_render::{Font, FrameStats, RenderFrame, Texture, UiScale};

use crate::{FrameAllocs, TickStats, WindowSize};

pub(crate) use places::FILE_NAME;

/// A window of the debug UI, which fills it with widgets each frame it is
/// open: a function of the world and the window's [`Ui`], added with
/// [`App::add_debug_window`](crate::App::add_debug_window).
pub type DebugWindow = fn(&World, &mut Ui<'_>);

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

/// What fills a window.
#[derive(Clone, Copy)]
enum Content {
    /// A game's window.
    Game(DebugWindow),
    /// The engine's stats overlay.
    Stats,
    /// The engine's profiler.
    Profiler,
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
    /// The free scale: the window's scale factor, whole and at least 1.
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
    /// The folder the profiler's traces go to, which
    /// [`App::run`](crate::App::run) names.
    traces: Option<PathBuf>,
}

impl DebugTools {
    pub(crate) fn new() -> Self {
        let engine = |(toggle, title, first): (Key, &'static str, Rect), content| {
            Window::closed(toggle, title, first, content)
        };
        Self {
            windows: vec![
                engine(STATS, Content::Stats),
                engine(PROFILER, Content::Profiler),
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
            traces: None,
        }
    }

    /// Adds a game's window, closed.
    pub(crate) fn add(&mut self, toggle: Key, title: &'static str, first: Rect, add: DebugWindow) {
        self.windows
            .push(Window::closed(toggle, title, first, Content::Game(add)));
    }

    /// Writes the profiler's traces into `folder`.
    pub(crate) fn write_traces_in(&mut self, folder: PathBuf) {
        self.traces = Some(folder);
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
    /// until its release or a loss of the focus.
    pub(crate) fn handle(&mut self, input: &mut Input, event: &Event) {
        match *event {
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
            }
            | Event::Focused(false) => self.left = None,
            Event::MouseWheel(_) => {
                let mouse = self.left.unwrap_or_else(|| self.is_over(input.pointer()));
                self.hold_mouse(input, mouse);
            }
            Event::ScaleFactorChanged(factor) => self.scale = whole_scale(factor),
            _ => {}
        }
        self.events.handle(event);
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
    /// writes the places if they changed.
    pub(crate) fn frame(&mut self, world: &World, virtual_resolution: UVec2) {
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
        if self.ui.is_none() && !self.failed && self.windows.iter().any(|window| window.open) {
            match make_ui(world) {
                Ok(mut ui) => {
                    restore(&mut ui, &mut self.places);
                    self.saved = ui.places_revision();
                    self.ui = Some(ui);
                }
                Err(cause) => {
                    error!("the debug UI cannot be shown: its font did not load: {cause}");
                    self.failed = true;
                }
            }
        }
        feed(&mut self.input, self.events.events());
        self.events.clear();
        let Some(ui) = &mut self.ui else {
            // Nothing takes the events: keep only where they leave the
            // pointer and what they leave held.
            self.input.clear();
            return;
        };
        ui.style_mut().scale = UiScale::Free(self.scale);
        let output = world.resource::<WindowSize>().0;
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
                        write_trace(&mut self.profiler, self.traces.as_deref());
                    }
                }),
            };
        }
        frame.finish(&mut world.resource_mut::<RenderFrame>().ui);
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

/// Writes the profiler's trace into `folder`, as `trace-<UTC time>.json`,
/// and says where in its note and the log; or why not.
fn write_trace(profiler: &mut Profiler, folder: Option<&Path>) {
    let Some(folder) = folder else {
        profiler.set_note("Traces are written only by App::run.");
        return;
    };
    let path = folder.join(format!("trace-{}.json", DateTime::now().file_stamp()));
    let written = fs::create_dir_all(folder).and_then(|()| fs::write(&path, profiler.trace_json()));
    match written {
        Ok(()) => {
            info!("the profiler's trace is in {}", path.display());
            profiler.set_note(format_args!("Wrote {}", path.display()));
        }
        Err(cause) => {
            warn!(
                "the profiler's trace is not written: {} cannot be: {cause}",
                path.display()
            );
            profiler.set_note(format_args!("Not written: {cause}"));
        }
    }
}

/// The debug UI, in the engine's font, which it loads in the global scope.
fn make_ui(world: &World) -> Result<DebugUi, AssetError> {
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
    Ok(DebugUi::new(font, metrics, white))
}

/// Gives `ui` the places read from the file, by their names: each a
/// window's explicit ID or title, which `set_place` would cut at a `##` it
/// holds unless it comes after one.
fn restore(ui: &mut DebugUi, places: &mut Vec<places::Place>) {
    for place in places.drain(..) {
        ui.set_place(&format!("##{}", place.name), place.rect, place.collapsed);
    }
}

/// Feeds the debug UI's input with `events`, in their order.
fn feed(input: &mut DebugInput, events: &[UiEvent]) {
    for event in events {
        match *event {
            UiEvent::PointerMoved(position) => input.pointer_moved(position),
            UiEvent::Button { button, pressed } => input.button(button, pressed),
            UiEvent::Wheel(lines) => input.wheel(lines),
            UiEvent::Key {
                key,
                pressed,
                repeat,
            } => input.key(key, pressed, repeat),
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
            self.tools.frame(world, UVec2::new(480, 270));
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
        write_trace(&mut tools.profiler, Some(&folder));
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
}
