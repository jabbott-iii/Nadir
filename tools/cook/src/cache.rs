//! The incremental cache, `.cache.kbj` in the output folder: for each job,
//! the hashes of the files it read and of those it wrote.
//!
//! ```text
//! { "version": 1, "cooker": "nadir-cook 0.1.0 …", "options": "",
//!   "jobs": [ { "job": "atlas/sprites",
//!               "inputs": [["sprites/person.png", "<fnv1a64, 16 hex digits>"], …],
//!               "outputs": [["atlas/sprites.kbj", "…"], …] } ] }
//! ```
//!
//! Jobs are in name order, and their files in path order. Every path must
//! be an asset path as [`AssetPath`] writes it, and every output a `.qoi`
//! or `.kbj` below `atlas/`: a cache that says anything else is not used.

use std::collections::BTreeMap;

use nadir_assets::AssetPath;
use nadir_codec::json::Value;
use nadir_codec::{Limits, kbj};

/// The cache file's version.
const VERSION: i64 = 1;

/// Files and their hashes, by path, in order.
pub(crate) type Hashes = Vec<(String, u64)>;

/// What one job read and wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Job {
    pub(crate) inputs: Hashes,
    pub(crate) outputs: Hashes,
}

/// The cache: what made it, and its jobs by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cache {
    pub(crate) cooker: String,
    pub(crate) options: String,
    pub(crate) jobs: BTreeMap<String, Job>,
}

fn hex(hash: u64) -> Value {
    Value::String(format!("{hash:016x}"))
}

fn files(hashes: &Hashes) -> Value {
    Value::Array(
        hashes
            .iter()
            .map(|(path, hash)| Value::Array(vec![Value::String(path.clone()), hex(*hash)]))
            .collect(),
    )
}

/// Whether `path` is an asset path as written.
fn asset_path(path: &str) -> bool {
    AssetPath::new(path).is_ok_and(|checked| checked.as_str() == path)
}

/// Whether `path` is a file the cook may write.
pub(crate) fn output_path(path: &str) -> bool {
    asset_path(path)
        && path.starts_with("atlas/")
        && (path.ends_with(".qoi") || path.ends_with(".kbj"))
}

fn read_files(value: Option<&Value>, check: fn(&str) -> bool) -> Result<Hashes, String> {
    let list = value
        .and_then(Value::as_array)
        .ok_or("a job's files are not a list")?;
    let mut hashes = Vec::with_capacity(list.len());
    for item in list {
        let (path, hash) = match item.as_array() {
            Some([path, hash]) => (path.as_str(), hash.as_str()),
            _ => (None, None),
        };
        let (Some(path), Some(hash)) = (path, hash) else {
            return Err("a file is not a path and a hash".to_string());
        };
        if !check(path) {
            return Err(format!("`{path}` is not a path the cook would write down"));
        }
        let hash = (hash.len() == 16)
            .then(|| u64::from_str_radix(hash, 16).ok())
            .flatten()
            .ok_or_else(|| format!("`{hash}` is not a hash"))?;
        hashes.push((path.to_string(), hash));
    }
    if !hashes.windows(2).all(|pair| pair[0].0 < pair[1].0) {
        return Err("a job's files are not in order, or one is there twice".to_string());
    }
    Ok(hashes)
}

