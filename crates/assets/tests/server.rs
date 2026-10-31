//! The asset server on real files, with made-up asset types.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use nadir_assets::{
    Asset, AssetError, AssetPath, AssetServer, DecodeCtx, FinalizeCtx, Handle, LoadState,
    OnFailure, Scope, Source,
};
use nadir_core::log;

/// Text, uppercased when it is finalized, with `?` as its placeholder.
#[derive(Debug, PartialEq)]
struct Text(String);

/// What the asset types did: a resource the context must hold.
#[derive(Debug, Default, PartialEq)]
struct Counts {
    /// The paths finalized, in order.
    finalized: Vec<String>,
    /// How many assets and placeholders were released.
    released: u32,
}

impl Asset for Text {
    type Decoded = String;
    const EXTENSIONS: &'static [&'static str] = &["txt", "text"];

    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<String, AssetError> {
        assert!(ctx.path().ends_with(".txt") || ctx.path().ends_with(".text"));
        String::from_utf8(bytes.to_vec()).map_err(|_| AssetError::Invalid("not UTF-8".to_string()))
    }

    fn finalize(decoded: String, ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        ctx.get_mut::<Counts>()?.finalized.push(decoded.clone());
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

/// Bytes as they are, with no placeholder, released by being dropped.
#[derive(Debug, PartialEq)]
struct Raw(Vec<u8>);

impl Asset for Raw {
    type Decoded = Vec<u8>;
    const EXTENSIONS: &'static [&'static str] = &["txt"];

    fn decode(bytes: &[u8], _: &DecodeCtx<'_>) -> Result<Vec<u8>, AssetError> {
        if bytes == b"panic" {
            panic!("a decoder with a bug");
        }
        Ok(bytes.to_vec())
    }

    fn finalize(decoded: Vec<u8>, _: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        Ok(Self(decoded))
    }
}

/// A fresh, empty asset root for one test, under `target/tmp`.
fn root(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("nadir-assets-server")
        .join(test);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("remove the previous run's directory");
    }
    fs::create_dir_all(dir.join("notes")).expect("create the test directory");
    fs::write(dir.join("notes/hello.txt"), "hello").expect("write a file");
    fs::write(dir.join("notes/other.txt"), "other").expect("write a file");
    fs::write(dir.join("notes/bad.txt"), [0xff, 0xfe]).expect("write a file");
    dir
}

/// Waits for every load and finishes them, with counters in the context.
fn finish(server: &mut AssetServer) -> Counts {
    let mut counts = Counts::default();
    server.finish_all_loads(&mut FinalizeCtx::new().with(&mut counts));
    counts
}

/// Finishes the loads the workers are done with, as a frame does.
fn finish_completed(server: &mut AssetServer) -> Counts {
    let mut counts = Counts::default();
    server.finish_completed_loads(&mut FinalizeCtx::new().with(&mut counts));
    counts
}

fn text(server: &AssetServer, handle: Handle<Text>) -> Option<&str> {
    server.get(handle).map(|text| text.0.as_str())
}

/// Files in memory that are read only once the test opens the gate, or
/// after 10 s, so that a failing test still ends.
struct Gated {
    files: HashMap<&'static str, &'static str>,
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl Gated {
    fn new(files: &[(&'static str, &'static str)]) -> (Self, Arc<(Mutex<bool>, Condvar)>) {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let source = Self {
            files: files.iter().copied().collect(),
            gate: Arc::clone(&gate),
        };
        (source, gate)
    }
}

fn open(gate: &(Mutex<bool>, Condvar)) {
    *gate.0.lock().expect("the gate") = true;
    gate.1.notify_all();
}

impl Source for Gated {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
        let (open, opened) = &*self.gate;
        let guard = open.lock().expect("the gate");
        let _guard = opened
            .wait_timeout_while(guard, Duration::from_secs(10), |open| !*open)
            .expect("the gate");
        let file = self.files.get(path.as_str()).ok_or(AssetError::NotFound)?;
        Ok(file.as_bytes().to_vec())
    }
}

