//! The sandbox's replays (roadmap M4.7, ADR-0025): `--record FILE` writes
//! the session's input as it comes, `--replay FILE` plays one in place of
//! the keys, and `--headless` plays one with no window, as fast as it runs.
//!
//! Each logs how many ticks it recorded or played, and the world's hash
//! after them: a replay of a recording, in the same scene, on the same
//! system, logs the same. After a failed write, a recording logs instead
//! how many ticks reached its file. A recording logs when the run ends cleanly, with
//! the window closed; after a crash or a kill its file keeps every tick,
//! but nothing is logged.

use std::cell::RefCell;
use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use nadir::input::{InputPlayer, InputRecorder};
use nadir::log::{error, info};
use nadir::prelude::*;
use nadir::{Replay, world_hash};

use crate::{Act, Report};

/// One tick's length, as a frame: it runs one tick.
const TICK: Duration = Duration::from_nanos(16_666_667);

/// What `--record` shares between the game, which records each tick, and
/// `main`, which reports when the run is over.
#[derive(Clone)]
pub(crate) struct Recording(Rc<RefCell<Session>>);

/// A recording's state.
struct Session {
    recorder: InputRecorder<Act>,
    /// The file, until a write to it fails.
    file: Option<File>,
    path: PathBuf,
    /// The bytes of the recording written to the file so far.
    written: usize,
    /// The ticks recorded, and of those, the ticks written to the file.
    ticks: u64,
    ticks_written: u64,
    /// The world's hash after the last frame's ticks.
    hash: u64,
}

/// Records each of `app`'s ticks into a new file at `path`, written as the
/// ticks come, so that a crash keeps what came before it. The file holds a
/// replay of no ticks from the start.
///
/// It hashes the world after each frame that runs a tick, which allocates
/// and takes time: about 16 ms a frame for `stress-sprites` in a debug
/// build. So the frame rate and the allocations that a recording run
/// reports are not the scene's own.
pub(crate) fn record(app: App, path: &Path) -> Result<(App, Recording), Box<dyn Error>> {
    let cannot = |error| format!("cannot write the replay {}: {error}", path.display());
    let mut file = File::create(path).map_err(cannot)?;
    let recorder = InputRecorder::new()?;
    file.write_all(recorder.bytes()).map_err(cannot)?;
    let recording = Recording(Rc::new(RefCell::new(Session {
        written: recorder.bytes().len(),
        recorder,
        file: Some(file),
        path: path.to_path_buf(),
        ticks: 0,
        ticks_written: 0,
        hash: world_hash(app.world()),
    })));
    let app = app
        .insert_resource(recording.clone())
        .add_fixed_system(FixedStage::Input, "record_input", record_input)
        .add_frame_system(FrameStage::Ui, "hash_recording", hash_recording);
    Ok((app, recording))
}

/// Adds this tick's actions to the recording and its file.
fn record_input(world: &World, _tick: &Tick) {
    let state = world.resource::<ActionState<Act>>();
    let recording = world.resource::<Recording>();
    let mut session = recording.0.borrow_mut();
    let Session {
        recorder,
        file,
        path,
        written,
        ticks,
        ticks_written,
        ..
    } = &mut *session;
    recorder.record(&state);
    *ticks += 1;
    if let Some(out) = file {
        match out.write_all(&recorder.bytes()[*written..]) {
            Ok(()) => *ticks_written += 1,
            Err(problem) => {
                error!(
                    "cannot write the replay {}: {problem}; it stops after {ticks_written} ticks",
                    path.display()
                );
                *file = None;
            }
        }
    }
    *written = recorder.bytes().len();
}

/// Notes the world's hash after the frame's ticks, if it had any: the
/// last is the one a replay of the recording should reach. It allocates,
/// so `--record` shows allocations in the report.
fn hash_recording(world: &World, _frame: &Frame) {
    if world.resource::<TickStats>().ticks == 0 {
        return;
    }
    let hash = world_hash(world);
    world.resource::<Recording>().0.borrow_mut().hash = hash;
}

impl Recording {
    /// Logs the ticks recorded, and the world's hash after them; or, if
    /// the file was lost, how many ticks reached it.
    pub(crate) fn report(&self) {
        let session = self.0.borrow();
        if session.file.is_some() {
            info!(
                "recorded {} ticks into {}; world hash {:016x}",
                session.ticks,
                session.path.display(),
                session.hash
            );
        } else {
            error!(
                "recorded {} ticks, but only {} reached {}: a write failed",
                session.ticks,
                session.ticks_written,
                session.path.display()
            );
        }
    }

    /// The recording so far.
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> Vec<u8> {
        self.0.borrow().recorder.bytes().to_vec()
    }

