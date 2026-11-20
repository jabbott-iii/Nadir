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

//! Crash reports: what the game was and did when it panicked, written into
//! the `crashes` folder of the game's data folder, and told of on the next
//! launch (`docs/05-release.md`, "Crash reporting").
//!
//! [`install`] sets the panic hook, once in a process, and [`App::run`]
//! calls it first. The hook writes a report for every panic, on any thread,
//! and names it in `unseen.txt` beside it; [`tell_of_unseen`] tells the
//! player of those on the next launch. A panic that ends `App::run` is told
//! of by [`run_reporting`] once the window has closed.
//!
//! [`App::run`]: crate::App::run

use std::backtrace::Backtrace;
use std::cell::RefCell;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::panic::{self, AssertUnwindSafe, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, Once, PoisonError};
use std::thread;
use std::time::SystemTime;

use nadir_core::civil::DateTime;
use nadir_core::log::{self, error, warn};
use nadir_platform::{os_description, show_error_dialog, user_dirs};

use crate::Config;

/// The folder of the crash reports, in the game's data folder.
pub(crate) const FOLDER: &str = "crashes";

/// The file in [`FOLDER`] that names the reports the player has not been
/// told of, one file name a line, oldest first.
const UNSEEN: &str = "unseen.txt";

/// How many of the log's last records a report holds.
const LOG_RECORDS: usize = 200;

/// The most read of [`UNSEEN`], from its end: a name is about 25 bytes, so
/// some 2,600 names.
const MAX_UNSEEN_BYTES: u64 = 64 * 1024;

/// How many reports one second's time stamp names: `<stamp>.txt`, then
/// `<stamp>-2.txt` and on.
const MAX_SAME_SECOND: u32 = 100;

/// The engine's version: the workspace's (`docs/05-release.md`,
/// "Versioning").
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The build's git hash, if whatever built it said (roadmap M11.1).
const BUILD_HASH: Option<&str> = option_env!("NADIR_BUILD_HASH");

/// What a report says of the game and the system, set by [`install`].
static CONTEXT: Mutex<Option<Context>> = Mutex::new(None);

/// Sets the panic hook once in the process.
static HOOK: Once = Once::new();

thread_local! {
    /// The report of this thread's last panic: where it was written, or
    /// why it was not.
    static LAST_REPORT: RefCell<Option<Result<PathBuf, String>>> = const { RefCell::new(None) };
}

/// What a report says besides the panic, and where it goes.
#[derive(Clone, Debug)]
struct Context {
    /// The folder of the reports, or why there is none.
    folder: Result<PathBuf, String>,
    /// The game, and who made it.
    game: String,
    /// The system ([`os_description`]).
    os: String,
    /// The GPU, once the device is made ([`set_gpu`]).
    gpu: String,
}

/// Starts writing a crash report for each panic, into the `crashes` folder
/// of the data folder of `config`'s game, and keeps the panic hook that was
/// set before, which runs first. A second call keeps the hook and changes
/// the game it reports.
pub(crate) fn install(config: &Config) {
    let folder = user_dirs(&config.studio, &config.game)
        .map(|dirs| dirs.data.join(FOLDER))
        .map_err(|error| error.to_string());
    install_context(Context {
        folder,
        game: format!("{}, by {}", config.game, config.studio),
        os: os_description(),
        gpu: "not known: there was no GPU device yet".to_string(),
    });
}

fn install_context(context: Context) {
    *lock() = Some(context);
    HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            previous(info);
            report(info);
        }));
    });
}

/// Sets what the reports say of the GPU: its renderer, maker and version,
/// or why there is no device.
pub(crate) fn set_gpu(description: String) {
    if let Some(context) = lock().as_mut() {
        context.gpu = description;
    }
}

/// Tells the player of the reports that the hook wrote since they were last
/// told, if any, in the log and with [`show_error_dialog`], and forgets
/// them.
pub(crate) fn tell_of_unseen(title: &str) {
    let folder = lock()
        .as_ref()
        .and_then(|context| context.folder.clone().ok());
    if let Some(message) = folder.and_then(|folder| take_unseen(&folder)) {
        warn!("{message}");
        show_error_dialog(title, &message);
    }
}

