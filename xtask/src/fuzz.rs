//! `cargo xtask fuzz-smoke`: throws mutated inputs at the parsers for a
//! while, and fails if one of them panics or hangs.
//!
//! This is the dependency-free "smoke" fuzzer of `docs/04-quality.md`. It
//! has no coverage feedback and keeps nothing between runs: every run
//! starts from the seed files in `fuzz/corpus/<target>/`.

mod engine;
mod mutate;

use std::env::{self, VarError};
use std::fmt::Display;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nadir_codec::{Limits, json};
use nadir_core::cli::Args;
use nadir_core::hash::fnv1a64;
use nadir_core::testing::{DEFAULT_SEED, SEED_VAR};

use self::engine::{Config, Finding, Kind, Outcome};
use crate::error::Error;

/// A parser that can be fuzzed.
struct Target {
    /// The name for `--target`, and of the corpus directory.
    name: &'static str,
    /// Hands one input to the parser. Rejecting the input is fine;
    /// panicking, or never returning, is a finding.
    run: fn(&[u8]),
}

/// Every fuzz target. A new parser adds itself here, with seeds in
/// `fuzz/corpus/<name>/` (`.claude/rules/parsers.md`).
const TARGETS: &[Target] = &[Target {
    name: "json",
    run: parse_json,
}];

/// How long each target is fuzzed when `--secs` is not given.
const DEFAULT_SECS: u64 = 60;

/// How long a target may spend on one input before that counts as a hang.
/// Parsing a small input takes microseconds, so this is far out of reach
/// of a busy machine, and still short next to a whole run.
const HANG_LIMIT: Duration = Duration::from_secs(10);

/// The longest input a mutation may produce.
const MAX_INPUT_LEN: usize = 64 * 1024;

/// Where the seeds of each target are, relative to the workspace root.
const CORPUS: &str = "fuzz/corpus";

/// Where failing inputs are saved, relative to the workspace root.
const CRASHES: &str = "fuzz/crashes";

/// The `json` target: `nadir_codec::json::parse`, with the limits the game
/// uses.
fn parse_json(input: &[u8]) {
    // The parser takes text. Bytes that are not UTF-8 become U+FFFD, so
    // that every input reaches it.
    let text = String::from_utf8_lossy(input);
    let _ = json::parse(&text, Limits::default());
}

/// Runs `fuzz-smoke` with the arguments that follow the command name.
/// Returns whether every target ran clean.
pub(crate) fn command(args: Args, root: &Path, out: &mut dyn Write) -> Result<bool, Error> {
    let seed = match env::var(SEED_VAR) {
        Ok(text) => Some(text),
        Err(VarError::NotPresent) => None,
        Err(VarError::NotUnicode(_)) => {
            return Err(Error::Usage(format!("{SEED_VAR} is not valid Unicode")));
        }
    };
    let options = Options::parse(args, TARGETS, seed.as_deref())?;
    let budget = Budget {
        max_inputs: u64::MAX,
        hang_limit: HANG_LIMIT,
    };
    fuzz(&options, root, budget, out)
}

/// The limits on one target's run that the command line does not set.
#[derive(Clone, Copy)]
struct Budget {
    /// Stop after this many inputs, if the time has not run out first.
    max_inputs: u64,
    /// How long the target may spend on one input.
    hang_limit: Duration,
}

/// What the command line and the environment asked for.
struct Options<'a> {
    targets: Vec<&'a Target>,
    /// How long each target is fuzzed.
    duration: Duration,
    seed: u64,
}

