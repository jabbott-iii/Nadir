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

//! The settings file, `settings.jsonc` in the game's config folder
//! (ADR-0077), which [`App::run`](crate::App::run) reads as the game starts
//! and writes after each frame that changed a setting. nadir-codec's
//! `settings` module reads and writes its text, each setting falling back
//! to its default alone; this module finds the file, applies what it holds
//! to the engine's resources, sees them change, and writes the file whole,
//! through a temporary file, so that a crash leaves the old file or the
//! new.
//!
//! The settings live where the engine already keeps them: the window's in
//! [`DisplaySettings`], the buses' in [`Volumes`], the interface's in
//! [`UiSettings`], the pads' deadzones in [`Input`], and the bindings in
//! the game's [`ActionMap`]. The log's levels are read and applied, and
//! written back as they were read: the debug console's `log` changes them
//! for a run alone.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use nadir_codec::json::Value;
use nadir_codec::settings as file;
use nadir_codec::{CodecError, Limits};
use nadir_core::log::{self, debug, info, warn};
use nadir_ecs::World;
use nadir_input::{ActionMap, ActionSet, AxisSign, Binding, Deadzones, Input, Stick};

use crate::{Config, DisplaySettings, Volumes};

/// The file's name, in the game's config folder.
pub(crate) const FILE_NAME: &str = "settings.jsonc";

// The file's slots are the action map's.
const _: () = assert!(file::SLOTS == nadir_input::SLOTS);

/// The interface's settings: a resource, which [`App::run`](crate::App::run)
/// reads from the settings file as the game starts, and writes to it after
/// a frame that changed it.
///
/// [`App::new`](crate::App::new) starts it at its [`Default`]: no scale of
/// its own, and English.
#[derive(Clone, Debug, PartialEq)]
pub struct UiSettings {
    /// The player's UI scale, for a free scale
    /// ([`UiScale::Free`](nadir_render::UiScale::Free)): output pixels,
    /// which are physical, per UI unit; or `None`, for a scale from the
    /// window's scale factor, as a game's menus have by default. The
    /// settings file takes 1/8 to 64, where the renderer holds a free
    /// scale; one written outside it is written held there.
    ///
    /// With the `dev` feature, the debug UI is drawn at it; at `None`, at
    /// the window's scale factor rounded to a whole number, for its pixel
    /// font. A game's own menus draw at the scale their style gives
    /// ([`MenuStyle::scale`](crate::menu::MenuStyle::scale)).
    pub scale: Option<f32>,
    /// The player's language, as a tag such as `en` or `pt-BR`: 1 to 35
    /// ASCII letters, digits and `-`. The engine reads nothing by it: a
    /// game's text is its own. A tag the file does not take is read back as
    /// the default, with a warning.
    pub language: String,
}

impl Default for UiSettings {
    /// No scale of its own, and `en`.
    fn default() -> Self {
        Self {
            scale: None,
            language: "en".to_string(),
        }
    }
}

/// The game's bindings, kept in the settings file: its action map's,
/// whatever its action set, for [`SettingsFile`].
pub(crate) trait KeptBindings {
    /// The bindings the game gave, as the file writes them.
    fn defaults(&self) -> Vec<file::ActionBindings>;

    /// Puts the file's `bindings` in the world's map: each the action's
    /// kind takes, and the default's, with a warning naming `path`, where
    /// it takes another.
    fn apply(&mut self, world: &World, bindings: &[file::ActionBindings], path: &Path);

    /// Whether the world's map changed since it was applied or written.
    fn changed(&self, world: &World) -> bool;

    /// The world's map, as the file writes it, kept as written.
    fn written(&mut self, world: &World) -> Vec<file::ActionBindings>;
}

/// The bindings of the action set `A`.
pub(crate) struct Kept<A> {
    /// The map the game gave [`App::add_actions`](crate::App::add_actions).
    defaults: ActionMap<A>,
    /// The map as last applied or written.
    written: ActionMap<A>,
}

impl<A: ActionSet> Kept<A> {
    /// The bindings of a game whose default map is `defaults`.
    pub(crate) fn new(defaults: ActionMap<A>) -> Self {
        Self {
            written: defaults.clone(),
            defaults,
        }
    }
}

impl<A: ActionSet> KeptBindings for Kept<A> {
    fn defaults(&self) -> Vec<file::ActionBindings> {
        entries(&self.defaults)
    }

