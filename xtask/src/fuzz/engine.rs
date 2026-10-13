//! Runs one fuzz target: feeds it the seeds and then mutated seeds, catches
//! panics, notices hangs, and saves the inputs that caused either.
//!
//! The target runs on a thread of its own. The calling thread is the
//! watchdog: a thread that is stuck inside a target cannot be stopped, but
//! it can be noticed, and the input it is stuck on can be saved.

use std::cell::{Cell, RefCell};
use std::fs;
use std::mem;
use std::panic;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, Once, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use nadir_core::Pcg32;
use nadir_core::hash::fnv1a64;

use super::mutate::mutate;
use crate::error::Error;

/// How one target is fuzzed.
#[derive(Clone, Debug)]
pub(crate) struct Config {
    /// Decides every mutation: the same seed gives the same inputs in the
    /// same order.
    pub(crate) seed: u64,
    /// How long to make new inputs for. The seeds themselves always all
    /// run, even if this is zero.
    pub(crate) duration: Duration,
    /// Stop after this many inputs, if the time has not run out first.
    pub(crate) max_inputs: u64,
    /// An input that keeps the target busy for longer than this is a hang.
    pub(crate) hang_limit: Duration,
    /// The longest input a mutation may produce, in bytes.
    pub(crate) max_len: usize,
    /// The directory that inputs which fail are saved to.
    pub(crate) crashes: PathBuf,
}

/// What went wrong with an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The target panicked.
    Panic,
    /// The target did not return within [`Config::hang_limit`].
    Hang,
}

/// One distinct failure and the first input that caused it.
#[derive(Debug)]
pub(crate) struct Finding {
    /// What went wrong.
    pub(crate) kind: Kind,
    /// Where the panic was raised, as `file:line:column`. Panics from the
    /// same place are one finding. Empty for a hang.
    pub(crate) site: String,
    /// The panic message, or how long the target was given.
    pub(crate) message: String,
    /// The file the input was saved to.
    pub(crate) path: PathBuf,
    /// The length of the input in bytes.
    pub(crate) len: usize,
    /// How many inputs failed this way.
    pub(crate) hits: u64,
}

/// The result of fuzzing one target.
#[derive(Debug)]
pub(crate) struct Outcome {
    /// How many inputs the target finished, seeds included.
    pub(crate) inputs: u64,
    /// How long it took.
    pub(crate) elapsed: Duration,
    /// What failed. Empty if the run was clean.
    pub(crate) findings: Vec<Finding>,
    /// Whether the run ended early because the target hung. The thread that
    /// ran it is then still stuck, and stays so until the process exits.
    pub(crate) hung: bool,
}

/// What the fuzzing thread and the watchdog share.
#[derive(Default)]
struct Shared {
    /// The input the target is working on, valid while `started` is set.
    input: Vec<u8>,
    /// When the target was handed `input`, until it returns.
    started: Option<Instant>,
    inputs: u64,
    findings: Vec<Finding>,
}

/// Fuzzes `target` with `seeds` as `config` says.
///
/// Every seed runs unchanged first, in order, so that an input which is
/// known to fail and has been added to the corpus fails right away. After
/// that each input is a random seed with random mutations.
pub(crate) fn run(
    target: fn(&[u8]),
    seeds: Vec<Vec<u8>>,
    config: Config,
) -> Result<Outcome, Error> {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let (done, finished) = mpsc::channel();
    let start = Instant::now();
    {
        let (shared, stop, config) = (Arc::clone(&shared), Arc::clone(&stop), config.clone());
        let spawned = thread::Builder::new()
            .name("fuzz".to_string())
            .spawn(move || {
                // Nobody is listening only if the watchdog gave up on this
                // thread, and then there is nothing left to report.
                let _ = done.send(fuzz(target, &seeds, &config, &shared, &stop));
            });
        if let Err(error) = spawned {
            return Err(Error::Internal(format!(
                "cannot start the fuzzing thread: {error}"
            )));
        }
    }

    let poll = (config.hang_limit / 4).clamp(Duration::from_millis(1), Duration::from_millis(50));
    let mut hung = false;
    loop {
        match finished.recv_timeout(poll) {
            Ok(result) => {
                result?;
                break;
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(Error::Internal(
                    "the fuzzing thread stopped without a result".to_string(),
                ));
            }
            Err(RecvTimeoutError::Timeout) => {
                let guard = lock(&shared);
                let overdue = guard
                    .started
                    .is_some_and(|started| started.elapsed() > config.hang_limit);
                if overdue {
                    let input = guard.input.clone();
                    drop(guard);
                    // This cannot interrupt the target. It only keeps the
                    // thread from going on if the target does return.
                    stop.store(true, Ordering::Relaxed);
                    let message = format!(
                        "did not finish within {} s",
                        config.hang_limit.as_secs_f64()
                    );
                    let finding = save(&config, Kind::Hang, String::new(), message, &input)?;
                    lock(&shared).findings.push(finding);
                    hung = true;
                    break;
                }
            }
        }
    }

    let mut guard = lock(&shared);
    Ok(Outcome {
        inputs: guard.inputs,
        elapsed: start.elapsed(),
        findings: mem::take(&mut guard.findings),
        hung,
    })
}

