//! Logging: the [`error!`], [`warn!`], [`info!`], [`debug!`] and [`trace!`]
//! macros, a level filter, and three sinks.
//!
//! ```
//! use nadir_core::log::{self, Level, info, warn};
//!
//! log::set_stderr(false);
//! info!("loaded {} assets", 12);
//! warn!("texture missing");
//! log::set_target_level("nadir_render", Some(Level::Trace));
//! assert!(log::tail(1)[0].ends_with("texture missing"));
//! ```
//!
//! Every record that passes the filter goes to all active sinks:
//!
//! - **stderr**, on by default ([`set_stderr`]);
//! - a **file** with rotation, off until [`open_file`] is called;
//! - an in-memory **ring buffer** of the last [`RING_CAPACITY`] records,
//!   always on, read with [`tail`]. The debug console and crash reports use
//!   it.
//!
//! A record's *target* is the module path of the code that logged it. The
//! filter has a default level ([`set_level`], initially [`Level::Info`]) and
//! optional per-target overrides ([`set_target_level`]).
//!
//! There is one logger per process. It can be used from any thread, but it
//! takes a lock and may write to a file, so do not log from the audio
//! callback.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fmt::{self, Write as _};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use crate::civil::DateTime;

/// How many records the in-memory ring buffer keeps.
pub const RING_CAPACITY: usize = 1000;

/// The severity of a log record. Lower is more severe: `Error < Trace`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Something failed and the result is wrong or missing.
    Error = 1,
    /// Something unexpected happened, but the engine carried on.
    Warn,
    /// A normal, notable event: startup, a level load.
    Info,
    /// Detail that helps when debugging.
    Debug,
    /// Very fine detail, possibly several records per frame.
    Trace,
}

impl Level {
    /// Returns the upper-case name used in log lines, such as `"WARN"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

/// How the file sink rotates. See [`open_file`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileConfig {
    /// Rotate before a record would grow the file beyond this many bytes.
    /// A single record larger than this still gets written, alone in its
    /// file.
    pub max_bytes: u64,
    /// How many rotated files to keep besides the current one.
    pub keep: u32,
    /// Whether opening rotates an existing, non-empty file first, so that
    /// every run starts a fresh file. If `false`, opening appends.
    pub rotate_on_open: bool,
}

impl Default for FileConfig {
    /// 4 MiB per file, 3 rotated files, appending on open.
    fn default() -> Self {
        Self {
            max_bytes: 4 * 1024 * 1024,
            keep: 3,
            rotate_on_open: false,
        }
    }
}

/// Logs at [`Level::Error`], with `format!` syntax.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_error {
    ($($arg:tt)+) => {
        $crate::log::__log($crate::log::Level::Error, ::core::module_path!(), ::core::format_args!($($arg)+))
    };
}

/// Logs at [`Level::Warn`], with `format!` syntax.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_warn {
    ($($arg:tt)+) => {
        $crate::log::__log($crate::log::Level::Warn, ::core::module_path!(), ::core::format_args!($($arg)+))
    };
}

/// Logs at [`Level::Info`], with `format!` syntax.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_info {
    ($($arg:tt)+) => {
        $crate::log::__log($crate::log::Level::Info, ::core::module_path!(), ::core::format_args!($($arg)+))
    };
}

/// Logs at [`Level::Debug`], with `format!` syntax.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_debug {
    ($($arg:tt)+) => {
        $crate::log::__log($crate::log::Level::Debug, ::core::module_path!(), ::core::format_args!($($arg)+))
    };
}

/// Logs at [`Level::Trace`], with `format!` syntax.
#[doc(hidden)]
#[macro_export]
macro_rules! __log_trace {
    ($($arg:tt)+) => {
        $crate::log::__log($crate::log::Level::Trace, ::core::module_path!(), ::core::format_args!($($arg)+))
    };
}

// The macros are exported from the crate root under hidden names, because
// that is the only place `macro_rules!` can export from, and re-exported
// here under their real ones.
#[doc(inline)]
pub use crate::{
    __log_debug as debug, __log_error as error, __log_info as info, __log_trace as trace,
    __log_warn as warn,
};

