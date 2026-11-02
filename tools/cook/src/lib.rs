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

//! Offline asset pipeline for the Nadir engine. This library does the
//! cooking; the `nadir-cook` binary is its command-line interface.
//!
//! So far (roadmap M5.4, M5.5), [`cook`] reads a source folder and writes,
//! into an output folder:
//!
//! - each atlas group's QOI pages and a KBJ of where each frame went, from
//!   its folder's PNG images, Aseprite exports (whose tags become clips)
//!   and BMFont fonts, with a file of each font's glyphs;
//! - each data file below `data/`, JSONC, as KBJ;
//! - and a cache that lets the next cook skip the jobs whose files did not
//!   change.
//!
//! Files no pipeline cooks yet are listed and left alone. The formats are
//! in `docs/subsystems/assets-and-codecs.md`, "`nadir-cook`".
//!
//! The output depends only on the source files: the same files cook to the
//! same bytes on every run and every system.

mod atlas;
mod cache;
mod config;
mod error;
mod image;
mod pack;
mod scan;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use nadir_assets::AssetPath;
use nadir_codec::{Limits, aseprite, bmfont, json, kbj};
use nadir_core::hash::fnv1a64;

use crate::cache::{Cache, Hashes, Job};
use crate::config::{Layout, Sprite};
use crate::scan::Source;

pub use error::CookError;

/// The name of the cache in the output folder.
pub const CACHE: &str = ".cache.kbj";

/// The folder of the output folder that the atlas pipeline owns: a file in
/// it that the cache does not list is removed.
const ATLAS: &str = "atlas";

/// The folder of the output folder that the data pipeline owns, as the
/// atlas pipeline owns [`ATLAS`].
const DATA: &str = "data";

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
    /// data file.
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

/// What a file of a folder makes: a sprite of an image, an export or a font.
#[derive(Clone, Copy)]
enum Made<'p> {
    Image(&'p (Source, Option<Source>)),
    Export(usize),
    Font(usize),
}

/// Cooks the files below `source` into `out`, which it makes if needed.
///
/// Each atlas group is a job, and so is each data file. A job is skipped
/// when the cache in `out` says the same cooker cooked the same files (by
/// their hashes) into files still there as it wrote them, unless
/// [`Options::force`] says to cook everything. Then every file below
/// `out/atlas` or `out/data` that no job wrote is removed, and the cache is
/// written.
///
/// # Errors
///
/// A [`CookError`] saying which file is wrong, or could not be read or
/// written. An error before the jobs are cooked leaves `out` as it was, and
/// makes no folder. One while they are leaves the files of the jobs cooked
/// before it written, and the cache as it was. One while stale files are
/// removed, or the cache is written, can leave some of the stale files
/// there.
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

    let written: BTreeSet<String> = cache
        .jobs
        .values()
        .flat_map(|job| job.outputs.iter().map(|(path, _)| path.clone()))
        .collect();
    for owned in [ATLAS, DATA] {
        remove_others(&out, owned, &written)?;
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