/// What the player is told of the reports named in [`UNSEEN`] in `folder`,
/// if any; and forgets them, removing the file. What fails is logged.
fn take_unseen(folder: &Path) -> Option<String> {
    let list = folder.join(UNSEEN);
    let reports = match read_unseen(folder) {
        Ok(reports) => reports?,
        Err(error) => {
            warn!(
                "the crash reports not told of yet are not known: {} could not be read: {error}",
                list.display()
            );
            return None;
        }
    };
    if let Err(error) = fs::remove_file(&list) {
        warn!(
            "{} could not be removed, so its crash reports are told of again next time: {error}",
            list.display()
        );
    }
    unseen_notice(&reports)
}

/// Runs `run`, and returns what it returns. If it panics, the player is
/// told with [`show_error_dialog`], once unwinding out of `run` has dropped
/// what it held (the window, in `App::run`), that the game crashed and where
/// this thread's last report is; then the panic unwinds on.
///
/// The dialog is not shown from the hook: that would show it for panics the
/// engine survives, and on Windows a message box runs the thread's window
/// messages, the game's window among them, in the middle of the panic.
pub(crate) fn run_reporting<R>(title: &str, run: impl FnOnce() -> R) -> R {
    match panic::catch_unwind(AssertUnwindSafe(run)) {
        Ok(result) => result,
        Err(payload) => {
            show_error_dialog(title, &panic_notice());
            panic::resume_unwind(payload)
        }
    }
}

/// What [`run_reporting`] tells the player of a panic.
fn panic_notice() -> String {
    let last = LAST_REPORT
        .try_with(|last| last.borrow().clone())
        .ok()
        .flatten();
    match last {
        Some(Ok(path)) => format!("The game crashed. Its crash report is {}.", path.display()),
        Some(Err(why)) => format!("The game crashed, and its crash report was not written: {why}."),
        None => "The game crashed.".to_string(),
    }
}

/// The panic hook's work: writes the report of `info`, names it in
/// [`UNSEEN`], and logs where it is.
fn report(info: &PanicHookInfo<'_>) {
    let now = SystemTime::now();
    let Some(context) = lock().clone() else {
        return;
    };
    let folder = context.folder.clone();
    let report = Report::new(info, context, now);
    let written = folder.and_then(|folder| {
        write_report(&folder, &report.to_string(), now).map_err(|error| error.to_string())
    });
    match &written {
        Ok(path) => error!("the game panicked; its crash report is {}", path.display()),
        // The log's records stay out of the log's copy: they are in the log
        // already, and the next report's records would hold this one, each
        // report twice the size of the one before.
        Err(why) => error!(
            "the game panicked, and its crash report was not written ({why}):\n{}",
            WithoutLog(&report)
        ),
    }
    // A thread that is ending has no locals left: it has nobody to tell.
    let _ = LAST_REPORT.try_with(|last| *last.borrow_mut() = Some(written));
}

/// The context, whatever a panic did while it was held: nothing here
/// leaves it half changed.
fn lock() -> MutexGuard<'static, Option<Context>> {
    CONTEXT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One crash report's parts.
#[derive(Debug)]
struct Report {
    context: Context,
    time: DateTime,
    thread: String,
    location: String,
    message: String,
    backtrace: String,
    log: Vec<String>,
}

impl Report {
    /// The report of `info`, with the backtrace of the thread now and the
    /// log's last records.
    fn new(info: &PanicHookInfo<'_>, context: Context, now: SystemTime) -> Self {
        let thread = thread::current();
        Self {
            context,
            time: DateTime::from_system_time(now),
            thread: thread.name().unwrap_or("unnamed").to_string(),
            location: info.location().map_or_else(
                || "an unknown place".to_string(),
                |location| location.to_string(),
            ),
            message: info
                .payload_as_str()
                .unwrap_or("(a payload that is not text)")
                .to_string(),
            backtrace: Backtrace::force_capture().to_string(),
            log: log::tail(LOG_RECORDS),
        }
    }
}

/// The report as it is written: a header of the game, the build, the time,
/// the system and the GPU; then the panic, the backtrace and the log.
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_head(f)?;
        match self.log.len() {
            1 => writeln!(f, "The log's last record:")?,
            n => writeln!(f, "The log's last {n} records:")?,
        }
        for record in &self.log {
            writeln!(f, "{record}")?;
        }
        Ok(())
    }
}