/// The body of the fuzzing thread.
fn fuzz(
    target: fn(&[u8]),
    seeds: &[Vec<u8>],
    config: &Config,
    shared: &Mutex<Shared>,
    stop: &AtomicBool,
) -> Result<(), Error> {
    install_panic_hook();
    let mut rng = Pcg32::new(config.seed, 0);
    let deadline = Instant::now() + config.duration;
    let mut input = Vec::new();
    let mut count = 0_u64;
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        input.clear();
        let unchanged_seed = usize::try_from(count)
            .ok()
            .and_then(|index| seeds.get(index));
        match unchanged_seed {
            Some(seed) => input.extend_from_slice(seed),
            None => {
                if now >= deadline || count >= config.max_inputs {
                    break;
                }
                let Some(seed) = rng.choose(seeds) else {
                    break;
                };
                input.extend_from_slice(seed);
                mutate(&mut rng, &mut input, seeds, config.max_len);
            }
        }
        count += 1;

        {
            let mut guard = lock(shared);
            guard.input.clone_from(&input);
            guard.started = Some(now);
        }
        let panicked = run_target(target, &input);
        let mut guard = lock(shared);
        guard.started = None;
        guard.inputs += 1;

        let Some(panicked) = panicked else {
            continue;
        };
        let known = guard
            .findings
            .iter_mut()
            .find(|finding| finding.kind == Kind::Panic && finding.site == panicked.site);
        match known {
            Some(finding) => finding.hits += 1,
            None => {
                drop(guard);
                let finding = save(config, Kind::Panic, panicked.site, panicked.message, &input)?;
                lock(shared).findings.push(finding);
            }
        }
    }
    Ok(())
}

/// A panic that a target raised.
struct Panicked {
    /// Where, as `file:line:column`; the message if the place is not known.
    site: String,
    message: String,
}

thread_local! {
    /// Whether this thread is inside a fuzz target right now.
    static IN_TARGET: Cell<bool> = const { Cell::new(false) };
    /// Where the last panic inside a target on this thread was raised.
    static PANIC_SITE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Makes panics inside a fuzz target quiet, and records where they were
/// raised. A fuzz run can hit the same panic many thousands of times; the
/// standard message for each one would bury everything else.
///
/// Panics anywhere else are reported as before.
fn install_panic_hook() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if IN_TARGET.get() {
                let site = info.location().map(|location| {
                    format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    )
                });
                PANIC_SITE.set(site);
            } else {
                previous(info);
            }
        }));
    });
}

/// Hands `input` to `target`. Returns the panic, if it panicked.
fn run_target(target: fn(&[u8]), input: &[u8]) -> Option<Panicked> {
    IN_TARGET.set(true);
    let result = panic::catch_unwind(|| target(input));
    IN_TARGET.set(false);
    let site = PANIC_SITE.take();
    let payload = result.err()?;
    let message = if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic whose payload is not a string".to_string()
    };
    Some(Panicked {
        site: site.unwrap_or_else(|| message.clone()),
        message,
    })
}

/// Saves `input` to the crash directory and describes the failure.
///
/// The file is named after the kind of failure and a hash of the input, so
/// saving the same input twice gives one file.
fn save(
    config: &Config,
    kind: Kind,
    site: String,
    message: String,
    input: &[u8],
) -> Result<Finding, Error> {
    let prefix = match kind {
        Kind::Panic => "panic",
        Kind::Hang => "hang",
    };
    let path = config
        .crashes
        .join(format!("{prefix}-{:016x}.bin", fnv1a64(input)));
    let written = fs::create_dir_all(&config.crashes).and_then(|()| fs::write(&path, input));
    match written {
        Ok(()) => Ok(Finding {
            kind,
            site,
            message,
            path,
            len: input.len(),
            hits: 1,
        }),
        Err(source) => Err(Error::Write { path, source }),
    }
}

