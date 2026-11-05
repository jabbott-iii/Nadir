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

//! Reloads behind a handle, and the watcher, on real files (M5.16).

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nadir_assets::{
    Asset, AssetError, AssetPath, AssetServer, DecodeCtx, FinalizeCtx, Handle, LoadState,
    OnFailure, Scope, Source,
};
use nadir_core::log;

/// Text, uppercased when it is made, with `?` as its placeholder.
#[derive(Debug, PartialEq)]
struct Text(String);

/// What the asset types did: a resource the context must hold.
#[derive(Debug, Default, PartialEq)]
struct Counts {
    /// What was made, in order.
    made: Vec<String>,
    /// How many values and placeholders were released.
    released: u32,
}

impl Asset for Text {
    type Decoded = String;
    const EXTENSIONS: &'static [&'static str] = &["txt"];

    fn decode(bytes: &[u8], _: &DecodeCtx<'_>) -> Result<String, AssetError> {
        String::from_utf8(bytes.to_vec()).map_err(|_| AssetError::Invalid("not UTF-8".to_string()))
    }

    fn finalize(decoded: String, ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        ctx.get_mut::<Counts>()?.made.push(decoded.clone());
        Ok(Self(decoded.to_uppercase()))
    }

    fn placeholder(_: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        Ok(Self("?".to_string()))
    }

    fn release(self, ctx: &mut FinalizeCtx<'_>) -> Result<(), AssetError> {
        ctx.get_mut::<Counts>()?.released += 1;
        Ok(())
    }
}

/// The bytes as they are, of the same files.
#[derive(Debug, PartialEq)]
struct Raw(Vec<u8>);

impl Asset for Raw {
    type Decoded = Vec<u8>;
    const EXTENSIONS: &'static [&'static str] = &["txt"];

    fn decode(bytes: &[u8], _: &DecodeCtx<'_>) -> Result<Vec<u8>, AssetError> {
        Ok(bytes.to_vec())
    }

    fn finalize(decoded: Vec<u8>, _: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        Ok(Self(decoded))
    }
}

/// A fresh asset root for one test, under `target/tmp`, with
/// `notes/hello.txt`.
fn root(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("nadir-assets-reload")
        .join(test);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("remove the previous run's directory");
    }
    fs::create_dir_all(dir.join("notes")).expect("create the test directory");
    fs::write(dir.join("notes/hello.txt"), "hello").expect("write a file");
    dir
}

/// Writes `bytes` to `file`, with a time of change of its own, so that a
/// system that keeps coarse times sees the change too. The bytes go into a
/// file beside it first, renamed over it once its time is set, as the cook
/// writes, so that the watcher never sees the new bytes with the time the
/// write gave them.
fn rewrite(file: &Path, bytes: &[u8], seconds: u64) {
    let beside = file.with_extension("tmp");
    fs::write(&beside, bytes).expect("write the new file");
    File::options()
        .write(true)
        .open(&beside)
        .and_then(|beside| beside.set_modified(time(seconds)))
        .expect("set the file's time of change");
    fs::rename(&beside, file).expect("rename it over the file");
}

fn time(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_000_000 + seconds)
}

/// Waits for every load and reload, and finishes them.
fn finish(server: &mut AssetServer) -> Counts {
    let mut counts = Counts::default();
    server.finish_all_loads(&mut FinalizeCtx::new().with(&mut counts));
    counts
}

fn text(server: &AssetServer, handle: Handle<Text>) -> Option<&str> {
    server.get(handle).map(|text| text.0.as_str())
}

fn logged(line: &str) -> bool {
    log::tail(log::RING_CAPACITY)
        .iter()
        .any(|logged| logged.contains(line))
}

/// How long the watcher took to hand over a change: polls
/// `reload_changed` until it reloads something, for up to 10 s.
fn wait_for_a_change(server: &mut AssetServer) -> Option<Duration> {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        if server.reload_changed() > 0 {
            return Some(started.elapsed());
        }
        thread::sleep(Duration::from_millis(5));
    }
    None
}

