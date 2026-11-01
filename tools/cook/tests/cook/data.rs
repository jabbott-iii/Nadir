//! Data files: each JSONC file below `data/` cooks to KBJ at its path, a
//! job of its own.

use std::fs;

use nadir_codec::{Limits, json, kbj};
use nadir_cook::{CookError, Counts};

use crate::common::{cook, dir, files, jobs, names, plain, write};

const SWORD: &str = r#"// A sword, as a prefab or an item table might give it.
{
  "name": "Sword",
  "damage": [3, 5],   // a range
  "weight": 1.5,
  "tags": ["blade", "iron",],
  /* nothing else */
}
"#;

#[test]
fn data_files_below_data_cook_to_kbj_at_their_paths() {
    let root = dir("data");
    let (source, out) = (root.join("source"), root.join("out"));
    write(&source, "data/Items/Sword.jsonc", SWORD);
    write(&source, "data/settings.jsonc", r#"{ "volume": 0.8 }"#);
    write(&source, "sprites/notes.jsonc", r#"{ "not": "data" }"#);
    write(&source, "sprites/rock.png", plain(2, 2, [1, 2, 3, 255]));
    let report = cook(&source, &out).expect("a cook");
    assert_eq!(
        jobs(&report),
        [
            ("atlas/sprites".to_string(), true),
            ("data/items/sword".to_string(), true),
            ("data/settings".to_string(), true),
        ]
    );
    assert_eq!(report.jobs[1].counts, Some(Counts::Data));
    let skipped: Vec<&str> = report
        .skipped
        .iter()
        .map(|(path, _)| path.as_str())
        .collect();
    assert_eq!(skipped, ["sprites/notes.jsonc"]);
    // The same value, its members in their order, comments gone.
    let bytes = fs::read(out.join("data/items/sword.kbj")).expect("the cooked file");
    assert_eq!(
        kbj::decode(&bytes, Limits::default()).expect("KBJ"),
        json::parse(SWORD, Limits::default()).expect("JSONC")
    );
    assert!(names(&out).contains(&"data/settings.kbj".to_string()));
}

#[test]
fn data_jobs_go_by_name_and_their_outputs_may_not_collide() {
    let root = dir("data_names");
    let (source, out) = (root.join("source"), root.join("out"));
    for path in ["data/Zeta/a.jsonc", "data/alpha/b.jsonc", "data/Beta.jsonc"] {
        write(&source, path, "{}");
    }
    let report = cook(&source, &out).expect("a cook");
    assert_eq!(
        jobs(&report),
        [
            ("data/alpha/b".to_string(), true),
            ("data/beta".to_string(), true),
            ("data/zeta/a".to_string(), true),
        ]
    );
    // A file and a folder of one name: refused before anything is written.
    let root = dir("data_collision");
    let (source, out) = (root.join("source"), root.join("out"));
    write(&source, "data/x.jsonc", "{}");
    write(&source, "data/x.kbj/y.jsonc", "{}");
    assert!(matches!(
        cook(&source, &out),
        Err(CookError::OutputCollision { first, second })
            if first == "data/x.kbj" && second == "data/x.kbj"
    ));
    assert!(!out.exists(), "nothing written");
}

#[test]
fn a_data_file_that_is_not_jsonc_is_named() {
    let root = dir("data_bad");
    let source = root.join("source");
    write(&source, "data/broken.jsonc", r#"{ "a": 1 "b": 2 }"#);
    assert!(matches!(
        cook(&source, &root.join("out")),
        Err(CookError::Decode { path, error: nadir_codec::CodecError::Json { line: 1, .. } })
            if path == "data/broken.jsonc"
    ));
    write(&source, "data/broken.jsonc", b"\xff\xfe{}");
    assert!(matches!(
        cook(&source, &root.join("out")),
        Err(CookError::Config { path, reason }) if path == "data/broken.jsonc" && reason == "it is not UTF-8"
    ));
}

#[test]
fn each_data_file_is_cooked_again_alone_and_the_cook_owns_the_data_folder() {
    let root = dir("data_changed");
    let (source, out) = (root.join("source"), root.join("out"));
    write(&source, "data/a.jsonc", "[1]");
    write(&source, "data/b.jsonc", "[2]");
    cook(&source, &out).expect("a cook");
    let first = files(&out);
    assert_eq!(
        jobs(&cook(&source, &out).expect("a cook")),
        [("data/a".to_string(), false), ("data/b".to_string(), false)]
    );
    assert_eq!(files(&out), first);
    write(&source, "data/b.jsonc", "[2, 3]");
    assert_eq!(
        jobs(&cook(&source, &out).expect("a cook")),
        [("data/a".to_string(), false), ("data/b".to_string(), true)]
    );
    // A file no job wrote, and a removed source's output, go.
    write(&out, "data/stray.kbj", "stray");
    write(&out, "elsewhere/kept.txt", "not the cook's");
    fs::remove_file(source.join("data/a.jsonc")).expect("remove a source");
    cook(&source, &out).expect("a cook");
    assert_eq!(
        names(&out),
        [".cache.kbj", "data/b.kbj", "elsewhere/kept.txt"]
    );
    let clean = root.join("clean");
    cook(&source, &clean).expect("a cook");
    assert_eq!(
        files(&out)
            .into_iter()
            .filter(|(name, _)| name != "elsewhere/kept.txt")
            .collect::<Vec<_>>(),
        files(&clean)
    );
}