#[test]
fn loading_the_same_path_twice_gives_the_same_handle() {
    let mut server = AssetServer::new(root("same_path"), OnFailure::Placeholder);
    let hello = server.load::<Text>("notes/hello.txt");
    assert_eq!(
        server.load::<Text>("notes/hello.txt"),
        hello,
        "while loading"
    );
    let other = server.load::<Text>("notes/other.txt");
    assert_ne!(other, hello);
    // Paths are lowercased.
    assert_eq!(server.load::<Text>("Notes/HELLO.TXT"), hello);
    assert_eq!(finish(&mut server).finalized.len(), 2);
    assert_eq!(server.load::<Text>("notes/hello.txt"), hello, "once loaded");
    let missing = server.load::<Text>("notes/missing.txt");
    finish(&mut server);
    assert_eq!(
        server.load::<Text>("notes/missing.txt"),
        missing,
        "once failed"
    );
    assert_eq!(
        finish(&mut server),
        Counts::default(),
        "nothing is loaded again"
    );

    let assets = server.assets::<Text>().expect("texts were loaded");
    assert_eq!(assets.len(), 3);
    assert_eq!(assets.handle("notes/other.txt"), Some(other));
    assert_eq!(assets.handle("Notes/Other.TXT"), Some(other));
    assert_eq!(assets.handle("notes/unknown.txt"), None);
    assert_eq!(assets.path(hello), Some("notes/hello.txt"));
    assert_eq!(server.path(missing), Some("notes/missing.txt"));
}

#[test]
fn an_asset_is_made_once_its_load_is_finished() {
    let mut server = AssetServer::new(root("finish"), OnFailure::Placeholder);
    assert!(!server.has_pending());
    let hello = server.load::<Text>("notes/hello.txt");
    assert_eq!(server.state(hello), Some(LoadState::Loading));
    assert_eq!(text(&server, hello), None);
    assert!(server.has_pending());

    assert_eq!(finish(&mut server).finalized, ["hello"]);
    assert_eq!(server.state(hello), Some(LoadState::Loaded));
    assert_eq!(text(&server, hello), Some("HELLO"));
    assert_eq!(server.error(hello), None);
    assert!(!server.has_pending());
    assert_eq!(finish(&mut server), Counts::default());

    // A load after a finish waits for the next one.
    let other = server.load::<Text>("notes/other.txt");
    assert_eq!(server.state(other), Some(LoadState::Loading));
    assert_eq!(finish(&mut server).finalized, ["other"]);
    assert_eq!(text(&server, other), Some("OTHER"));
}

