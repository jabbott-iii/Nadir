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

//! Finding the sources: the walk of the source folder, then the rules on
//! what it found, which are kept apart so that they can be tested on lists
//! made by hand, on any system.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use nadir_assets::AssetPath;

use crate::CookError;

/// The extension of the images the cook packs.
const IMAGE: &str = ".png";
/// The extension of an image's or an export's sidecar.
const SIDECAR: &str = ".sprite.jsonc";
/// The name of a folder's settings.
const FOLDER_CONFIG: &str = "atlas.jsonc";
/// The extension of an Aseprite export: every `.json` is read as one.
pub(crate) const EXPORT: &str = ".json";
/// The extension of a BMFont font, in the text format.
pub(crate) const FONT: &str = ".fnt";
/// The extension of an LDtk project.
pub(crate) const PROJECT: &str = ".ldtk";
/// The extension of a level file of an LDtk project that saves its levels
/// apart.
pub(crate) const LDTK_LEVEL: &str = ".ldtkl";
/// The folder of the data files, below the source folder: each `.jsonc` in
/// it, or below it, is data.
const DATA_FOLDER: &str = "data/";
/// The extension of a data file.
pub(crate) const DATA: &str = ".jsonc";

/// Something the walk found below the source folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    /// Its path below the source folder, `/` between parts, as found. A
    /// name that is not UTF-8 has its bad bytes replaced.
    pub(crate) path: String,
    pub(crate) kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    File,
    Folder,
    /// A symbolic link, which is not followed.
    Link,
    /// A file or folder whose name is not UTF-8, which is not looked in.
    NotUtf8,
}

/// A file the cook reads: its path as found, to read it, and normalized.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Source {
    /// Below the source folder, as found.
    pub(crate) found: String,
    /// Lowercased and checked: an asset path.
    pub(crate) path: String,
}

/// What the cook does with what the walk found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    /// The images, by path, each with its sidecar if it has one.
    pub(crate) images: Vec<(Source, Option<Source>)>,
    /// The Aseprite exports, by path, each with its sidecar if it has one.
    pub(crate) exports: Vec<(Source, Option<Source>)>,
    /// The BMFont fonts, by path.
    pub(crate) fonts: Vec<Source>,
    /// The LDtk projects, by path.
    pub(crate) projects: Vec<Source>,
    /// The level files of LDtk projects, by path.
    pub(crate) ldtk_levels: Vec<Source>,
    /// The data files, by path.
    pub(crate) data: Vec<Source>,
    /// The folders' `atlas.jsonc` files, by path.
    pub(crate) configs: Vec<Source>,
    /// What is not cooked, by path as found, and why.
    pub(crate) skipped: Vec<(String, String)>,
}

/// Lists what is below `source`, folders before what they hold, each
/// folder's names in byte order. Names starting with `.` are left out, and
/// so is what such a folder holds.
///
/// # Errors
///
/// [`CookError::Io`] if a folder cannot be read.
pub(crate) fn walk(source: &Path) -> Result<Vec<Entry>, CookError> {
    let mut found = Vec::new();
    let mut folders = vec![String::new()];
    while let Some(folder) = folders.pop() {
        let dir = source.join(&folder);
        let read = fs::read_dir(&dir).map_err(|error| CookError::io(&dir, "read", &error))?;
        let mut here = Vec::new();
        for entry in read {
            let entry = entry.map_err(|error| CookError::io(&dir, "read", &error))?;
            let name = entry.file_name();
            let kind = entry
                .file_type()
                .map_err(|error| CookError::io(entry.path(), "read", &error))?;
            let (name, kind) = match name.into_string() {
                Ok(name) if kind.is_symlink() => (name, Kind::Link),
                Ok(name) if kind.is_dir() => (name, Kind::Folder),
                Ok(name) => (name, Kind::File),
                Err(name) => (name.to_string_lossy().into_owned(), Kind::NotUtf8),
            };
            if !name.starts_with('.') {
                here.push((name, kind));
            }
        }
        here.sort();
        // Popped last first, so pushed in reverse to be walked in order.
        let mut deeper = Vec::new();
        for (name, kind) in here {
            let path = if folder.is_empty() {
                name
            } else {
                format!("{folder}/{name}")
            };
            if kind == Kind::Folder {
                deeper.push(path.clone());
            }
            found.push(Entry { path, kind });
        }
        folders.extend(deeper.into_iter().rev());
    }
    Ok(found)
}

/// What a file is to the cook, from its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Image,
    Sidecar,
    Config,
    Export,
    Font,
    Project,
    LdtkLevel,
    Data,
    Other,
}