/// Sets the level for every target that has no override of its own.
/// Records less severe than this are dropped.
pub fn set_level(level: Level) {
    let mut logger = logger();
    logger.filter.default = level;
    publish_most_verbose(&logger.filter);
}

/// Sets, or with `None` removes, the level override for `target` and the
/// modules inside it.
///
/// `target` is a module path such as `nadir_render` or
/// `nadir_render::batch`. When several overrides match a record, the
/// longest wins.
pub fn set_target_level(target: &str, level: Option<Level>) {
    let mut logger = logger();
    logger.filter.set_target(target, level);
    publish_most_verbose(&logger.filter);
}

/// Returns `true` if a record at `level` from `target` would be logged.
///
/// The macros check this themselves. Call it only to skip expensive work
/// that exists purely to build a log message.
pub fn enabled(level: Level, target: &str) -> bool {
    // Most disabled records are rejected here, without taking the lock.
    level as u8 <= MOST_VERBOSE.load(Ordering::Relaxed)
        && level <= logger().filter.level_for(target)
}

/// Turns the stderr sink on or off. It is on by default.
pub fn set_stderr(enabled: bool) {
    logger().stderr = enabled;
}

/// Starts writing records to the file at `path`, replacing any file sink
/// already open. The parent directory must exist.
///
/// When the file is full (see [`FileConfig`]) it is renamed to `<path>.1`,
/// the previous `<path>.1` to `<path>.2` and so on, the oldest is deleted,
/// and a fresh file is started. If rotating fails, for example because
/// another program holds one of the files, logging continues in the current
/// file.
///
/// Returns an error if the file cannot be opened, in which case the
/// previous file sink (if any) stays in place.
pub fn open_file(path: impl AsRef<Path>, config: FileConfig) -> io::Result<()> {
    let file = RotatingFile::open(path.as_ref(), config)?;
    logger().file = Some(file);
    Ok(())
}

/// Stops writing records to a file. Does nothing if no file is open.
pub fn close_file() {
    logger().file = None;
}

/// Returns up to `max_records` of the most recent records, oldest first,
/// formatted as they appear in the log file.
pub fn tail(max_records: usize) -> Vec<String> {
    logger().ring.tail(max_records)
}

/// The entry point behind the logging macros.
#[doc(hidden)]
pub fn __log(level: Level, target: &str, args: fmt::Arguments<'_>) {
    thread_local! {
        /// Reused buffer for the formatted message.
        static MESSAGE: RefCell<String> = const { RefCell::new(String::new()) };
    }

    if !enabled(level, target) {
        return;
    }
    let time = SystemTime::now();
    if let Some(literal) = args.as_str() {
        logger().write(time, level, target, literal);
        return;
    }
    // Format before taking the lock. Formatting runs arbitrary `Display`
    // code, and if that code logs, it must not find the lock held.
    let formatted = MESSAGE.try_with(|buffer| {
        let Ok(mut buffer) = buffer.try_borrow_mut() else {
            return false;
        };
        buffer.clear();
        // Writing to a `String` only fails if a `Display` impl reports an
        // error; whatever was formatted up to that point is still logged.
        let _ = buffer.write_fmt(args);
        logger().write(time, level, target, &buffer);
        true
    });
    // The buffer is unavailable in two rare cases: this record is being
    // logged from inside another record's `Display` impl, or the thread is
    // shutting down. Fall back to a temporary string.
    if !matches!(formatted, Ok(true)) {
        let message = args.to_string();
        logger().write(time, level, target, &message);
    }
}

/// The most verbose level any target is set to, as a `Level as u8`.
static MOST_VERBOSE: AtomicU8 = AtomicU8::new(Level::Info as u8);

static LOGGER: Mutex<Logger> = Mutex::new(Logger::new());

fn logger() -> MutexGuard<'static, Logger> {
    // A panic while the lock was held (say, in the panic hook's own
    // logging) must not silence the log for good: the state behind the
    // lock is still usable.
    LOGGER.lock().unwrap_or_else(PoisonError::into_inner)
}

