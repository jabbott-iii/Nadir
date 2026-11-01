//! Command-line interface for the Nadir offline asset pipeline.
//!
//! `nadir-cook [--force] SOURCE OUT` cooks the files below `SOURCE` into
//! `OUT`. `cargo xtask cook` runs it on `assets/` and `target/cooked/`.

use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use nadir_cook::{Counts, Options, Report};
use nadir_core::cli::Args;
use nadir_core::hash::fnv1a64;

/// The exit code for a cook that failed.
const EXIT_FAILED: u8 = 1;
/// The exit code for a command line that was not understood.
const EXIT_USAGE: u8 = 2;

const USAGE: &str = "Usage: nadir-cook [--force] SOURCE OUT

Cooks the files below SOURCE into OUT: each folder's PNG images, Aseprite
exports and BMFont fonts into an atlas, `atlas/<group>_<n>.qoi` and
`atlas/<group>.kbj`, with each font's `atlas/<font>.font.kbj`; and each
JSONC file below `data/` into KBJ, `data/<name>.kbj`. A job whose files did
not change since the last cook into OUT is left as it is.

Options:
  --force  Cook every job, whatever the cache in OUT says
  --help   Show this
";

fn main() -> ExitCode {
    // A folder's name is used as given, never with bad bytes replaced.
    if std::env::args_os()
        .skip(1)
        .any(|arg| arg.to_str().is_none())
    {
        return usage("an argument is not valid Unicode");
    }
    let mut args = Args::from_env();
    if args.flag("--help") {
        return say(&mut io::stdout(), USAGE, ExitCode::SUCCESS);
    }
    let force = args.flag("--force");
    let paths = match args.finish() {
        Ok(paths) => paths,
        Err(error) => return usage(&error.to_string()),
    };
    let [source, out] = paths.as_slice() else {
        return usage("give the source folder and the output folder");
    };
    let options = Options {
        force,
        cooker: cooker(),
    };
    match nadir_cook::cook(Path::new(source), Path::new(out), &options) {
        Ok(report) => {
            let (text, warnings) = describe(&report, out);
            // If standard error is closed, there is nobody left to warn.
            let _ = io::stderr().write_all(warnings.as_bytes());
            say(&mut io::stdout(), &text, ExitCode::SUCCESS)
        }
        Err(error) => say(
            &mut io::stderr(),
            &format!("nadir-cook: {error}\n"),
            ExitCode::from(EXIT_FAILED),
        ),
    }
}

/// Writes `text` to `to`, and returns `code`, or a failure if it could not
/// be written.
fn say(to: &mut dyn Write, text: &str, code: ExitCode) -> ExitCode {
    match to.write_all(text.as_bytes()) {
        Ok(()) => code,
        Err(_) => ExitCode::from(EXIT_FAILED),
    }
}

fn usage(error: &str) -> ExitCode {
    say(
        &mut io::stderr(),
        &format!("nadir-cook: {error}\n\n{USAGE}"),
        ExitCode::from(EXIT_USAGE),
    )
}

/// The cooker: this version and a hash of this executable, so that a cook
/// built anew cooks everything again.
fn cooker() -> String {
    let version = concat!("nadir-cook ", env!("CARGO_PKG_VERSION"));
    let hash = std::env::current_exe()
        .and_then(std::fs::read)
        .map(|bytes| format!("{:016x}", fnv1a64(&bytes)));
    match hash {
        Ok(hash) => format!("{version} {hash}"),
        // Every cook then cooks everything, which is slow but right.
        Err(_) => format!("{version} unknown {}", std::process::id()),
    }
}

/// What to print: the jobs, then the warnings and the skipped files.
fn describe(report: &Report, out: &str) -> (String, String) {
    let mut text = String::new();
    let mut cooked = 0;
    for job in &report.jobs {
        let line = match job.counts {
            Some(Counts::Atlas {
                sprites,
                pages,
                clips,
                fonts,
            }) => {
                let mut made = format!("{} on {}", count(sprites, "sprite"), count(pages, "page"));
                for (n, thing) in [(clips, "clip"), (fonts, "font")] {
                    if n > 0 {
                        made.push_str(&format!(", {}", count(n, thing)));
                    }
                }
                format!("cook: {}: {made}\n", job.name)
            }
            Some(Counts::Data) => format!("cook: {}: cooked\n", job.name),
            None => format!("cook: {}: up to date\n", job.name),
        };
        if job.counts.is_some() {
            cooked += 1;
        }
        text.push_str(&line);
    }
    text.push_str(&format!(
        "cook: {} cooked, {} up to date, into {out}\n",
        count(cooked, "job"),
        report.jobs.len() - cooked
    ));
    let mut warnings = String::new();
    for warning in &report.warnings {
        warnings.push_str(&format!("warning: {warning}\n"));
    }
    if !report.skipped.is_empty() {
        warnings.push_str(&format!(
            "warning: {} not cooked:\n",
            count(report.skipped.len(), "file")
        ));
        for (path, why) in &report.skipped {
            warnings.push_str(&format!("  {path}: {why}\n"));
        }
    }
    (text, warnings)
}

/// `n things`, or `1 thing`.
fn count(n: usize, thing: &str) -> String {
    if n == 1 {
        format!("1 {thing}")
    } else {
        format!("{n} {thing}s")
    }
}

#[cfg(test)]
mod tests {
    use nadir_cook::JobReport;

    use super::*;

    #[test]
    fn a_report_names_each_job_then_what_was_not_cooked() {
        let report = Report {
            jobs: vec![
                JobReport {
                    name: "atlas/fonts".to_string(),
                    cooked: true,
                    files: 3,
                    counts: Some(Counts::Atlas {
                        sprites: 1,
                        pages: 1,
                        clips: 0,
                        fonts: 1,
                    }),
                },
                JobReport {
                    name: "atlas/sprites".to_string(),
                    cooked: true,
                    files: 3,
                    counts: Some(Counts::Atlas {
                        sprites: 5,
                        pages: 1,
                        clips: 2,
                        fonts: 0,
                    }),
                },
                JobReport {
                    name: "atlas/ui".to_string(),
                    cooked: false,
                    files: 2,
                    counts: None,
                },
                JobReport {
                    name: "data/items".to_string(),
                    cooked: true,
                    files: 1,
                    counts: Some(Counts::Data),
                },
            ],
            skipped: vec![(
                "sprites/atlas.qoi".to_string(),
                "no pipeline cooks it yet".to_string(),
            )],
            warnings: vec!["`sprites/rock.png` has gAMA chunks".to_string()],
        };
        let (text, warnings) = describe(&report, "target/cooked");
        assert_eq!(
            text,
            "cook: atlas/fonts: 1 sprite on 1 page, 1 font\n\
             cook: atlas/sprites: 5 sprites on 1 page, 2 clips\n\
             cook: atlas/ui: up to date\n\
             cook: data/items: cooked\n\
             cook: 3 jobs cooked, 1 up to date, into target/cooked\n"
        );
        assert_eq!(
            warnings,
            "warning: `sprites/rock.png` has gAMA chunks\n\
             warning: 1 file not cooked:\n  sprites/atlas.qoi: no pipeline cooks it yet\n"
        );
    }

    #[test]
    fn the_cooker_is_this_build() {
        let cooker = cooker();
        assert!(cooker.starts_with("nadir-cook "), "{cooker}");
        assert_eq!(cooker, super::cooker(), "the same build each time");
    }
}