    fn apply(&mut self, world: &World, bindings: &[file::ActionBindings], path: &Path) {
        let mut map = world.resource_mut::<ActionMap<A>>();
        for entry in bindings {
            // The file's reader gives the defaults' actions alone.
            let Some(action) = A::from_name(&entry.action) else {
                continue;
            };
            for (slot, binding) in entry.slots.iter().enumerate() {
                let Some(binding) = binding else {
                    map.unbind(action, slot);
                    continue;
                };
                if let Err(error) = map.bind(action, slot, from_file(*binding)) {
                    warn!(
                        "{}: `input.bindings.{}[{slot}]`: {error}; the slot keeps its default",
                        path.display(),
                        entry.action.escape_debug()
                    );
                    match self.defaults.bindings(action)[slot] {
                        // The default fits: it is the game's.
                        Some(default) => drop(map.bind(action, slot, default)),
                        None => drop(map.unbind(action, slot)),
                    }
                }
            }
        }
        self.written = map.clone();
    }

    fn changed(&self, world: &World) -> bool {
        let map = world.resource::<ActionMap<A>>();
        A::ALL
            .iter()
            .any(|&action| map.bindings(action) != self.written.bindings(action))
    }

    fn written(&mut self, world: &World) -> Vec<file::ActionBindings> {
        let map = world.resource::<ActionMap<A>>();
        self.written = map.clone();
        entries(&map)
    }
}

/// Every action's bindings in `map`, as the file writes them.
fn entries<A: ActionSet>(map: &ActionMap<A>) -> Vec<file::ActionBindings> {
    A::ALL
        .iter()
        .map(|&action| file::ActionBindings {
            action: action.name().to_string(),
            slots: map.bindings(action).map(|slot| slot.map(to_file)),
        })
        .collect()
}

/// `binding` as the file writes it.
fn to_file(binding: Binding) -> file::Binding {
    match binding {
        Binding::Key(key) => file::Binding::Key(key),
        Binding::MouseButton(button) => file::Binding::Mouse(button),
        Binding::KeyComposite2 {
            up,
            down,
            left,
            right,
        } => file::Binding::Keys {
            up,
            down,
            left,
            right,
        },
        Binding::PadButton(button) => file::Binding::Pad(button),
        Binding::PadAxis { axis, sign } => file::Binding::PadAxis {
            axis,
            sign: match sign {
                AxisSign::Negative => file::Sign::Negative,
                AxisSign::Positive => file::Sign::Positive,
            },
        },
        Binding::PadStick(stick) => file::Binding::Stick(match stick {
            Stick::Left => file::Stick::Left,
            Stick::Right => file::Stick::Right,
        }),
    }
}

/// The binding the file's `binding` is.
fn from_file(binding: file::Binding) -> Binding {
    match binding {
        file::Binding::Key(key) => Binding::Key(key),
        file::Binding::Mouse(button) => Binding::MouseButton(button),
        file::Binding::Keys {
            up,
            down,
            left,
            right,
        } => Binding::KeyComposite2 {
            up,
            down,
            left,
            right,
        },
        file::Binding::Pad(button) => Binding::PadButton(button),
        file::Binding::PadAxis { axis, sign } => Binding::PadAxis {
            axis,
            sign: match sign {
                file::Sign::Negative => AxisSign::Negative,
                file::Sign::Positive => AxisSign::Positive,
            },
        },
        file::Binding::Stick(stick) => Binding::PadStick(match stick {
            file::Stick::Left => Stick::Left,
            file::Stick::Right => Stick::Right,
        }),
    }
}

/// What became of the file that was there as the game started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OldFile {
    /// There was none, or it was read: it is written over.
    Read,
    /// It could not be read: the first write keeps it beside, as
    /// `settings.jsonc.bad`, so that a hand edit gone wrong is not lost.
    Unreadable,
    /// A newer game wrote it: it is never written over, and the settings
    /// changed in this run last for the run. `warned` once a change said so.
    Newer { warned: bool },
}

/// The settings the engine's resources hold, as last read or written.
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    display: DisplaySettings,
    volumes: Volumes,
    ui: UiSettings,
    deadzones: Deadzones,
}

impl Snapshot {
    fn of(world: &World) -> Self {
        Self {
            display: *world.resource::<DisplaySettings>(),
            volumes: *world.resource::<Volumes>(),
            ui: world.resource::<UiSettings>().clone(),
            deadzones: world.resource::<Input>().deadzones(),
        }
    }

    /// Whether the world holds these settings. It allocates nothing.
    fn holds(&self, world: &World) -> bool {
        *world.resource::<DisplaySettings>() == self.display
            && *world.resource::<Volumes>() == self.volumes
            && *world.resource::<UiSettings>() == self.ui
            && world.resource::<Input>().deadzones() == self.deadzones
    }