fn publish_most_verbose(filter: &Filter) {
    MOST_VERBOSE.store(filter.most_verbose() as u8, Ordering::Relaxed);
}

struct Logger {
    filter: Filter,
    stderr: bool,
    file: Option<RotatingFile>,
    ring: Ring,
    /// Reused buffer for the line being written.
    line: String,
}

impl Logger {
    const fn new() -> Self {
        Self {
            filter: Filter::new(),
            stderr: true,
            file: None,
            ring: Ring::new(),
            line: String::new(),
        }
    }

    fn write(&mut self, time: SystemTime, level: Level, target: &str, message: &str) {
        self.line.clear();
        // Writing to a `String` cannot fail.
        let _ = writeln!(
            self.line,
            "{} {level:<5} {target}: {message}",
            DateTime::from_system_time(time)
        );
        if self.stderr {
            // If stderr is closed or full there is nowhere to report it.
            let _ = io::stderr().lock().write_all(self.line.as_bytes());
        }
        if let Some(file) = &mut self.file {
            file.write_line(&self.line);
        }
        self.ring.push(self.line.trim_end_matches('\n'));
    }
}

struct Filter {
    default: Level,
    /// Per-target overrides, in no particular order.
    targets: Vec<(String, Level)>,
}

impl Filter {
    const fn new() -> Self {
        Self {
            default: Level::Info,
            targets: Vec::new(),
        }
    }

    fn set_target(&mut self, target: &str, level: Option<Level>) {
        self.targets.retain(|(existing, _)| existing != target);
        if let Some(level) = level {
            self.targets.push((target.to_string(), level));
        }
    }

    fn level_for(&self, target: &str) -> Level {
        self.targets
            .iter()
            .filter(|(prefix, _)| is_inside(target, prefix))
            .max_by_key(|(prefix, _)| prefix.len())
            .map_or(self.default, |&(_, level)| level)
    }

    fn most_verbose(&self) -> Level {
        self.targets
            .iter()
            .map(|&(_, level)| level)
            .fold(self.default, Level::max)
    }
}

/// Returns `true` if `target` is the module `module` or is nested inside it.
fn is_inside(target: &str, module: &str) -> bool {
    target
        .strip_prefix(module)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
}

struct Ring {
    records: VecDeque<String>,
}

impl Ring {
    const fn new() -> Self {
        Self {
            records: VecDeque::new(),
        }
    }

    fn push(&mut self, record: &str) {
        // Once full, recycle the oldest record's allocation.
        let mut slot = if self.records.len() >= RING_CAPACITY {
            self.records.pop_front().unwrap_or_default()
        } else {
            String::new()
        };
        slot.clear();
        slot.push_str(record);
        self.records.push_back(slot);
    }

    fn tail(&self, max_records: usize) -> Vec<String> {
        let skip = self.records.len().saturating_sub(max_records);
        self.records.iter().skip(skip).cloned().collect()
    }
}

struct RotatingFile {
    path: PathBuf,
    config: FileConfig,
    file: File,
    /// Bytes in the current file.
    size: u64,
    /// Rotate before the file would grow past this size. It is
    /// `config.max_bytes`, pushed further out after a failed rotation.
    rotate_at: u64,
}

impl RotatingFile {
    fn open(path: &Path, config: FileConfig) -> io::Result<Self> {
        if config.rotate_on_open && fs::metadata(path).is_ok_and(|existing| existing.len() > 0) {
            shift_files(path, config.keep)?;
        }
        let (file, size) = open_for_append(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            config,
            file,
            size,
            rotate_at: config.max_bytes,
        })
    }

    /// Appends `line`, which ends with a newline, rotating first if needed.
    fn write_line(&mut self, line: &str) {
        let len = line.len() as u64;
        if self.size > 0 && self.size.saturating_add(len) > self.rotate_at && self.rotate().is_err()
        {
            // Keep logging to the current file, and do not retry on every
            // line: wait until another `max_bytes` have been written.
            self.rotate_at = self.size.saturating_add(self.config.max_bytes);
        }
        // A failed write (disk full, file removed) cannot be reported
        // through the log itself; the record still reaches the other sinks.
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.size += len;
        }
    }

    fn rotate(&mut self) -> io::Result<()> {
        shift_files(&self.path, self.config.keep)?;
        let (file, size) = open_for_append(&self.path)?;
        // Assigning drops the old handle, which by now refers to the file
        // that was renamed to `<path>.1`.
        self.file = file;
        self.size = size;
        self.rotate_at = self.config.max_bytes;
        Ok(())
    }
}

