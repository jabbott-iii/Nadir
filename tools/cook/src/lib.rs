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

//! The offline asset pipeline of the Nadir engine, as a library: [`cook`]
//! turns a folder of source assets into the files the game loads.
//!
//! It is the cook of [Nadir](https://github.com/jabbott-iii/Nadir). The
//! `nadir-cook` command is its command-line interface,
//! `nadir-cook [--force] SOURCE OUT`, and in Nadir's workspace
//! `cargo xtask cook` runs that on `assets/` and `target/cooked/`. A game
//! does not link this crate: it loads what the cook wrote, from the folder
//! that `nadir_engine`'s `Config::assets` names. Embed the library to cook
//! from a tool, an editor or a build step of your own.
//!
//! # What it cooks
//!
//! So far (roadmap M5.4, M5.5, M5.12, M5.14), [`cook`] reads a source folder and
//! writes, into an output folder:
//!
//! - each atlas group's QOI pages, `atlas/<group>_<n>.qoi`, and a KBJ of
//!   where each frame went, `atlas/<group>.kbj`, from its folder's PNG
//!   images (each cut into frames, or made a tile set, by a
//!   `<name>.sprite.jsonc` sidecar), Aseprite exports (whose tags become
//!   clips) and BMFont fonts, with a file of each font's glyphs,
//!   `atlas/<font>.font.kbj`;
//! - each data file below `data/`, JSONC, as KBJ, `data/<path>.kbj`, the
//!   prefabs of `data/prefabs/` checked as prefab files;
//! - each level of each LDtk 1.5.3 project, `.ldtk`, and of its level files,
//!   `.ldtkl`, as a level file, `levels/<name>.klvl`: its tile layers, its
//!   `Collision` grid, its tall tiles and its entities, which spawn the
//!   prefabs of their names;
//! - a pack, [`PACK`], `game.kpak`, of every file above, which release
//!   builds read (ADR-0041): each stored once however many paths have its
//!   bytes, at a multiple of 16, compressed as KLZ where that makes it
//!   smaller, and read back with the game's reader before it is written;
//! - and a cache, [`CACHE`], that lets the next cook skip the jobs whose
//!   files did not change.
//!
//! An image's folder names its atlas group, unless the folder's
//! `atlas.jsonc` names another. Files no pipeline cooks yet are listed in
//! the [`Report`] and left alone; audio comes with roadmap M6.6. The rules,
//! formats and LDtk conventions (ADR-0038) are in
//! [`docs/subsystems/assets-and-codecs.md`](https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/assets-and-codecs.md),
//! "`nadir-cook` (library + CLI)".
//!
//! # The API
//!
//! - [`cook`] takes the source folder, the output folder and the
//!   [`Options`]: whether to [force](Options::force) every job, and the
//!   [cooker](Options::cooker) that the cache records.
//! - It gives a [`Report`]: each job, a [`JobReport`] with what it made
//!   ([`Counts`]) when it was cooked; the files not cooked, with why; and
//!   the warnings.
//! - Or a [`CookError`], saying which file is wrong, or could not be read
//!   or written.
//!
//! # Example
//!
//! A data file cooks to KBJ, the same value, and a second cook of the same
//! files leaves it as it is:
//!
//! ```
//! use std::fs;
//!
//! use nadir_codec::{Limits, json, kbj};
//! use nadir_cook::{CookError, Counts, Options, cook};
//!
//! let root = std::env::temp_dir().join(format!("nadir-cook-doc-{}", std::process::id()));
//! # let _ = fs::remove_dir_all(&root);
//! let (source, out) = (root.join("assets"), root.join("cooked"));
//! let items = "// What each item does.\n{ \"sword\": { \"damage\": 3 }, }";
//! fs::create_dir_all(source.join("data"))?;
//! fs::write(source.join("data/items.jsonc"), items)?;
//! fs::write(source.join("notes.txt"), "not an asset")?;
//!
//! let options = Options {
//!     force: false,
//!     cooker: "my-tool 1.0".to_string(),
//! };
//! let report = cook(&source, &out, &options)?;
//! assert_eq!(report.jobs[0].name, "data/items");
//! assert_eq!(report.jobs[0].counts, Some(Counts::Data));
//! assert_eq!(report.skipped[0].0, "notes.txt");
//! let cooked = kbj::decode(&fs::read(out.join("data/items.kbj"))?, Limits::default())?;
//! assert_eq!(cooked, json::parse(items, Limits::default())?);
//!
//! // Nothing changed, so nothing is cooked.
//! let again = cook(&source, &out, &options)?;
//! assert!(!again.jobs[0].cooked);
//!
//! // The output folder may not be in the source folder.
//! let error = cook(&source, &source.join("cooked"), &options).unwrap_err();
//! assert!(matches!(error, CookError::Overlap { .. }));
//! # fs::remove_dir_all(&root)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Using it well
//!
//! - **The cooker:** set [`Options::cooker`] to something that changes
//!   whenever your cook does, such as your tool's version. A cache another
//!   cooker wrote is not used, so a new build cooks everything once. The
//!   `nadir-cook` command gives its version and a hash of its executable.
//! - **What it owns:** the cook removes every file below the output's
//!   `atlas/`, `data/` and `levels/` that no job wrote, and writes
//!   [`PACK`] over whatever is there; keep nothing of your own there. The
//!   output folder and the source folder may not hold each other.
//! - **Errors:** [`cook`]'s `# Errors` says what a failure leaves in the
//!   output folder. Each output is written through a `.tmp` beside it and
//!   a rename, so none is left half written. A file not cooked, or a
//!   warning, such as a PNG with color chunks that the cook ignores, taking
//!   it as sRGB, is not an error: show [`Report::skipped`] and
//!   [`Report::warnings`] to whoever made the files.
//! - **Limits:** sources are decoded within nadir-codec's default
//!   `Limits`, but for an image's width and height, which only its decoded
//!   size bounds, since a sheet is cut up. A frame larger than a page of
//!   4096 pixels, with its edge extrusion, is an error
//!   ([`CookError::TooBig`]).
//! - **Determinism:** the output depends only on the source files: the
//!   same files cook to the same bytes on every run and every system.
//!   Sources are taken in sorted order, never in the order the system
//!   lists them, and nothing records a time.
//! - **Threads:** [`cook`] runs on the calling thread, and starts none.

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod atlas;
mod cache;
mod config;
mod error;
mod image;
mod kpak;
mod level;
mod pack;
mod scan;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use nadir_assets::AssetPath;
use nadir_codec::ldtk::{self, ImageExport};
use nadir_codec::{Limits, aseprite, bmfont, json, kbj, png, prefab};
use nadir_core::hash::fnv1a64;

