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

//! The hot-reload watcher: a thread that polls the loaded paths' times of
//! change, and hands the changed paths to the main thread (ADR-0042).

use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use nadir_core::FastHashMap;

use crate::{AssetPath, Source};

/// How often the watcher looks at the times of change.
pub(crate) const INTERVAL: Duration = Duration::from_millis(250);

/// When a watched path's file last changed, as the watcher knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seen {
    /// Not looked at yet: the next look, the watcher's or a read's, sets it.
    Unread,
    /// Its time of change, or `None` for a file the source cannot tell of,
    /// such as a missing one.
    At(Option<SystemTime>),
}

struct Watched {
    path: Arc<AssetPath>,
    seen: Seen,
    /// Sent, and not received yet: it is not sent again until it is, so
    /// that a path that changes often is reloaded once a drain.
    queued: bool,
}

/// What the main thread, the workers and the watcher share: the watched
/// paths, and what stops the watcher.
pub(crate) struct Shared {
    paths: Mutex<FastHashMap<String, Watched>>,
    stop: Mutex<bool>,
    woken: Condvar,
    /// Set with `stop`, and read between a look's asks, so that stopping
    /// waits for one ask at most, not for a whole look.
    stopping: AtomicBool,
}

/// What the source says of when `path` changed: a source that panics tells
/// nothing.
fn modified(source: &dyn Source, path: &AssetPath) -> Option<SystemTime> {
    panic::catch_unwind(AssertUnwindSafe(|| source.modified(path))).unwrap_or(None)
}

impl Shared {
    fn new() -> Self {
        Self {
            paths: Mutex::new(FastHashMap::default()),
            stop: Mutex::new(false),
            woken: Condvar::new(),
            stopping: AtomicBool::new(false),
        }
    }

    fn paths(&self) -> MutexGuard<'_, FastHashMap<String, Watched>> {
        // Nothing is left half-changed by a panic while it is held: a
        // poisoned map is still a map.
        self.paths.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Watches `path` too, if it was not watched. Its time is taken by the
    /// next look at it.
    pub(crate) fn watch(&self, path: &AssetPath) {
        let mut paths = self.paths();
        if !paths.contains_key(path.as_str()) {
            paths.insert(
                path.as_str().to_string(),
                Watched {
                    path: Arc::new(path.clone()),
                    seen: Seen::Unread,
                    queued: false,
                },
            );
        }
    }

    /// Takes `path`'s time of change from `source`, if no look and no read
    /// has yet: called just before each read of a watched path, so that a
    /// change after the first read is seen if it gives the file a new time
    /// of change.
    ///
    /// The source is asked with the map unlocked, so that loads and other
    /// reads never wait for it. That is sound: whoever sets the time first,
    /// this read, another one or a look, asked before this read reads.
    pub(crate) fn before_read(&self, path: &AssetPath, source: &dyn Source) {
        let unread = |paths: &FastHashMap<String, Watched>| {
            paths
                .get(path.as_str())
                .is_some_and(|watched| watched.seen == Seen::Unread)
        };
        if !unread(&self.paths()) {
            return;
        }
        let time = modified(source, path);
        let mut paths = self.paths();
        if let Some(watched) = paths.get_mut(path.as_str())
            && watched.seen == Seen::Unread
        {
            watched.seen = Seen::At(time);
        }
    }

    /// Notes that the main thread received `path`, so that the next change
    /// is sent again.
    fn received(&self, path: &AssetPath) {
        if let Some(watched) = self.paths().get_mut(path.as_str()) {
            watched.queued = false;
        }
    }

    /// Stops watching the paths for which `keep` is false.
    pub(crate) fn retain(&self, mut keep: impl FnMut(&str) -> bool) {
        self.paths().retain(|path, _| keep(path));
    }

    #[cfg(test)]
    fn seen(&self, path: &str) -> Option<Seen> {
        self.paths().get(path).map(|watched| watched.seen)
    }
}

/// The watcher thread, and the main thread's end of what it finds.
pub(crate) struct Watch {
    shared: Arc<Shared>,
    /// In a mutex only so that the server stays `Sync`, as a `Receiver` is
    /// not: it is reached through `&mut self`, and never locked.
    changed: Mutex<Receiver<Arc<AssetPath>>>,
    thread: Option<JoinHandle<()>>,
}