fn open_for_append(path: &Path) -> io::Result<(File, u64)> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let size = file.metadata()?.len();
    Ok((file, size))
}

/// Moves `path` out of the way: `path` becomes `path.1`, `path.1` becomes
/// `path.2`, and so on up to `path.<keep>`, whose old contents are lost.
/// With `keep == 0`, `path` is simply deleted.
fn shift_files(path: &Path, keep: u32) -> io::Result<()> {
    if keep == 0 {
        return ignore_not_found(fs::remove_file(path));
    }
    for index in (1..keep).rev() {
        ignore_not_found(fs::rename(numbered(path, index), numbered(path, index + 1)))?;
    }
    ignore_not_found(fs::rename(path, numbered(path, 1)))
}

/// Returns `path` with `.<index>` appended to the file name.
fn numbered(path: &Path, index: u32) -> PathBuf {
    let mut name = OsString::from(path);
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

fn ignore_not_found(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    // These tests use private `Logger`, `Filter` and `Ring` values, never
    // the process-wide logger: unit tests run in parallel and would step on
    // each other's settings. The process-wide logger and the file sink are
    // tested in `tests/log.rs`.

    fn quiet_logger() -> Logger {
        let mut logger = Logger::new();
        logger.stderr = false;
        logger
    }

    #[test]
    fn levels_order_from_most_to_least_severe() {
        assert!(Level::Error < Level::Warn);
        assert!(Level::Warn < Level::Info);
        assert!(Level::Info < Level::Debug);
        assert!(Level::Debug < Level::Trace);
    }

    #[test]
    fn level_names() {
        let names = [
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
        ]
        .map(Level::as_str);
        assert_eq!(names, ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"]);
        assert_eq!(Level::Warn.to_string(), "WARN");
        assert_eq!(format!("[{:<5}]", Level::Warn), "[WARN ]");
    }

    #[test]
    fn default_config_matches_the_documented_policy() {
        let config = FileConfig::default();
        assert_eq!(config.keep, 3);
        assert_eq!(config.max_bytes, 4 * 1024 * 1024);
        assert!(!config.rotate_on_open);
    }

    #[test]
    fn filter_defaults_to_info() {
        let filter = Filter::new();
        assert_eq!(filter.level_for("nadir_render"), Level::Info);
        assert_eq!(filter.most_verbose(), Level::Info);
    }

    #[test]
    fn target_overrides_apply_to_a_module_and_everything_inside_it() {
        let mut filter = Filter::new();
        filter.set_target("nadir_render", Some(Level::Trace));
        assert_eq!(filter.level_for("nadir_render"), Level::Trace);
        assert_eq!(filter.level_for("nadir_render::batch"), Level::Trace);
        assert_eq!(filter.level_for("nadir_render::batch::sort"), Level::Trace);
        // A name that merely starts with the same letters is another crate.
        assert_eq!(filter.level_for("nadir_renderer"), Level::Info);
        assert_eq!(filter.level_for("nadir_audio"), Level::Info);
        assert_eq!(filter.level_for("nadir"), Level::Info);
    }

    #[test]
    fn the_longest_matching_override_wins() {
        let mut filter = Filter::new();
        filter.set_target("nadir_render::batch", Some(Level::Error));
        filter.set_target("nadir_render", Some(Level::Debug));
        assert_eq!(filter.level_for("nadir_render::batch::sort"), Level::Error);
        assert_eq!(filter.level_for("nadir_render::camera"), Level::Debug);
        // An override can make a target quieter than the default as well.
        assert_eq!(filter.most_verbose(), Level::Debug);
    }

    #[test]
    fn overrides_can_be_replaced_and_removed() {
        let mut filter = Filter::new();
        filter.set_target("game", Some(Level::Trace));
        filter.set_target("game", Some(Level::Warn));
        assert_eq!(filter.targets.len(), 1);
        assert_eq!(filter.level_for("game::player"), Level::Warn);
        assert_eq!(filter.most_verbose(), Level::Info);

        filter.set_target("game", None);
        assert!(filter.targets.is_empty());
        assert_eq!(filter.level_for("game::player"), Level::Info);
        // Removing an override that does not exist is fine.
        filter.set_target("game", None);
    }

    #[test]
    fn most_verbose_follows_the_default_and_the_overrides() {
        let mut filter = Filter::new();
        filter.default = Level::Error;
        assert_eq!(filter.most_verbose(), Level::Error);
        filter.set_target("a", Some(Level::Debug));
        filter.set_target("b", Some(Level::Warn));
        assert_eq!(filter.most_verbose(), Level::Debug);
        filter.default = Level::Trace;
        assert_eq!(filter.most_verbose(), Level::Trace);
    }

    #[test]
    fn lines_have_a_timestamp_level_target_and_message() {
        let mut logger = quiet_logger();
        let time = UNIX_EPOCH + Duration::from_millis(1_234_567_890_123);
        logger.write(
            time,
            Level::Info,
            "nadir_assets::server",
            "loaded 12 assets",
        );
        logger.write(time, Level::Error, "game", "boom");
        assert_eq!(
            logger.ring.tail(10),
            [
                "2009-02-13T23:31:30.123Z INFO  nadir_assets::server: loaded 12 assets",
                "2009-02-13T23:31:30.123Z ERROR game: boom",
            ]
        );
    }

    #[test]
    fn multi_line_messages_stay_one_record() {
        let mut logger = quiet_logger();
        logger.write(UNIX_EPOCH, Level::Warn, "game", "first\nsecond\n");
        let records = logger.ring.tail(10);
        assert_eq!(
            records,
            ["1970-01-01T00:00:00.000Z WARN  game: first\nsecond"]
        );
    }

    #[test]
    fn ring_keeps_only_the_most_recent_records() {
        let mut ring = Ring::new();
        assert!(ring.tail(5).is_empty());
        for n in 0..RING_CAPACITY + 250 {
            ring.push(&format!("record {n}"));
        }
        assert_eq!(ring.records.len(), RING_CAPACITY);
        let all = ring.tail(usize::MAX);
        assert_eq!(all.len(), RING_CAPACITY);
        assert_eq!(all[0], "record 250");
        assert_eq!(
            all[RING_CAPACITY - 1],
            format!("record {}", RING_CAPACITY + 249)
        );
        // Oldest first, and no more than asked for.
        assert_eq!(
            ring.tail(2),
            [
                format!("record {}", RING_CAPACITY + 248),
                format!("record {}", RING_CAPACITY + 249)
            ]
        );
        assert!(ring.tail(0).is_empty());
    }

    #[test]
    fn a_full_ring_recycles_allocations() {
        let mut ring = Ring::new();
        for _ in 0..RING_CAPACITY {
            ring.push("a record of typical length, give or take");
        }
        let oldest = ring.records[0].as_ptr();
        ring.push("short");
        assert_eq!(ring.records[RING_CAPACITY - 1], "short");
        assert_eq!(ring.records[RING_CAPACITY - 1].as_ptr(), oldest);
    }

    #[test]
    fn rotated_files_are_numbered_after_the_full_file_name() {
        assert_eq!(
            numbered(Path::new("logs/nadir.log"), 1),
            Path::new("logs/nadir.log.1")
        );
        assert_eq!(
            numbered(Path::new("nadir.log"), 12),
            Path::new("nadir.log.12")
        );
    }

    #[test]
    fn only_not_found_is_ignored() {
        assert!(ignore_not_found(Ok(())).is_ok());
        assert!(ignore_not_found(Err(io::ErrorKind::NotFound.into())).is_ok());
        let denied = ignore_not_found(Err(io::ErrorKind::PermissionDenied.into()));
        assert_eq!(
            denied.map_err(|error| error.kind()),
            Err(io::ErrorKind::PermissionDenied)
        );
    }
}