/// A report without the log's records, for the log itself.
struct WithoutLog<'a>(&'a Report);

impl fmt::Display for WithoutLog<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.write_head(f)?;
        write!(f, "The log's last records: the log's, before this one.")
    }
}

impl Report {
    /// Writes everything but the log's records: the header, the panic and
    /// the backtrace, each followed by a blank line.
    fn write_head(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let build = match BUILD_HASH {
            Some(hash) => format!("build {hash}"),
            None => "build unknown".to_string(),
        };
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        let dev = if cfg!(feature = "dev") { ", dev" } else { "" };
        writeln!(f, "Nadir crash report")?;
        writeln!(f)?;
        writeln!(f, "Game:    {}", self.context.game)?;
        writeln!(
            f,
            "Engine:  nadir_engine {VERSION}, {build}, {profile}{dev}"
        )?;
        writeln!(f, "Time:    {}", self.time)?;
        writeln!(f, "System:  {}", self.context.os)?;
        writeln!(f, "GPU:     {}", self.context.gpu)?;
        writeln!(f, "Thread:  {}", self.thread)?;
        writeln!(f)?;
        writeln!(f, "The panic, at {}:", self.location)?;
        writeln!(f, "{}", self.message)?;
        writeln!(f)?;
        writeln!(f, "Backtrace:")?;
        writeln!(f, "{}", self.backtrace.trim_end())?;
        writeln!(f)
    }
}

/// Writes `text` into a new file in `folder`, which it makes if need be,
/// named after the time `now` (`2026-11-19_12-34-56.txt`, then `-2` and on
/// within the same second), names it in [`UNSEEN`], and returns its path.
/// A report written but not named there is still written: a warning says
/// that the next launch will not tell of it.
fn write_report(folder: &Path, text: &str, now: SystemTime) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let stamp = DateTime::from_system_time(now).file_stamp();
    let (mut file, name) = create_named(folder, &stamp)?;
    file.write_all(text.as_bytes())?;
    let path = folder.join(&name);
    // One write of a whole line, appended: two threads' lines do not mix.
    let listed = OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join(UNSEEN))
        .and_then(|mut unseen| unseen.write_all(format!("{name}\n").as_bytes()));
    if let Err(error) = listed {
        warn!(
            "the crash report {} is not named in {UNSEEN}, so the next launch does not tell of \
             it: {error}",
            path.display()
        );
    }
    Ok(path)
}