impl Watch {
    /// Starts the thread, `nadir-assets-watch`, which looks at every
    /// watched path through `source` each [`INTERVAL`].
    pub(crate) fn start(source: Arc<dyn Source>) -> io::Result<Self> {
        let shared = Arc::new(Shared::new());
        let (sender, changed) = mpsc::channel();
        let thread = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("nadir-assets-watch".to_string())
                .spawn(move || run(&shared, source.as_ref(), &sender))?
        };
        Ok(Self {
            shared,
            changed: Mutex::new(changed),
            thread: Some(thread),
        })
    }

    /// What the server, its loads and the workers' jobs share with the
    /// thread.
    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// The next path the thread saw change, if there is one, sent once
    /// however often it changed since. It never waits; with nothing
    /// received it takes no lock and allocates nothing.
    pub(crate) fn next_changed(&mut self) -> Option<Arc<AssetPath>> {
        let path = self
            .changed
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .try_recv()
            .ok()?;
        // Before the reload is asked for, so that it reads after any change
        // a look saw while the path was queued.
        self.shared.received(&path);
        Some(path)
    }
}

impl Drop for Watch {
    /// Stops the thread, and waits for it.
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Relaxed);
        *self
            .shared
            .stop
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = true;
        self.shared.woken.notify_all();
        if let Some(thread) = self.thread.take() {
            // A panic in the source is caught in each look, so the thread
            // ends cleanly; if it did not, there is nothing left to do.
            let _ = thread.join();
        }
    }
}

/// The thread's loop: a look each [`INTERVAL`], until stopped or until
/// nothing receives what it finds.
fn run(shared: &Shared, source: &dyn Source, changed: &Sender<Arc<AssetPath>>) {
    // Kept between looks, so that it grows only when more paths are
    // watched. What the source allocates to answer is its own: a
    // `LooseSource` makes each file's full path.
    let mut times = Vec::new();
    loop {
        {
            let stop = shared.stop.lock().unwrap_or_else(PoisonError::into_inner);
            let (stop, _) = shared
                .woken
                .wait_timeout_while(stop, INTERVAL, |stop| !*stop)
                .unwrap_or_else(PoisonError::into_inner);
            if *stop {
                return;
            }
        }
        if !look(shared, source, &mut times, changed) {
            return;
        }
    }
}

/// What a look copies of each watched path: the path, what was seen of it
/// then, and the time the source gives now.
type Look = (Arc<AssetPath>, Seen, Option<SystemTime>);

