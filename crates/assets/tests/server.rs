//! The asset server on real files, with two made-up asset types.

use std::fs;
use std::path::{Path, PathBuf};

use nadir_assets::{
    Asset, AssetError, AssetServer, DecodeCtx, FinalizeCtx, Handle, LoadState, OnFailure,
};
use nadir_core::log;

/// Text, uppercased when it is finalized, with `?` as its placeholder.
#[derive(Debug, PartialEq)]
struct Text(String);

/// How many assets were finalized: a resource the context must hold.
struct Finalized(u32);

impl Asset for Text {
    type Decoded = String;
    const EXTENSIONS: &'static [&'static str] = &["txt", "text"];

    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<String, AssetError> {
        assert!(ctx.path().ends_with(".txt") || ctx.path().ends_with(".text"));
        String::from_utf8(bytes.to_vec()).map_err(|_| AssetError::Invalid("not UTF-8".to_string()))
    }

    fn finalize(decoded: String, ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        ctx.get_mut::<Finalized>()?.0 += 1;
        Ok(Self(decoded.to_uppercase()))
    }

    fn placeholder(_: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        Ok(Self("?".to_string()))
    }
}

/// Bytes as they are, with no placeholder.
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

/// Finishes the loads with a counter in the context, and returns the count.
fn finish(server: &mut AssetServer) -> u32 {
    let mut finalized = Finalized(0);
    server.finish_completed_loads(&mut FinalizeCtx::new().with(&mut finalized));
    finalized.0
}

fn text(server: &AssetServer, handle: Handle<Text>) -> Option<&str> {
    server.get(handle).map(|text| text.0.as_str())
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
    assert_eq!(finish(&mut server), 2);
    assert_eq!(server.load::<Text>("notes/hello.txt"), hello, "once loaded");
    let missing = server.load::<Text>("notes/missing.txt");
    finish(&mut server);
    assert_eq!(
        server.load::<Text>("notes/missing.txt"),
        missing,
        "once failed"
    );
    assert_eq!(finish(&mut server), 0, "nothing is loaded again");

    let assets = server.assets::<Text>().expect("texts were loaded");
    assert_eq!(assets.len(), 3);
    assert_eq!(assets.handle("notes/other.txt"), Some(other));
    assert_eq!(assets.handle("Notes/Other.TXT"), Some(other));
    assert_eq!(assets.handle("notes/unknown.txt"), None);
    assert_eq!(assets.path(hello), Some("notes/hello.txt"));
    assert_eq!(server.path(missing), Some("notes/missing.txt"));
}

#[test]
fn an_asset_is_made_by_the_next_finish() {
    let mut server = AssetServer::new(root("finish"), OnFailure::Placeholder);
    assert!(!server.has_pending());
    let hello = server.load::<Text>("notes/hello.txt");
    assert_eq!(server.state(hello), Some(LoadState::Loading));
    assert_eq!(text(&server, hello), None);
    assert!(server.has_pending());

    assert_eq!(finish(&mut server), 1);
    assert_eq!(server.state(hello), Some(LoadState::Loaded));
    assert_eq!(text(&server, hello), Some("HELLO"));
    assert_eq!(server.error(hello), None);
    assert!(!server.has_pending());
    assert_eq!(finish(&mut server), 0);

    // A load after a finish waits for the next one.
    let other = server.load::<Text>("notes/other.txt");
    assert_eq!(server.state(other), Some(LoadState::Loading));
    assert_eq!(finish(&mut server), 1);
    assert_eq!(text(&server, other), Some("OTHER"));
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
fn a_resource_missing_from_the_context_fails_the_asset() {
    let mut server = AssetServer::new(root("no_context"), OnFailure::Placeholder);
    let hello = server.load::<Text>("notes/hello.txt");
    server.finish_completed_loads(&mut FinalizeCtx::new());
    let Some(AssetError::MissingContext(resource)) = server.error(hello) else {
        panic!("{:?}", server.error(hello));
    };
    assert!(resource.ends_with("Finalized"), "{resource}");
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