    /// The file's settings for these, `bindings` and `log`.
    fn to_file(&self, bindings: Vec<file::ActionBindings>, log: &file::Log) -> file::Settings {
        let Volumes {
            music,
            sfx,
            ui,
            ambience,
        } = self.volumes;
        let zones = self.deadzones;
        file::Settings {
            display: file::Display {
                fullscreen: self.display.fullscreen,
                vsync: self.display.vsync,
            },
            audio: file::Audio {
                // A percent above 100 counts as 100 (`Volumes`).
                music: music.min(100),
                sfx: sfx.min(100),
                ui: ui.min(100),
                ambience: ambience.min(100),
            },
            ui: file::Ui {
                scale: self.ui.scale.map(held_scale),
                language: self.ui.language.clone(),
            },
            input: file::Input {
                deadzones: file::Deadzones {
                    stick_inner: decimal(zones.stick_inner),
                    stick_outer: decimal(zones.stick_outer),
                    trigger: decimal(zones.trigger),
                    press: decimal(zones.press),
                    release: decimal(zones.release),
                },
                bindings,
            },
            log: log.clone(),
        }
    }
}

/// The settings file of a running game: where it is, what it held, and
/// what was last written to it.
pub(crate) struct SettingsFile {
    path: PathBuf,
    /// The file as it was read, written over so that what it holds and the
    /// engine does not know stays; `None` if there was none, or it could not
    /// be read.
    source: Option<Value>,
    /// The log's levels, as read: written back as they were.
    log: file::Log,
    written: Snapshot,
    old: OldFile,
}