#[test]
fn a_reloaded_asset_has_its_new_value_behind_its_handle_and_the_old_one_is_released_once() {
    let dir = root("new_value");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    let hello = server.load::<Text>("notes/hello.txt");
    finish(&mut server);
    fs::write(dir.join("notes/hello.txt"), "hello again").expect("rewrite the file");
    assert_eq!(server.reload("Notes/Hello.TXT"), 1);
    // Until a finish makes it, the asset is as it was.
    assert!(server.has_pending());
    assert_eq!(server.state(hello), Some(LoadState::Loaded));
    assert_eq!(text(&server, hello), Some("HELLO"));
    let counts = finish(&mut server);
    assert_eq!(counts.made, ["hello again"]);
    assert_eq!(counts.released, 1, "the value it had");
    assert_eq!(text(&server, hello), Some("HELLO AGAIN"));
    assert_eq!(server.state(hello), Some(LoadState::Loaded));
    assert_eq!(server.load::<Text>("notes/hello.txt"), hello);
    assert_eq!(server.drain_swapped::<Text>().collect::<Vec<_>>(), [hello]);
    assert_eq!(server.drain_swapped::<Text>().count(), 0);
    assert!(!server.has_pending());
    assert_eq!(finish(&mut server), Counts::default(), "released once");
    assert!(logged("asset `notes/hello.txt`: reloaded"));
}

#[test]
fn a_reload_that_fails_keeps_the_value_it_had() {
    let dir = root("fails");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    let hello = server.load::<Text>("notes/hello.txt");
    finish(&mut server);
    // Bytes the type refuses.
    fs::write(dir.join("notes/hello.txt"), [0xff, 0xfe]).expect("rewrite the file");
    assert_eq!(server.reload("notes/hello.txt"), 1);
    assert_eq!(finish(&mut server), Counts::default());
    assert_eq!(text(&server, hello), Some("HELLO"));
    assert_eq!(server.state(hello), Some(LoadState::Loaded));
    assert_eq!(server.error(hello), None);
    assert!(logged(
        "asset `notes/hello.txt`: its reload failed, so it keeps the value it had: not UTF-8"
    ));
    // A file that is gone.
    fs::remove_file(dir.join("notes/hello.txt")).expect("remove the file");
    assert_eq!(server.reload("notes/hello.txt"), 1);
    assert_eq!(finish(&mut server), Counts::default());
    assert_eq!(text(&server, hello), Some("HELLO"));
    assert_eq!(server.drain_swapped::<Text>().count(), 0);
    assert!(!server.has_pending());
}

#[test]
fn a_failed_asset_reloaded_once_its_file_is_there_is_loaded() {
    let dir = root("fixed");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    let missing = server.load::<Text>("notes/missing.txt");
    finish(&mut server);
    assert_eq!(server.state(missing), Some(LoadState::Failed));
    assert_eq!(text(&server, missing), Some("?"));
    fs::write(dir.join("notes/missing.txt"), "found").expect("write the file");
    assert_eq!(server.reload("notes/missing.txt"), 1);
    let counts = finish(&mut server);
    assert_eq!(counts.made, ["found"]);
    assert_eq!(counts.released, 1, "the placeholder");
    assert_eq!(server.state(missing), Some(LoadState::Loaded));
    assert_eq!(server.error(missing), None);
    assert_eq!(text(&server, missing), Some("FOUND"));
    assert_eq!(
        server.drain_swapped::<Text>().collect::<Vec<_>>(),
        [missing]
    );
}

#[test]
fn a_reload_reaches_every_type_loaded_at_the_path() {
    let dir = root("types");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    let text_handle = server.load::<Text>("notes/hello.txt");
    let raw = server.load::<Raw>("notes/hello.txt");
    finish(&mut server);
    fs::write(dir.join("notes/hello.txt"), "bye").expect("rewrite the file");
    assert_eq!(server.reload("notes/hello.txt"), 2);
    finish(&mut server);
    assert_eq!(text(&server, text_handle), Some("BYE"));
    assert_eq!(server.get(raw), Some(&Raw(b"bye".to_vec())));
    assert_eq!(server.drain_swapped::<Raw>().collect::<Vec<_>>(), [raw]);
    assert_eq!(
        server.drain_swapped::<Text>().collect::<Vec<_>>(),
        [text_handle]
    );
}

