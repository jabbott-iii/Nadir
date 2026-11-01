//! What the cook refuses, warns about and leaves alone.

use nadir_cook::CookError;

use crate::common::{case_sensitive, cook, dir, plain, png, write};
use crate::tree;

#[test]
fn names_that_differ_only_in_case_are_refused() {
    let root = dir("case");
    let source = root.join("source");
    std::fs::create_dir_all(&source).expect("make the folder");
    // Only where the file system can hold both names: the rules are tested
    // on lists of names in `scan.rs` on every system.
    if !case_sensitive(&source) {
        return;
    }
    write(&source, "chars/Hero.png", plain(2, 2, [1, 2, 3, 255]));
    write(&source, "chars/hero.png", plain(2, 2, [1, 2, 3, 255]));
    assert_eq!(
        cook(&source, &root.join("out")).map(drop),
        Err(CookError::CaseCollision {
            first: "chars/Hero.png".to_string(),
            second: "chars/hero.png".to_string(),
        })
    );
}

#[test]
fn two_groups_that_one_file_name_would_hold_are_refused() {
    let root = dir("groups");
    let source = root.join("source");
    write(&source, "Été/a.png", plain(2, 2, [1, 2, 3, 255]));
    write(&source, "other/b.png", plain(2, 2, [1, 2, 3, 255]));
    write(&source, "other/atlas.jsonc", r#"{ "group": "été" }"#);
    assert_eq!(
        cook(&source, &root.join("out")).map(drop),
        Err(CookError::OutputCollision {
            first: "atlas/Été.kbj".to_string(),
            second: "atlas/été.kbj".to_string(),
        })
    );
}

#[test]
fn two_groups_whose_folders_one_name_would_hold_are_refused() {
    let root = dir("group_folders");
    let source = root.join("source");
    write(&source, "Été/t/a.png", plain(2, 2, [1, 2, 3, 255]));
    write(&source, "other/b.png", plain(2, 2, [1, 2, 3, 255]));
    write(&source, "other/atlas.jsonc", r#"{ "group": "été/x" }"#);
    assert_eq!(
        cook(&source, &root.join("out")).map(drop),
        Err(CookError::OutputCollision {
            first: "atlas/Été".to_string(),
            second: "atlas/été".to_string(),
        })
    );
}

#[test]
fn a_wrong_file_is_named_in_its_error() {
    let root = dir("wrong");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    write(&source, "chars/hero.sprite.jsonc", r#"{ "frame": [8, 8] }"#);
    assert!(matches!(
        cook(&source, &out),
        Err(CookError::Config { path, .. }) if path == "chars/hero.sprite.jsonc"
    ));
    write(
        &source,
        "chars/hero.sprite.jsonc",
        r#"{ "frames": [7, 8] }"#,
    );
    assert_eq!(
        cook(&source, &out).map(drop),
        Err(CookError::Layout {
            path: "chars/hero.png".to_string(),
            reason: "a 16 × 8 image is not a whole number of frames of 7 × 8".to_string(),
        })
    );
    write(
        &source,
        "chars/hero.sprite.jsonc",
        r#"{ "frames": [8, 8] }"#,
    );
    write(
        &source,
        "tiles/ground.sprite.jsonc",
        r#"{ "tiles": [16, 5] }"#,
    );
    assert_eq!(
        cook(&source, &out).map(drop),
        Err(CookError::Layout {
            path: "tiles/ground.png".to_string(),
            reason: "a 32 × 16 image is not a whole number of tiles of 16 × 5".to_string(),
        })
    );
    write(
        &source,
        "tiles/ground.sprite.jsonc",
        r#"{ "tiles": [16, 16] }"#,
    );
    write(&source, "props/atlas.jsonc", r#"{ "group": "World" }"#);
    assert!(matches!(
        cook(&source, &out),
        Err(CookError::Config { path, .. }) if path == "props/atlas.jsonc"
    ));
    write(&source, "props/atlas.jsonc", "{}");
    write(&source, "props/rock.png", b"not a PNG");
    assert!(matches!(
        cook(&source, &out),
        Err(CookError::Decode { path, .. }) if path == "props/rock.png"
    ));
}

#[test]
fn the_output_folder_and_the_source_folder_must_not_hold_each_other() {
    let root = dir("overlap");
    let source = root.join("source");
    tree(&source);
    assert!(matches!(
        cook(&source, &source.join("cooked/deeper")),
        Err(CookError::Overlap { .. })
    ));
    assert!(
        !source.join("cooked").exists(),
        "a refused cook makes no folder"
    );
    assert!(matches!(
        cook(&source, &source),
        Err(CookError::Overlap { .. })
    ));
    assert!(matches!(
        cook(&source, &root),
        Err(CookError::Overlap { .. })
    ));
}

#[test]
fn a_png_warns_for_color_chunks_but_not_for_srgb_or_text() {
    let root = dir("chunks");
    let source = root.join("source");
    let pixels = [9, 9, 9, 255];
    let gamma: [u8; 4] = 45_455u32.to_be_bytes();
    write(
        &source,
        "g/gamma.png",
        png(1, 1, &pixels, &[(b"gAMA", &gamma), (b"iCCP", b"p\0\0x")]),
    );
    write(
        &source,
        "g/srgb.png",
        png(1, 1, &pixels, &[(b"sRGB", &[0])]),
    );
    write(
        &source,
        "g/text.png",
        png(1, 1, &pixels, &[(b"tEXt", b"Software\0paint")]),
    );
    let report = cook(&source, &root.join("out")).expect("a cook");
    assert_eq!(
        report.warnings,
        ["`g/gamma.png` has gAMA, iCCP chunks, which the cook ignores: it takes every PNG as sRGB"]
    );
}

#[test]
fn files_no_pipeline_cooks_are_listed_and_nothing_fails_for_them() {
    let root = dir("skipped");
    let source = root.join("source");
    tree(&source);
    write(&source, "sfx/jump.wav", "RIFF");
    write(&source, ".git/config", "hidden");
    write(&source, "empty/atlas.jsonc", "{}");
    let report = cook(&source, &root.join("out")).expect("a cook");
    let skipped: Vec<&str> = report
        .skipped
        .iter()
        .map(|(path, _)| path.as_str())
        .collect();
    assert_eq!(skipped, ["readme.md", "sfx/jump.wav"]);
    assert_eq!(
        report.warnings,
        ["`empty/atlas.jsonc` is in a folder with no image, so it does nothing"]
    );
}