    /// The ticks recorded and written, and the world's hash after the last
    /// frame's ticks.
    #[cfg(test)]
    fn counts(&self) -> (u64, u64, u64) {
        let session = self.0.borrow();
        (session.ticks, session.ticks_written, session.hash)
    }
}

/// The ticks the replay played and the world's hash after them, once
/// logged at its end.
struct ReplayLogged(Option<(u64, u64)>);

/// Plays the replay in the file at `path` into `app`'s actions, from its
/// first tick ([`App::play_replay`]). It must have been recorded in the same
/// scene: nothing in a replay says which.
pub(crate) fn play(app: App, path: &Path) -> Result<App, Box<dyn Error>> {
    let bytes = fs::read(path)
        .map_err(|error| format!("cannot read the replay {}: {error}", path.display()))?;
    // The replay plays for the rest of the run, so its bytes live as long.
    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    let player = InputPlayer::<Act>::new(bytes)
        .map_err(|error| format!("{} is not a replay of the sandbox: {error}", path.display()))?;
    Ok(app
        .play_replay(player)
        .insert_resource(ReplayLogged(None))
        .add_fixed_system(FixedStage::Input, "log_replay_end", log_replay_end))
}

/// In the first tick after the replay, before the tick moves anything:
/// logs the ticks it played and the world's hash after them, once.
fn log_replay_end(world: &World, _tick: &Tick) {
    if world.resource::<ReplayLogged>().0.is_some() {
        return;
    }
    let replay = world.resource::<Replay>().clone();
    if let Some(logged) = log_end(&replay, world_hash(world)) {
        world.resource_mut::<ReplayLogged>().0 = Some(logged);
    }
}

/// Logs how the replay ended, if it has, and returns the ticks it played
/// and `hash`.
fn log_end(replay: &Replay, hash: u64) -> Option<(u64, u64)> {
    match *replay {
        Replay::Ended { ticks } => {
            info!("replayed {ticks} ticks; world hash {hash:016x}");
            Some((ticks, hash))
        }
        Replay::Failed { ticks, ref error } => {
            error!("the replay failed after {ticks} ticks: {error}; world hash {hash:016x}");
            Some((ticks, hash))
        }
        Replay::Playing { .. } => None,
    }
}