#[test]
fn a_reload_of_a_path_no_asset_is_at_does_nothing() {
    let dir = root("nothing");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    assert_eq!(server.reload("notes/hello.txt"), 0, "nothing loaded yet");
    server.load::<Text>("notes/hello.txt");
    // Paths that fail their type's check have no file to read again.
    server.load::<Text>("notes/hello.text");
    server.load::<Text>("../hello.txt");
    finish(&mut server);
    assert_eq!(server.reload("notes/other.txt"), 0);
    assert_eq!(server.reload("notes/hello.text"), 0);
    assert_eq!(server.reload("../hello.txt"), 0);
    assert!(!server.has_pending());
    assert_eq!(
        server.drain_swapped::<Raw>().count(),
        0,
        "a type never loaded"
    );
}

#[test]
fn an_asset_unloaded_while_it_reloads_is_never_made() {
    let dir = root("unloaded");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    let hello = server.load_in::<Text>("notes/hello.txt", Scope::Level);
    finish(&mut server);
    fs::write(dir.join("notes/hello.txt"), "hello again").expect("rewrite the file");
    assert_eq!(server.reload("notes/hello.txt"), 1);
    assert_eq!(server.unload_scope(Scope::Level), 1);
    let counts = finish(&mut server);
    assert_eq!(counts.made, Vec::<String>::new());
    assert_eq!(counts.released, 1, "the value it had, by the unload");
    assert_eq!(server.state(hello), None);
    assert_eq!(server.drain_swapped::<Text>().count(), 0);
    assert!(!server.has_pending());
}

