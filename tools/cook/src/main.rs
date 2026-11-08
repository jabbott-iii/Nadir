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

//! Command-line interface for the Nadir offline asset pipeline.
//!
//! `nadir-cook [--force] SOURCE OUT` cooks the files below `SOURCE` into
//! `OUT`. `cargo xtask cook` runs it on `assets/` and `target/cooked/`.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use nadir_codec::{Limits, kpak};
use nadir_cook::{Counts, Options, PACK, Report, SoundEncoding};
use nadir_core::cli::Args;
use nadir_core::hash::fnv1a64;

/// The exit code for a cook that failed.
const EXIT_FAILED: u8 = 1;
/// The exit code for a command line that was not understood.
const EXIT_USAGE: u8 = 2;

/// What `--version` prints, and the start of [`cooker`].
const VERSION: &str = concat!("nadir-cook ", env!("CARGO_PKG_VERSION"));

const USAGE: &str = "Usage: nadir-cook [--force] SOURCE OUT

Cooks the files below SOURCE into OUT: each folder's PNG images, Aseprite
exports and BMFont fonts into an atlas, `atlas/<group>_<n>.qoi` and
`atlas/<group>.kbj`, with each font's `atlas/<font>.font.kbj`; each WAV
below `audio/` into `audio/<name>.kpcm` or `audio/<name>.qoa`; each JSONC
file below `data/` into KBJ, `data/<name>.kbj`; each LDtk project's levels
into `levels/<name>.klvl`; and every file it writes into one pack,
`game.kpak`. A job whose files did not change since the last cook into OUT
is left as it is.

Options:
  --force    Cook every job, whatever the cache in OUT says
  --help     Show this
  --version  Show the version
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
    if args.flag("--version") {
        return say(
            &mut io::stdout(),
            &format!("{VERSION}\n"),
            ExitCode::SUCCESS,
        );
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
            let (mut text, warnings) = describe(&report, out);
            text.push_str(&describe_pack(Path::new(out)));
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
    let version = VERSION;
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
            Some(Counts::Levels { levels, entities }) => {
                let entities = match entities {
                    1 => "1 entity".to_string(),
                    n => format!("{n} entities"),
                };
                format!(
                    "cook: {}: {}, {entities}\n",
                    job.name,
                    count(levels, "level")
                )
            }
            Some(Counts::Sound {
                encoding,
                frames,
                sample_rate,
            }) => {
                let encoding = match encoding {
                    SoundEncoding::Qoa => "QOA",
                    _ => "PCM16",
                };
                let seconds = frames as f64 / f64::from(sample_rate.max(1));
                format!(
                    "cook: {}: {encoding}, {seconds:.2} s at {sample_rate} Hz\n",
                    job.name
                )
            }
            Some(_) => format!("cook: {}: cooked\n", job.name),
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

/// The pack's line: how many files it holds, in how many bytes, from its
/// header.
fn describe_pack(out: &Path) -> String {
    let header = File::open(out.join(PACK)).and_then(|mut file| {
        let size = file.metadata()?.len();
        let mut start = [0; kpak::HEADER_SIZE];
        file.read_exact(&mut start)?;
        Ok((kpak::header(&start, size, Limits::default()), size))
    });
    match header {
        Ok((Ok(header), size)) => format!(
            "cook: {PACK}: {} in {size} bytes\n",
            count(header.entry_count as usize, "file")
        ),
        // The cook read the pack back before it wrote it, so only
        // something else can have changed it since.
        _ => format!("cook: {PACK}: cannot be read\n"),
    }
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
                    name: "audio/music/theme".to_string(),
                    cooked: true,
                    files: 1,
                    counts: Some(Counts::Sound {
                        encoding: SoundEncoding::Qoa,
                        frames: 96_000,
                        sample_rate: 48_000,
                    }),
                },
                JobReport {
                    name: "audio/sfx/step".to_string(),
                    cooked: true,
                    files: 1,
                    counts: Some(Counts::Sound {
                        encoding: SoundEncoding::Pcm16,
                        frames: 11_025,
                        sample_rate: 22_050,
                    }),
                },
                JobReport {
                    name: "data/items".to_string(),
                    cooked: true,
                    files: 1,
                    counts: Some(Counts::Data),
                },
                JobReport {
                    name: "ldtk/levels/world".to_string(),
                    cooked: true,
                    files: 2,
                    counts: Some(Counts::Levels {
                        levels: 2,
                        entities: 1,
                    }),
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
             cook: audio/music/theme: QOA, 2.00 s at 48000 Hz\n\
             cook: audio/sfx/step: PCM16, 0.50 s at 22050 Hz\n\
             cook: data/items: cooked\n\
             cook: ldtk/levels/world: 2 levels, 1 entity\n\
             cook: 6 jobs cooked, 1 up to date, into target/cooked\n"
        );
        assert_eq!(
            warnings,
            "warning: `sprites/rock.png` has gAMA chunks\n\
             warning: 1 file not cooked:\n  sprites/atlas.qoi: no pipeline cooks it yet\n"
        );
    }

    #[test]
    fn the_pack_s_line_says_what_it_holds() {
        let root =
            std::env::temp_dir().join(format!("nadir-cook-pack-line-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (source, out) = (root.join("assets"), root.join("cooked"));
        std::fs::create_dir_all(source.join("data")).unwrap();
        std::fs::write(source.join("data/items.jsonc"), "[1, 2, 3]").unwrap();
        assert_eq!(describe_pack(&out), "cook: game.kpak: cannot be read\n");
        let options = Options {
            force: false,
            cooker: "test".to_string(),
        };
        nadir_cook::cook(&source, &out, &options).unwrap();
        let size = std::fs::metadata(out.join(PACK)).unwrap().len();
        assert_eq!(
            describe_pack(&out),
            format!("cook: game.kpak: 1 file in {size} bytes\n")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_cooker_is_this_build() {
        let cooker = cooker();
        assert!(cooker.starts_with("nadir-cook "), "{cooker}");
        assert_eq!(cooker, super::cooker(), "the same build each time");
    }
}