impl<'a> Options<'a> {
    /// Reads `[--secs N] [--target NAME]`. `seed` is the value of
    /// `NADIR_SEED`, if it is set.
    fn parse(mut args: Args, targets: &'a [Target], seed: Option<&str>) -> Result<Self, Error> {
        let usage = |error: nadir_core::CoreError| Error::Usage(error.to_string());
        let secs: Option<u64> = args.parsed("--secs").map_err(usage)?;
        let name = args.value("--target").map_err(usage)?;
        if let Some(argument) = args.finish().map_err(usage)?.first() {
            return Err(Error::Usage(format!(
                "`fuzz-smoke` takes only options, but got `{argument}`"
            )));
        }

        let chosen = match &name {
            None => targets.iter().collect(),
            Some(name) => match targets.iter().find(|target| target.name == name) {
                Some(target) => vec![target],
                None => {
                    let known: Vec<&str> = targets.iter().map(|target| target.name).collect();
                    return Err(Error::Usage(format!(
                        "unknown fuzz target `{name}`; the targets are: {}",
                        known.join(", ")
                    )));
                }
            },
        };
        let seed = match seed {
            None => DEFAULT_SEED,
            Some(text) => parse_seed(text).ok_or_else(|| {
                Error::Usage(format!(
                    "{SEED_VAR} must be a u64, in decimal or as 0x-prefixed hex, but it is {text:?}"
                ))
            })?,
        };
        Ok(Self {
            targets: chosen,
            duration: Duration::from_secs(secs.unwrap_or(DEFAULT_SECS)),
            seed,
        })
    }
}

/// Reads a seed the way `nadir_core::testing` does.
fn parse_seed(text: &str) -> Option<u64> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// Fuzzes each chosen target in turn and prints what happened.
fn fuzz(
    options: &Options<'_>,
    root: &Path,
    budget: Budget,
    out: &mut dyn Write,
) -> Result<bool, Error> {
    let output = Error::Output;
    writeln!(
        out,
        "fuzz-smoke: {}, {} s each, seed {:#x}",
        plural(options.targets.len(), "target"),
        options.duration.as_secs(),
        options.seed
    )
    .map_err(output)?;
    // The first lines are worth seeing before a long run, not after it.
    out.flush().map_err(output)?;

    let mut findings = 0;
    for target in &options.targets {
        let seeds = load_corpus(&root.join(CORPUS).join(target.name))?;
        let seed_count = seeds.len();
        let config = Config {
            // Each target draws from its own random stream, so adding a
            // target does not change what the others are given.
            seed: options.seed ^ fnv1a64(target.name.as_bytes()),
            duration: options.duration,
            max_inputs: budget.max_inputs,
            hang_limit: budget.hang_limit,
            max_len: MAX_INPUT_LEN,
            crashes: root.join(CRASHES).join(target.name),
        };
        let outcome = engine::run(target.run, seeds, config)?;
        out.write_all(describe(target.name, seed_count, &outcome, root).as_bytes())
            .map_err(output)?;
        out.flush().map_err(output)?;
        findings += outcome.findings.len();
        if outcome.hung {
            // The thread that hung is still busy, and stays so until this
            // process exits. Carrying on would only skew the other targets.
            break;
        }
    }

    if findings == 0 {
        writeln!(out, "fuzz-smoke: ok").map_err(output)?;
    } else {
        writeln!(
            out,
            "To see a finding again, copy its input into {CORPUS}/<target>/: seeds run first, \
             unchanged.\nfuzz-smoke: FAILED ({})",
            plural(findings, "finding")
        )
        .map_err(output)?;
    }
    Ok(findings == 0)
}

/// The lines that report one target's run.
fn describe(name: &str, seeds: usize, outcome: &Outcome, root: &Path) -> String {
    let seconds = outcome.elapsed.as_secs_f64();
    let rate = if seconds > 0.0 {
        outcome.inputs as f64 / seconds
    } else {
        0.0
    };
    let mut text = format!(
        "{name}: {}, {} in {seconds:.1} s ({rate:.0} per second)\n",
        plural(seeds, "seed"),
        plural(outcome.inputs, "input"),
    );
    for finding in &outcome.findings {
        text.push_str(&describe_finding(name, finding, root));
    }
    if outcome.hung {
        text.push_str(&format!("{name}: stopped early, because the target hung\n"));
    }
    text
}