impl SettingsFile {
    /// Reads the settings file at `path` and applies it: to `config`'s
    /// fullscreen, vsync and volumes, before the window opens; to the
    /// world's [`DisplaySettings`], [`Volumes`], [`UiSettings`] and
    /// [`Input`]'s deadzones; to the log; and to the game's bindings.
    ///
    /// What the file does not give is the defaults: what those resources
    /// and bindings hold now, and the log's level `info`. A file that is
    /// not there is the defaults, quietly; one that cannot be read, the
    /// defaults, with a warning. The buses are the caller's to set to the
    /// [`Volumes`].
    pub(crate) fn load(
        path: PathBuf,
        config: &mut Config,
        world: &World,
        bindings: Option<&mut (dyn KeptBindings + 'static)>,
    ) -> Self {
        let defaults = defaults(world, bindings.as_deref());
        let shown = path.display();
        let unreadable = |cause: &dyn std::fmt::Display| {
            warn!(
                "{shown}: {cause}; the game starts with its default settings, and keeps this file \
                 as {FILE_NAME}.bad when it next writes them"
            );
            OldFile::Unreadable
        };
        let (settings, source, old) = match read_text(&path) {
            Ok(None) => {
                info!("no settings file at {shown} yet: the game starts with its defaults");
                (defaults, None, OldFile::Read)
            }
            Ok(Some(text)) => match file::read(&text, &defaults, Limits::default()) {
                Ok(loaded) => {
                    for warning in &loaded.warnings {
                        warn!("{shown}: {warning}");
                    }
                    (loaded.settings, Some(loaded.source), OldFile::Read)
                }
                Err(CodecError::Settings {
                    kind: file::ErrorKind::NewerVersion { found },
                }) => {
                    warn!(
                        "{shown} is of version {found}, which a newer game wrote, and this one \
                         reads version {} and older: the game starts with its default settings, \
                         and does not write over the file, so the settings changed in this run \
                         last for the run",
                        file::VERSION
                    );
                    (defaults, None, OldFile::Newer { warned: false })
                }
                Err(error) => {
                    let old = unreadable(&error);
                    (defaults, None, old)
                }
            },
            Err(cause) => {
                let old = unreadable(&cause);
                (defaults, None, old)
            }
        };
        apply(&settings, config, world, bindings, &path);
        Self {
            path,
            source,
            log: settings.log,
            written: Snapshot::of(world),
            old,
        }
    }

    /// After a frame: writes the file if a setting changed since it was
    /// read or last written. It allocates nothing when none did.
    ///
    /// A write that fails is a warning, and is tried again at the next
    /// change, not at each frame.
    pub(crate) fn save_if_changed(
        &mut self,
        world: &World,
        bindings: Option<&mut (dyn KeptBindings + 'static)>,
    ) {
        let bindings_changed = bindings.as_deref().is_some_and(|kept| kept.changed(world));
        if !bindings_changed && self.written.holds(world) {
            return;
        }
        self.written = Snapshot::of(world);
        let bindings = bindings.map_or_else(Vec::new, |kept| kept.written(world));
        if let OldFile::Newer { warned } = &mut self.old {
            if !*warned {
                warn!(
                    "the settings changed, and last for this run: {} is a newer game's, which \
                     this one does not write over",
                    self.path.display()
                );
                *warned = true;
            }
            return;
        }
        if self.old == OldFile::Unreadable {
            let mut bad = self.path.clone();
            bad.as_mut_os_string().push(".bad");
            match fs::rename(&self.path, &bad) {
                Ok(()) => info!(
                    "the settings file that could not be read is kept as {}",
                    bad.display()
                ),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    warn!(
                        "the settings are not written: {} cannot be kept as {}, and would be \
                         lost: {error}",
                        self.path.display(),
                        bad.display()
                    );
                    return;
                }
            }
            self.old = OldFile::Read;
        }
        let settings = self.written.to_file(bindings, &self.log);
        let text = file::write(&settings, self.source.as_ref());
        match write_whole(&self.path, &text) {
            Ok(()) => debug!("the settings are written to {}", self.path.display()),
            Err(cause) => warn!(
                "the settings are not kept: {} cannot be written: {cause}",
                self.path.display()
            ),
        }
    }
}

/// The settings the engine holds now, for the file's reader to fall back
/// to: the world's resources, the game's default bindings, and the log's
/// level `info` with no target's own.
fn defaults(world: &World, bindings: Option<&dyn KeptBindings>) -> file::Settings {
    let snapshot = Snapshot::of(world);
    let log = file::Log {
        level: log::Level::Info,
        targets: Vec::new(),
    };
    let bindings = bindings.map_or_else(Vec::new, KeptBindings::defaults);
    snapshot.to_file(bindings, &log)
}

/// Applies `settings`, read from the file at `path`.
fn apply(
    settings: &file::Settings,
    config: &mut Config,
    world: &World,
    bindings: Option<&mut (dyn KeptBindings + 'static)>,
    path: &Path,
) {
    let display = DisplaySettings {
        fullscreen: settings.display.fullscreen,
        vsync: settings.display.vsync,
    };
    config.fullscreen = display.fullscreen;
    config.vsync = display.vsync;
    *world.resource_mut::<DisplaySettings>() = display;
    let audio = settings.audio;
    let volumes = Volumes {
        music: audio.music,
        sfx: audio.sfx,
        ui: audio.ui,
        ambience: audio.ambience,
    };
    config.volumes = volumes;
    *world.resource_mut::<Volumes>() = volumes;
    *world.resource_mut::<UiSettings>() = UiSettings {
        scale: settings.ui.scale.map(|scale| scale as f32),
        language: settings.ui.language.clone(),
    };
    let zones = settings.input.deadzones;
    let deadzones = Deadzones {
        stick_inner: zones.stick_inner as f32,
        stick_outer: zones.stick_outer as f32,
        trigger: zones.trigger as f32,
        press: zones.press as f32,
        release: zones.release as f32,
    };
    // Refused, the deadzones are left as they were: the defaults.
    if let Err(error) = world.resource_mut::<Input>().set_deadzones(deadzones) {
        warn!(
            "{}: `input.deadzones`: {error}; the deadzones keep their defaults",
            path.display()
        );
    }
    log::set_level(settings.log.level);
    for (target, level) in &settings.log.targets {
        log::set_target_level(target, Some(*level));
    }
    if let Some(bindings) = bindings {
        bindings.apply(world, &settings.input.bindings, path);
    }
}

/// `scale` held where the file takes it, as the renderer holds a free
/// scale: 1/8 to 64, and NaN 1.
fn held_scale(scale: f32) -> f64 {
    if scale.is_nan() {
        1.0
    } else {
        decimal(scale).clamp(file::MIN_UI_SCALE, file::MAX_UI_SCALE)
    }
}

/// The `f64` of `value`'s shortest decimal, so that the file says `0.2`
/// for the `f32` 0.2, not `0.20000000298023224`.
fn decimal(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::from(value))
}

/// The file's text, or `None` if it is not there.
fn read_text(path: &Path) -> Result<Option<String>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let limit = file::MAX_BYTES as u64;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!("it is larger than {limit} bytes"));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "it is not UTF-8".to_string())
}