use crate::cache::{Cache, Hashes, Job};
use crate::config::{Layout, Sprite};
use crate::scan::{Plan, Source};

pub use error::CookError;

/// The name of the cache in the output folder.
pub const CACHE: &str = ".cache.kbj";

/// The name of the pack in the output folder, `game.kpak`: every file the
/// cache lists, in one file, which nadir-assets' `PackSource` reads
/// (ADR-0041).
pub const PACK: &str = nadir_assets::PackSource::FILE_NAME;

/// The folder of the output folder that the atlas pipeline owns: a file in
/// it that the cache does not list is removed.
const ATLAS: &str = "atlas";

/// The folder of the output folder that the data pipeline owns, as the
/// atlas pipeline owns [`ATLAS`].
const DATA: &str = "data";

/// The folder of the output folder that the level pipeline owns, as the
/// atlas pipeline owns [`ATLAS`].
const LEVELS: &str = "levels";

/// What the names of the level pipeline's jobs start with: an LDtk
/// project's job is `ldtk/<its path without .ldtk>`.
const LDTK: &str = "ldtk";

/// The folder of the prefabs, below the source folder: each
/// `data/prefabs/<name>.jsonc` is a prefab, checked as a prefab file.
const PREFABS: &str = "data/prefabs/";

/// What the options that change output say, in the cache. None does yet.
const OPTIONS: &str = "";

/// How to cook.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Cook every job, whatever the cache says.
    pub force: bool,
    /// What did the cooking, such as the cook's version and a hash of its
    /// executable. A cache written by another cooker is not used, so that a
    /// cook built anew cooks everything again.
    pub cooker: String,
}

/// What a job did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobReport {
    /// Its name: `atlas/<group>` for an atlas group, `data/<path>` for a
    /// data file, `ldtk/<path>` for an LDtk project (each without its
    /// extension).
    pub name: String,
    /// Whether it was cooked; otherwise its files were up to date.
    pub cooked: bool,
    /// How many files it wrote, or left as they were.
    pub files: usize,
    /// What it made, when it was cooked.
    pub counts: Option<Counts>,
}

/// What a cooked job made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Counts {
    /// An atlas group.
    Atlas {
        /// Its sprites.
        sprites: usize,
        /// Its pages.
        pages: usize,
        /// Its Aseprite exports' clips.
        clips: usize,
        /// Its fonts.
        fonts: usize,
    },
    /// A data file.
    Data,
    /// An LDtk project.
    Levels {
        /// Its level files.
        levels: usize,
        /// The entities they spawn.
        entities: usize,
    },
}

/// What a cook did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Each job, in name order.
    pub jobs: Vec<JobReport>,
    /// The files no pipeline cooks, or that cannot be cooked, and why, by
    /// path as found.
    pub skipped: Vec<(String, String)>,
    /// What the user should know, in the order met.
    pub warnings: Vec<String>,
}

/// A file the cook read, with its hash.
struct Read {
    bytes: Vec<u8>,
    hash: u64,
}

fn read(path: &Path) -> Result<Vec<u8>, CookError> {
    fs::read(path).map_err(|error| CookError::io(path, "read", &error))
}

/// Writes `bytes` as `path` below `out`, through a file beside it that is
/// then renamed, so that a cook stopped halfway leaves no file half
/// written under the output's name.
fn write(out: &Path, path: &str, bytes: &[u8]) -> Result<(), CookError> {
    let target = out.join(path);
    if let Some(folder) = target.parent() {
        fs::create_dir_all(folder).map_err(|error| CookError::io(folder, "create", &error))?;
    }
    let temporary = out.join(format!("{path}.tmp"));
    fs::write(&temporary, bytes).map_err(|error| CookError::io(&temporary, "write", &error))?;
    fs::rename(&temporary, &target).map_err(|error| CookError::io(&target, "write", &error))
}

/// An Aseprite export, read, with the sheet it takes and its sidecar's
/// settings.
struct ExportFile<'p> {
    file: &'p Source,
    sidecar: Option<&'p Source>,
    sheet: &'p Source,
    export: aseprite::Export,
    sprite: Sprite,
}

/// A BMFont font, read, with the pages it takes, by id.
struct FontFile<'p> {
    file: &'p Source,
    pages: Vec<&'p Source>,
    font: bmfont::Font,
}

/// One atlas group's files: its images that no export or font takes, with
/// their sidecars, its exports and fonts (indices), and its folders'
/// settings.
#[derive(Default)]
struct Group<'p> {
    images: Vec<(&'p Source, Option<&'p Source>)>,
    exports: Vec<usize>,
    fonts: Vec<usize>,
    configs: Vec<Source>,
}

/// An LDtk project, read, with its levels and the tile sets they draw.
struct ProjectFile<'p> {
    file: &'p Source,
    /// Its tile sets, as it defines them.
    tilesets: Vec<ldtk::TilesetDef>,
    /// Its levels, each with its layers, from the project or its level file.
    levels: Vec<level::LevelSource>,
    /// The tile sets its levels draw.
    images: Vec<TileImage<'p>>,
    /// Their images' bytes, by path, each read once for its hash and its
    /// size, however many tile sets take it.
    pngs: BTreeMap<String, Vec<u8>>,
    /// Every file its job reads, with its hash, by path.
    inputs: Hashes,
}