/// Creates a file in `folder` named `<stamp>.txt`, or the first of
/// `<stamp>-2.txt` and on that no file has. Returns it and its name.
fn create_named(folder: &Path, stamp: &str) -> io::Result<(File, String)> {
    for n in 1..=MAX_SAME_SECOND {
        let name = if n == 1 {
            format!("{stamp}.txt")
        } else {
            format!("{stamp}-{n}.txt")
        };
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(folder.join(&name))
        {
            Ok(file) => return Ok((file, name)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{MAX_SAME_SECOND} reports already have the time {stamp}"),
    ))
}

/// The reports named in [`UNSEEN`] in `folder` that are still there,
/// oldest first, or `None` if the file is not there. A line that is not a
/// report's name in the folder is skipped, and so are the oldest names of a
/// list longer than [`MAX_UNSEEN_BYTES`].
fn read_unseen(folder: &Path) -> io::Result<Option<Vec<PathBuf>>> {
    let mut file = match File::open(folder.join(UNSEEN)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let len = file.metadata()?.len();
    let cut = len > MAX_UNSEEN_BYTES;
    if cut {
        file.seek(SeekFrom::Start(len - MAX_UNSEEN_BYTES))?;
    }
    let mut bytes = Vec::new();
    file.take(MAX_UNSEEN_BYTES).read_to_end(&mut bytes)?;
    // A line that is not UTF-8 was not written here, and names no report.
    let text = String::from_utf8_lossy(&bytes);
    Ok(Some(
        text.lines()
            // Where the list was cut, its first line is a part of a name.
            .skip(usize::from(cut))
            .map(str::trim)
            .filter(|name| is_report_name(name))
            .map(|name| folder.join(name))
            .filter(|report| report.is_file())
            .collect(),
    ))
}

/// Whether `name` can be a report's file name in the folder, and nothing
/// that reaches out of it.
fn is_report_name(name: &str) -> bool {
    name.ends_with(".txt")
        && name != UNSEEN
        && Path::new(name).file_name().and_then(|file| file.to_str()) == Some(name)
}

/// What the player is told of `reports`, the oldest first: `None` if there
/// are none.
fn unseen_notice(reports: &[PathBuf]) -> Option<String> {
    let latest = reports.last()?.display();
    Some(match reports.len() {
        1 => format!("The game crashed the last time it ran. Its crash report is {latest}."),
        n => format!(
            "The game crashed the last time it ran, and wrote {n} crash reports. The latest \
             is {latest}."
        ),
    })
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::process::{self, Command};
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    /// Set in the child process of [`a_panic_writes_a_complete_report`]: the
    /// folder its reports go to.
    const CHILD: &str = "NADIR_CRASH_TEST_FOLDER";

    /// The full name of [`a_panic_writes_a_complete_report`], for the child
    /// to run alone.
    const CHILD_TEST: &str = "crash::tests::a_panic_writes_a_complete_report";

    /// 2009-02-13 23:31:30 UTC.
    fn time() -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(1_234_567_890_123)
    }

    fn test_folder(name: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!("nadir-engine-crash-{name}-{}", process::id()));
        // Left by a run that failed.
        let _ = fs::remove_dir_all(&folder);
        folder
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
    }

    fn context(folder: &Path) -> Context {
        Context {
            folder: Ok(folder.to_path_buf()),
            game: "Test, by Nadir".to_string(),
            os: os_description(),
            gpu: "not known: there was no GPU device yet".to_string(),
        }
    }

    #[test]
    fn a_report_has_the_game_the_system_the_panic_its_backtrace_and_the_log() {
        let mut report = Report {
            context: Context {
                folder: Err("no folder".to_string()),
                game: "Sandbox, by Nadir".to_string(),
                os: "Pop!_OS 24.04 LTS (Linux 7.2.15-generic, x86_64)".to_string(),
                gpu: "llvmpipe, by Mesa; OpenGL 4.5".to_string(),
            },
            time: DateTime::from_system_time(time()),
            thread: "main".to_string(),
            location: "src/main.rs:12:5".to_string(),
            message: "it broke\non two lines".to_string(),
            backtrace: "   0: main\n   1: start\n".to_string(),
            log: vec!["first record".to_string(), "second record".to_string()],
        };
        let build = BUILD_HASH.unwrap_or("unknown");
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        let dev = if cfg!(feature = "dev") { ", dev" } else { "" };
        assert_eq!(
            report.to_string(),
            format!(
                "Nadir crash report\n\
                 \n\
                 Game:    Sandbox, by Nadir\n\
                 Engine:  nadir_engine {VERSION}, build {build}, {profile}{dev}\n\
                 Time:    2009-02-13T23:31:30.123Z\n\
                 System:  Pop!_OS 24.04 LTS (Linux 7.2.15-generic, x86_64)\n\
                 GPU:     llvmpipe, by Mesa; OpenGL 4.5\n\
                 Thread:  main\n\
                 \n\
                 The panic, at src/main.rs:12:5:\n\
                 it broke\n\
                 on two lines\n\
                 \n\
                 Backtrace:\n   \
                 0: main\n   \
                 1: start\n\
                 \n\
                 The log's last 2 records:\n\
                 first record\n\
                 second record\n"
            )
        );
        report.log.truncate(1);
        let written = report.to_string();
        assert!(
            written.ends_with("\n\nThe log's last record:\nfirst record\n"),
            "{written}"
        );
        // The log's copy leaves the log's records out.
        let logged = WithoutLog(&report).to_string();
        assert_eq!(
            logged,
            written.replace(
                "The log's last record:\nfirst record\n",
                "The log's last records: the log's, before this one."
            )
        );
    }

    #[test]
    fn reports_are_named_after_their_time_and_never_overwrite_one_another() {
        let root = test_folder("names");
        let folder = root.join("crashes");
        let names: Vec<String> = ["one", "two", "three"]
            .iter()
            .map(|text| {
                let path = write_report(&folder, text, time()).expect("write a report");
                assert_eq!(read(&path), *text);
                path.file_name()
                    .and_then(|name| name.to_str())
                    .expect("a name")
                    .to_string()
            })
            .collect();
        assert_eq!(
            names,
            [
                "2009-02-13_23-31-30.txt",
                "2009-02-13_23-31-30-2.txt",
                "2009-02-13_23-31-30-3.txt"
            ]
        );
        assert_eq!(read(&folder.join(UNSEEN)), names.join("\n") + "\n");

        // A second whose names are all taken, to `-100`, writes nothing.
        assert_eq!(MAX_SAME_SECOND, 100);
        let full = root.join("full");
        fs::create_dir_all(&full).expect("make the folder");
        for n in 1..=MAX_SAME_SECOND {
            let (_, name) = create_named(&full, "stamp").expect("a free name");
            let expected = if n == 1 {
                "stamp.txt".to_string()
            } else {
                format!("stamp-{n}.txt")
            };
            assert_eq!(name, expected);
            assert!(full.join(&name).is_file());
        }
        let error = create_named(&full, "stamp").expect_err("no name left");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);

        // A report that cannot be listed is written all the same.
        let unlisted = root.join("unlisted");
        fs::create_dir_all(unlisted.join(UNSEEN)).expect("a folder where the list goes");
        let path = write_report(&unlisted, "report", time()).expect("written");
        assert_eq!(read(&path), "report");

        fs::remove_dir_all(&root).expect("remove the test folder");
    }

    #[test]
    fn the_unseen_reports_are_told_of_once_and_only_those_still_there() {
        let root = test_folder("unseen");
        let folder = root.join("crashes");
        assert_eq!(read_unseen(&folder).expect("no folder"), None);
        let paths: Vec<PathBuf> = (0..3)
            .map(|n| {
                write_report(&folder, "report", time() + Duration::from_secs(n)).expect("write")
            })
            .collect();
        fs::remove_file(&paths[1]).expect("remove a report");
        // Lines that name no report of the folder: `../outside.txt` is a
        // file beside it, in the test's own folder.
        fs::write(root.join("outside.txt"), "x").expect("a file outside");
        let mut unseen = OpenOptions::new()
            .append(true)
            .open(folder.join(UNSEEN))
            .expect("open the list");
        unseen
            .write_all(b"\n../outside.txt\nsub/inner.txt\nunseen.txt\nnot-a-report.log\n  \n")
            .expect("add lines");
        unseen
            .write_all(b"\xff\xfe.txt\n")
            .expect("add a line not UTF-8");

        let told = read_unseen(&folder).expect("read the list");
        assert_eq!(told, Some(vec![paths[0].clone(), paths[2].clone()]));
        // Told of once: the list goes, and the reports stay.
        assert_eq!(
            take_unseen(&folder),
            unseen_notice(&[paths[0].clone(), paths[2].clone()])
        );
        assert!(!folder.join(UNSEEN).exists());
        assert_eq!(read_unseen(&folder).expect("told already"), None);
        assert_eq!(take_unseen(&folder), None);
        assert!(paths[0].is_file() && paths[2].is_file());

        // Of a list longer than the most read, the newest names are kept.
        let long = root.join("long");
        let oldest = write_report(&long, "report", time()).expect("write");
        let newest = write_report(&long, "report", time()).expect("write");
        let name = |path: &Path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .expect("a name")
                .to_string()
        };
        // The cut falls two bytes into the line `zz<oldest>`: what is read
        // of it is the oldest's name, but not a line of the list.
        let cut_line = format!("zz{}\n", name(&oldest));
        let last = name(&newest) + "\n";
        let read = usize::try_from(MAX_UNSEEN_BYTES).expect("a small number");
        let fill = read - (cut_line.len() - 2) - last.len();
        let mut filler = String::new();
        while filler.len() + 100 <= fill {
            filler.push_str(&"f".repeat(95));
            filler.push_str(".txt\n");
        }
        if fill > filler.len() {
            filler.push_str(&"g".repeat(fill - filler.len() - 1));
            filler.push('\n');
        }
        let list = format!("{}\n{cut_line}{filler}{last}", name(&oldest));
        assert_eq!(list.len(), read + name(&oldest).len() + 3);
        fs::write(long.join(UNSEEN), list).expect("write the list");
        assert_eq!(
            read_unseen(&long).expect("read the list"),
            Some(vec![newest])
        );

        // A list that cannot be removed is told of now, and again next time.
        let locked = root.join("locked");
        let report = write_report(&locked, "report", time()).expect("write");
        let permissions = fs::metadata(&locked).expect("the folder").permissions();
        let mut read_only = permissions.clone();
        read_only.set_readonly(true);
        fs::set_permissions(&locked, read_only).expect("make the folder read-only");
        // Where the folder stays writable (as root, or on Windows), there is
        // nothing to check.
        if File::create(locked.join("probe")).is_err() {
            assert_eq!(take_unseen(&locked), unseen_notice(&[report]));
            assert!(locked.join(UNSEEN).is_file());
        }
        fs::set_permissions(&locked, permissions).expect("make the folder writable again");

        fs::remove_dir_all(&root).expect("remove the test folder");
    }

    #[test]
    fn the_notice_names_the_latest_report() {
        assert_eq!(unseen_notice(&[]), None);
        let one = [PathBuf::from("/data/crashes/a.txt")];
        assert_eq!(
            unseen_notice(&one).as_deref(),
            Some("The game crashed the last time it ran. Its crash report is /data/crashes/a.txt.")
        );
        let three = ["a", "b", "c"].map(|name| PathBuf::from(format!("/data/crashes/{name}.txt")));
        assert_eq!(
            unseen_notice(&three).as_deref(),
            Some(
                "The game crashed the last time it ran, and wrote 3 crash reports. The latest is \
                 /data/crashes/c.txt."
            )
        );
        assert!(is_report_name("2026-11-19_12-34-56-2.txt"));
        for name in ["", ".txt/..", "../a.txt", "a/b.txt", "unseen.txt", "a.log"] {
            assert!(!is_report_name(name), "{name}");
        }
    }

    /// The hook is the whole process's, so it is set in a child process,
    /// which runs this test alone with [`CHILD`] set: there it panics on a
    /// thread of its own, on the test's, with no folder for the reports, and
    /// last out of [`run_reporting`], which ends the child.
    #[test]
    fn a_panic_writes_a_complete_report() {
        if let Some(folder) = env::var_os(CHILD) {
            panic_in_child(Path::new(&folder));
            return;
        }
        let folder = test_folder("hook");
        let output = Command::new(env::current_exe().expect("the test binary"))
            .args([CHILD_TEST, "--exact", "--nocapture", "--test-threads", "1"])
            .env(CHILD, &folder)
            // Only `force_capture` captures then, as for players.
            .env("RUST_LIB_BACKTRACE", "0")
            .output()
            .expect("run the child");
        let stderr = String::from_utf8_lossy(&output.stderr);
        // The child's checks passed, then its last panic ended it, as one
        // that ends `App::run` ends the game.
        assert!(
            folder.join("checks-passed").is_file(),
            "the child's checks failed: {stderr}"
        );
        assert_eq!(output.status.code(), Some(101), "{stderr}");

        // Three reports, named in `unseen.txt` in the order of their panics.
        let unseen = read(&folder.join(UNSEEN));
        let names: Vec<&str> = unseen.lines().collect();
        assert_eq!(names.len(), 3, "{unseen}");
        let reports: Vec<String> = names.iter().map(|name| read(&folder.join(name))).collect();
        for (report, thread, message) in [
            (
                &reports[0],
                "Thread:  worker",
                "a panic on a thread of its own",
            ),
            (&reports[1], "Thread:  ", "a panic on the test's thread"),
            (&reports[2], "Thread:  ", "a panic that ends the run"),
        ] {
            for expected in [
                // The second install's game.
                "Nadir crash report\n\nGame:    Test, by Nadir\n",
                &format!("Engine:  nadir_engine {VERSION}, build "),
                &format!("System:  {}\n", os_description()),
                "GPU:     a test GPU\n",
                thread,
                &format!("The panic, at {}:", file!()),
                &format!(":\n{message}\n\nBacktrace:\n"),
                // The backtrace names the function that panicked.
                "panic_in_child",
                "The log's last ",
                "before the panics",
            ] {
                assert!(report.contains(expected), "{expected:?} in:\n{report}");
            }
        }
        // The second's log has the first panic in it.
        assert!(
            reports[1].contains("the game panicked; its crash report is "),
            "{}",
            reports[1]
        );
        // The hook set before ran first, every time: at the first panic
        // there was no list yet.
        assert_eq!(
            read(&folder.join("previous.txt")),
            "false\ntrue\ntrue\ntrue\ntrue\n"
        );
        // The thread that panicked second is told of its own report.
        let told = read(&folder.join("told.txt"));
        assert_eq!(
            told,
            format!(
                "The game crashed. Its crash report is {}.",
                folder.join(names[1]).display()
            )
        );
        // The panic that ended the run: what it held dropped, then the
        // player was told of its report.
        let dropped = stderr.find("the guard dropped").expect("the guard's line");
        let ending = format!(
            "Test: The game crashed. Its crash report is {}.\n",
            folder.join(names[2]).display()
        );
        let shown = stderr.find(&ending).expect("the dialog's line");
        assert!(dropped < shown, "{stderr}");

        let listed = read_unseen(&folder).expect("read the list");
        assert_eq!(
            listed,
            Some(names.iter().map(|name| folder.join(name)).collect())
        );
        fs::remove_dir_all(&folder).expect("remove the test folder");
    }

    /// What the child process does (see [`a_panic_writes_a_complete_report`]).
    fn panic_in_child(folder: &Path) {
        log::set_stderr(false);
        fs::create_dir_all(folder).expect("make the folder");
        // A hook set before, which must run first: it says whether the list
        // was there yet.
        let list = folder.join(UNSEEN);
        let previous = folder.join("previous.txt");
        panic::set_hook(Box::new(move |_| {
            let there = list.exists();
            let _ = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&previous)
                .and_then(|mut file| writeln!(file, "{there}"));
        }));
        // A second install keeps one hook, and reports the second game.
        install_context(Context {
            game: "First, by Nadir".to_string(),
            ..context(folder)
        });
        install_context(context(folder));
        set_gpu("a test GPU".to_string());
        nadir_core::log::info!("before the panics");
        // Nothing was told of on this thread yet.
        assert_eq!(panic_notice(), "The game crashed.");
        let worker = thread::Builder::new()
            .name("worker".to_string())
            .spawn(|| panic!("a panic on a thread of its own"))
            .expect("start the thread");
        assert!(worker.join().is_err());
        // The worker's panic is not this thread's.
        assert_eq!(panic_notice(), "The game crashed.");
        let caught = panic::catch_unwind(|| panic!("a panic on the test's thread"));
        assert!(caught.is_err());
        fs::write(folder.join("told.txt"), panic_notice()).expect("write the notice");

        // With no folder, the report goes into the log, without the log's
        // records: the second holds no copy of the first.
        install_context(Context {
            folder: Err("no data folder".to_string()),
            ..context(folder)
        });
        for _ in 0..2 {
            assert!(panic::catch_unwind(|| panic!("a panic with nowhere to go")).is_err());
        }
        assert_eq!(
            panic_notice(),
            "The game crashed, and its crash report was not written: no data folder."
        );
        let logged: Vec<String> = log::tail(LOG_RECORDS)
            .into_iter()
            .filter(|record| record.contains("its crash report was not written (no data folder):"))
            .collect();
        assert_eq!(logged.len(), 2, "{logged:?}");
        for record in &logged {
            for expected in [
                "\nNadir crash report\n\nGame:    Test, by Nadir\n",
                "\na panic with nowhere to go\n\nBacktrace:\n",
                "panic_in_child",
            ] {
                assert!(record.contains(expected), "{expected:?} in:\n{record}");
            }
            assert!(
                record.ends_with("\n\nThe log's last records: the log's, before this one."),
                "{record}"
            );
            assert!(!record.contains("before the panics"), "{record}");
            assert_eq!(record.matches("Nadir crash report").count(), 1, "{record}");
        }

        install_context(context(folder));
        set_gpu("a test GPU".to_string());
        fs::write(folder.join("checks-passed"), "").expect("write the marker");
        // Last, a panic out of `run_reporting`, which unwinds on and ends the
        // child.
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                let _ = writeln!(io::stderr(), "the guard dropped");
            }
        }
        run_reporting("Test", || {
            let _guard = Guard;
            panic!("a panic that ends the run");
        });
    }
}
