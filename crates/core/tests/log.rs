//! Tests of the process-wide logger and its file sink, through the public
//! API only.
//!
//! Everything here shares one logger, so each test takes `LOGGER_LOCK` and
//! leaves the logger as it found it.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use nadir_core::log::{self, FileConfig, Level, debug, error, info, trace, warn};

static LOGGER_LOCK: Mutex<()> = Mutex::new(());

/// Serializes the tests, silences stderr and restores the default filter.
/// A test that fails while holding the lock must not fail the others.
fn exclusive_logger() -> MutexGuard<'static, ()> {
    let guard = LOGGER_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    log::close_file();
    log::set_stderr(false);
    log::set_level(Level::Info);
    log::set_target_level(module_path!(), None);
    guard
}

/// Returns a fresh, empty directory for one test, under `target/tmp`.
fn scratch_dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("nadir-core-log")
        .join(test);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("remove the previous run's directory");
    }
    fs::create_dir_all(&dir).expect("create the test directory");
    dir
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The message part of each line: everything after `<timestamp> <LEVEL> <target>: `.
fn messages(text: &str) -> Vec<&str> {
    text.lines()
        .map(|line| line.split_once(": ").expect("line has a target").1)
        .collect()
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("list the test directory")
        .map(|entry| {
            entry
                .expect("read a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn records_reach_the_ring_buffer_with_level_and_target() {
    let _guard = exclusive_logger();
    error!("disk {} is on fire", 3);
    warn!("plain literal");
    let tail = log::tail(2);
    // The target is this file's module path: the test crate's name.
    assert!(
        tail[0].ends_with(" ERROR log: disk 3 is on fire"),
        "{tail:?}"
    );
    assert!(tail[1].ends_with(" WARN  log: plain literal"), "{tail:?}");
    // The line starts with an RFC 3339 UTC timestamp.
    let stamp = tail[0].split(' ').next().expect("line has a timestamp");
    assert_eq!(stamp.len(), "2026-10-12T19:28:03.123Z".len(), "{stamp}");
    assert!(
        stamp.ends_with('Z') && stamp.as_bytes()[10] == b'T',
        "{stamp}"
    );
}

#[test]
fn the_level_filter_drops_less_severe_records() {
    let _guard = exclusive_logger();
    let target = module_path!();

    // Default: Info and more severe.
    assert!(log::enabled(Level::Info, target));
    assert!(!log::enabled(Level::Debug, target));
    info!("marker one");
    debug!("dropped: debug at the default level");
    assert!(log::tail(1)[0].ends_with("marker one"));

    log::set_level(Level::Warn);
    assert!(!log::enabled(Level::Info, target));
    info!("dropped: info at warn level");
    warn!("marker two");
    assert!(log::tail(1)[0].ends_with("marker two"));

    // A per-target override beats the default, in either direction.
    log::set_target_level(target, Some(Level::Trace));
    assert!(log::enabled(Level::Trace, target));
    assert!(!log::enabled(Level::Info, "some_other_crate"));
    trace!("marker three");
    assert!(log::tail(1)[0].ends_with(" TRACE log: marker three"));

    log::set_target_level(target, None);
    assert!(!log::enabled(Level::Trace, target));
    trace!("dropped: trace after the override was removed");
    assert!(log::tail(1)[0].ends_with("marker three"));

    log::set_level(Level::Info);
}

#[test]
fn arguments_of_dropped_records_are_not_formatted() {
    struct Explosive;
    impl fmt::Display for Explosive {
        fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
            panic!("a dropped record was formatted");
        }
    }

    let _guard = exclusive_logger();
    trace!("{Explosive}");
}

#[test]
fn a_display_impl_that_logs_does_not_deadlock() {
    struct Chatty(u32);
    impl fmt::Display for Chatty {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if self.0 > 0 {
                info!("inner {} sees {}", self.0, Chatty(self.0 - 1));
            }
            write!(f, "chatty-{}", self.0)
        }
    }

    let _guard = exclusive_logger();
    info!("outer sees {}", Chatty(2));
    let tail = log::tail(3);
    // Inner records are written first, because they are logged while the
    // outer message is still being formatted.
    assert!(tail[0].ends_with("inner 1 sees chatty-0"), "{tail:?}");
    assert!(tail[1].ends_with("inner 2 sees chatty-1"), "{tail:?}");
    assert!(tail[2].ends_with("outer sees chatty-2"), "{tail:?}");
}

#[test]
fn logging_works_from_several_threads() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("threads");
    let path = dir.join("nadir.log");
    log::open_file(&path, FileConfig::default()).expect("open the log file");
    std::thread::scope(|scope| {
        for thread in 0..4 {
            scope.spawn(move || {
                for n in 0..50 {
                    info!("thread {thread} record {n}");
                }
            });
        }
    });
    log::close_file();

    let text = read(&path);
    let mut seen = messages(&text);
    assert_eq!(seen.len(), 200, "every record is one intact line");
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 200, "no record is lost or duplicated");
}