/// Looks at every watched path's time of change, and sends each that is not
/// the time seen before, once until it is received; one not looked at yet
/// only has its time taken. Returns false once the watcher is stopping or
/// nothing receives what it sends.
///
/// The paths are copied out first, so that the files are asked about with
/// the map unlocked. A path whose `seen` changed since it was copied, as a
/// read takes its time, or as it is watched anew, is left for the next
/// look, which compares a fresh time with it. `times` is left empty, with
/// its capacity.
fn look(
    shared: &Shared,
    source: &dyn Source,
    times: &mut Vec<Look>,
    changed: &Sender<Arc<AssetPath>>,
) -> bool {
    times.clear();
    times.extend(
        shared
            .paths()
            .values()
            .map(|watched| (Arc::clone(&watched.path), watched.seen, None)),
    );
    for (path, _, time) in times.iter_mut() {
        if shared.stopping.load(Ordering::Relaxed) {
            times.clear();
            return false;
        }
        *time = modified(source, path);
    }
    let mut receiving = true;
    {
        let mut paths = shared.paths();
        for (path, copied, time) in times.iter() {
            // A path no longer watched since it was copied is left alone.
            let Some(watched) = paths.get_mut(path.as_str()) else {
                continue;
            };
            if watched.seen != *copied {
                continue;
            }
            match watched.seen {
                Seen::Unread => watched.seen = Seen::At(*time),
                Seen::At(seen) if seen != *time => {
                    watched.seen = Seen::At(*time);
                    if !watched.queued {
                        watched.queued = true;
                        receiving &= changed.send(Arc::clone(path)).is_ok();
                    }
                }
                Seen::At(_) => {}
            }
        }
    }
    times.clear();
    receiving
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::{Instant, UNIX_EPOCH};

    use super::*;
    use crate::AssetError;

    /// A source whose files are nothing, with times of change the test sets.
    #[derive(Default)]
    struct Times(Mutex<HashMap<String, SystemTime>>);

    impl Times {
        fn set(&self, path: &str, seconds: u64) {
            self.0
                .lock()
                .expect("the times")
                .insert(path.to_string(), UNIX_EPOCH + Duration::from_secs(seconds));
        }
    }

    impl Source for Times {
        fn read(&self, _: &AssetPath) -> Result<Vec<u8>, AssetError> {
            Ok(Vec::new())
        }

        fn modified(&self, path: &AssetPath) -> Option<SystemTime> {
            self.0
                .lock()
                .expect("the times")
                .get(path.as_str())
                .copied()
        }
    }

    fn path(path: &str) -> AssetPath {
        AssetPath::new(path).expect("an asset path")
    }

    /// What the looks sent since the last call, received as the main
    /// thread receives them.
    fn sent(shared: &Shared, changed: &Receiver<Arc<AssetPath>>) -> Vec<String> {
        changed
            .try_iter()
            .map(|path| {
                shared.received(&path);
                path.as_str().to_string()
            })
            .collect()
    }

    #[test]
    fn a_look_sends_each_path_whose_time_changed_once() {
        let (shared, source) = (Shared::new(), Times::default());
        let (sender, changed) = mpsc::channel();
        let mut times = Vec::new();
        source.set("a.txt", 1);
        source.set("b.txt", 1);
        shared.watch(&path("a.txt"));
        shared.watch(&path("b.txt"));
        shared.watch(&path("missing.txt"));
        // The first look only takes the times.
        assert!(look(&shared, &source, &mut times, &sender));
        assert_eq!(sent(&shared, &changed), Vec::<String>::new());
        assert_eq!(shared.seen("missing.txt"), Some(Seen::At(None)));
        source.set("b.txt", 2);
        assert!(look(&shared, &source, &mut times, &sender));
        assert_eq!(sent(&shared, &changed), ["b.txt"]);
        // Seen once, sent once.
        assert!(look(&shared, &source, &mut times, &sender));
        assert_eq!(sent(&shared, &changed), Vec::<String>::new());
        // A file that appears changed too; so does one that goes.
        source.set("missing.txt", 5);
        source.0.lock().expect("the times").remove("a.txt");
        assert!(look(&shared, &source, &mut times, &sender));
        let mut both = sent(&shared, &changed);
        both.sort();
        assert_eq!(both, ["a.txt", "missing.txt"]);
    }

    #[test]
    fn with_nothing_changed_a_look_sends_nothing_and_grows_no_buffer() {
        let (shared, source) = (Shared::new(), Times::default());
        let (sender, changed) = mpsc::channel();
        let mut times = Vec::new();
        for name in ["a.txt", "b.txt", "c.txt"] {
            source.set(name, 1);
            shared.watch(&path(name));
        }
        assert!(look(&shared, &source, &mut times, &sender));
        let capacity = times.capacity();
        assert!(capacity >= 3);
        for _ in 0..100 {
            assert!(look(&shared, &source, &mut times, &sender));
            assert!(times.is_empty());
            assert_eq!(times.capacity(), capacity, "the buffer grew");
        }
        assert_eq!(changed.try_iter().count(), 0);
    }

    #[test]
    fn a_time_taken_before_the_read_is_the_one_compared() {
        let (shared, source) = (Shared::new(), Times::default());
        let (sender, changed) = mpsc::channel();
        let mut times = Vec::new();
        source.set("a.txt", 1);
        shared.watch(&path("a.txt"));
        shared.before_read(&path("a.txt"), &source);
        // The file changes after it was read, before the watcher's first
        // look: the look sees the change.
        source.set("a.txt", 2);
        assert!(look(&shared, &source, &mut times, &sender));
        assert_eq!(sent(&shared, &changed), ["a.txt"]);
        // A later read takes nothing: the path has its time.
        source.set("a.txt", 3);
        shared.before_read(&path("a.txt"), &source);
        assert!(look(&shared, &source, &mut times, &sender));
        assert_eq!(sent(&shared, &changed), ["a.txt"]);
        // Watching it again keeps its time.
        shared.watch(&path("a.txt"));
        assert_eq!(
            shared.seen("a.txt"),
            Some(Seen::At(Some(UNIX_EPOCH + Duration::from_secs(3))))
        );
    }

    #[test]
    fn a_path_no_longer_watched_is_not_looked_at() {
        let (shared, source) = (Shared::new(), Times::default());
        let (sender, changed) = mpsc::channel();
        let mut times = Vec::new();
        source.set("a.txt", 1);
        source.set("b.txt", 1);
        shared.watch(&path("a.txt"));
        shared.watch(&path("b.txt"));
        assert!(look(&shared, &source, &mut times, &sender));
        shared.retain(|path| path != "a.txt");
        source.set("a.txt", 2);
        source.set("b.txt", 2);
        assert!(look(&shared, &source, &mut times, &sender));
        assert_eq!(sent(&shared, &changed), ["b.txt"]);
        assert_eq!(shared.seen("a.txt"), None);
    }

    #[test]
    fn a_source_that_panics_tells_nothing_and_the_watcher_goes_on() {
        struct Panics;
        impl Source for Panics {
            fn read(&self, _: &AssetPath) -> Result<Vec<u8>, AssetError> {
                Ok(Vec::new())
            }
            fn modified(&self, _: &AssetPath) -> Option<SystemTime> {
                panic!("a source with a bug");
            }
        }
        let shared = Shared::new();
        let (sender, changed) = mpsc::channel();
        let mut times = Vec::new();
        shared.watch(&path("a.txt"));
        assert!(look(&shared, &Panics, &mut times, &sender));
        assert!(look(&shared, &Panics, &mut times, &sender));
        assert_eq!(shared.seen("a.txt"), Some(Seen::At(None)));
        assert_eq!(changed.try_iter().count(), 0);
    }

    #[test]
    fn a_look_says_when_nothing_receives() {
        let (shared, source) = (Shared::new(), Times::default());
        let (sender, changed) = mpsc::channel();
        let mut times = Vec::new();
        source.set("a.txt", 1);
        shared.watch(&path("a.txt"));
        assert!(look(&shared, &source, &mut times, &sender));
        drop(changed);
        source.set("a.txt", 2);
        assert!(!look(&shared, &source, &mut times, &sender));
    }

    #[test]
    fn the_thread_hands_over_a_change_within_a_look_and_stops_when_dropped() {
        let source = Arc::new(Times::default());
        source.set("a.txt", 1);
        let mut watch = Watch::start(Arc::clone(&source) as Arc<dyn Source>).expect("a thread");
        watch.shared().watch(&path("a.txt"));
        watch.shared().before_read(&path("a.txt"), source.as_ref());
        source.set("a.txt", 2);
        let started = Instant::now();
        let found = loop {
            if let Some(path) = watch.next_changed() {
                break Some(path);
            }
            if started.elapsed() > Duration::from_secs(10) {
                break None;
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(found.as_deref(), Some(&path("a.txt")));
        assert!(watch.next_changed().is_none());
        // Dropping it ends the thread at once, not after its wait: the
        // look that sent the change was a few milliseconds ago.
        let dropping = Instant::now();
        drop(watch);
        assert!(
            dropping.elapsed() < INTERVAL / 2,
            "the drop waited for the next look"
        );
    }

    #[test]
    fn a_path_that_changes_again_before_it_is_received_is_sent_once() {
        let (shared, source) = (Shared::new(), Times::default());
        let (sender, changed) = mpsc::channel();
        let mut times = Vec::new();
        source.set("a.txt", 1);
        shared.watch(&path("a.txt"));
        assert!(look(&shared, &source, &mut times, &sender));
        for seconds in 2..5 {
            source.set("a.txt", seconds);
            assert!(look(&shared, &source, &mut times, &sender));
        }
        assert_eq!(sent(&shared, &changed), ["a.txt"]);
        // Received, it is sent again at its next change.
        source.set("a.txt", 9);
        assert!(look(&shared, &source, &mut times, &sender));
        assert_eq!(sent(&shared, &changed), ["a.txt"]);
    }

    /// Times of change, set by the test, where a read takes the path's time
    /// while a look is asking about it, as a worker can.
    struct Racing {
        times: Times,
        shared: Arc<Shared>,
        raced: AtomicBool,
    }

    impl Source for Racing {
        fn read(&self, _: &AssetPath) -> Result<Vec<u8>, AssetError> {
            Ok(Vec::new())
        }

        fn modified(&self, path: &AssetPath) -> Option<SystemTime> {
            let time = self.times.modified(path);
            if !self.raced.swap(true, Ordering::Relaxed) {
                // The look's ask: the file changes, and a read takes its
                // new time, before the look compares.
                self.times.set(path.as_str(), 2);
                self.shared.before_read(path, &self.times);
            }
            time
        }
    }

    #[test]
    fn a_time_a_read_takes_during_a_look_is_not_replaced_by_the_looks() {
        let shared = Arc::new(Shared::new());
        let source = Racing {
            times: Times::default(),
            shared: Arc::clone(&shared),
            raced: AtomicBool::new(false),
        };
        source.times.set("a.txt", 1);
        shared.watch(&path("a.txt"));
        let (sender, changed) = mpsc::channel();
        let mut times = Vec::new();
        for _ in 0..3 {
            assert!(look(&shared, &source, &mut times, &sender));
        }
        // The read had the new bytes: there is nothing to reload.
        assert_eq!(sent(&shared, &changed), Vec::<String>::new());
        assert_eq!(
            shared.seen("a.txt"),
            Some(Seen::At(Some(UNIX_EPOCH + Duration::from_secs(2))))
        );
    }

    /// A source whose ask blocks until the test lets it go.
    struct Blocking {
        asked: Mutex<mpsc::Sender<()>>,
        go: Mutex<mpsc::Receiver<()>>,
    }

    impl Source for Blocking {
        fn read(&self, _: &AssetPath) -> Result<Vec<u8>, AssetError> {
            Ok(Vec::new())
        }

        fn modified(&self, _: &AssetPath) -> Option<SystemTime> {
            let _ = self.asked.lock().expect("the sender").send(());
            let _ = self
                .go
                .lock()
                .expect("the receiver")
                .recv_timeout(Duration::from_secs(10));
            Some(UNIX_EPOCH)
        }
    }

    #[test]
    fn a_read_asks_the_source_with_the_paths_unlocked() {
        let shared = Arc::new(Shared::new());
        let (asked, asking) = mpsc::channel();
        let (go, wait) = mpsc::channel();
        let source = Arc::new(Blocking {
            asked: Mutex::new(asked),
            go: Mutex::new(wait),
        });
        shared.watch(&path("a.txt"));
        let reader = {
            let (shared, source) = (Arc::clone(&shared), Arc::clone(&source));
            thread::spawn(move || shared.before_read(&path("a.txt"), source.as_ref()))
        };
        asking
            .recv_timeout(Duration::from_secs(10))
            .expect("the read asks");
        // While the read waits for the source, a load watches another path.
        let (done, watched) = mpsc::channel();
        {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                shared.watch(&path("b.txt"));
                let _ = done.send(());
            });
        }
        let waited = watched.recv_timeout(Duration::from_secs(5));
        go.send(()).expect("the read waits");
        reader.join().expect("the read ends");
        assert!(waited.is_ok(), "watching a path waited for the source");
        assert_eq!(shared.seen("a.txt"), Some(Seen::At(Some(UNIX_EPOCH))));
    }

    #[test]
    fn a_source_that_panics_before_a_read_tells_nothing() {
        struct Panics;
        impl Source for Panics {
            fn read(&self, _: &AssetPath) -> Result<Vec<u8>, AssetError> {
                Ok(Vec::new())
            }
            fn modified(&self, _: &AssetPath) -> Option<SystemTime> {
                panic!("a source with a bug");
            }
        }
        let shared = Shared::new();
        shared.watch(&path("a.txt"));
        shared.before_read(&path("a.txt"), &Panics);
        assert_eq!(shared.seen("a.txt"), Some(Seen::At(None)));
    }

    #[test]
    fn stopping_ends_a_look_before_its_next_ask() {
        let (shared, source) = (Shared::new(), Times::default());
        let (sender, changed) = mpsc::channel();
        let mut times = Vec::new();
        for name in ["a.txt", "b.txt"] {
            source.set(name, 1);
            shared.watch(&path(name));
        }
        shared.stopping.store(true, Ordering::Relaxed);
        assert!(!look(&shared, &source, &mut times, &sender));
        assert!(times.is_empty());
        assert_eq!(
            shared.seen("a.txt"),
            Some(Seen::Unread),
            "nothing was taken"
        );
        assert_eq!(changed.try_iter().count(), 0);
    }
}