fn role(path: &str) -> Role {
    let path = path.to_ascii_lowercase();
    let name = path.rsplit('/').next().unwrap_or(&path);
    if path.starts_with(DATA_FOLDER) && name.ends_with(DATA) {
        Role::Data
    } else if name == FOLDER_CONFIG {
        Role::Config
    } else if name.ends_with(SIDECAR) {
        Role::Sidecar
    } else if name.ends_with(IMAGE) {
        Role::Image
    } else if name.ends_with(EXPORT) {
        Role::Export
    } else if name.ends_with(FONT) {
        Role::Font
    } else if name.ends_with(PROJECT) {
        Role::Project
    } else if name.ends_with(LDTK_LEVEL) {
        Role::LdtkLevel
    } else {
        Role::Other
    }
}

/// The folder a path is in, `""` for the source folder itself.
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// A name as a system that ignores case sees it: each character
/// uppercased, then lowercased, so that `Σ`, `σ` and `ς` are one, as they
/// are to Windows and macOS. It can join names those systems keep apart
/// (`ß` and `ss`), which is the safe side.
pub(crate) fn fold(name: &str) -> String {
    name.chars()
        .flat_map(char::to_uppercase)
        .flat_map(char::to_lowercase)
        .collect()
}

/// Applies the rules to what [`walk`] found, in any order.
///
/// - A file the cook reads (an image, a sidecar, an `atlas.jsonc`, an
///   Aseprite export, a font, an LDtk project or level file, a data file)
///   must have an asset path and a
///   name that is UTF-8, and not be a symbolic link; and no other name in a
///   folder that holds it, or in its folders, may equal its own once
///   [`fold`]ed. Other files with such names are skipped, and say why.
/// - An image, an export or a font must be in a folder, and a sidecar
///   beside its export, or else its image, of the same name.
///
/// # Errors
///
/// [`CookError::CaseCollision`] first, for the first pair in path order;
/// then [`CookError::BadPath`], [`CookError::Link`] and
/// [`CookError::ImageAtRoot`], for the first file in path order; then
/// [`CookError::OrphanSidecar`].
pub(crate) fn plan(entries: &[Entry]) -> Result<Plan, CookError> {
    let mut entries = entries.to_vec();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let mut skipped: BTreeMap<String, String> = BTreeMap::new();

    // What the cook reads, and the folders that hold it.
    let read: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.kind == Kind::File && role(&entry.path) != Role::Other)
        .collect();
    let mut cooked: BTreeSet<&str> = BTreeSet::new();
    for entry in &read {
        let mut path = entry.path.as_str();
        cooked.insert(path);
        while !parent(path).is_empty() {
            path = parent(path);
            cooked.insert(path);
        }
    }

    // Names that differ only in case, folder by folder.
    let mut siblings: BTreeMap<(&str, String), Vec<&str>> = BTreeMap::new();
    for entry in &entries {
        let name = entry.path.rsplit('/').next().unwrap_or(&entry.path);
        siblings
            .entry((parent(&entry.path), fold(name)))
            .or_default()
            .push(&entry.path);
    }
    for paths in siblings.values().filter(|paths| paths.len() > 1) {
        if paths.iter().any(|path| cooked.contains(path)) {
            return Err(CookError::CaseCollision {
                first: paths[0].to_string(),
                second: paths[1].to_string(),
            });
        }
        for &path in paths {
            let others: Vec<&str> = paths
                .iter()
                .copied()
                .filter(|&other| other != path)
                .collect();
            skipped.insert(
                path.to_string(),
                format!(
                    "its name differs from `{}` only in case",
                    others.join("`, `")
                ),
            );
        }
    }

    let mut images = Vec::new();
    let mut sidecars = Vec::new();
    let mut configs = Vec::new();
    let mut exports = Vec::new();
    let mut fonts = Vec::new();
    let mut projects = Vec::new();
    let mut ldtk_levels = Vec::new();
    let mut data = Vec::new();
    for entry in &entries {
        let read = role(&entry.path) != Role::Other;
        let reason = match entry.kind {
            Kind::Folder => continue,
            Kind::Link if read => {
                return Err(CookError::Link {
                    path: entry.path.clone(),
                });
            }
            Kind::Link => Some("a symbolic link, which the cook does not follow".to_string()),
            Kind::NotUtf8 if read => {
                return Err(CookError::BadPath {
                    path: entry.path.clone(),
                    reason: "its name is not UTF-8",
                });
            }
            Kind::NotUtf8 => Some("its name is not UTF-8".to_string()),
            Kind::File if skipped.contains_key(&entry.path) => continue,
            Kind::File => None,
        };
        if let Some(reason) = reason {
            skipped.insert(entry.path.clone(), reason);
            continue;
        }
        let role = role(&entry.path);
        let path = match AssetPath::new(&entry.path) {
            Ok(path) => path.as_str().to_string(),
            Err(error) => {
                let reason = match error {
                    nadir_assets::AssetError::BadPath(reason) => reason,
                    _ => "it cannot be an asset's",
                };
                if role == Role::Other {
                    skipped.insert(
                        entry.path.clone(),
                        format!("its path is not an asset path: {reason}"),
                    );
                    continue;
                }
                return Err(CookError::BadPath {
                    path: entry.path.clone(),
                    reason,
                });
            }
        };
        let source = Source {
            found: entry.path.clone(),
            path,
        };
        match role {
            Role::Image | Role::Export | Role::Font if parent(&source.path).is_empty() => {
                return Err(CookError::ImageAtRoot { path: source.found });
            }
            Role::Image => images.push(source),
            Role::Export => exports.push(source),
            Role::Font => fonts.push(source),
            Role::Project => projects.push(source),
            Role::LdtkLevel => ldtk_levels.push(source),
            Role::Data => data.push(source),
            Role::Sidecar => sidecars.push(source),
            Role::Config => configs.push(source),
            Role::Other => {
                skipped.insert(source.found, "no pipeline cooks it yet".to_string());
            }
        }
    }

    // A sidecar goes with the export of its name, or else the image.
    let mut paired: BTreeMap<String, Option<Source>> = images
        .iter()
        .chain(&exports)
        .map(|file| (file.path.clone(), None))
        .collect();
    for sidecar in sidecars {
        let stem = &sidecar.path[..sidecar.path.len() - SIDECAR.len()];
        let export = format!("{stem}{EXPORT}");
        let owner = if paired.contains_key(&export) {
            export
        } else {
            format!("{stem}{IMAGE}")
        };
        match paired.get_mut(&owner) {
            Some(slot) => *slot = Some(sidecar),
            None => {
                return Err(CookError::OrphanSidecar {
                    path: sidecar.found,
                });
            }
        }
    }
    let mut with_sidecars = |files: Vec<Source>| -> Vec<(Source, Option<Source>)> {
        files
            .into_iter()
            .map(|file| {
                let sidecar = paired.remove(&file.path).flatten();
                (file, sidecar)
            })
            .collect()
    };
    let images = with_sidecars(images);
    let exports = with_sidecars(exports);
    Ok(Plan {
        images,
        exports,
        fonts,
        projects,
        ldtk_levels,
        data,
        configs,
        skipped: skipped.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> Entry {
        Entry {
            path: path.to_string(),
            kind: Kind::File,
        }
    }

    fn folder(path: &str) -> Entry {
        Entry {
            path: path.to_string(),
            kind: Kind::Folder,
        }
    }

    fn source(found: &str) -> Source {
        Source {
            found: found.to_string(),
            path: found.to_ascii_lowercase(),
        }
    }

    #[test]
    fn images_sidecars_and_folder_settings_are_cooked_and_the_rest_listed() {
        let plan = plan(&[
            folder("sprites"),
            file("sprites/Hero.PNG"),
            file("sprites/hero.sprite.jsonc"),
            file("sprites/tree.png"),
            file("sprites/atlas.jsonc"),
            file("sprites/tree.qoi"),
            file("readme.md"),
        ])
        .expect("a plan");
        assert_eq!(
            plan.images,
            [
                (
                    source("sprites/Hero.PNG"),
                    Some(source("sprites/hero.sprite.jsonc"))
                ),
                (source("sprites/tree.png"), None),
            ]
        );
        assert_eq!(plan.configs, [source("sprites/atlas.jsonc")]);
        assert_eq!(
            plan.skipped,
            [
                (
                    "readme.md".to_string(),
                    "no pipeline cooks it yet".to_string()
                ),
                (
                    "sprites/tree.qoi".to_string(),
                    "no pipeline cooks it yet".to_string()
                ),
            ]
        );
    }

    #[test]
    fn exports_fonts_and_data_are_cooked_and_a_sidecar_goes_with_its_export() {
        let found = plan(&[
            folder("data"),
            folder("data/Items"),
            file("data/Items/Sword.jsonc"),
            file("data/settings.jsonc"),
            file("data/notes.txt"),
            folder("fonts"),
            file("fonts/pixel.fnt"),
            file("fonts/pixel_0.png"),
            folder("sprites"),
            file("sprites/person.json"),
            file("sprites/person.png"),
            file("sprites/person.sprite.jsonc"),
            file("sprites/tree.png"),
            file("sprites/tree.sprite.jsonc"),
            file("sprites/notes.jsonc"),
        ])
        .expect("a plan");
        assert_eq!(
            found.exports,
            [(
                source("sprites/person.json"),
                Some(source("sprites/person.sprite.jsonc"))
            )]
        );
        // The export's sheet is an image until the export claims it.
        assert_eq!(
            found.images,
            [
                (source("fonts/pixel_0.png"), None),
                (source("sprites/person.png"), None),
                (
                    source("sprites/tree.png"),
                    Some(source("sprites/tree.sprite.jsonc"))
                ),
            ]
        );
        assert_eq!(found.fonts, [source("fonts/pixel.fnt")]);
        assert_eq!(
            found.data,
            [
                source("data/Items/Sword.jsonc"),
                source("data/settings.jsonc")
            ]
        );
        // A `.jsonc` outside `data/` that is not a sidecar is no one's.
        let skipped: Vec<&str> = found
            .skipped
            .iter()
            .map(|(path, _)| path.as_str())
            .collect();
        assert_eq!(skipped, ["data/notes.txt", "sprites/notes.jsonc"]);
        // Exports and fonts need a folder, as images do.
        for path in ["person.json", "pixel.fnt"] {
            assert_eq!(
                plan(&[file(path)]),
                Err(CookError::ImageAtRoot {
                    path: path.to_string()
                })
            );
        }
    }

    #[test]
    fn ldtk_projects_and_their_level_files_are_read_and_never_as_exports() {
        let found = plan(&[
            file("field.ldtk"),
            folder("levels"),
            file("levels/Cave.LDTK"),
            folder("levels/cave"),
            file("levels/cave/0000-Entry.ldtkl"),
            file("levels/cave/notes.ldtk.txt"),
            folder("sprites"),
            file("sprites/person.json"),
        ])
        .expect("a plan");
        // A project needs no folder: no atlas group is named by it.
        assert_eq!(
            found.projects,
            [source("field.ldtk"), source("levels/Cave.LDTK")]
        );
        assert_eq!(found.ldtk_levels, [source("levels/cave/0000-Entry.ldtkl")]);
        assert_eq!(found.exports, [(source("sprites/person.json"), None)]);
        assert_eq!(found.skipped.len(), 1);
        assert_eq!(found.skipped[0].0, "levels/cave/notes.ldtk.txt");
    }

    #[test]
    fn the_order_found_does_not_matter() {
        let mut entries = vec![
            folder("b"),
            file("b/z.png"),
            file("b/a.png"),
            folder("a"),
            file("a/m.png"),
            file("a/m.sprite.jsonc"),
            file("notes.txt"),
        ];
        let first = plan(&entries).expect("a plan");
        entries.reverse();
        assert_eq!(plan(&entries).expect("a plan"), first);
        entries.swap(1, 4);
        assert_eq!(plan(&entries).expect("a plan"), first);
    }

    #[test]
    fn names_that_differ_only_in_case_fail_if_the_cook_reads_one() {
        let collision = |entries: &[Entry]| match plan(entries) {
            Err(CookError::CaseCollision { first, second }) => (first, second),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            collision(&[folder("s"), file("s/A.png"), file("s/a.png")]),
            ("s/A.png".to_string(), "s/a.png".to_string())
        );
        assert_eq!(
            collision(&[folder("s"), file("s/hero.png"), file("s/HERO.png")]).0,
            "s/HERO.png"
        );
        // Folders that hold images, and a file beside such a folder.
        assert_eq!(
            collision(&[
                folder("Sprites"),
                folder("sprites"),
                file("Sprites/a.png"),
                file("sprites/b.png")
            ]),
            ("Sprites".to_string(), "sprites".to_string())
        );
        assert_eq!(
            collision(&[file("props"), folder("Props"), file("Props/rock.png")]).0,
            "Props"
        );
        // Greek's final sigma too.
        assert_eq!(
            collision(&[folder("s"), file("s/ΟΔΟΣ_1.png"), file("s/οδοσ_1.png")]).1,
            "s/οδοσ_1.png"
        );
        assert_eq!(
            collision(&[folder("s"), file("s/σ.png"), file("s/ς.png")]).0,
            "s/ς.png"
        );
        // Unicode's case, not ASCII's only.
        assert_eq!(
            collision(&[folder("s"), file("s/É.png"), file("s/é.png")]),
            ("s/É.png".to_string(), "s/é.png".to_string())
        );
    }

    #[test]
    fn names_that_differ_only_in_case_are_listed_if_nothing_reads_them() {
        let plan = plan(&[
            folder("sfx"),
            file("sfx/Jump.wav"),
            file("sfx/jump.wav"),
            folder("docs"),
            folder("Docs"),
            file("docs/a.md"),
        ])
        .expect("a plan");
        let reasons: BTreeMap<String, String> = plan.skipped.into_iter().collect();
        assert_eq!(
            reasons.get("sfx/Jump.wav").map(String::as_str),
            Some("its name differs from `sfx/jump.wav` only in case")
        );
        assert!(reasons.contains_key("sfx/jump.wav"));
        assert!(reasons.contains_key("Docs") && reasons.contains_key("docs"));
    }

    #[test]
    fn a_file_the_cook_reads_needs_an_asset_path_and_others_are_listed() {
        let bad = |entries: &[Entry]| match plan(entries) {
            Err(CookError::BadPath { path, reason }) => (path, reason),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            bad(&[folder("s"), file("s/con.png")]),
            ("s/con.png".to_string(), "a part is a Windows device name")
        );
        assert_eq!(
            bad(&[folder("s "), file("s /a.png")]).1,
            "a part ends in a dot or a space"
        );
        assert_eq!(bad(&[folder("s"), file("s/a:b.png")]).1, "it has a `:`");
        assert_eq!(
            bad(&[folder("s"), file("s/a\\b.sprite.jsonc")]).0,
            "s/a\\b.sprite.jsonc"
        );
        // Finder's `Icon\r` in a folder with a custom icon, and a device's
        // name, are only listed.
        let entries = [
            folder("s"),
            file("s/Icon\r"),
            file("s/a.png"),
            file("s/con.txt"),
        ];
        let plan = plan(&entries).expect("a plan");
        assert_eq!(plan.images.len(), 1);
        assert_eq!(
            plan.skipped,
            [
                (
                    "s/Icon\r".to_string(),
                    "its path is not an asset path: it has a control character".to_string()
                ),
                (
                    "s/con.txt".to_string(),
                    "its path is not an asset path: a part is a Windows device name".to_string()
                ),
            ]
        );
    }

    #[test]
    fn links_and_names_not_utf8_fail_if_the_cook_reads_them_and_are_listed_if_not() {
        let link = |path: &str| Entry {
            path: path.to_string(),
            kind: Kind::Link,
        };
        let not_utf8 = |path: &str| Entry {
            path: path.to_string(),
            kind: Kind::NotUtf8,
        };
        assert_eq!(
            plan(&[folder("s"), file("s/a.png"), link("s/a.sprite.jsonc")]),
            Err(CookError::Link {
                path: "s/a.sprite.jsonc".to_string()
            })
        );
        assert_eq!(
            plan(&[folder("s"), not_utf8("s/b\u{fffd}d.png")]),
            Err(CookError::BadPath {
                path: "s/b\u{fffd}d.png".to_string(),
                reason: "its name is not UTF-8",
            })
        );
        let plan = plan(&[
            folder("s"),
            link("s/link.qoi"),
            not_utf8("s/b\u{fffd}d.wav"),
        ])
        .expect("a plan");
        assert!(plan.images.is_empty());
        assert_eq!(
            plan.skipped,
            [
                (
                    "s/b\u{fffd}d.wav".to_string(),
                    "its name is not UTF-8".to_string()
                ),
                (
                    "s/link.qoi".to_string(),
                    "a symbolic link, which the cook does not follow".to_string()
                ),
            ]
        );
    }

    #[test]
    fn an_image_needs_a_folder_and_a_sidecar_its_image() {
        assert_eq!(
            plan(&[file("Logo.png")]),
            Err(CookError::ImageAtRoot {
                path: "Logo.png".to_string()
            })
        );
        assert_eq!(
            plan(&[folder("s"), file("s/a.png"), file("s/b.sprite.jsonc")]),
            Err(CookError::OrphanSidecar {
                path: "s/b.sprite.jsonc".to_string()
            })
        );
        // A sidecar in another folder is no image's.
        assert!(matches!(
            plan(&[
                folder("s"),
                folder("t"),
                file("s/a.png"),
                file("t/a.sprite.jsonc")
            ]),
            Err(CookError::OrphanSidecar { .. })
        ));
        // Whatever the case of either name.
        let plan = plan(&[folder("s"), file("s/A.png"), file("s/a.SPRITE.jsonc")]).expect("a plan");
        assert!(plan.images[0].1.is_some());
    }
}