impl Cache {
    /// The cache from a file's bytes.
    ///
    /// # Errors
    ///
    /// Why it cannot be used.
    pub(crate) fn read(bytes: &[u8]) -> Result<Self, String> {
        let root = kbj::decode(bytes, Limits::default()).map_err(|error| error.to_string())?;
        if root.get("version").and_then(Value::as_i64) != Some(VERSION) {
            return Err("it is not version 1".to_string());
        }
        let text = |key: &str| {
            root.get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("`{key}` is not a string"))
        };
        let (cooker, options) = (text("cooker")?, text("options")?);
        let mut jobs = BTreeMap::new();
        for job in root
            .get("jobs")
            .and_then(Value::as_array)
            .ok_or("`jobs` is not a list")?
        {
            let name = job
                .get("job")
                .and_then(Value::as_str)
                .filter(|name| asset_path(name))
                .ok_or("a job has no name")?;
            let entry = Job {
                inputs: read_files(job.get("inputs"), asset_path)?,
                outputs: read_files(job.get("outputs"), output_path)?,
            };
            if jobs.insert(name.to_string(), entry).is_some() {
                return Err(format!("the job `{name}` is there twice"));
            }
        }
        Ok(Self {
            cooker,
            options,
            jobs,
        })
    }

    /// The file's bytes.
    pub(crate) fn write(&self) -> Result<Vec<u8>, nadir_codec::CodecError> {
        let jobs = self
            .jobs
            .iter()
            .map(|(name, job)| {
                Value::Object(vec![
                    ("job".to_string(), Value::String(name.clone())),
                    ("inputs".to_string(), files(&job.inputs)),
                    ("outputs".to_string(), files(&job.outputs)),
                ])
            })
            .collect();
        let root = Value::Object(vec![
            ("version".to_string(), Value::Int(VERSION)),
            ("cooker".to_string(), Value::String(self.cooker.clone())),
            ("options".to_string(), Value::String(self.options.clone())),
            ("jobs".to_string(), Value::Array(jobs)),
        ]);
        kbj::encode(&root, Limits::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> Cache {
        let mut jobs = BTreeMap::new();
        jobs.insert(
            "atlas/sprites".to_string(),
            Job {
                inputs: vec![
                    ("sprites/a.png".to_string(), 1),
                    ("sprites/a.sprite.jsonc".to_string(), u64::MAX),
                ],
                outputs: vec![
                    ("atlas/sprites.kbj".to_string(), 0x1234_5678_9abc_def0),
                    ("atlas/sprites_0.qoi".to_string(), 7),
                ],
            },
        );
        jobs.insert("atlas/ui".to_string(), Job::default());
        Cache {
            cooker: "nadir-cook test".to_string(),
            options: String::new(),
            jobs,
        }
    }

    /// The cache's value, changed by `change`, written.
    fn changed(change: impl FnOnce(&mut Value)) -> Vec<u8> {
        let mut value =
            kbj::decode(&cache().write().expect("written"), Limits::default()).expect("decoded");
        change(&mut value);
        kbj::encode(&value, Limits::default()).expect("encoded")
    }

    fn first_output(value: &mut Value) -> &mut Value {
        let Value::Object(root) = value else {
            panic!("an object")
        };
        let Value::Array(jobs) = &mut root[3].1 else {
            panic!("the jobs")
        };
        let Value::Object(job) = &mut jobs[0] else {
            panic!("a job")
        };
        let Value::Array(outputs) = &mut job[2].1 else {
            panic!("the outputs")
        };
        let Value::Array(file) = &mut outputs[0] else {
            panic!("a file")
        };
        &mut file[0]
    }

    #[test]
    fn a_cache_reads_back_as_it_was_written() {
        let bytes = cache().write().expect("written");
        assert_eq!(Cache::read(&bytes), Ok(cache()));
        assert_eq!(
            cache().write().expect("written"),
            bytes,
            "the same bytes each time"
        );
    }

    #[test]
    fn a_cache_that_names_a_file_outside_the_cook_s_own_is_not_used() {
        for path in [
            "../x.qoi",
            "/tmp/x.qoi",
            "atlas/../../x.qoi",
            "atlas/X.qoi",
            "atlas/x.png",
            "x.qoi",
            "C:/x.qoi",
            "atlas\\x.qoi",
        ] {
            let bytes = changed(|value| *first_output(value) = Value::String(path.to_string()));
            let result = Cache::read(&bytes);
            assert!(result.is_err(), "{path}: {result:?}");
        }
    }

    #[test]
    fn a_damaged_cache_says_why_it_is_not_used() {
        assert!(Cache::read(b"KBJ1").is_err());
        assert!(Cache::read(b"").is_err());
        let other = changed(|value| {
            let Value::Object(root) = value else {
                panic!("an object")
            };
            root[0].1 = Value::Int(2);
        });
        assert_eq!(Cache::read(&other), Err("it is not version 1".to_string()));
        let bad_hash = changed(|value| {
            let Value::Object(root) = value else {
                panic!("an object")
            };
            let Value::Array(jobs) = &mut root[3].1 else {
                panic!("the jobs")
            };
            let Value::Object(job) = &mut jobs[0] else {
                panic!("a job")
            };
            let Value::Array(outputs) = &mut job[2].1 else {
                panic!("the outputs")
            };
            let Value::Array(file) = &mut outputs[0] else {
                panic!("a file")
            };
            file[1] = Value::String("123".to_string());
        });
        assert_eq!(
            Cache::read(&bad_hash),
            Err("`123` is not a hash".to_string())
        );
    }
}