/// A tile set that a project's levels draw, and its image.
struct TileImage<'p> {
    uid: i64,
    /// Its place in its project's tile sets.
    def: usize,
    image: &'p Source,
    /// The atlas file and the sprite the image cooks to.
    atlas: String,
    sprite: String,
    /// Its sidecar's tiles.
    tiles: (u32, u32),
}

/// What the cook found that the LDtk projects' levels draw from or spawn,
/// by asset path.
struct Found<'a, 'p> {
    source: &'a Path,
    images: &'a BTreeMap<&'p str, &'p (Source, Option<Source>)>,
    /// The images an export or a font takes, and which.
    claims: &'a BTreeMap<String, String>,
    sprites: &'a BTreeMap<String, Sprite>,
    /// Which group each folder's images join, if not its own.
    joins: &'a BTreeMap<String, String>,
    /// Each folder's `atlas.jsonc`.
    configs: &'a BTreeMap<String, Source>,
    /// The prefabs, by name.
    prefabs: &'a BTreeMap<String, &'p Source>,
}

/// What a file of a folder makes: a sprite of an image, an export or a font.
#[derive(Clone, Copy)]
enum Made<'p> {
    Image(&'p (Source, Option<Source>)),
    Export(usize),
    Font(usize),
}

/// Cooks the files below `source` into `out`, which it makes if needed.
///
/// Each atlas group is a job, and so is each data file and each LDtk
/// project. A job is skipped
/// when the cache in `out` says the same cooker cooked the same files (by
/// their hashes) into files still there as it wrote them, unless
/// [`Options::force`] says to cook everything. Then every file below
/// `out/atlas`, `out/data` or `out/levels` that no job wrote is removed,
/// the pack, [`PACK`], is written of the files the jobs wrote, unless it
/// already holds those bytes, and the cache is written.
///
/// # Errors
///
/// A [`CookError`] saying which file is wrong, or could not be read or
/// written. An error before the jobs are cooked leaves `out` as it was, and
/// makes no folder. One while they are leaves the files of the jobs cooked
/// before it written, and the cache as it was. One while stale files are
/// removed, or the pack is made or written, can leave some of the stale
/// files there, and the pack and the cache as they were; one while the
/// cache is written leaves the new pack beside the old cache, which the
/// next cook writes anew. Two paths of one hash in the pack are a
/// [`CookError::OutputCollision`].
pub fn cook(source: &Path, out: &Path, options: &Options) -> Result<Report, CookError> {
    let source = fs::canonicalize(source).map_err(|error| CookError::io(source, "open", &error))?;
    let resolved = resolve(out)?;
    if resolved.starts_with(&source) || source.starts_with(&resolved) {
        return Err(CookError::Overlap {
            source,
            out: resolved,
        });
    }

    let plan = scan::plan(&scan::walk(&source)?)?;
    let mut report = Report {
        skipped: plan.skipped.clone(),
        ..Report::default()
    };
    let mut reads: BTreeMap<String, Read> = BTreeMap::new();

    // The exports and the fonts, read, and the images they take as their
    // sheets and pages, which are then sprites of no one's.
    let images: BTreeMap<&str, &(Source, Option<Source>)> = plan
        .images
        .iter()
        .map(|entry| (entry.0.path.as_str(), entry))
        .collect();
    let mut claims: BTreeMap<String, String> = BTreeMap::new();
    let mut exports = Vec::with_capacity(plan.exports.len());
    for (file, sidecar) in &plan.exports {
        let bytes = &take(&source, &mut reads, file)?.bytes;
        let export = aseprite::parse(utf8(bytes, file)?, Limits::default()).map_err(|error| {
            CookError::Decode {
                path: file.path.clone(),
                error,
            }
        })?;
        let sheet = claim(&images, &mut claims, file, &export.image, "sheet")?;
        let sprite = match sidecar {
            Some(sidecar) => {
                let bytes = &take(&source, &mut reads, sidecar)?.bytes;
                let config = |reason| CookError::Config {
                    path: sidecar.found.clone(),
                    reason,
                };
                let sprite = config::sprite(bytes).map_err(config)?;
                if sprite.layout != Layout::Whole {
                    return Err(config(
                        "an Aseprite export's frames come from its JSON: its sidecar takes `trim` and `pivot` only"
                            .to_string(),
                    ));
                }
                sprite
            }
            None => Sprite::default(),
        };
        exports.push(ExportFile {
            file,
            sidecar: sidecar.as_ref(),
            sheet,
            export,
            sprite,
        });
    }
    let mut fonts = Vec::with_capacity(plan.fonts.len());
    for file in &plan.fonts {
        let bytes = &take(&source, &mut reads, file)?.bytes;
        let font = bmfont::parse(utf8(bytes, file)?, Limits::default()).map_err(|error| {
            CookError::Decode {
                path: file.path.clone(),
                error,
            }
        })?;
        let pages = font
            .pages
            .iter()
            .map(|page| claim(&images, &mut claims, file, page, "page"))
            .collect::<Result<Vec<_>, _>>()?;
        fonts.push(FontFile { file, pages, font });
    }

    // What each folder makes, by the folder of the file that makes it.
    let mut made: Vec<(&str, &str, &Source, Made<'_>)> = Vec::new();
    for entry in &plan.images {
        if !claims.contains_key(&entry.0.path) {
            let file = &entry.0;
            made.push((
                folder(&file.path),
                stem(&file.path),
                file,
                Made::Image(entry),
            ));
        }
    }
    for (index, export) in exports.iter().enumerate() {
        let file = export.file;
        made.push((
            folder(&file.path),
            stem(&file.path),
            file,
            Made::Export(index),
        ));
    }
    for (index, font) in fonts.iter().enumerate() {
        let file = font.file;
        made.push((
            folder(&file.path),
            stem(&file.path),
            file,
            Made::Font(index),
        ));
    }

    // Which group each folder's files join.
    let mut joins: BTreeMap<String, String> = BTreeMap::new();
    let folders: BTreeSet<&str> = made.iter().map(|&(here, ..)| here).collect();
    let mut configs: BTreeMap<String, Source> = BTreeMap::new();
    for config in &plan.configs {
        let here = folder(&config.path);
        if !folders.contains(here) {
            report.warnings.push(format!(
                "`{}` is in a folder with no image, export or font, so it does nothing",
                config.found
            ));
            continue;
        }
        let bytes = &take(&source, &mut reads, config)?.bytes;
        let group = config::folder(bytes).map_err(|reason| CookError::Config {
            path: config.found.clone(),
            reason,
        })?;
        joins.insert(here.to_string(), group.unwrap_or_else(|| here.to_string()));
        configs.insert(here.to_string(), config.clone());
    }

    let mut groups: BTreeMap<String, Group<'_>> = BTreeMap::new();
    let mut sprites: BTreeMap<String, Sprite> = BTreeMap::new();
    let mut names: BTreeMap<&str, &Source> = BTreeMap::new();
    for &(here, name, file, what) in &made {
        if let Some(first) = names.insert(name, file) {
            return Err(CookError::SpriteNameTaken {
                name: name.to_string(),
                first: first.found.clone(),
                second: file.found.clone(),
            });
        }
        let group_name = joins.get(here).cloned().unwrap_or_else(|| here.to_string());
        let group = groups.entry(group_name).or_default();
        if let Some(config) = configs.get(here)
            && !group.configs.contains(config)
        {
            group.configs.push(config.clone());
        }
        match what {
            Made::Image((image, sidecar)) => {
                let sprite = match sidecar {
                    Some(sidecar) => {
                        let bytes = &take(&source, &mut reads, sidecar)?.bytes;
                        config::sprite(bytes).map_err(|reason| CookError::Config {
                            path: sidecar.found.clone(),
                            reason,
                        })?
                    }
                    None => Sprite::default(),
                };
                sprites.insert(image.path.clone(), sprite);
                group.images.push((image, sidecar.as_ref()));
            }
            Made::Export(index) => group.exports.push(index),
            Made::Font(index) => group.fonts.push(index),
        }
    }
    // The data files, by their jobs' names, which are their outputs'.
    let mut data: Vec<(String, &Source)> = plan
        .data
        .iter()
        .map(|file| (stem(&file.path).to_string(), file))
        .collect();
    data.sort();
    let font_names: Vec<&str> = fonts.iter().map(|font| stem(&font.file.path)).collect();
    let data_files: Vec<String> = data.iter().map(|(job, _)| format!("{job}.kbj")).collect();
    check_names(&groups, &sprites, &font_names, &data_files)?;

    // The LDtk projects, their level files, and what their levels draw and
    // spawn.
    let prefabs: BTreeMap<String, &Source> = plan
        .data
        .iter()
        .filter_map(|file| prefab_name(&file.path).map(|name| (name.to_string(), file)))
        .collect();
    let found = Found {
        source: &source,
        images: &images,
        claims: &claims,
        sprites: &sprites,
        joins: &joins,
        configs: &configs,
        prefabs: &prefabs,
    };
    let mut projects = projects(&found, &plan, &mut reads)?;
    // In name order, as the other jobs are; the scan refused names that
    // differ only in case, so no two are one.
    projects.sort_by(|a, b| stem(&a.file.path).cmp(stem(&b.file.path)));
    let prefab_names: BTreeSet<String> = prefabs.keys().cloned().collect();

    fs::create_dir_all(&resolved).map_err(|error| CookError::io(&resolved, "create", &error))?;
    let out =
        fs::canonicalize(&resolved).map_err(|error| CookError::io(&resolved, "open", &error))?;
    let previous = match fs::read(out.join(CACHE)) {
        Ok(bytes) => Cache::read(&bytes).unwrap_or_else(|why| {
            report.warnings.push(format!(
                "the cache `{CACHE}` cannot be used, so every job is cooked again: {why}"
            ));
            Cache::default()
        }),
        Err(_) => Cache::default(),
    };
    let fresh = !options.force && previous.cooker == options.cooker && previous.options == OPTIONS;
    let mut cache = Cache {
        cooker: options.cooker.clone(),
        options: OPTIONS.to_string(),
        jobs: BTreeMap::new(),
    };

    for (name, group) in &groups {
        let job = format!("{ATLAS}/{name}");
        // Each file is read once: its hash and what is cooked come from the
        // same bytes. The PNGs: its images, its exports' sheets and its
        // fonts' pages, each taken by one at most.
        let mut inputs: Hashes = Vec::new();
        let mut pngs: BTreeMap<&str, Vec<u8>> = BTreeMap::new();
        let read_pngs = group
            .images
            .iter()
            .map(|&(image, _)| image)
            .chain(group.exports.iter().map(|&index| exports[index].sheet))
            .chain(
                group
                    .fonts
                    .iter()
                    .flat_map(|&index| fonts[index].pages.iter().copied()),
            );
        for image in read_pngs {
            let bytes = read(&source.join(&image.found))?;
            inputs.push((image.path.clone(), fnv1a64(&bytes)));
            pngs.insert(&image.path, bytes);
        }
        // The files read before: sidecars, exports, fonts, settings.
        let others = group
            .images
            .iter()
            .filter_map(|&(_, sidecar)| sidecar)
            .chain(group.exports.iter().flat_map(|&index| {
                std::iter::once(exports[index].file).chain(exports[index].sidecar)
            }))
            .chain(group.fonts.iter().map(|&index| fonts[index].file))
            .chain(&group.configs);
        for file in others {
            inputs.push((file.path.clone(), reads[&file.path].hash));
        }
        inputs.sort();
        if let Some(kept) = kept(&previous, &job, &inputs, &out, fresh) {
            report.jobs.push(JobReport {
                name: job.clone(),
                cooked: false,
                files: kept.outputs.len(),
                counts: None,
            });
            cache.jobs.insert(job, kept.clone());
            continue;
        }

        let mut members: Vec<atlas::Member<'_>> = Vec::new();
        for &(image, _) in &group.images {
            members.push(atlas::Member {
                name: stem(&image.path),
                kind: atlas::Kind::Image {
                    png: (&image.path, &pngs[image.path.as_str()]),
                },
                sprite: sprites[&image.path],
            });
        }
        for &index in &group.exports {
            let export = &exports[index];
            members.push(atlas::Member {
                name: stem(&export.file.path),
                kind: atlas::Kind::Export {
                    path: &export.file.path,
                    export: &export.export,
                    sheet: (&export.sheet.path, &pngs[export.sheet.path.as_str()]),
                },
                sprite: export.sprite,
            });
        }
        for &index in &group.fonts {
            let font = &fonts[index];
            members.push(atlas::Member {
                name: stem(&font.file.path),
                kind: atlas::Kind::Font {
                    path: &font.file.path,
                    font: &font.font,
                    pages: font
                        .pages
                        .iter()
                        .map(|page| (page.path.as_str(), pngs[page.path.as_str()].as_slice()))
                        .collect(),
                },
                sprite: Sprite::default(),
            });
        }
        let cooked = atlas::cook(name, &members)?;
        report.warnings.extend(cooked.warnings);
        let mut outputs = Vec::with_capacity(cooked.files.len());
        for (path, bytes) in &cooked.files {
            write(&out, path, bytes)?;
            outputs.push((path.clone(), fnv1a64(bytes)));
        }
        report.jobs.push(JobReport {
            name: job.clone(),
            cooked: true,
            files: outputs.len(),
            counts: Some(Counts::Atlas {
                sprites: cooked.sprites,
                pages: cooked.pages,
                clips: cooked.clips,
                fonts: cooked.fonts,
            }),
        });
        cache.jobs.insert(job, Job { inputs, outputs });
    }

    // Each data file: JSONC, written as KBJ below `data/`, at its path.
    for (job, file) in data {
        let output = format!("{job}.kbj");
        let bytes = read(&source.join(&file.found))?;
        let inputs = vec![(file.path.clone(), fnv1a64(&bytes))];
        if let Some(kept) = kept(&previous, &job, &inputs, &out, fresh) {
            report.jobs.push(JobReport {
                name: job.clone(),
                cooked: false,
                files: kept.outputs.len(),
                counts: None,
            });
            cache.jobs.insert(job, kept.clone());
            continue;
        }
        let value = json::parse(utf8(&bytes, file)?, Limits::default()).map_err(|error| {
            CookError::Decode {
                path: file.path.clone(),
                error,
            }
        })?;
        if let Some(name) = prefab_name(&file.path) {
            let read = prefab::read(&value).map_err(|error| CookError::Decode {
                path: file.path.clone(),
                error,
            })?;
            if read.name != name {
                return Err(CookError::Config {
                    path: file.found.clone(),
                    reason: format!(
                        "the prefab `{}` is in the file of the prefab `{name}`: a prefab's file is named for it",
                        read.name
                    ),
                });
            }
        }
        let encoded =
            kbj::encode(&value, Limits::default()).map_err(|error| CookError::Encode {
                path: output.clone(),
                error,
            })?;
        write(&out, &output, &encoded)?;
        report.jobs.push(JobReport {
            name: job.clone(),
            cooked: true,
            files: 1,
            counts: Some(Counts::Data),
        });
        cache.jobs.insert(
            job,
            Job {
                inputs,
                outputs: vec![(output, fnv1a64(&encoded))],
            },
        );
    }

    // Each LDtk project: its levels, each written as a level file.
    for project in &projects {
        let job = format!("{LDTK}/{}", stem(&project.file.path));
        if let Some(kept) = kept(&previous, &job, &project.inputs, &out, fresh) {
            report.jobs.push(JobReport {
                name: job.clone(),
                cooked: false,
                files: kept.outputs.len(),
                counts: None,
            });
            cache.jobs.insert(job, kept.clone());
            continue;
        }
        let mut sizes: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
        for (path, bytes) in &project.pngs {
            let pixels =
                png::decode(bytes, atlas::source_limits()).map_err(|error| CookError::Decode {
                    path: path.clone(),
                    error,
                })?;
            sizes.insert(path, (pixels.width, pixels.height));
        }
        let mut tilesets = BTreeMap::new();
        for image in &project.images {
            let set = level::tileset(
                &project.file.found,
                image.def,
                &project.tilesets[image.def],
                (image.atlas.clone(), image.sprite.clone()),
                sizes[image.image.path.as_str()],
                image.tiles,
            )?;
            tilesets.insert(image.uid, set);
        }
        let cooked = level::cook(&project.levels, &tilesets, &prefab_names)?;
        report.warnings.extend(cooked.warnings);
        let mut outputs = Vec::with_capacity(cooked.files.len());
        for (path, bytes) in &cooked.files {
            write(&out, path, bytes)?;
            outputs.push((path.clone(), fnv1a64(bytes)));
        }
        report.jobs.push(JobReport {
            name: job.clone(),
            cooked: true,
            files: outputs.len(),
            counts: Some(Counts::Levels {
                levels: cooked.files.len(),
                entities: cooked.entities,
            }),
        });
        cache.jobs.insert(
            job,
            Job {
                inputs: project.inputs.clone(),
                outputs,
            },
        );
    }

    let written: BTreeSet<String> = cache
        .jobs
        .values()
        .flat_map(|job| job.outputs.iter().map(|(path, _)| path.clone()))
        .collect();
    for owned in [ATLAS, DATA, LEVELS] {
        remove_others(&out, owned, &written)?;
    }
    // The pack of what the cache lists, as the jobs left it: written only
    // when its bytes change, so that a cook that changes nothing leaves it
    // as it was.
    let mut packed = Vec::with_capacity(written.len());
    for path in &written {
        packed.push((path.clone(), read(&out.join(path))?));
    }
    let pack = kpak::write(&packed)?;
    if fs::read(out.join(PACK)).ok().as_deref() != Some(&pack[..]) {
        write(&out, PACK, &pack)?;
    }
    let bytes = cache.write().map_err(|error| CookError::Encode {
        path: CACHE.to_string(),
        error,
    })?;
    write(&out, CACHE, &bytes)?;
    Ok(report)
}

/// The cache's entry for `job`, if it may be kept: `fresh` (not forced, and
/// the same cooker and options), the same inputs, and its outputs still
/// there as it wrote them.
fn kept<'c>(
    previous: &'c Cache,
    job: &str,
    inputs: &Hashes,
    out: &Path,
    fresh: bool,
) -> Option<&'c Job> {
    previous.jobs.get(job).filter(|kept| {
        fresh
            && kept.inputs == *inputs
            && kept.outputs.iter().all(|(path, hash)| {
                fs::read(out.join(path)).is_ok_and(|bytes| fnv1a64(&bytes) == *hash)
            })
    })
}