/// Runs `app`, which plays a replay, with no window: frames of one tick,
/// as fast as they run, until the replay ends. Then logs its ticks and the
/// world's hash after them, and returns both.
///
/// # Errors
///
/// If the replay has a damaged tick.
pub(crate) fn headless(mut app: App) -> Result<(u64, u64), Box<dyn Error>> {
    // The once-a-second report goes by the wall clock, which this run does
    // not keep to.
    app.world().resource_mut::<Report>().rate.reported_at = Duration::MAX;
    loop {
        let replay = app.world().resource::<Replay>().clone();
        match &replay {
            Replay::Playing { .. } => app.update(TICK, &[]),
            Replay::Ended { ticks } => {
                let hash = app.world_hash();
                log_end(&replay, hash);
                return Ok((*ticks, hash));
            }
            Replay::Failed { ticks, error } => {
                log_end(&replay, app.world_hash());
                return Err(format!("the replay failed after {ticks} ticks: {error}").into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process;

    use super::*;
    use crate::Scene;
    use crate::tests::scene;

    fn key(key: Key, pressed: bool) -> nadir::Event {
        nadir::Event::Key {
            key,
            pressed,
            repeat: false,
        }
    }

    /// A file for a test's replay, which the test removes.
    fn replay_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("nadir-sandbox-{name}-{}.nrpl", process::id()))
    }

    /// Records a walk in the field: 120 ticks, a frame each. Returns the
    /// world's hash before the first tick and after each.
    fn record_a_walk(path: &Path) -> (Vec<u64>, Recording) {
        let (mut app, recording) = record(scene(Scene::Field), path).expect("a recording");
        // D, then W too, then A in place of D, then W alone.
        let script = [
            (0, Key::D, true),
            (30, Key::W, true),
            (60, Key::D, false),
            (60, Key::A, true),
            (100, Key::A, false),
        ];
        let mut hashes = vec![app.world_hash()];
        for frame in 0..120 {
            let events: Vec<_> = script
                .iter()
                .filter(|&&(at, _, _)| at == frame)
                .map(|&(_, code, pressed)| key(code, pressed))
                .collect();
            app.update(TICK, &events);
            hashes.push(app.world_hash());
        }
        (hashes, recording)
    }

    #[test]
    fn a_recorded_walk_replays_to_the_same_world_whatever_the_frames() {
        let path = replay_file("walk");
        let (hashes, recording) = record_a_walk(&path);
        let written = fs::read(&path).expect("the replay's file");
        assert_eq!(written, recording.bytes(), "written as the ticks came");
        assert_ne!(hashes[0], hashes[120], "the person moved");
        // What the recording logs at its end.
        assert_eq!(recording.counts(), (120, 120, hashes[120]));

        let mut app = play(scene(Scene::Field), &path).expect("a replay");
        fs::remove_file(&path).expect("remove the replay's file");
        let pacing = [
            Duration::from_nanos(25_000_000),
            Duration::ZERO,
            Duration::from_nanos(4_166_667),
            Duration::from_nanos(66_666_667),
            Duration::from_nanos(8_333_333),
            TICK,
        ];
        let mut compared = 0;
        for frame in 0..1_000 {
            if !matches!(*app.world().resource::<Replay>(), Replay::Playing { .. }) {
                break;
            }
            app.update(pacing[frame % pacing.len()], &[]);
            if let Replay::Playing { ticks } = *app.world().resource::<Replay>() {
                let ticks = usize::try_from(ticks).expect("a few");
                assert_eq!(app.world_hash(), hashes[ticks], "after tick {ticks}");
                compared += 1;
            }
        }
        assert_eq!(
            *app.world().resource::<Replay>(),
            Replay::Ended { ticks: 120 }
        );
        assert!(compared > 30, "{compared} frames compared");
        // The first live tick logs the replay's end, with the world as its
        // last tick left it, once; D held since changes nothing logged.
        app.update(TICK, &[key(Key::D, true)]);
        for _ in 0..10 {
            app.update(TICK, &[]);
        }
        let logged = app.world().resource::<ReplayLogged>().0;
        assert_eq!(logged, Some((120, hashes[120])));
    }

    #[test]
    fn a_headless_replay_plays_to_its_end_and_fails_on_a_damaged_tick() {
        let path = replay_file("headless");
        let (hashes, _) = record_a_walk(&path);
        let mut app = play(scene(Scene::Field), &path).expect("a replay");
        // Played as `headless` plays it, to compare the world after.
        while matches!(*app.world().resource::<Replay>(), Replay::Playing { .. }) {
            app.update(TICK, &[]);
        }
        assert_eq!(app.world_hash(), hashes[120]);
        let played = headless(play(scene(Scene::Field), &path).expect("a replay"));
        assert_eq!(played.expect("played"), (120, hashes[120]));

        // The header is 16 bytes and a kind; each tick a flag byte and an
        // x and a y. Bit 7 of the sixth tick's flags belongs to no action.
        let mut bytes = fs::read(&path).expect("the replay's file");
        bytes[17 + 5 * 5] |= 0x80;
        fs::write(&path, bytes).expect("damage the replay");
        let failed = headless(play(scene(Scene::Field), &path).expect("a replay"));
        fs::remove_file(&path).expect("remove the replay's file");
        let error = failed.expect_err("a damaged tick").to_string();
        assert!(error.contains("after 5 ticks"), "{error}");
    }

    #[test]
    fn a_recording_of_no_ticks_is_a_replay_of_none() {
        let path = replay_file("empty");
        let field = scene(Scene::Field);
        let start = field.world_hash();
        let (app, recording) = record(field, &path).expect("a recording");
        drop(app);
        assert_eq!(recording.counts(), (0, 0, start));
        let played = headless(play(scene(Scene::Field), &path).expect("a replay"));
        fs::remove_file(&path).expect("remove the replay's file");
        assert_eq!(played.expect("played"), (0, start));
    }

    #[test]
    fn a_recording_whose_file_fails_counts_only_the_ticks_written() {
        let path = replay_file("lost");
        let (mut app, recording) = record(scene(Scene::Field), &path).expect("a recording");
        for _ in 0..3 {
            app.update(TICK, &[]);
        }
        // A file open only to read: the next write fails.
        recording.0.borrow_mut().file = Some(File::open(&path).expect("the replay's file"));
        for _ in 0..2 {
            app.update(TICK, &[]);
        }
        let (ticks, written, _) = recording.counts();
        assert_eq!((ticks, written), (5, 3));
        assert!(recording.0.borrow().file.is_none(), "the file is given up");
        let kept = fs::read(&path).expect("the replay's file");
        fs::remove_file(&path).expect("remove the replay's file");
        assert_eq!(kept.len(), 17 + 3 * 5, "the header and three ticks");
    }

    #[test]
    fn a_replay_that_is_not_one_of_the_sandbox_is_refused() {
        let path = replay_file("not");
        fs::write(&path, b"NRPL").expect("write a cut replay");
        let refused = play(scene(Scene::Field), &path);
        fs::remove_file(&path).expect("remove the file");
        let error = refused.err().expect("refused").to_string();
        assert!(error.contains("is not a replay of the sandbox"), "{error}");
        let missing = play(scene(Scene::Field), &replay_file("missing"));
        assert!(missing.is_err());
    }
}