#[test]
fn the_file_rotates_by_size_and_keeps_three_copies() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("rotate_by_size");
    let path = dir.join("nadir.log");
    let config = FileConfig {
        max_bytes: 400,
        keep: 3,
        rotate_on_open: false,
    };
    log::open_file(&path, config).expect("open the log file");
    for n in 0..100 {
        info!("record {n:03}");
    }
    log::close_file();

    // The current file plus exactly three rotated copies; older ones are gone.
    assert_eq!(
        file_names(&dir),
        ["nadir.log", "nadir.log.1", "nadir.log.2", "nadir.log.3"]
    );

    // No file exceeds the limit, and every rotated file is as full as it
    // can be: one more line would not have fit.
    let line_len = read(&path)
        .lines()
        .next()
        .expect("the current file has a line")
        .len() as u64
        + 1;
    for name in file_names(&dir) {
        let size = fs::metadata(dir.join(&name))
            .expect("stat a log file")
            .len();
        assert!(size <= config.max_bytes, "{name} is {size} bytes");
        if name != "nadir.log" {
            assert!(
                size + line_len > config.max_bytes,
                "{name} was rotated early, at {size} bytes"
            );
        }
    }

    // Oldest to newest, the files hold a gapless run of records that ends
    // with the last one written.
    let texts = ["nadir.log.3", "nadir.log.2", "nadir.log.1", "nadir.log"]
        .map(|name| read(&dir.join(name)));
    let kept: Vec<&str> = texts.iter().flat_map(|text| messages(text)).collect();
    let first: usize = kept[0]
        .strip_prefix("record ")
        .expect("record prefix")
        .parse()
        .expect("record number");
    assert!(
        first > 0,
        "with 100 records and 400-byte files, the oldest records must have been dropped"
    );
    let expected: Vec<String> = (first..100).map(|n| format!("record {n:03}")).collect();
    assert_eq!(kept, expected);
}

#[test]
fn opening_appends_by_default() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("append");
    let path = dir.join("nadir.log");
    fs::write(&path, "from an earlier run\n").expect("seed the log file");

    log::open_file(&path, FileConfig::default()).expect("open the log file");
    info!("from this run");
    log::close_file();

    let text = read(&path);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "from an earlier run");
    assert!(lines[1].ends_with("from this run"));
    assert_eq!(file_names(&dir), ["nadir.log"]);
}

#[test]
fn rotate_on_open_starts_every_run_with_a_fresh_file() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("rotate_on_open");
    let path = dir.join("nadir.log");
    let config = FileConfig {
        rotate_on_open: true,
        ..FileConfig::default()
    };

    // Five runs, one record each. An empty file is not worth rotating, so
    // the very first open creates no `.1`.
    for run in 0..5 {
        log::open_file(&path, config).expect("open the log file");
        info!("run {run}");
        log::close_file();
    }

    assert_eq!(
        file_names(&dir),
        ["nadir.log", "nadir.log.1", "nadir.log.2", "nadir.log.3"]
    );
    assert_eq!(messages(&read(&path)), ["run 4"]);
    assert_eq!(messages(&read(&dir.join("nadir.log.1"))), ["run 3"]);
    assert_eq!(messages(&read(&dir.join("nadir.log.2"))), ["run 2"]);
    assert_eq!(messages(&read(&dir.join("nadir.log.3"))), ["run 1"]);
}

#[test]
fn keeping_no_copies_truncates_instead_of_rotating() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("keep_zero");
    let path = dir.join("nadir.log");
    log::open_file(
        &path,
        FileConfig {
            max_bytes: 200,
            keep: 0,
            rotate_on_open: false,
        },
    )
    .expect("open the log file");
    for n in 0..20 {
        info!("record {n:03}");
    }
    log::close_file();

    assert_eq!(file_names(&dir), ["nadir.log"]);
    let text = read(&path);
    assert!(text.len() <= 200, "{} bytes", text.len());
    assert_eq!(messages(&text).last(), Some(&"record 019"));
}

#[test]
fn an_oversized_record_is_written_whole() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("oversized");
    let path = dir.join("nadir.log");
    log::open_file(
        &path,
        FileConfig {
            max_bytes: 100,
            keep: 3,
            rotate_on_open: false,
        },
    )
    .expect("open the log file");
    let long = "x".repeat(500);
    info!("{long}");
    info!("after");
    log::close_file();

    // The long record went into the empty first file without rotating, and
    // the next record rotated it out.
    assert_eq!(file_names(&dir), ["nadir.log", "nadir.log.1"]);
    assert_eq!(messages(&read(&dir.join("nadir.log.1"))), [long.as_str()]);
    assert_eq!(messages(&read(&path)), ["after"]);
}

#[test]
fn a_failed_rotation_keeps_logging_to_the_current_file() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("rotation_fails");
    let path = dir.join("nadir.log");
    // A non-empty directory where `nadir.log.1` should go makes the rename fail.
    fs::create_dir(dir.join("nadir.log.1")).expect("create the obstacle");
    fs::write(dir.join("nadir.log.1").join("occupied"), "x").expect("fill the obstacle");

    log::open_file(
        &path,
        FileConfig {
            max_bytes: 200,
            keep: 1,
            rotate_on_open: false,
        },
    )
    .expect("open the log file");
    for n in 0..20 {
        info!("record {n:03}");
    }
    log::close_file();

    // Nothing was lost: every record is in the one file that could be written.
    let text = read(&path);
    let expected: Vec<String> = (0..20).map(|n| format!("record {n:03}")).collect();
    assert_eq!(messages(&text), expected);
}

#[test]
fn opening_a_file_in_a_missing_directory_fails_and_keeps_the_old_sink() {
    let _guard = exclusive_logger();
    let dir = scratch_dir("open_fails");
    let path = dir.join("nadir.log");
    log::open_file(&path, FileConfig::default()).expect("open the log file");

    let missing = dir.join("no").join("such").join("dir").join("nadir.log");
    assert!(log::open_file(&missing, FileConfig::default()).is_err());

    info!("still going to the first file");
    log::close_file();
    assert_eq!(messages(&read(&path)), ["still going to the first file"]);
}