/// `bytes` as text, or an error naming `file`.
fn utf8<'b>(bytes: &'b [u8], file: &Source) -> Result<&'b str, CookError> {
    std::str::from_utf8(bytes).map_err(|_| CookError::Config {
        path: file.found.clone(),
        reason: "it is not UTF-8".to_string(),
    })
}

/// The image `name` beside `by`, an export or a font that names it as its
/// `what` (sheet or page), taken for `by`: it must be a file name, of a PNG
/// the scan found, with no sidecar, that nothing else takes. Names are
/// matched as asset paths, so that a name's case does not matter.
fn claim<'p>(
    images: &BTreeMap<&str, &'p (Source, Option<Source>)>,
    claims: &mut BTreeMap<String, String>,
    by: &Source,
    name: &str,
    what: &str,
) -> Result<&'p Source, CookError> {
    let config = |reason| CookError::Config {
        path: by.found.clone(),
        reason,
    };
    if name.contains(['/', '\\']) {
        return Err(config(format!(
            "its {what} `{name}` must be a file name, of a PNG beside it"
        )));
    }
    let joined = format!("{}/{name}", folder(&by.path));
    let path = AssetPath::new(&joined)
        .map_err(|error| config(format!("its {what} `{name}` cannot be an asset: {error}")))?;
    let Some((image, sidecar)) = images.get(path.as_str()).map(|entry| (&entry.0, &entry.1)) else {
        return Err(config(format!(
            "its {what} `{name}` is not a PNG the cook found beside it"
        )));
    };
    if let Some(sidecar) = sidecar {
        return Err(CookError::Config {
            path: sidecar.found.clone(),
            reason: format!(
                "`{}` is the {what} of `{}`, so it takes no sidecar of its own",
                image.found, by.found
            ),
        });
    }
    if let Some(first) = claims.insert(image.path.clone(), by.found.clone()) {
        return Err(CookError::Claimed {
            path: image.found.clone(),
            first,
            second: by.found.clone(),
        });
    }
    Ok(image)
}