fn describe_finding(name: &str, finding: &Finding, root: &Path) -> String {
    let what = match finding.kind {
        Kind::Panic if finding.site == finding.message => format!("panic: {}", finding.message),
        Kind::Panic => format!("panic at {}: {}", finding.site, finding.message),
        Kind::Hang => format!("hang: {}", finding.message),
    };
    let path = finding.path.strip_prefix(root).unwrap_or(&finding.path);
    format!(
        "finding: {name}: {what}\n         input saved to {} ({}), which is 1 of {} that did this\n",
        path.display(),
        plural(finding.len, "byte"),
        plural(finding.hits, "input"),
    )
}

/// `1 seed`, `2 seeds`.
fn plural<N: Display + PartialEq + From<u8>>(count: N, noun: &str) -> String {
    if count == N::from(1) {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Reads the seeds of one target: every file in `dir`, in name order.
/// Hidden files are skipped. A target without seeds cannot be fuzzed.
fn load_corpus(dir: &Path) -> Result<Vec<Vec<u8>>, Error> {
    let read_error = |source| Error::Read {
        path: dir.to_path_buf(),
        source,
    };
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        let hidden = entry.file_name().to_string_lossy().starts_with('.');
        if !hidden && entry.file_type().map_err(read_error)?.is_file() {
            files.push(entry.path());
        }
    }
    files.sort();

    let mut seeds = Vec::with_capacity(files.len());
    for path in files {
        match fs::read(&path) {
            Ok(seed) => seeds.push(seed),
            Err(source) => return Err(Error::Read { path, source }),
        }
    }
    if seeds.is_empty() {
        return Err(Error::NoSeeds {
            path: dir.to_path_buf(),
        });
    }
    Ok(seeds)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh workspace root for one test, under `target/tmp`, holding
    /// the given files (path relative to the root, and contents).
    fn root(test: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/tmp")
            .join(format!("xtask-fuzz-{test}"));
        // It may be left over from an earlier run.
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for (path, contents) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        root
    }

    fn accept(_: &[u8]) {}

    fn reject_long_inputs(input: &[u8]) {
        assert!(input.len() <= 40, "more than 40 bytes");
    }

    const FAKES: &[Target] = &[
        Target {
            name: "accept",
            run: accept,
        },
        Target {
            name: "short",
            run: reject_long_inputs,
        },
    ];

    /// A budget of `max_inputs` inputs, with no hang limit to speak of.
    fn inputs(max_inputs: u64) -> Budget {
        Budget {
            max_inputs,
            hang_limit: Duration::from_secs(3600),
        }
    }

    fn options<'a>(args: &[&str], seed: Option<&str>) -> Result<Options<'a>, Error> {
        Options::parse(Args::new(args.iter().copied()), FAKES, seed)
    }

    fn usage_error(args: &[&str], seed: Option<&str>) -> String {
        match options(args, seed) {
            Err(Error::Usage(message)) => message,
            Err(other) => panic!("expected a usage error, got {other:?}"),
            Ok(_) => panic!("expected a usage error for {args:?}"),
        }
    }

    /// Runs `fuzz` on fake targets and returns its verdict and output, with
    /// the measured time and rate replaced by `?`.
    fn fuzz_output(args: &[&str], root: &Path, max_inputs: u64) -> (bool, String) {
        let options = options(args, Some("7")).unwrap();
        let mut out = Vec::new();
        let passed = fuzz(&options, root, inputs(max_inputs), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<String> = text
            .lines()
            .map(|line| match line.split_once(" in ") {
                Some((counts, _)) if line.ends_with("per second)") => {
                    format!("{counts} in ? s (? per second)")
                }
                _ => line.to_string(),
            })
            .collect();
        (passed, lines.join("\n"))
    }

    #[test]
    fn options_default_to_every_target_for_a_minute_with_the_fixed_seed() {
        let options = options(&[], None).unwrap();
        let names: Vec<&str> = options.targets.iter().map(|target| target.name).collect();
        assert_eq!(names, ["accept", "short"]);
        assert_eq!(options.duration, Duration::from_secs(60));
        assert_eq!(options.seed, DEFAULT_SEED);
    }

    #[test]
    fn options_take_the_time_and_one_target() {
        let options = options(&["--target", "short", "--secs=5"], None).unwrap();
        let names: Vec<&str> = options.targets.iter().map(|target| target.name).collect();
        assert_eq!(names, ["short"]);
        assert_eq!(options.duration, Duration::from_secs(5));
        assert_eq!(
            self::options(&["--secs", "0"], None).unwrap().duration,
            Duration::ZERO
        );
    }

    #[test]
    fn options_take_the_seed_from_the_environment() {
        assert_eq!(options(&[], Some("12345")).unwrap().seed, 12345);
        assert_eq!(options(&[], Some(" 0x1F ")).unwrap().seed, 0x1f);
        assert_eq!(options(&[], Some("0XFF")).unwrap().seed, 0xff);
        assert_eq!(
            options(&[], Some("18446744073709551615")).unwrap().seed,
            u64::MAX
        );
        for bad in [
            "",
            "soon",
            "-1",
            "0x",
            "0xZZ",
            "18446744073709551616",
            "1.5",
        ] {
            assert_eq!(
                usage_error(&[], Some(bad)),
                format!(
                    "NADIR_SEED must be a u64, in decimal or as 0x-prefixed hex, but it is {bad:?}"
                )
            );
        }
    }

    #[test]
    fn bad_options_are_usage_errors() {
        assert_eq!(
            usage_error(&["--target", "yaml"], None),
            "unknown fuzz target `yaml`; the targets are: accept, short"
        );
        // Names are matched whole.
        assert!(usage_error(&["--target", "acc"], None).starts_with("unknown fuzz target `acc`"));
        assert_eq!(
            usage_error(&["--secs", "soon"], None),
            "invalid value `soon` for option `--secs`"
        );
        assert_eq!(
            usage_error(&["--secs", "-5"], None),
            "invalid value `-5` for option `--secs`"
        );
        assert_eq!(
            usage_error(&["--secs"], None),
            "option `--secs` needs a value"
        );
        assert_eq!(
            usage_error(&["--target"], None),
            "option `--target` needs a value"
        );
        assert_eq!(
            usage_error(&["--fast"], None),
            "unexpected argument `--fast`"
        );
        assert_eq!(
            usage_error(&["json"], None),
            "`fuzz-smoke` takes only options, but got `json`"
        );
    }

    #[test]
    fn the_real_targets_have_distinct_names_and_include_json() {
        let names: Vec<&str> = TARGETS.iter().map(|target| target.name).collect();
        assert!(names.contains(&"json"));
        for (index, name) in names.iter().enumerate() {
            assert!(!names[..index].contains(name), "{name} is listed twice");
        }
    }

    #[test]
    fn the_json_target_takes_any_bytes() {
        for input in [
            &b""[..],
            b"[1, 2, 3]",
            b"{\"a\": tru",
            b"\xff\xfe\x00[",
            &[b'['; 5000],
        ] {
            parse_json(input);
        }
    }

    #[test]
    fn every_real_target_has_seeds_and_accepts_them() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for target in TARGETS {
            let seeds = load_corpus(&workspace.join(CORPUS).join(target.name)).unwrap();
            assert!(seeds.len() >= 3, "{}", target.name);
            for seed in &seeds {
                (target.run)(seed);
            }
        }
    }

    #[test]
    fn the_json_seeds_are_documents_the_parser_accepts() {
        // A seed that is rejected right away makes a poor starting point.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(CORPUS)
            .join("json");
        let seeds = load_corpus(&dir).unwrap();
        let accepted = seeds
            .iter()
            .filter(|seed| {
                std::str::from_utf8(seed)
                    .is_ok_and(|text| json::parse(text, Limits::default()).is_ok())
            })
            .count();
        assert_eq!(accepted, seeds.len());
    }

    #[test]
    fn a_clean_run_reports_each_target_and_passes() {
        let root = root(
            "clean",
            &[
                ("fuzz/corpus/accept/a.json", b"[1]"),
                ("fuzz/corpus/accept/b.json", b"[2]"),
            ],
        );
        assert_eq!(
            fuzz_output(&["--target", "accept", "--secs", "30"], &root, 200),
            (
                true,
                "fuzz-smoke: 1 target, 30 s each, seed 0x7\n\
                 accept: 2 seeds, 200 inputs in ? s (? per second)\n\
                 fuzz-smoke: ok"
                    .to_string()
            )
        );
        assert!(!root.join("fuzz/crashes").exists());
    }

    #[test]
    fn a_run_with_no_time_runs_only_the_seeds() {
        // Each target has its own corpus directory.
        let root = root(
            "seeds-only",
            &[
                ("fuzz/corpus/accept/a.json", b"[1]"),
                ("fuzz/corpus/accept/b.json", b"[2]"),
                ("fuzz/corpus/short/only.json", b"{}"),
            ],
        );
        assert_eq!(
            fuzz_output(&["--secs", "0"], &root, u64::MAX),
            (
                true,
                "fuzz-smoke: 2 targets, 0 s each, seed 0x7\n\
                 accept: 2 seeds, 2 inputs in ? s (? per second)\n\
                 short: 1 seed, 1 input in ? s (? per second)\n\
                 fuzz-smoke: ok"
                    .to_string()
            )
        );
    }

    #[test]
    fn a_finding_fails_the_run_and_names_the_saved_input() {
        let root = root("finding", &[("fuzz/corpus/short/only.json", b"[1, 2, 3]")]);
        let (passed, output) = fuzz_output(&["--target", "short"], &root, 3000);
        assert!(!passed);

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 6, "{output}");
        assert_eq!(lines[0], "fuzz-smoke: 1 target, 60 s each, seed 0x7");
        assert_eq!(lines[1], "short: 1 seed, 3000 inputs in ? s (? per second)");
        assert!(
            lines[2].starts_with("finding: short: panic at xtask/src/fuzz.rs:")
                && lines[2].ends_with(": more than 40 bytes"),
            "{output}"
        );
        // The path is relative to the root, and the file is there.
        let (path, rest) = lines[3]
            .strip_prefix("         input saved to ")
            .and_then(|rest| rest.split_once(" ("))
            .unwrap();
        assert!(
            path.replace('\\', "/")
                .starts_with("fuzz/crashes/short/panic-")
                && path.ends_with(".bin"),
            "{output}"
        );
        let saved = fs::read(root.join(path)).unwrap();
        assert!(saved.len() > 40);
        assert!(
            rest.starts_with(&format!("{} bytes), which is 1 of ", saved.len()))
                && rest.ends_with(" inputs that did this"),
            "{output}"
        );
        assert_eq!(
            lines[4],
            "To see a finding again, copy its input into fuzz/corpus/<target>/: seeds run \
             first, unchanged."
        );
        assert_eq!(lines[5], "fuzz-smoke: FAILED (1 finding)");
    }

    #[test]
    fn a_saved_input_in_the_corpus_fails_at_once() {
        let long = [b'7'; 41];
        let root = root(
            "regression",
            &[
                ("fuzz/corpus/short/a.json", b"[1]"),
                ("fuzz/corpus/short/panic-0123.bin", &long),
            ],
        );
        let (passed, output) = fuzz_output(&["--target", "short", "--secs", "0"], &root, u64::MAX);
        assert!(!passed);
        assert!(output.contains("short: 2 seeds, 2 inputs in"), "{output}");
        assert!(
            output.contains("(41 bytes), which is 1 of 1 input that did this"),
            "{output}"
        );
    }

    #[test]
    fn findings_are_described_by_kind() {
        let root = Path::new("/work/nadir");
        let finding = |kind, site: &str, message: &str, hits| Finding {
            kind,
            site: site.to_string(),
            message: message.to_string(),
            path: root.join("fuzz/crashes/json/x.bin"),
            len: 1,
            hits,
        };
        let outcome = Outcome {
            inputs: 1500,
            elapsed: Duration::from_millis(2500),
            findings: vec![
                finding(Kind::Panic, "a.rs:1:2", "index out of bounds", 7),
                finding(Kind::Panic, "no place known", "no place known", 1),
                finding(Kind::Hang, "", "did not finish within 10 s", 1),
            ],
            hung: true,
        };
        let path = Path::new("fuzz/crashes/json/x.bin").display().to_string();
        assert_eq!(
            describe("json", 9, &outcome, root),
            format!(
                "json: 9 seeds, 1500 inputs in 2.5 s (600 per second)\n\
                 finding: json: panic at a.rs:1:2: index out of bounds\n         \
                 input saved to {path} (1 byte), which is 1 of 7 inputs that did this\n\
                 finding: json: panic: no place known\n         \
                 input saved to {path} (1 byte), which is 1 of 1 input that did this\n\
                 finding: json: hang: did not finish within 10 s\n         \
                 input saved to {path} (1 byte), which is 1 of 1 input that did this\n\
                 json: stopped early, because the target hung\n"
            )
        );
    }

    #[test]
    fn a_run_that_took_no_time_has_no_rate() {
        let outcome = Outcome {
            inputs: 3,
            elapsed: Duration::ZERO,
            findings: Vec::new(),
            hung: false,
        };
        assert_eq!(
            describe("json", 3, &outcome, Path::new(".")),
            "json: 3 seeds, 3 inputs in 0.0 s (0 per second)\n"
        );
    }

    #[test]
    fn the_corpus_is_read_in_name_order_without_hidden_files() {
        let root = root(
            "corpus-order",
            &[
                ("fuzz/corpus/accept/b.json", b"second"),
                ("fuzz/corpus/accept/a.json", b"first"),
                ("fuzz/corpus/accept/c", b"\xff\x00third"),
                ("fuzz/corpus/accept/.gitkeep", b""),
                ("fuzz/corpus/accept/nested/d.json", b"not a seed"),
            ],
        );
        assert_eq!(
            load_corpus(&root.join("fuzz/corpus/accept")).unwrap(),
            [&b"first"[..], b"second", b"\xff\x00third"]
        );
    }

    #[test]
    fn a_missing_or_empty_corpus_is_an_error() {
        let root = root("corpus-empty", &[("fuzz/corpus/accept/.gitkeep", b"")]);
        let empty = root.join("fuzz/corpus/accept");
        let error = load_corpus(&empty).unwrap_err();
        assert!(
            matches!(&error, Error::NoSeeds { path } if *path == empty),
            "{error:?}"
        );
        let missing = root.join("fuzz/corpus/short");
        let error = load_corpus(&missing).unwrap_err();
        assert!(
            matches!(&error, Error::Read { path, .. } if *path == missing),
            "{error:?}"
        );
        // And so the whole command fails, before any target has run.
        let options = options(&[], None).unwrap();
        let mut out = Vec::new();
        let error = fuzz(&options, &root, inputs(1), &mut out).unwrap_err();
        assert!(matches!(error, Error::NoSeeds { .. }), "{error:?}");
    }

    #[test]
    fn each_target_draws_from_its_own_random_stream() {
        use std::sync::Mutex;
        static FIRST: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        static SECOND: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        fn first(input: &[u8]) {
            FIRST.lock().unwrap().push(input.to_vec());
        }
        fn second(input: &[u8]) {
            SECOND.lock().unwrap().push(input.to_vec());
        }
        let targets = [
            Target {
                name: "first",
                run: first,
            },
            Target {
                name: "second",
                run: second,
            },
        ];
        let root = root(
            "streams",
            &[
                ("fuzz/corpus/first/seed", b"[1, 2, 3]"),
                ("fuzz/corpus/second/seed", b"[1, 2, 3]"),
            ],
        );
        let options = Options::parse(Args::new(Vec::<String>::new()), &targets, None).unwrap();
        let mut out = Vec::new();
        assert!(fuzz(&options, &root, inputs(100), &mut out).unwrap());
        // The same seeds and the same seed number, yet different inputs.
        assert_eq!(FIRST.lock().unwrap().len(), 100);
        assert_ne!(*FIRST.lock().unwrap(), *SECOND.lock().unwrap());
    }

    #[test]
    fn the_seed_decides_the_inputs() {
        use std::sync::Mutex;
        static SEEN: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        fn record(input: &[u8]) {
            SEEN.lock().unwrap().push(input.to_vec());
        }
        let targets = [Target {
            name: "record",
            run: record,
        }];
        let root = root("seed", &[("fuzz/corpus/record/seed", b"[1, 2, 3]")]);
        for seed in ["1", "2", "1"] {
            let args = Args::new(Vec::<String>::new());
            let options = Options::parse(args, &targets, Some(seed)).unwrap();
            let mut out = Vec::new();
            assert!(fuzz(&options, &root, inputs(60), &mut out).unwrap());
        }
        let seen = SEEN.lock().unwrap();
        assert_eq!(seen.len(), 180);
        let (first, second, third) = (&seen[..60], &seen[60..120], &seen[120..]);
        assert_eq!(first, third);
        assert_ne!(first, second);
    }

    #[test]
    fn a_hang_ends_the_run_before_the_remaining_targets() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static RELEASE: AtomicBool = AtomicBool::new(false);
        fn stuck(_: &[u8]) {
            while !RELEASE.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        fn after(_: &[u8]) {
            panic!("the target after the one that hung was run");
        }
        let targets = [
            Target {
                name: "stuck",
                run: stuck,
            },
            Target {
                name: "after",
                run: after,
            },
        ];
        let root = root(
            "hang",
            &[
                ("fuzz/corpus/stuck/seed", b"[1]"),
                ("fuzz/corpus/after/seed", b"[1]"),
            ],
        );
        let options = Options::parse(Args::new(Vec::<String>::new()), &targets, None).unwrap();
        let budget = Budget {
            max_inputs: 10,
            hang_limit: Duration::from_millis(40),
        };
        let mut out = Vec::new();
        let passed = fuzz(&options, &root, budget, &mut out);
        // Let the stuck thread go, whatever the assertions below say.
        RELEASE.store(true, Ordering::Relaxed);

        assert!(!passed.unwrap());
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 7, "{text}");
        assert!(
            lines[1].starts_with("stuck: 1 seed, 0 inputs in "),
            "{text}"
        );
        assert_eq!(
            lines[2],
            "finding: stuck: hang: did not finish within 0.04 s"
        );
        assert!(
            lines[3].ends_with("(3 bytes), which is 1 of 1 input that did this"),
            "{text}"
        );
        assert_eq!(lines[4], "stuck: stopped early, because the target hung");
        assert_eq!(lines[6], "fuzz-smoke: FAILED (1 finding)");
        assert!(!text.contains("after:"), "{text}");
        assert!(!root.join("fuzz/crashes/after").exists());
    }

    #[test]
    fn counts_are_spelled_out_in_the_singular_and_plural() {
        assert_eq!(plural(0_u64, "seed"), "0 seeds");
        assert_eq!(plural(1_u64, "seed"), "1 seed");
        assert_eq!(plural(2_usize, "seed"), "2 seeds");
    }
}
