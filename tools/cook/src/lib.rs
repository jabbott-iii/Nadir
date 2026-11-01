//! Offline asset pipeline for the Nadir engine. This library does the
//! cooking; the `nadir-cook` binary is its command-line interface.
//!
//! So far (roadmap M5.4), it packs PNG images into atlases: [`cook`] reads
//! a source folder and writes, into an output folder, each atlas group's
//! QOI pages and a KBJ of where each frame went, and a cache that lets the
//! next cook skip the groups whose files did not change. Files no pipeline
//! cooks yet are listed and left alone. The formats are in
//! `docs/subsystems/assets-and-codecs.md`, "`nadir-cook`".
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
    /// Its name: `atlas/<group>` for an atlas group.
    pub name: String,
    /// Whether it was cooked; otherwise its files were up to date.
    pub cooked: bool,
    /// How many files it wrote, or left as they were.
    pub files: usize,
    /// How many sprites and pages, when it was cooked.
    pub cooked_counts: Option<(usize, usize)>,
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

/// One atlas group's files.
#[derive(Default)]
struct Group {
    images: Vec<(Source, Option<Source>)>,
    configs: Vec<Source>,
}

/// Cooks the files below `source` into `out`, which it makes if needed.
///
/// Each atlas group is a job. A job is skipped when the cache in `out`
/// says the same cooker cooked the same files (by their hashes) into
/// files still there as it wrote them, unless [`Options::force`] says to
/// cook everything. Then every file below `out/atlas` that no job wrote is
/// removed, and the cache is written.
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
        skipped: plan.skipped,
        ..Report::default()
    };

    // Which group each folder's images join.
    let mut joins: BTreeMap<String, String> = BTreeMap::new();
    let mut reads: BTreeMap<String, Read> = BTreeMap::new();
    let folders: BTreeSet<&str> = plan
        .images
        .iter()
        .map(|(image, _)| folder(&image.path))
        .collect();
    let mut configs: BTreeMap<String, Source> = BTreeMap::new();
    for config in &plan.configs {
        let here = folder(&config.path);
        if !folders.contains(here) {
            report.warnings.push(format!(
                "`{}` is in a folder with no image, so it does nothing",
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

    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    let mut sprites: BTreeMap<String, Sprite> = BTreeMap::new();
    for (image, sidecar) in &plan.images {
        let here = folder(&image.path);
        let name = joins.get(here).cloned().unwrap_or_else(|| here.to_string());
        let group = groups.entry(name).or_default();
        if let Some(config) = configs.get(here)
            && !group.configs.contains(config)
        {
            group.configs.push(config.clone());
        }
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
        group.images.push((image.clone(), sidecar.clone()));
    }
    check_names(&groups, &sprites)?;

    fs::create_dir_all(&resolved).map_err(|error| CookError::io(&resolved, "create", &error))?;
    let out =
        fs::canonicalize(&resolved).map_err(|error| CookError::io(&resolved, "open", &error))?;
    let previous = match fs::read(out.join(CACHE)) {
        Ok(bytes) => Cache::read(&bytes).unwrap_or_else(|why| {
            report.warnings.push(format!(
                "the cache `{CACHE}` cannot be used, so every group is cooked again: {why}"
            ));
            Cache::default()
        }),
        Err(_) => Cache::default(),
    };
    let same_cooker = previous.cooker == options.cooker && previous.options == OPTIONS;
    let mut cache = Cache {
        cooker: options.cooker.clone(),
        options: OPTIONS.to_string(),
        jobs: BTreeMap::new(),
    };

    for (name, group) in &groups {
        let job = format!("{ATLAS}/{name}");
        // Each file is read once: its hash and what is cooked come from the
        // same bytes.
        let mut inputs: Hashes = Vec::new();
        let mut images: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for (image, _) in &group.images {
            let bytes = read(&source.join(&image.found))?;
            inputs.push((image.path.clone(), fnv1a64(&bytes)));
            images.insert(image.path.clone(), bytes);
        }
        let others = group
            .images
            .iter()
            .filter_map(|(_, sidecar)| sidecar.as_ref())
            .chain(&group.configs);
        for file in others {
            inputs.push((file.path.clone(), reads[&file.path].hash));
        }
        inputs.sort();

        let kept = previous.jobs.get(&job).filter(|kept| {
            !options.force
                && same_cooker
                && kept.inputs == inputs
                && kept.outputs.iter().all(|(path, hash)| {
                    fs::read(out.join(path)).is_ok_and(|bytes| fnv1a64(&bytes) == *hash)
                })
        });
        if let Some(kept) = kept {
            report.jobs.push(JobReport {
                name: job.clone(),
                cooked: false,
                files: kept.outputs.len(),
                cooked_counts: None,
            });
            cache.jobs.insert(job, kept.clone());
            continue;
        }

        let members: Vec<atlas::Member<'_>> = group
            .images
            .iter()
            .map(|(image, _)| atlas::Member {
                path: &image.path,
                png: &images[&image.path],
                sprite: sprites[&image.path],
            })
            .collect();
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
            cooked_counts: Some((cooked.sprites, cooked.pages)),
        });
        cache.jobs.insert(job, Job { inputs, outputs });
    }

    let written: BTreeSet<String> = cache
        .jobs
        .values()
        .flat_map(|job| job.outputs.iter().map(|(path, _)| path.clone()))
        .collect();
    remove_others(&out, &written)?;
    let bytes = cache.write().map_err(|error| CookError::Encode {
        path: CACHE.to_string(),
        error,
    })?;
    write(&out, CACHE, &bytes)?;
    Ok(report)
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

/// Checks that no two groups' atlas files or tile sets' pages, nor the
/// folders they are in, would be one on a system that ignores case.
fn check_names(
    groups: &BTreeMap<String, Group>,
    sprites: &BTreeMap<String, Sprite>,
) -> Result<(), CookError> {
    let groups = groups.keys().map(|group| format!("{ATLAS}/{group}.kbj"));
    let tile_sets = sprites
        .iter()
        .filter(|(_, sprite)| matches!(sprite.layout, Layout::Tiles(..)))
        .map(|(path, _)| {
            format!(
                "{ATLAS}/{}.tiles.qoi",
                path.strip_suffix(".png").unwrap_or(path)
            )
        });
    // Each name and each folder it is in, by its folded form.
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for name in groups.chain(tile_sets) {
        let folders = name
            .match_indices('/')
            .map(|(end, _)| name[..end].to_string());
        for part in folders.chain([name.clone()]) {
            match seen.get(&scan::fold(&part)) {
                Some(first) if *first != part => {
                    return Err(CookError::OutputCollision {
                        first: first.clone(),
                        second: part,
                    });
                }
                Some(_) => {}
                None => {
                    seen.insert(scan::fold(&part), part);
                }
            }
        }
    }
    Ok(())
}

/// Removes every file below `out/atlas` that `written` does not list, and
/// the folders left empty.
fn remove_others(out: &Path, written: &BTreeSet<String>) -> Result<(), CookError> {
    let root = out.join(ATLAS);
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