/// The name of the prefab of the data file `path`, an asset path: its stem,
/// if it is directly in `data/prefabs/`.
fn prefab_name(path: &str) -> Option<&str> {
    path.strip_prefix(PREFABS)
        .and_then(|name| name.strip_suffix(scan::DATA))
        .filter(|name| !name.contains('/'))
}

/// An error at the member `at` of `file`'s.
fn fault(file: &Source, at: &str, reason: impl std::fmt::Display) -> CookError {
    level::fault(&file.found, at, reason)
}

/// Reads the LDtk projects of `plan` and the level files they save their
/// levels in, checks what the cook refuses before it cooks, and finds what
/// each project's job reads: its level files, the images, sidecars and
/// folders' settings of the tile sets its levels draw, and the prefabs its
/// entities spawn.
///
/// # Errors
///
/// [`CookError::Decode`] for a project or level file the LDtk reader
/// refuses; [`CookError::Config`] for a project that saves files beside
/// itself, a level whose name cannot name its file or names another's, a
/// path that leaves the source folder or names a file the cook did not
/// find, a tile set that a level draws whose image is not a tile set, a
/// level file of the wrong level, of two levels, or of none.
fn projects<'p>(
    found: &Found<'_, 'p>,
    plan: &'p Plan,
    reads: &mut BTreeMap<String, Read>,
) -> Result<Vec<ProjectFile<'p>>, CookError> {
    let apart: BTreeMap<&str, &Source> = plan
        .ldtk_levels
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    // Each level file's project, as found, and each level file's level.
    let mut owners: BTreeMap<String, String> = BTreeMap::new();
    let mut outputs: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut projects = Vec::with_capacity(plan.projects.len());
    for file in &plan.projects {
        // Its job's name is an asset path, as the cache keeps it.
        let job = format!("{LDTK}/{}", stem(&file.path));
        if !AssetPath::new(&job).is_ok_and(|checked| checked.as_str() == job) {
            return Err(CookError::BadPath {
                path: file.found.clone(),
                reason: "its name without `.ldtk` ends in a dot or a space",
            });
        }
        let mut project = {
            let bytes = &take(found.source, reads, file)?.bytes;
            ldtk::parse_project(utf8(bytes, file)?, Limits::default()).map_err(|error| {
                CookError::Decode {
                    path: file.path.clone(),
                    error,
                }
            })?
        };
        // What LDtk would save beside the project, the cook would find.
        for (on, member, what, fix) in [
            (
                project.backup_on_save,
                "backupOnSave",
                "backups",
                "turn it off",
            ),
            (
                project.simplified_export,
                "simplifiedExport",
                "a simplified export",
                "turn it off",
            ),
            (
                project.image_export != ImageExport::None,
                "imageExportMode",
                "images of its layers or levels",
                "set it to `None`",
            ),
            (
                project.export_tiled,
                "exportTiled",
                "a Tiled export",
                "turn it off",
            ),
        ] {
            if on {
                return Err(fault(
                    file,
                    member,
                    format!(
                        "it saves {what} beside itself, which the cook would find among the sources: {fix}"
                    ),
                ));
            }
        }
        let mut inputs: BTreeMap<String, u64> = BTreeMap::new();
        inputs.insert(file.path.clone(), reads[&file.path].hash);

        let mut levels = Vec::with_capacity(project.levels.len());
        for (index, known) in project.levels.iter().enumerate() {
            let identifier = &known.identifier;
            let Some(output) = level::file_name(identifier) else {
                return Err(fault(
                    file,
                    &format!("levels[{index}].identifier"),
                    format!(
                        "the level `{identifier}` would be `levels/{}.klvl`: a level's name in snake_case is lowercase letters, digits and `_`, and names a file",
                        level::snake_case(identifier)
                    ),
                ));
            };
            if let Some((other, theirs)) =
                outputs.insert(output.clone(), (file.found.clone(), identifier.clone()))
            {
                return Err(fault(
                    file,
                    &format!("levels[{index}].identifier"),
                    format!(
                        "the level `{identifier}` would be `{output}`, as the level `{theirs}` of `{other}` would"
                    ),
                ));
            }
            let source = match &known.external_path {
                None => level::LevelSource {
                    found: file.found.clone(),
                    at: format!("levels[{index}]."),
                    output,
                    level: known.clone(),
                },
                Some(relative) => {
                    let at = format!("levels[{index}].externalRelPath");
                    let path = level::resolve(folder(&file.path), relative)
                        .map_err(|why| fault(file, &at, why))?;
                    let Some(&level_file) = apart.get(path.as_str()) else {
                        return Err(fault(
                            file,
                            &at,
                            format!(
                                "the level `{identifier}` is saved in `{relative}`, which is not a level file the cook found"
                            ),
                        ));
                    };
                    if let Some(first) = owners.insert(level_file.path.clone(), file.found.clone())
                    {
                        let reason = if first == file.found {
                            format!(
                                "two levels of `{first}` are saved here: a level file holds one level"
                            )
                        } else {
                            format!(
                                "`{first}` saves a level here, and so does `{}`: a level file holds one level",
                                file.found
                            )
                        };
                        return Err(fault(level_file, "", reason));
                    }
                    let read = take(found.source, reads, level_file)?;
                    inputs.insert(level_file.path.clone(), read.hash);
                    let level = ldtk::parse_level(
                        utf8(&read.bytes, level_file)?,
                        &project,
                        Limits::default(),
                    )
                    .map_err(|error| CookError::Decode {
                        path: level_file.path.clone(),
                        error,
                    })?;
                    if level.iid != known.iid {
                        return Err(fault(
                            level_file,
                            "iid",
                            format!(
                                "it is the level of iid {:?}, and `{}` saves its level `{identifier}`, of iid {:?}, here",
                                level.iid, file.found, known.iid
                            ),
                        ));
                    }
                    level::LevelSource {
                        found: level_file.found.clone(),
                        at: String::new(),
                        output,
                        level,
                    }
                }
            };
            levels.push(source);
        }

        // The tile sets the levels draw, and their images.
        let places: BTreeMap<i64, usize> = project
            .tilesets
            .iter()
            .enumerate()
            .map(|(index, tileset)| (tileset.uid, index))
            .collect();
        let drawn: BTreeSet<i64> = levels
            .iter()
            .flat_map(|source| source.level.layers.iter().flatten())
            .filter(|layer| level::drawn(layer))
            .filter_map(|layer| layer.tileset)
            .collect();
        let mut images = Vec::with_capacity(drawn.len());
        let mut pngs: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for uid in drawn {
            // The reader checked that each layer's tile set is defined.
            let Some(&def) = places.get(&uid) else {
                continue;
            };
            let tileset = &project.tilesets[def];
            let named = &tileset.identifier;
            let at = format!("defs.tilesets[{def}].relPath");
            let relative = match &tileset.rel_path {
                Some(relative) if !tileset.embedded => relative,
                _ => {
                    return Err(fault(
                        file,
                        &at,
                        format!(
                            "the tile set `{named}` has no image of the project's, and a layer the cook draws takes tiles of it"
                        ),
                    ));
                }
            };
            let path = level::resolve(folder(&file.path), relative)
                .map_err(|why| fault(file, &at, why))?;
            let Some((image, sidecar)) = found.images.get(path.as_str()).map(|e| (&e.0, &e.1))
            else {
                return Err(fault(
                    file,
                    &at,
                    format!(
                        "the image `{relative}` of the tile set `{named}` is not a PNG the cook found"
                    ),
                ));
            };
            if let Some(by) = found.claims.get(&image.path) {
                return Err(fault(
                    file,
                    &at,
                    format!(
                        "the image `{}` of the tile set `{named}` is the sheet or page of `{by}`, not a tile set",
                        image.found
                    ),
                ));
            }
            let tiles = match found.sprites.get(&image.path).map(|sprite| sprite.layout) {
                Some(Layout::Tiles(w, h)) => (w, h),
                _ => {
                    return Err(fault(
                        file,
                        &at,
                        format!(
                            "the image `{}` of the tile set `{named}` is not cooked as a tile set: give it a sidecar, `{}.sprite.jsonc`, of `{{ \"tiles\": [{size}, {size}] }}`",
                            image.found,
                            stem(&image.found),
                            size = tileset.tile_size
                        ),
                    ));
                }
            };
            let here = folder(&image.path);
            let group = found
                .joins
                .get(here)
                .cloned()
                .unwrap_or_else(|| here.to_string());
            if !pngs.contains_key(&image.path) {
                let png = read(&found.source.join(&image.found))?;
                inputs.insert(image.path.clone(), fnv1a64(&png));
                pngs.insert(image.path.clone(), png);
            }
            for file in sidecar.iter().chain(found.configs.get(here)) {
                inputs.insert(file.path.clone(), reads[&file.path].hash);
            }
            images.push(TileImage {
                uid,
                def,
                image,
                atlas: format!("{ATLAS}/{group}.kbj"),
                sprite: stem(&image.path).to_string(),
                tiles,
            });
        }

        // The prefabs the entities spawn, so that the level is cooked again
        // when one goes; the cook says which is missing.
        let spawned: BTreeSet<String> = levels
            .iter()
            .flat_map(|source| source.level.layers.iter().flatten())
            .flat_map(|layer| &layer.entities)
            .map(|entity| level::snake_case(&entity.identifier))
            .collect();
        for name in &spawned {
            if let Some(prefab) = found.prefabs.get(name) {
                let hash = take(found.source, reads, prefab)?.hash;
                inputs.insert(prefab.path.clone(), hash);
            }
        }

        projects.push(ProjectFile {
            file,
            tilesets: std::mem::take(&mut project.tilesets),
            levels,
            images,
            pngs,
            inputs: inputs.into_iter().collect(),
        });
    }
    for level_file in &plan.ldtk_levels {
        if !owners.contains_key(&level_file.path) {
            return Err(fault(
                level_file,
                "",
                "no LDtk project the cook found saves a level here: a level file is cooked with its project",
            ));
        }
    }
    Ok(projects)
}