/// Locks `mutex`. A lock is only ever poisoned by a bug in this file, and
/// the numbers behind it are still the best there are.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// A fresh, empty directory for one test, under `target/tmp`.
    fn crash_dir(test: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/tmp")
            .join(format!("xtask-fuzz-engine-{test}"));
        // It may be left over from an earlier run.
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// A configuration that stops after `max_inputs` inputs, long before
    /// any of its time limits.
    fn config(test: &str, max_inputs: u64) -> Config {
        Config {
            seed: 1,
            duration: Duration::from_secs(3600),
            max_inputs,
            hang_limit: Duration::from_secs(3600),
            max_len: 4096,
            crashes: crash_dir(test),
        }
    }

    fn seeds(texts: &[&str]) -> Vec<Vec<u8>> {
        texts.iter().map(|text| text.as_bytes().to_vec()).collect()
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_target_that_accepts_everything_runs_clean() {
        fn accept(_: &[u8]) {}
        let config = config("clean", 500);
        let outcome = run(accept, seeds(&["[1, 2]", "{}"]), config.clone()).unwrap();
        assert_eq!(outcome.inputs, 500);
        assert!(outcome.findings.is_empty(), "{:?}", outcome.findings);
        assert!(!outcome.hung);
        // Nothing to save, so the directory is not even created.
        assert!(!config.crashes.exists());
    }

    #[test]
    fn the_seeds_run_first_and_unchanged() {
        static SEEN: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        fn record(input: &[u8]) {
            lock(&SEEN).push(input.to_vec());
        }
        let corpus = seeds(&["first", "second", "third"]);
        let outcome = run(record, corpus.clone(), config("seeds-first", 50)).unwrap();
        assert_eq!(outcome.inputs, 50);
        let seen = lock(&SEEN);
        assert_eq!(seen.len(), 50);
        assert_eq!(seen[..3], corpus[..]);
        // After the seeds, inputs are mutated.
        let changed = seen[3..]
            .iter()
            .filter(|input| !corpus.contains(input))
            .count();
        assert!(changed > 40, "{changed}");
    }

    #[test]
    fn without_time_only_the_seeds_run() {
        fn accept(_: &[u8]) {}
        let mut config = config("no-time", u64::MAX);
        config.duration = Duration::ZERO;
        let outcome = run(accept, seeds(&["a", "b", "c"]), config).unwrap();
        assert_eq!(outcome.inputs, 3);

        // The input limit does not cut the seeds short either.
        let outcome = run(
            accept,
            seeds(&["a", "b", "c"]),
            self::config("no-inputs", 0),
        )
        .unwrap();
        assert_eq!(outcome.inputs, 3);
    }

    #[test]
    fn without_seeds_nothing_runs() {
        fn accept(_: &[u8]) {}
        let outcome = run(accept, Vec::new(), config("no-seeds", 100)).unwrap();
        assert_eq!(outcome.inputs, 0);
        assert!(outcome.findings.is_empty());
    }

    #[test]
    fn the_same_seed_gives_the_same_inputs() {
        static FIRST: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        static SECOND: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        static OTHER: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        fn first(input: &[u8]) {
            lock(&FIRST).push(input.to_vec());
        }
        fn second(input: &[u8]) {
            lock(&SECOND).push(input.to_vec());
        }
        fn other(input: &[u8]) {
            lock(&OTHER).push(input.to_vec());
        }
        let corpus = seeds(&["{\"a\": [1, 2, 3]}", "[true, null]"]);
        run(first, corpus.clone(), config("same-seed-1", 300)).unwrap();
        run(second, corpus.clone(), config("same-seed-2", 300)).unwrap();
        let mut reseeded = config("same-seed-3", 300);
        reseeded.seed = 2;
        run(other, corpus, reseeded).unwrap();
        assert_eq!(*lock(&FIRST), *lock(&SECOND));
        assert_ne!(*lock(&FIRST), *lock(&OTHER));
    }

    #[test]
    fn mutated_inputs_respect_the_length_limit() {
        static LONGEST: Mutex<usize> = Mutex::new(0);
        fn measure(input: &[u8]) {
            let mut longest = lock(&LONGEST);
            *longest = (*longest).max(input.len());
        }
        let mut config = config("max-len", 2000);
        config.max_len = 24;
        // The seed is shorter than the limit; mutations would grow it.
        run(measure, seeds(&["0123456789"]), config).unwrap();
        assert_eq!(*lock(&LONGEST), 24);
    }

    #[test]
    fn a_panic_is_caught_and_its_input_saved() {
        fn only_the_seed(input: &[u8]) {
            assert!(input == b"[1, 2, 3]", "not the seed");
        }
        let config = config("panic", 400);
        let outcome = run(only_the_seed, seeds(&["[1, 2, 3]"]), config.clone()).unwrap();
        // The run goes on after a panic.
        assert_eq!(outcome.inputs, 400);
        assert!(!outcome.hung);
        assert_eq!(outcome.findings.len(), 1, "{:?}", outcome.findings);

        let finding = &outcome.findings[0];
        assert_eq!(finding.kind, Kind::Panic);
        assert_eq!(finding.message, "not the seed");
        // `file:line:column` of the `assert!` above.
        let parts: Vec<&str> = finding.site.split(':').collect();
        assert_eq!(parts.len(), 3, "{}", finding.site);
        assert!(parts[0].ends_with("engine.rs"), "{}", finding.site);
        assert!(
            parts[1..].iter().all(|part| part.parse::<u32>().is_ok()),
            "{}",
            finding.site
        );
        // Nearly every mutated input fails, and they all count as this one.
        assert!(finding.hits > 300, "{}", finding.hits);

        // Exactly one file, which holds an input that fails.
        let saved = fs::read(&finding.path).unwrap();
        assert_eq!(saved.len(), finding.len);
        assert_ne!(saved, b"[1, 2, 3]");
        let name = format!("panic-{:016x}.bin", fnv1a64(&saved));
        assert_eq!(finding.path, config.crashes.join(&name));
        assert_eq!(files_in(&config.crashes), [name]);
    }

    #[test]
    fn a_seed_that_panics_is_found_before_any_mutation() {
        fn reject_empty_arrays(input: &[u8]) {
            assert!(input != b"[]", "an empty array");
        }
        let mut config = config("panicking-seed", u64::MAX);
        config.duration = Duration::ZERO;
        let outcome = run(reject_empty_arrays, seeds(&["[1]", "[]", "[2]"]), config).unwrap();
        assert_eq!(outcome.inputs, 3);
        assert_eq!(outcome.findings.len(), 1);
        assert_eq!(outcome.findings[0].hits, 1);
        assert_eq!(fs::read(&outcome.findings[0].path).unwrap(), b"[]");
    }

    #[test]
    fn panics_from_different_places_are_different_findings() {
        fn two_ways_to_fail(input: &[u8]) {
            if input.len() < 4 {
                panic!("too short: {}", input.len());
            }
            if input.len() > 4 {
                panic!("too long: {}", input.len());
            }
        }
        let outcome = run(two_ways_to_fail, seeds(&["abcd"]), config("two-sites", 600)).unwrap();
        assert_eq!(outcome.findings.len(), 2, "{:?}", outcome.findings);
        let mut messages: Vec<&str> = outcome
            .findings
            .iter()
            .map(|finding| finding.message.split(':').next().unwrap())
            .collect();
        messages.sort_unstable();
        assert_eq!(messages, ["too long", "too short"]);
        assert_ne!(outcome.findings[0].site, outcome.findings[1].site);
        assert_ne!(outcome.findings[0].path, outcome.findings[1].path);
        // The messages differ from input to input, but the place does not.
        let hits: u64 = outcome.findings.iter().map(|finding| finding.hits).sum();
        assert!(hits > 100, "{hits}");
    }

    #[test]
    fn a_panic_with_an_unusual_payload_is_still_a_finding() {
        fn odd(_: &[u8]) {
            std::panic::panic_any(42_u32);
        }
        let outcome = run(odd, seeds(&["x"]), config("odd-payload", 5)).unwrap();
        assert_eq!(outcome.findings.len(), 1);
        assert_eq!(
            outcome.findings[0].message,
            "a panic whose payload is not a string"
        );
        assert_eq!(outcome.findings[0].hits, 5);
    }

    #[test]
    fn a_hang_is_noticed_and_its_input_saved() {
        static RELEASE: AtomicBool = AtomicBool::new(false);
        static WENT_ON: AtomicBool = AtomicBool::new(false);
        fn stuck_on_the_second_seed(input: &[u8]) {
            while input == b"stuck" && !RELEASE.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(1));
            }
            if input == b"never reached" {
                WENT_ON.store(true, Ordering::Relaxed);
            }
        }
        let mut config = config("hang", 1000);
        config.hang_limit = Duration::from_millis(40);
        let start = Instant::now();
        let outcome = run(
            stuck_on_the_second_seed,
            seeds(&["fine", "stuck", "never reached"]),
            config.clone(),
        )
        .unwrap();
        // Let the fuzzing thread go, whatever the assertions below say.
        RELEASE.store(true, Ordering::Relaxed);

        assert!(outcome.hung);
        assert!(start.elapsed() < Duration::from_secs(30));
        // Only the first seed finished.
        assert_eq!(outcome.inputs, 1);
        assert_eq!(outcome.findings.len(), 1, "{:?}", outcome.findings);
        let finding = &outcome.findings[0];
        assert_eq!(finding.kind, Kind::Hang);
        assert_eq!(finding.message, "did not finish within 0.04 s");
        assert_eq!(fs::read(&finding.path).unwrap(), b"stuck");
        let name = format!("hang-{:016x}.bin", fnv1a64(b"stuck"));
        assert_eq!(files_in(&config.crashes), [name]);

        // Now that the target has been let go, its thread stops instead of
        // fuzzing on behind the caller's back.
        thread::sleep(Duration::from_millis(100));
        assert!(!WENT_ON.load(Ordering::Relaxed));
    }

    #[test]
    fn a_slow_input_within_the_limit_is_not_a_hang() {
        fn slow(_: &[u8]) {
            thread::sleep(Duration::from_millis(30));
        }
        let mut config = config("slow", 4);
        config.hang_limit = Duration::from_secs(60);
        let outcome = run(slow, seeds(&["a"]), config).unwrap();
        assert_eq!(outcome.inputs, 4);
        assert!(!outcome.hung && outcome.findings.is_empty());
        assert!(outcome.elapsed >= Duration::from_millis(120));
    }

    #[test]
    fn the_time_limit_ends_the_run() {
        fn accept(_: &[u8]) {}
        let mut config = config("time-limit", u64::MAX);
        config.duration = Duration::from_millis(60);
        let outcome = run(accept, seeds(&["a"]), config).unwrap();
        assert!(outcome.inputs > 1);
        assert!(outcome.elapsed >= Duration::from_millis(60));
        assert!(outcome.elapsed < Duration::from_secs(30));
    }

    #[test]
    fn a_crash_directory_that_cannot_be_written_is_an_error() {
        fn reject(_: &[u8]) {
            panic!("always");
        }
        let mut config = config("unwritable", 10);
        // A file where the directory should be.
        fs::create_dir_all(config.crashes.parent().unwrap()).unwrap();
        fs::write(&config.crashes, b"in the way").unwrap();
        let blocked = config.crashes.clone();
        config.crashes = blocked.join("json");
        let error = run(reject, seeds(&["a"]), config).unwrap_err();
        assert!(matches!(error, Error::Write { .. }), "{error:?}");
        fs::remove_file(blocked).unwrap();
    }

    #[test]
    fn only_panics_inside_a_target_are_kept_quiet() {
        fn inside(_: &[u8]) {
            panic!("inside");
        }
        fn outside() {
            panic!("outside");
        }
        install_panic_hook();

        // Inside a target: the place is recorded and handed over.
        let panicked = run_target(inside, b"x").unwrap();
        assert_eq!(panicked.message, "inside");
        assert!(panicked.site.contains("engine.rs:"), "{}", panicked.site);
        assert_eq!(PANIC_SITE.take(), None);

        // Afterwards this thread is no longer inside a target, so its
        // panics take the ordinary route and leave nothing behind.
        assert!(!IN_TARGET.get());
        let caught = panic::catch_unwind(outside);
        assert_eq!(caught.unwrap_err().downcast_ref::<&str>(), Some(&"outside"));
        assert_eq!(PANIC_SITE.take(), None);

        // A target that returns is not a panic.
        fn accept(_: &[u8]) {}
        assert!(run_target(accept, b"x").is_none());
        assert!(!IN_TARGET.get());
    }
}