/// Writes `text` to `path`, through a temporary file beside it that is
/// synced and then renamed over it, making the folder if it is not there.
fn write_whole(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut temporary = PathBuf::from(path);
    temporary.as_mut_os_string().push(".tmp");
    let mut file = File::create(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use nadir_core::{GamepadButton, Key};
    use nadir_input::ActionKind;

    use super::*;
    use crate::App;

    /// A game's actions: one of each kind of binding the tests need.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Act {
        Move,
        Jump,
    }

    impl ActionSet for Act {
        const ALL: &'static [Self] = &[Act::Move, Act::Jump];

        fn index(self) -> usize {
            self as usize
        }

        fn name(self) -> &'static str {
            match self {
                Act::Move => "move",
                Act::Jump => "jump",
            }
        }

        fn kind(self) -> ActionKind {
            match self {
                Act::Move => ActionKind::Axis2,
                Act::Jump => ActionKind::Button,
            }
        }
    }

    const WASD: Binding = Binding::KeyComposite2 {
        up: Key::W,
        down: Key::S,
        left: Key::A,
        right: Key::D,
    };

    /// The game's defaults: WASD and the left stick move; Space and the
    /// pad's A jump.
    fn defaults_map() -> ActionMap<Act> {
        let mut map = ActionMap::new().expect("a valid set");
        map.bind(Act::Move, 0, WASD).expect("it fits");
        map.bind(Act::Move, 1, Binding::PadStick(Stick::Left))
            .expect("it fits");
        map.bind(Act::Jump, 0, Binding::Key(Key::Space))
            .expect("it fits");
        map.bind(Act::Jump, 1, Binding::PadButton(GamepadButton::A))
            .expect("it fits");
        map
    }

    /// An empty folder of the test's own, removed as it drops.
    struct Folder(PathBuf);

    impl Folder {
        fn new(test: &str) -> Self {
            let dir =
                env::temp_dir().join(format!("nadir-engine-settings-{test}-{}", process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("the folder is made");
            Self(dir)
        }

        /// The settings file's path in it.
        fn file(&self) -> PathBuf {
            self.0.join(FILE_NAME)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A run of the game, its settings read from the file at `path`.
    struct Run {
        app: App,
        kept: Kept<Act>,
        config: Config,
        file: SettingsFile,
    }

    impl Run {
        fn start(path: &Path) -> Self {
            let app = App::new(Config::default()).add_actions(defaults_map());
            let mut kept = Kept::new(defaults_map());
            let mut config = Config::default();
            let file = SettingsFile::load(
                path.to_path_buf(),
                &mut config,
                app.world(),
                Some(&mut kept),
            );
            Self {
                app,
                kept,
                config,
                file,
            }
        }

        fn world(&self) -> &World {
            self.app.world()
        }

        /// The end of a frame: the file is written if a setting changed.
        fn end_frame(&mut self) {
            self.file
                .save_if_changed(self.app.world(), Some(&mut self.kept));
        }

        fn slots(&self, action: Act) -> [Option<Binding>; nadir_input::SLOTS] {
            *self.world().resource::<ActionMap<Act>>().bindings(action)
        }
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    }

    /// The file's value tree.
    fn tree(path: &Path) -> Value {
        nadir_codec::json::parse(&read(path), Limits::default()).expect("JSONC")
    }

    /// The records of the log so far that hold every one of `parts`.
    fn logged(parts: &[&str]) -> usize {
        log::tail(log::RING_CAPACITY)
            .iter()
            .filter(|record| parts.iter().all(|part| record.contains(part)))
            .count()
    }

    #[test]
    fn with_no_file_the_game_starts_at_its_defaults_and_writes_one_once_a_setting_changes() {
        let folder = Folder::new("none");
        let path = folder.file();
        let mut run = Run::start(&path);
        assert_eq!(run.config, Config::default());
        assert_eq!(
            *run.world().resource::<DisplaySettings>(),
            DisplaySettings {
                fullscreen: false,
                vsync: true
            }
        );
        assert_eq!(*run.world().resource::<Volumes>(), Volumes::default());
        assert_eq!(*run.world().resource::<UiSettings>(), UiSettings::default());
        assert_eq!(run.slots(Act::Move), *defaults_map().bindings(Act::Move));
        run.end_frame();
        assert!(!path.exists(), "nothing changed, so nothing is written");
        run.world().resource_mut::<DisplaySettings>().fullscreen = true;
        run.world().resource_mut::<Volumes>().music = 40;
        run.end_frame();
        assert!(path.exists());
        assert!(read(&path).starts_with(file::HEADER));
        // The next run starts as this one left them.
        let next = Run::start(&path);
        assert!(next.config.fullscreen);
        assert_eq!(next.config.volumes.music, 40);
        assert!(next.world().resource::<DisplaySettings>().fullscreen);
        assert_eq!(next.world().resource::<Volumes>().music, 40);
    }

    #[test]
    fn every_setting_the_file_gives_is_applied_as_the_game_starts() {
        let folder = Folder::new("every");
        let path = folder.file();
        fs::write(
            &path,
            r#"{
              "version": 1,
              "display": {"fullscreen": true, "vsync": false},
              "audio": {"music": 10, "sfx": 20, "ui": 30, "ambience": 40},
              "ui": {"scale": 1.5, "language": "fr"},
              "input": {
                "deadzones": {"stick_inner": 0.1, "stick_outer": 0.9, "trigger": 0.2,
                              "press": 0.7, "release": 0.6},
                "bindings": {
                  "move": [{"keys": {"up": "Up", "down": "Down", "left": "Left", "right": "Right"}}],
                  "jump": [null, {"pad": "B"}, {"pad_axis": "RightTrigger", "sign": "+"}]
                }
              },
              "log": {"level": "info", "targets": {"nadir_engine_settings_test_target": "trace"}}
            }"#,
        )
        .expect("written");
        let run = Run::start(&path);
        assert!(run.config.fullscreen);
        assert!(!run.config.vsync);
        let volumes = Volumes {
            music: 10,
            sfx: 20,
            ui: 30,
            ambience: 40,
        };
        assert_eq!(run.config.volumes, volumes);
        assert_eq!(*run.world().resource::<Volumes>(), volumes);
        assert_eq!(
            *run.world().resource::<DisplaySettings>(),
            DisplaySettings {
                fullscreen: true,
                vsync: false
            }
        );
        assert_eq!(
            *run.world().resource::<UiSettings>(),
            UiSettings {
                scale: Some(1.5),
                language: "fr".to_string()
            }
        );
        assert_eq!(
            run.world().resource::<Input>().deadzones(),
            Deadzones {
                stick_inner: 0.1,
                stick_outer: 0.9,
                trigger: 0.2,
                press: 0.7,
                release: 0.6
            }
        );
        assert_eq!(
            run.slots(Act::Move),
            [
                Some(Binding::KeyComposite2 {
                    up: Key::Up,
                    down: Key::Down,
                    left: Key::Left,
                    right: Key::Right
                }),
                None,
                None,
                None
            ],
            "the slots past the array's end are empty"
        );
        assert_eq!(
            run.slots(Act::Jump),
            [
                None,
                Some(Binding::PadButton(GamepadButton::B)),
                Some(Binding::PadAxis {
                    axis: nadir_core::GamepadAxis::RightTrigger,
                    sign: AxisSign::Positive
                }),
                None
            ]
        );
        let target = "nadir_engine_settings_test_target";
        assert!(log::enabled(log::Level::Trace, target));
        log::set_target_level(target, None);
    }

    #[test]
    fn a_setting_out_of_its_range_keeps_its_default_with_a_warning() {
        let folder = Folder::new("range");
        let path = folder.file();
        fs::write(
            &path,
            r#"{"version": 1, "audio": {"music": 500, "sfx": 60}, "display": {"vsync": "off"}}"#,
        )
        .expect("written");
        let run = Run::start(&path);
        assert_eq!(run.world().resource::<Volumes>().music, 100);
        assert_eq!(run.world().resource::<Volumes>().sfx, 60);
        assert!(run.config.vsync);
        let shown = path.display().to_string();
        assert_eq!(
            logged(&[&shown, "`audio.music` is not a whole number from 0 to 100"]),
            1
        );
        assert_eq!(
            logged(&[&shown, "`display.vsync` is not `true` or `false`"]),
            1
        );
    }

    #[test]
    fn a_binding_its_actions_kind_does_not_take_keeps_the_slots_default() {
        let folder = Folder::new("kinds");
        let path = folder.file();
        fs::write(
            &path,
            r#"{"version": 1, "input": {"bindings": {
                 "move": [{"key": "W"}, {"stick": "Right"}, {"pad": "A"}],
                 "jump": [{"key": "J"}, {"keys": {"up": "W", "down": "S", "left": "A", "right": "D"}}]
               }}}"#,
        )
        .expect("written");
        let run = Run::start(&path);
        // A key cannot move, so slot 0 keeps WASD and slot 2 stays empty;
        // the right stick can.
        assert_eq!(
            run.slots(Act::Move),
            [
                Some(WASD),
                Some(Binding::PadStick(Stick::Right)),
                None,
                None
            ]
        );
        // Four keys cannot jump, so slot 1 keeps the pad's A.
        assert_eq!(
            run.slots(Act::Jump),
            [
                Some(Binding::Key(Key::J)),
                Some(Binding::PadButton(GamepadButton::A)),
                None,
                None
            ]
        );
        let shown = path.display().to_string();
        assert_eq!(
            logged(&[
                &shown,
                "`input.bindings.move[0]`",
                "the slot keeps its default"
            ]),
            1
        );
        assert_eq!(logged(&[&shown, "`input.bindings.move[2]`"]), 1);
        assert_eq!(logged(&[&shown, "`input.bindings.jump[1]`"]), 1);
    }

    #[test]
    fn deadzones_that_break_their_rules_together_keep_their_defaults() {
        let folder = Folder::new("deadzones");
        let path = folder.file();
        // Each is from 0 to 1, but the inner edge is past the outer.
        fs::write(
            &path,
            r#"{"version": 1, "input": {"deadzones": {"stick_inner": 0.9, "stick_outer": 0.5}}}"#,
        )
        .expect("written");
        let run = Run::start(&path);
        assert_eq!(
            run.world().resource::<Input>().deadzones(),
            Deadzones::DEFAULT
        );
        let shown = path.display().to_string();
        assert_eq!(logged(&[&shown, "the deadzones keep their defaults"]), 1);
        // One that keeps the rules with the defaults is taken alone.
        let folder = Folder::new("deadzone");
        let path = folder.file();
        fs::write(
            &path,
            r#"{"version": 1, "input": {"deadzones": {"trigger": 0.3}}}"#,
        )
        .expect("written");
        let run = Run::start(&path);
        assert_eq!(
            run.world().resource::<Input>().deadzones(),
            Deadzones {
                trigger: 0.3,
                ..Deadzones::DEFAULT
            }
        );
    }

    #[test]
    fn a_file_that_cannot_be_read_is_the_defaults_and_is_kept_aside_when_the_settings_are_written()
    {
        for (test, bytes) in [
            ("jsonc", b"{ \"version\": 1, ".as_slice()),
            (
                "utf8",
                b"{\"version\": 1, \"ui\": {\"language\": \"\xff\"}}".as_slice(),
            ),
            (
                "version",
                b"{\"display\": {\"fullscreen\": true}}".as_slice(),
            ),
        ] {
            let folder = Folder::new(&format!("unreadable-{test}"));
            let path = folder.file();
            fs::write(&path, bytes).expect("written");
            let mut run = Run::start(&path);
            assert_eq!(run.config, Config::default(), "{test}");
            let shown = path.display().to_string();
            assert_eq!(
                logged(&[&shown, "keeps this file as settings.jsonc.bad"]),
                1,
                "{test}"
            );
            run.end_frame();
            assert_eq!(
                fs::read(&path).expect("there"),
                bytes,
                "untouched until a change"
            );
            run.world().resource_mut::<Volumes>().ui = 0;
            run.end_frame();
            let mut bad = path.clone();
            bad.as_mut_os_string().push(".bad");
            assert_eq!(fs::read(&bad).expect("kept"), bytes, "{test}");
            assert_eq!(Run::start(&path).config.volumes.ui, 0, "{test}");
        }
    }

    #[test]
    fn a_newer_games_file_is_the_defaults_and_is_never_written_over() {
        let folder = Folder::new("newer");
        let path = folder.file();
        let text = r#"{"version": 2, "display": {"fullscreen": true}}"#;
        fs::write(&path, text).expect("written");
        let mut run = Run::start(&path);
        assert!(!run.config.fullscreen);
        run.world().resource_mut::<Volumes>().music = 0;
        run.end_frame();
        run.world().resource_mut::<Volumes>().music = 10;
        run.end_frame();
        assert_eq!(read(&path), text);
        let shown = path.display().to_string();
        assert_eq!(logged(&[&shown, "is of version 2"]), 1);
        assert_eq!(logged(&[&shown, "last for this run: "]), 1, "said once");
    }

    #[test]
    fn what_the_file_holds_and_the_game_does_not_know_is_written_back() {
        let folder = Folder::new("unknown");
        let path = folder.file();
        fs::write(
            &path,
            r#"// mine
            {"version": 1, "theme": "dark", "audio": {"master": 70},
             "input": {"bindings": {"dash": [{"key": "LeftShift"}]}}}"#,
        )
        .expect("written");
        let mut run = Run::start(&path);
        let shown = path.display().to_string();
        assert_eq!(logged(&[&shown, "`theme` is not a setting"]), 1);
        assert_eq!(
            logged(&[
                &shown,
                "`input.bindings.dash` is not one of the game's actions"
            ]),
            1
        );
        run.world().resource_mut::<Volumes>().sfx = 90;
        run.end_frame();
        let tree = tree(&path);
        assert_eq!(tree.get("theme"), Some(&Value::String("dark".to_string())));
        let audio = tree.get("audio").expect("audio");
        assert_eq!(audio.get("master"), Some(&Value::Int(70)));
        assert_eq!(audio.get("sfx"), Some(&Value::Int(90)));
        let bindings = tree
            .get("input")
            .and_then(|input| input.get("bindings"))
            .expect("bindings");
        assert!(bindings.get("dash").is_some());
        assert!(bindings.get("move").is_some());
        assert!(!read(&path).contains("// mine"), "comments are not kept");
    }

    #[test]
    fn a_rebinding_is_written_and_read_back_on_the_next_run() {
        let folder = Folder::new("rebind");
        let path = folder.file();
        let mut run = Run::start(&path);
        run.world()
            .resource_mut::<ActionMap<Act>>()
            .bind(
                Act::Jump,
                2,
                Binding::MouseButton(nadir_core::MouseButton::Right),
            )
            .expect("it fits");
        run.end_frame();
        let next = Run::start(&path);
        assert_eq!(
            next.slots(Act::Jump)[2],
            Some(Binding::MouseButton(nadir_core::MouseButton::Right))
        );
        // Unbound, it is written empty.
        run.world()
            .resource_mut::<ActionMap<Act>>()
            .unbind(Act::Jump, 0);
        run.end_frame();
        assert_eq!(Run::start(&path).slots(Act::Jump)[0], None);
    }

    #[test]
    fn the_ui_scale_and_the_deadzones_are_written_as_the_file_takes_them() {
        let folder = Folder::new("held");
        let path = folder.file();
        let mut run = Run::start(&path);
        run.world().resource_mut::<UiSettings>().scale = Some(100.0);
        run.world()
            .resource_mut::<Input>()
            .set_deadzones(Deadzones {
                trigger: 0.3,
                ..Deadzones::DEFAULT
            })
            .expect("valid");
        run.end_frame();
        let text = read(&path);
        assert!(text.contains("\"scale\": 64.0"), "{text}");
        assert!(text.contains("\"trigger\": 0.3,"), "{text}");
        assert!(text.contains("\"stick_inner\": 0.2,"), "{text}");
        run.world().resource_mut::<UiSettings>().scale = Some(f32::NAN);
        run.end_frame();
        assert!(read(&path).contains("\"scale\": 1.0"));
        run.world().resource_mut::<UiSettings>().scale = None;
        run.end_frame();
        assert!(read(&path).contains("\"scale\": null"));
    }

    #[test]
    fn a_write_that_fails_is_warned_of_once_and_the_game_runs_on() {
        // A folder where the temporary file goes, whoever runs the test.
        let folder = Folder::new("fails");
        let path = folder.file();
        let mut temporary = path.clone();
        temporary.as_mut_os_string().push(".tmp");
        fs::create_dir(&temporary).expect("made");
        let mut run = Run::start(&path);
        run.world().resource_mut::<Volumes>().music = 5;
        run.end_frame();
        run.end_frame();
        let shown = path.display().to_string();
        assert_eq!(
            logged(&["the settings are not kept", &shown, "cannot be written"]),
            1,
            "once, not each frame"
        );
        assert!(!path.exists());
        // Tried again at the next change, which can be written.
        fs::remove_dir(&temporary).expect("removed");
        run.world().resource_mut::<Volumes>().music = 6;
        run.end_frame();
        assert_eq!(Run::start(&path).config.volumes.music, 6);
    }

    #[cfg(feature = "dev")]
    #[test]
    fn a_frame_that_changes_no_setting_allocates_nothing() {
        let folder = Folder::new("allocations");
        let path = folder.file();
        let mut run = Run::start(&path);
        run.end_frame();
        let before = nadir_platform::thread_alloc_counts();
        for _ in 0..10 {
            run.end_frame();
        }
        let after = nadir_platform::thread_alloc_counts();
        assert_eq!(after.allocations, before.allocations);
    }
}