#[test]
fn loading_never_waits_for_the_file() {
    let (source, gate) = Gated::new(&[("notes/hello.txt", "hello")]);
    let mut server = AssetServer::with_source(source, OnFailure::Placeholder);
    // The source holds every read until the gate opens, so a load that read
    // the file would never return.
    let hello = server.load::<Text>("notes/hello.txt");
    let missing = server.load::<Text>("notes/missing.txt");
    // A frame's finish does not wait either.
    for _ in 0..3 {
        assert_eq!(finish_completed(&mut server), Counts::default());
    }
    assert_eq!(server.state(hello), Some(LoadState::Loading));
    assert_eq!(server.state(missing), Some(LoadState::Loading));
    assert!(server.has_pending());

    open(&gate);
    // Each frame finishes what the workers are done with.
    let mut finalized = Vec::new();
    for _ in 0..1000 {
        finalized.extend(finish_completed(&mut server).finalized);
        if !server.has_pending() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(finalized, ["hello"]);
    assert_eq!(text(&server, hello), Some("HELLO"));
    assert_eq!(server.error(missing), Some(&AssetError::NotFound));
}

#[test]
fn assets_are_made_in_the_order_they_were_loaded() {
    let dir = root("order");
    let names: Vec<String> = (0..40).map(|i| format!("{i:02}")).collect();
    for name in &names {
        fs::write(dir.join(format!("notes/{name}.txt")), name).expect("write a file");
    }
    let mut server = AssetServer::new(dir, OnFailure::Placeholder);
    // Backwards, so that the order is the loads', not the names'.
    for name in names.iter().rev() {
        server.load::<Text>(&format!("notes/{name}.txt"));
    }
    let mut expected = names.clone();
    expected.reverse();
    assert_eq!(finish(&mut server).finalized, expected);
}

#[test]
fn each_finish_keeps_to_its_budget_but_makes_one_asset_at_least() {
    let mut server = AssetServer::new(root("budget"), OnFailure::Placeholder);
    let failed = |server: &AssetServer, handles: &[Handle<Text>]| {
        let failed = handles
            .iter()
            .filter(|&&handle| server.state(handle) == Some(LoadState::Failed));
        failed.count()
    };
    // Paths of the wrong type fail at once, with no worker, so all of them
    // are ready for the first call.
    let wrong: Vec<Handle<Text>> = (0..5).map(|i| server.load(&format!("{i}.qoi"))).collect();
    server.set_finish_budget(Duration::ZERO);
    for call in 1..=5 {
        finish_completed(&mut server);
        assert_eq!(failed(&server, &wrong), call, "a call of no budget");
    }
    let more: Vec<Handle<Text>> = (5..10).map(|i| server.load(&format!("{i}.qoi"))).collect();
    server.set_finish_budget(Duration::from_secs(60));
    finish_completed(&mut server);
    assert_eq!(failed(&server, &more), 5, "a call with time to spare");

    // Files the workers decode keep to it too.
    server.set_finish_budget(Duration::ZERO);
    let read: Vec<Handle<Text>> = ["notes/hello.txt", "notes/other.txt"]
        .into_iter()
        .map(|path| server.load(path))
        .collect();
    while server.has_pending() {
        let made = finish_completed(&mut server).finalized.len();
        assert!(made <= 1, "a call of no budget made {made} assets");
        thread::sleep(Duration::from_millis(1));
    }
    assert!(
        read.iter()
            .all(|&handle| server.state(handle) == Some(LoadState::Loaded))
    );
}

#[test]
fn the_types_take_turns_at_going_first() {
    let mut server = AssetServer::new(root("turns"), OnFailure::Placeholder);
    server.set_finish_budget(Duration::ZERO);
    // Paths of the wrong type fail with no worker, so all are ready at
    // once: a backlog of raw bytes, loaded first, and one text after it.
    let raws: Vec<Handle<Raw>> = (0..5).map(|i| server.load(&format!("{i}.qoi"))).collect();
    let text = server.load::<Text>("x.qoi");
    finish_completed(&mut server);
    finish_completed(&mut server);
    // A call of no budget makes one asset: the raw types' turn, then the
    // text's.
    assert_eq!(server.state(text), Some(LoadState::Failed));
    let made = |server: &AssetServer| {
        raws.iter()
            .filter(|&&handle| server.state(handle) == Some(LoadState::Failed))
            .count()
    };
    assert_eq!(made(&server), 1);
    // In a type, the order is the loads'.
    assert_eq!(server.state(raws[0]), Some(LoadState::Failed));
    for _ in 0..4 {
        finish_completed(&mut server);
    }
    assert_eq!(made(&server), 5);
}

#[test]
fn a_missing_file_shows_the_placeholder() {
    let mut server = AssetServer::new(root("missing"), OnFailure::Placeholder);
    let missing = server.load::<Text>("notes/missing-placeholder.txt");
    assert_eq!(server.state(missing), Some(LoadState::Loading));
    finish(&mut server);
    assert_eq!(server.state(missing), Some(LoadState::Failed));
    assert_eq!(server.error(missing), Some(&AssetError::NotFound));
    assert_eq!(text(&server, missing), Some("?"));
    // The failure is logged with the path.
    assert!(
        log::tail(log::RING_CAPACITY).iter().any(|line| {
            line.contains("notes/missing-placeholder.txt") && line.contains("no such file")
        }),
        "no log line names the file"
    );
}

#[test]
fn without_placeholders_a_failed_asset_is_nothing() {
    let mut server = AssetServer::new(root("nothing"), OnFailure::Nothing);
    let missing = server.load::<Text>("notes/missing.txt");
    finish(&mut server);
    assert_eq!(server.state(missing), Some(LoadState::Failed));
    assert_eq!(text(&server, missing), None);
}

#[test]
fn a_type_with_no_placeholder_has_nothing_to_show() {
    let mut server = AssetServer::new(root("no_placeholder"), OnFailure::Placeholder);
    let missing = server.load::<Raw>("notes/missing.txt");
    finish(&mut server);
    assert_eq!(server.error(missing), Some(&AssetError::NotFound));
    assert_eq!(server.get(missing), None);
}

#[test]
fn each_failure_says_why() {
    let mut server = AssetServer::new(root("failures"), OnFailure::Nothing);
    let cases = [
        (
            "notes/bad.txt",
            AssetError::Invalid("not UTF-8".to_string()),
        ),
        (
            "notes/hello.qoi",
            AssetError::Extension {
                expected: Text::EXTENSIONS,
            },
        ),
        (
            "notes/hello",
            AssetError::Extension {
                expected: Text::EXTENSIONS,
            },
        ),
        (
            "notes",
            AssetError::Extension {
                expected: Text::EXTENSIONS,
            },
        ),
        (
            "../hello.txt",
            AssetError::BadPath("it has a `.` or `..` part"),
        ),
        (
            "/notes/hello.txt",
            AssetError::BadPath("it is absolute, or has an empty part"),
        ),
        ("C:notes/hello.txt", AssetError::BadPath("it has a `:`")),
    ];
    let handles: Vec<_> = cases
        .iter()
        .map(|(path, _)| server.load::<Text>(path))
        .collect();
    finish(&mut server);
    for ((path, error), handle) in cases.iter().zip(handles) {
        assert_eq!(server.error(handle), Some(error), "{path}");
    }
    // A directory is no file.
    let dir = root("failures_dir");
    fs::create_dir(dir.join("dir.txt")).expect("make a directory");
    let mut server = AssetServer::new(dir, OnFailure::Nothing);
    let directory = server.load::<Text>("dir.txt");
    finish(&mut server);
    assert!(
        matches!(server.error(directory), Some(AssetError::Read(_))),
        "{:?}",
        server.error(directory)
    );
}

#[test]
fn a_decoder_that_panics_fails_only_its_asset() {
    let dir = root("panic");
    fs::write(dir.join("notes/panic.txt"), "panic").expect("write a file");
    let mut server = AssetServer::new(dir, OnFailure::Placeholder);
    let broken = server.load::<Raw>("notes/panic.txt");
    let hello = server.load::<Raw>("notes/hello.txt");
    // Frames, not `finish_all_loads`, so that a panic that went uncaught,
    // and so a result that never came, fails the test instead of hanging
    // it.
    for _ in 0..10_000 {
        finish_completed(&mut server);
        if !server.has_pending() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(!server.has_pending(), "a load never finished");
    assert_eq!(
        server.error(broken),
        Some(&AssetError::Invalid(
            "reading or decoding it panicked".to_string()
        ))
    );
    assert_eq!(server.get(hello), Some(&Raw(b"hello".to_vec())));
    // The workers go on.
    let other = server.load::<Raw>("notes/other.txt");
    finish(&mut server);
    assert_eq!(server.get(other), Some(&Raw(b"other".to_vec())));
}

#[test]
fn a_resource_missing_from_the_context_fails_the_asset() {
    let mut server = AssetServer::new(root("no_context"), OnFailure::Placeholder);
    let hello = server.load::<Text>("notes/hello.txt");
    server.finish_all_loads(&mut FinalizeCtx::new());
    let Some(AssetError::MissingContext(resource)) = server.error(hello) else {
        panic!("{:?}", server.error(hello));
    };
    assert!(resource.ends_with("Counts"), "{resource}");
    // The placeholder needs no resource, so it shows.
    assert_eq!(text(&server, hello), Some("?"));
}

#[test]
fn types_keep_their_assets_apart() {
    let mut server = AssetServer::new(root("types"), OnFailure::Placeholder);
    let text_handle = server.load::<Text>("notes/hello.txt");
    let raw = server.load::<Raw>("notes/hello.txt");
    assert!(server.assets::<Raw>().is_some());
    finish(&mut server);
    assert_eq!(text(&server, text_handle), Some("HELLO"));
    assert_eq!(server.get(raw), Some(&Raw(b"hello".to_vec())));
    assert_eq!(server.assets::<Text>().map(|assets| assets.len()), Some(1));

    // A server that holds nothing under a handle gives nothing for it.
    let other = AssetServer::new(root("types_other"), OnFailure::Placeholder);
    assert_eq!(other.state(text_handle), None);
    assert_eq!(other.get(text_handle), None);
}

#[test]
fn unloading_a_scope_frees_the_assets_no_other_scope_holds() {
    let mut server = AssetServer::new(root("scopes"), OnFailure::Placeholder);
    let level = server.load_in::<Text>("notes/hello.txt", Scope::Level);
    let both = server.load_in::<Text>("notes/other.txt", Scope::Level);
    assert_eq!(
        server.load::<Text>("notes/other.txt"),
        both,
        "the same asset"
    );
    let missing = server.load_in::<Text>("notes/missing.txt", Scope::Level);
    let raw = server.load_in::<Raw>("notes/hello.txt", Scope::Transient);
    assert_eq!(finish(&mut server).finalized.len(), 2);

    // The level's own asset, and its failed one with its placeholder, go.
    assert_eq!(server.unload_scope(Scope::Level), 2);
    for gone in [level, missing] {
        assert_eq!(server.state(gone), None);
        assert_eq!(server.get(gone), None);
        assert_eq!(server.path(gone), None);
    }
    let texts = server.assets::<Text>().expect("texts were loaded");
    assert_eq!(texts.handle("notes/hello.txt"), None);
    assert_eq!(texts.len(), 1);
    // The global scope still holds the other, and the transient one is
    // another type's.
    assert_eq!(text(&server, both), Some("OTHER"));
    assert_eq!(server.get(raw), Some(&Raw(b"hello".to_vec())));
    // They are freed by the next finish: the loaded text and the
    // placeholder.
    assert!(server.has_pending());
    assert_eq!(finish(&mut server).released, 2);
    assert!(!server.has_pending());
    assert_eq!(
        server.unload_scope(Scope::Level),
        0,
        "nothing is left in it"
    );

    // Loading the path again loads it anew, under another handle.
    let again = server.load_in::<Text>("notes/hello.txt", Scope::Level);
    assert_ne!(again, level);
    assert_eq!(server.state(level), None, "the old handle stays stale");
    assert_eq!(finish(&mut server).finalized, ["hello"]);
    assert_eq!(text(&server, again), Some("HELLO"));

    assert_eq!(server.unload_scope(Scope::Global), 1);
    assert_eq!(server.state(both), None);
    assert_eq!(server.unload_scope(Scope::Transient), 1);
    assert_eq!(server.get(raw), None);
    assert_eq!(
        finish(&mut server).released,
        1,
        "only the text releases itself"
    );
    assert_eq!(server.assets::<Text>().map(|assets| assets.len()), Some(1));
}

#[test]
fn an_asset_unloaded_while_it_loads_is_never_made() {
    let (source, gate) = Gated::new(&[("notes/hello.txt", "hello")]);
    let mut server = AssetServer::with_source(source, OnFailure::Placeholder);
    let hello = server.load_in::<Text>("notes/hello.txt", Scope::Transient);
    assert_eq!(server.unload_scope(Scope::Transient), 1);
    assert_eq!(server.state(hello), None);
    // The worker still owes its result, so the server is not done.
    assert!(server.has_pending());
    open(&gate);
    assert_eq!(finish(&mut server), Counts::default());
    assert!(!server.has_pending());
    assert_eq!(server.assets::<Text>().map(|assets| assets.len()), Some(0));
}

#[test]
fn a_release_that_fails_is_logged_with_the_path() {
    let mut server = AssetServer::new(root("release_fails"), OnFailure::Placeholder);
    let hello = server.load_in::<Text>("notes/hello.txt", Scope::Level);
    finish(&mut server);
    assert_eq!(server.unload_scope(Scope::Level), 1);
    assert_eq!(server.state(hello), None);
    server.finish_completed_loads(&mut FinalizeCtx::new());
    assert!(!server.has_pending(), "it is dropped all the same");
    assert!(
        log::tail(log::RING_CAPACITY).iter().any(|line| {
            line.contains("asset `notes/hello.txt`: it could not be freed")
                && line.contains("Counts")
        }),
        "no log line names the file"
    );
}