#[test]
fn a_frame_finish_makes_a_reload_and_has_pending_says_so_until_then() {
    let dir = root("frames");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    let hello = server.load::<Text>("notes/hello.txt");
    finish(&mut server);
    fs::write(dir.join("notes/hello.txt"), "hi").expect("rewrite the file");
    server.reload("notes/hello.txt");
    let started = Instant::now();
    while text(&server, hello) != Some("HI") && started.elapsed() < Duration::from_secs(10) {
        assert!(server.has_pending());
        let mut counts = Counts::default();
        server.finish_completed_loads(&mut FinalizeCtx::new().with(&mut counts));
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(text(&server, hello), Some("HI"));
    assert!(!server.has_pending());
}

#[test]
fn a_loose_file_changed_while_watched_is_reloaded_within_a_look_and_a_finish() {
    let dir = root("watched");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    server.start_watcher().expect("the watcher starts");
    server
        .start_watcher()
        .expect("starting it again does nothing");
    let hello = server.load::<Text>("notes/hello.txt");
    finish(&mut server);
    rewrite(&dir.join("notes/hello.txt"), b"changed", 1);
    let took = wait_for_a_change(&mut server).expect("the watcher saw the change");
    let counts = finish(&mut server);
    assert_eq!(counts.made, ["changed"]);
    assert_eq!(counts.released, 1);
    assert_eq!(text(&server, hello), Some("CHANGED"));
    // A look every 250 ms: the change comes with the first look after it.
    assert!(took < Duration::from_secs(1), "the change took {took:?}");
    // Seen once, reloaded once.
    thread::sleep(Duration::from_millis(600));
    assert_eq!(server.reload_changed(), 0);
}

#[test]
fn with_nothing_changed_the_watcher_hands_over_nothing() {
    let dir = root("unchanged");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    server.start_watcher().expect("the watcher starts");
    server.load::<Text>("notes/hello.txt");
    finish(&mut server);
    for _ in 0..4 {
        thread::sleep(Duration::from_millis(200));
        assert_eq!(server.reload_changed(), 0);
        assert!(!server.has_pending());
    }
    assert_eq!(server.drain_swapped::<Text>().count(), 0);
}

#[test]
fn a_path_loaded_before_the_watcher_started_is_watched_from_its_first_look() {
    let dir = root("before");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    let hello = server.load::<Text>("notes/hello.txt");
    finish(&mut server);
    server.start_watcher().expect("the watcher starts");
    // Two looks: the first takes the time.
    thread::sleep(Duration::from_millis(600));
    assert_eq!(server.reload_changed(), 0);
    rewrite(&dir.join("notes/hello.txt"), b"later", 2);
    wait_for_a_change(&mut server).expect("the watcher saw the change");
    finish(&mut server);
    assert_eq!(text(&server, hello), Some("LATER"));
}

#[test]
fn a_file_that_appears_while_watched_loads_its_failed_asset() {
    let dir = root("appears");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    server.start_watcher().expect("the watcher starts");
    let missing = server.load::<Text>("notes/missing.txt");
    finish(&mut server);
    assert_eq!(server.state(missing), Some(LoadState::Failed));
    rewrite(&dir.join("notes/missing.txt"), b"here", 3);
    wait_for_a_change(&mut server).expect("the watcher saw the file");
    finish(&mut server);
    assert_eq!(server.state(missing), Some(LoadState::Loaded));
    assert_eq!(text(&server, missing), Some("HERE"));
}

#[test]
fn a_path_no_asset_is_at_any_more_is_not_watched() {
    let dir = root("unwatched");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    server.start_watcher().expect("the watcher starts");
    server.load_in::<Text>("notes/hello.txt", Scope::Level);
    server.load_in::<Raw>("notes/hello.txt", Scope::Transient);
    finish(&mut server);
    assert_eq!(server.unload_scope(Scope::Level), 1);
    // A `Raw` is still there.
    rewrite(&dir.join("notes/hello.txt"), b"one", 4);
    wait_for_a_change(&mut server).expect("the watcher saw the change");
    finish(&mut server);
    assert_eq!(server.unload_scope(Scope::Transient), 1);
    // A change made while nothing is at the path is not handed over once
    // the path is loaded again: the new load reads the new bytes, and is
    // watched from that read.
    rewrite(&dir.join("notes/hello.txt"), b"two", 5);
    thread::sleep(Duration::from_millis(600));
    let again = server.load::<Text>("notes/hello.txt");
    finish(&mut server);
    assert_eq!(text(&server, again), Some("TWO"));
    thread::sleep(Duration::from_millis(600));
    assert_eq!(server.reload_changed(), 0, "a change from before the load");
}

#[test]
fn a_reload_whose_value_cannot_be_made_keeps_the_value_it_had() {
    let dir = root("make_fails");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    let hello = server.load::<Text>("notes/hello.txt");
    finish(&mut server);
    fs::write(dir.join("notes/hello.txt"), "hello again").expect("rewrite the file");
    assert_eq!(server.reload("notes/hello.txt"), 1);
    // A context without the `Counts` that making a `Text` needs.
    server.finish_all_loads(&mut FinalizeCtx::new());
    assert_eq!(text(&server, hello), Some("HELLO"));
    assert_eq!(server.state(hello), Some(LoadState::Loaded));
    assert_eq!(server.error(hello), None);
    assert_eq!(server.drain_swapped::<Text>().count(), 0);
    assert!(logged(
        "asset `notes/hello.txt`: its reload failed, so it keeps the value it had: making it needs"
    ));
}

#[test]
fn a_failed_asset_with_no_placeholder_reloaded_once_its_file_is_there_is_loaded() {
    let dir = root("fixed_nothing");
    let mut server = AssetServer::new(&dir, OnFailure::Nothing);
    let missing = server.load::<Text>("notes/missing.txt");
    finish(&mut server);
    assert_eq!(server.state(missing), Some(LoadState::Failed));
    assert_eq!(text(&server, missing), None);
    fs::write(dir.join("notes/missing.txt"), "found").expect("write the file");
    assert_eq!(server.reload("notes/missing.txt"), 1);
    let counts = finish(&mut server);
    assert_eq!(counts.made, ["found"]);
    assert_eq!(counts.released, 0, "it had no value to release");
    assert_eq!(server.state(missing), Some(LoadState::Loaded));
    assert_eq!(text(&server, missing), Some("FOUND"));
    assert_eq!(
        server.drain_swapped::<Text>().collect::<Vec<_>>(),
        [missing]
    );
}

#[test]
fn an_asset_swapped_then_unloaded_is_not_drained() {
    let dir = root("swapped_unloaded");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    server.load_in::<Text>("notes/hello.txt", Scope::Level);
    finish(&mut server);
    fs::write(dir.join("notes/hello.txt"), "again").expect("rewrite the file");
    assert_eq!(server.reload("notes/hello.txt"), 1);
    assert_eq!(finish(&mut server).made, ["again"]);
    assert_eq!(server.unload_scope(Scope::Level), 1);
    assert_eq!(server.drain_swapped::<Text>().count(), 0);
}

#[test]
fn every_path_that_changed_is_reloaded_by_one_call() {
    let dir = root("two_paths");
    fs::write(dir.join("notes/other.txt"), "other").expect("write a file");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    server.start_watcher().expect("the watcher starts");
    let hello = server.load::<Text>("notes/hello.txt");
    let other = server.load::<Text>("notes/other.txt");
    finish(&mut server);
    rewrite(&dir.join("notes/hello.txt"), b"one", 7);
    rewrite(&dir.join("notes/other.txt"), b"two", 8);
    thread::sleep(Duration::from_millis(600));
    assert_eq!(server.reload_changed(), 2);
    finish(&mut server);
    assert_eq!(text(&server, hello), Some("ONE"));
    assert_eq!(text(&server, other), Some("TWO"));
}

/// One file in memory, `notes/hello.txt`, whose first read is followed at
/// once by a save: new bytes and a new time of change, as an editor could
/// write them just after the game read the file.
struct SavedAfterTheFirstRead(Mutex<(Vec<u8>, u64, bool)>);

impl Source for SavedAfterTheFirstRead {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
        if path.as_str() != "notes/hello.txt" {
            return Err(AssetError::NotFound);
        }
        let mut file = self.0.lock().expect("the file");
        let bytes = file.0.clone();
        if !file.2 {
            *file = (b"saved".to_vec(), file.1 + 1, true);
        }
        Ok(bytes)
    }

    fn modified(&self, path: &AssetPath) -> Option<SystemTime> {
        (path.as_str() == "notes/hello.txt").then(|| time(self.0.lock().expect("the file").1))
    }
}

#[test]
fn a_change_made_just_after_the_first_read_is_seen() {
    let source = SavedAfterTheFirstRead(Mutex::new((b"hello".to_vec(), 1, false)));
    let mut server = AssetServer::with_source(source, OnFailure::Placeholder);
    server.start_watcher().expect("the watcher starts");
    let hello = server.load::<Text>("notes/hello.txt");
    finish(&mut server);
    assert_eq!(text(&server, hello), Some("HELLO"));
    wait_for_a_change(&mut server).expect("the watcher saw the save");
    finish(&mut server);
    assert_eq!(text(&server, hello), Some("SAVED"));
}

#[test]
fn a_path_made_with_load_now_is_watched_too() {
    let dir = root("load_now");
    let mut server = AssetServer::new(&dir, OnFailure::Placeholder);
    server.start_watcher().expect("the watcher starts");
    let mut counts = Counts::default();
    let hello = server
        .load_now::<Text>("notes/hello.txt", &mut FinalizeCtx::new().with(&mut counts))
        .expect("the file is there");
    rewrite(&dir.join("notes/hello.txt"), b"now", 6);
    wait_for_a_change(&mut server).expect("the watcher saw the change");
    finish(&mut server);
    assert_eq!(text(&server, hello), Some("NOW"));
}