/// A file's path without its extension: the name of the sprite it makes.
fn stem(path: &str) -> &str {
    path.rsplit_once('.').map_or(path, |(stem, _)| stem)
}

/// `path` made absolute, with the links in the part of it that exists
/// resolved, as `fs::canonicalize` would give it once it all exists.
fn resolve(path: &Path) -> Result<PathBuf, CookError> {
    let absolute =
        std::path::absolute(path).map_err(|error| CookError::io(path, "open", &error))?;
    let mut existing = absolute.as_path();
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => break,
        }
    }
    let mut resolved =
        fs::canonicalize(existing).map_err(|error| CookError::io(existing, "open", &error))?;
    resolved.extend(rest.iter().rev());
    Ok(resolved)
}

/// Reads `file` below `source` into `reads`, and gives it back.
fn take<'a>(
    source: &Path,
    reads: &'a mut BTreeMap<String, Read>,
    file: &Source,
) -> Result<&'a Read, CookError> {
    let bytes = read(&source.join(&file.found))?;
    let hash = fnv1a64(&bytes);
    Ok(reads
        .entry(file.path.clone())
        .or_insert(Read { bytes, hash }))
}

/// The folder a normalized path is in.
fn folder(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// Checks that no two of the groups' atlas files, the tile sets' pages, the
/// fonts' files (of the sprites `fonts`) and the data files' outputs
/// (`data`), nor the folders they are in, are one, or would be on a system
/// that ignores case.
fn check_names(
    groups: &BTreeMap<String, Group<'_>>,
    sprites: &BTreeMap<String, Sprite>,
    fonts: &[&str],
    data: &[String],
) -> Result<(), CookError> {
    let groups = groups.keys().map(|group| format!("{ATLAS}/{group}.kbj"));
    let tile_sets = sprites
        .iter()
        .filter(|(_, sprite)| matches!(sprite.layout, Layout::Tiles(..)))
        .map(|(path, _)| format!("{ATLAS}/{}.tiles.qoi", stem(path)));
    let fonts = fonts.iter().map(|name| atlas::font_file(name));
    // Each name and each folder it is in, by its folded form, with whether
    // it is a file: a folder may be named twice, but a file only once.
    let mut seen: BTreeMap<String, (String, bool)> = BTreeMap::new();
    for name in groups
        .chain(tile_sets)
        .chain(fonts)
        .chain(data.iter().cloned())
    {
        let folders = name
            .match_indices('/')
            .map(|(end, _)| (name[..end].to_string(), false));
        for (part, file) in folders.chain([(name.clone(), true)]) {
            match seen.get(&scan::fold(&part)) {
                Some((first, first_file)) if *first != part || file || *first_file => {
                    return Err(CookError::OutputCollision {
                        first: first.clone(),
                        second: part,
                    });
                }
                Some(_) => {}
                None => {
                    seen.insert(scan::fold(&part), (part, file));
                }
            }
        }
    }
    Ok(())
}

/// Removes every file below `out/<owned>` that `written` does not list, and
/// the folders left empty.
fn remove_others(out: &Path, owned: &str, written: &BTreeSet<String>) -> Result<(), CookError> {
    let root = out.join(owned);
    if !root.exists() {
        return Ok(());
    }
    let mut folders: Vec<PathBuf> = vec![root.clone()];
    let mut seen: Vec<PathBuf> = Vec::new();
    while let Some(dir) = folders.pop() {
        let entries = fs::read_dir(&dir).map_err(|error| CookError::io(&dir, "read", &error))?;
        for entry in entries {
            let entry = entry.map_err(|error| CookError::io(&dir, "read", &error))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|error| CookError::io(&path, "read", &error))?;
            if kind.is_dir() {
                folders.push(path);
                continue;
            }
            let name = path
                .strip_prefix(out)
                .ok()
                .and_then(Path::to_str)
                .map(|name| name.replace('\\', "/"));
            if name.is_none_or(|name| !written.contains(&name)) {
                fs::remove_file(&path).map_err(|error| CookError::io(&path, "remove", &error))?;
            }
        }
        seen.push(dir);
    }
    // The deepest first: a folder is seen after the one that holds it.
    for dir in seen.iter().rev() {
        if fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none()) {
            fs::remove_dir(dir).map_err(|error| CookError::io(dir, "remove", &error))?;
        }
    }
    Ok(())
}
