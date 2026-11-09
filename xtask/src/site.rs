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

//! `cargo xtask site [--out DIR]`: builds the demo page that CI publishes
//! at <https://jabbott.dev/Nadir/> (ADR-0029), into `target/site` or
//! `DIR`.
//!
//! 1. Checks `docs/features.jsonc` as `cargo xtask features` does, and
//!    cooks `assets/` into `target/cooked` as `cargo xtask cook` does: the
//!    sandbox draws the cooked sprites (ADR-0033).
//! 2. Builds the sandbox in release and captures each of [`SCENES`]
//!    offscreen (`--capture`), which needs an offscreen OpenGL context,
//!    and no audio output: a capture plays no sound.
//! 3. Encodes each capture with GStreamer (`gst-launch-1.0`) into a VP9
//!    WebM and a PNG poster, at twice the size: every pixel of the game a
//!    square of four.
//! 4. Packs the sandbox and its cooked assets, the pack `game.kpak` of
//!    `target/cooked` in `assets/` (ADR-0041), with `LICENSE` and
//!    `NOTICE`, into `downloads/`, with `tar`, and takes its SHA-256 with
//!    `sha256sum`.
//! 5. Fills `site/index.html` from the commit (`git`), the workspace, the
//!    roadmap, the features and the captures, and `site/llms.txt`, the
//!    same as plain text for language models, and copies `site/nadir.css`
//!    and `site/nadir.js` beside them.
//!
//! It reads and writes only the workspace and the output directory, and
//! makes no network calls: the page loads jabbott.dev's stylesheet and
//! script when it is viewed. The output directory must be new, empty, or
//! one an earlier run made, which it marks with [`MARK`]; a run removes
//! only what an earlier one wrote ([`WRITTEN`]).

mod page;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use nadir_codec::Limits;
use nadir_codec::json::{self, Value};
use nadir_core::cli::Args;

use crate::error::Error;
use crate::features::{self, FEATURES, ROADMAP};
use crate::{cargo, metadata};

use self::page::{Build, Capture, Download, Milestone, Page, Recording, Scene};

/// Where the page goes, relative to the workspace root, without `--out`.
const OUTPUT: &str = "target/site";

/// The file that marks a directory as one `site` made, which a later run
/// may empty. Pages leaves out files whose names start with a dot.
const MARK: &str = ".nadir-site";

/// Everything `site` writes into its directory besides [`MARK`]: all that
/// a later run removes.
const WRITTEN: &[&str] = &[
    ".work",
    "media",
    "downloads",
    "index.html",
    "llms.txt",
    "nadir.css",
    "nadir.js",
];

/// The page's own files, relative to the workspace root.
const TEMPLATE: &str = "site/index.html";
const TEXT_TEMPLATE: &str = "site/llms.txt";
const OWN_FILES: &[&str] = &["site/nadir.css", "site/nadir.js"];

/// The cooked assets, relative to the workspace root, where
/// `cargo xtask cook` puts them.
pub(crate) const COOKED: &str = "target/cooked";

/// The pack the cook writes there, of every file it cooked, which the
/// download and the release files take, and the sandbox reads in release
/// (ADR-0041): `nadir_assets::PackSource::FILE_NAME`, whose crate `xtask`
/// may not link.
pub(crate) const PACK: &str = "game.kpak";

/// The repository, where `GITHUB_REPOSITORY` does not name it.
const REPOSITORY: &str = "jabbott-iii/Nadir";

/// The scenes the page shows, in the order of its tabs.
const SCENES: &[Scene] = &[
    Scene {
        name: "field",
        title: "The field",
        caption: "A person walks a field made in LDtk, of tiles, trees and rocks: down to the path and along it \
                  past the pond, down it past the flowers and through a gap in a row of \
                  tall walls, then along the face of one until it ends. The camera follows \
                  them. They walk in four frames, facing the way they go, and are drawn in \
                  front of the walls they slide along, whose feet are above theirs: sprites \
                  are y-sorted by their feet. The water ripples and the flowers sway: \
                  animated tiles.",
        poster_frame: 470,
    },
    Scene {
        name: "stress-sprites",
        title: "10,000 sprites",
        caption: "10,000 people walk about on a lawn of grass sprites, among trees and \
                  rocks, each an entity that the engine moves and animates. The lawn and \
                  everything on it are one page of the cooked atlas, y-sorted, so a frame is \
                  one batch: the budget is 10,000 sprites at 60 frames a second in at most 30 \
                  draw calls, with no allocation in a frame.",
        poster_frame: 300,
    },
    Scene {
        name: "stress-entities",
        title: "5,000 entities",
        caption: "5,000 people jostle in a maze of walls 128 tiles square. Each collides at \
                  their feet, and every tick the engine moves them and slides them along the \
                  walls and one another, through the collision crate's spatial hash; a tenth \
                  are triggers, which walk through the others and notice them. The budget \
                  for 5,000 entities with collision is 4 ms a tick on a min-spec machine.",
        poster_frame: 300,
    },
];

/// The frame rate the sandbox captures at, and the size of each frame: it
/// says so in `capture.json`, which must agree.
const CAPTURE_FPS: u32 = 60;

/// Runs `cargo xtask site`.
pub(crate) fn command(mut args: Args, root: &Path, out: &mut dyn Write) -> Result<bool, Error> {
    let usage = |error: nadir_core::CoreError| Error::Usage(error.to_string());
    let output = args
        .value("--out")
        .map_err(usage)?
        .map_or_else(|| root.join(OUTPUT), PathBuf::from);
    if let Some(argument) = args.finish().map_err(usage)?.first() {
        return Err(Error::Usage(format!(
            "`site` takes only `--out DIR`, but got `{argument}`"
        )));
    }
    // GStreamer runs in each capture's directory, so every path it is given
    // must be absolute.
    let output = std::path::absolute(&output).map_err(|source| Error::Read {
        path: output.clone(),
        source,
    })?;

    let report = features::run(root)?;
    if !report.violations.is_empty() {
        for line in report.notes.iter().chain(&report.violations) {
            writeln!(out, "site: {line}").map_err(Error::Output)?;
        }
        return Ok(false);
    }
    writeln!(out, "site: cooking {COOKED}").map_err(Error::Output)?;
    out.flush().map_err(Error::Output)?;
    cargo::run(root, &crate::cook_command(false))?;
    let roadmap = features::read(root, ROADMAP)?;
    let tasks = features::tasks(&roadmap);
    let features: Vec<_> = features::parse(&features::read(root, FEATURES)?)
        .map_err(|problems| Error::Internal(problems.join("\n")))?
        .into_iter()
        .map(|feature| {
            let status = features::status(&feature, &tasks);
            (feature, status)
        })
        .collect();

    prepare(&output)?;
    let work = output.join(".work");
    let media = output.join("media");
    for dir in [&work, &media] {
        make_dir(dir)?;
    }

    let sandbox = build_sandbox(root)?;
    let mut recordings = Vec::new();
    for scene in SCENES {
        writeln!(out, "site: capturing {}", scene.name).map_err(Error::Output)?;
        out.flush().map_err(Error::Output)?;
        let dir = work.join(scene.name);
        run(Command::new(&sandbox)
            .args(["--scene", scene.name, "--capture"])
            .arg(&dir))?;
        let text = read_text(&dir.join("capture.json"))?;
        let capture = parse_capture(&text).map_err(|detail| Error::UnexpectedOutput {
            command: format!("sandbox --scene {} --capture", scene.name),
            detail,
        })?;
        encode(&dir, scene, &capture, &media)?;
        // The frames take 300 MB a scene.
        remove(&dir.join(FRAMES))?;
        recordings.push(Recording { scene, capture });
    }

    let build = build_info(root)?;
    let download = package(root, &sandbox, &build, &work, &output.join("downloads"))?;
    let page = Page {
        build: &build,
        recordings: &recordings,
        download: &download,
        milestones: &milestones(&roadmap, &tasks),
        features: &features,
    };
    let html = page::render(&read_text(&root.join(TEMPLATE))?, &page).map_err(Error::Internal)?;
    write(&output.join("index.html"), html.as_bytes())?;
    let text = page::render_text(&read_text(&root.join(TEXT_TEMPLATE))?, &page)
        .map_err(Error::Internal)?;
    write(&output.join("llms.txt"), text.as_bytes())?;
    for file in OWN_FILES {
        let from = root.join(file);
        let name = from.file_name().map(PathBuf::from).unwrap_or_default();
        copy(&from, &output.join(name))?;
    }
    fs::remove_dir_all(&work).map_err(|source| Error::Write { path: work, source })?;
    writeln!(
        out,
        "site: wrote {} for {}: {} scenes, {} features",
        output.join("index.html").display(),
        build.short(),
        recordings.len(),
        features.len()
    )
    .map_err(Error::Output)?;
    Ok(true)
}

/// The frames a capture writes, in its directory.
const FRAMES: &str = "frames.rgba";

/// Builds the sandbox in release, and returns where its executable is.
fn build_sandbox(root: &Path) -> Result<PathBuf, Error> {
    const BUILD: &[&str] = &[
        "build",
        "--release",
        "--package",
        "sandbox",
        "--message-format",
        "json-render-diagnostics",
    ];
    let messages = cargo::capture(root, BUILD)?;
    executable(&messages, "sandbox").ok_or_else(|| Error::UnexpectedOutput {
        command: cargo::display(BUILD),
        detail: "no executable named `sandbox` among the artifacts".to_string(),
    })
}

/// The executable of the binary target `name`, from cargo's JSON messages,
/// one a line.
pub(crate) fn executable(messages: &str, name: &str) -> Option<PathBuf> {
    messages.lines().rev().find_map(|line| {
        let message = json::parse(line, Limits::default()).ok()?;
        if message.get("reason")?.as_str()? != "compiler-artifact"
            || message.get("target")?.get("name")?.as_str()? != name
        {
            return None;
        }
        message.get("executable")?.as_str().map(PathBuf::from)
    })
}

/// Reads `capture.json`.
fn parse_capture(text: &str) -> Result<Capture, String> {
    let root = json::parse(text, Limits::default()).map_err(|error| error.to_string())?;
    let number = |key: &str| {
        root.get(key)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && *n >= 0.0)
            .ok_or_else(|| format!("`{key}` is not a number of 0 or more"))
    };
    let whole = |key: &str| {
        let n = number(key)?;
        u32::try_from(n as u64)
            .ok()
            .filter(|&whole| f64::from(whole) == n)
            .ok_or_else(|| format!("`{key}` is not a whole number that fits in 32 bits"))
    };
    let text = |key: &str| {
        root.get(key)
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| format!("`{key}` is not a string"))
    };
    let capture = Capture {
        width: whole("width")?,
        height: whole("height")?,
        fps: whole("fps")?,
        frames: whole("frames")?,
        ticks: u64::from(whole("ticks")?),
        world_hash: text("world_hash")?,
        mean_sprites: number("mean_sprites")?,
        mean_batches: number("mean_batches")?,
        mean_tiles: number("mean_tiles")?,
        mean_tick_ms: number("mean_tick_ms")?,
        longest_tick_ms: number("longest_tick_ms")?,
        renderer: text("renderer")?,
    };
    if capture.width == 0 || capture.height == 0 || capture.frames == 0 {
        return Err("the capture has no frames".to_string());
    }
    if capture.fps != CAPTURE_FPS || capture.ticks != u64::from(capture.frames) {
        return Err(format!(
            "the capture ran {} ticks in {} frames at {} a second: one tick a frame, {CAPTURE_FPS} \
             a second, is what the page says",
            capture.ticks, capture.frames, capture.fps
        ));
    }
    Ok(capture)
}

/// Encodes the frames in `dir` into `media`: the scene's WebM and poster,
/// at twice the size. Each pixel of the frames becomes a square of four,
/// which 4:2:0 chroma keeps whole.
///
/// The colors are converted as GStreamer does by default, BT.601, which
/// browsers decode back within 2 levels; `colorimetry=bt709` came back 10
/// levels too green in Chromium. H.264 is left out: Ubuntu's openh264enc
/// ignored its bitrate, and wrote the crowd at 14 Mbps.
fn encode(dir: &Path, scene: &Scene, capture: &Capture, media: &Path) -> Result<(), Error> {
    let raw = |rate: u32| {
        format!(
            "rawvideoparse width={} height={} format=rgba framerate={rate}/1 ! videoscale \
             method=nearest-neighbour ! video/x-raw,width={},height={}",
            capture.width,
            capture.height,
            capture.width * 2,
            capture.height * 2
        )
    };
    let target = |extension: &str| media.join(format!("{}.{extension}", scene.name));
    gst(
        dir,
        &format!(
            "filesrc location={FRAMES} ! {} ! videoconvert ! video/x-raw,format=I420 ! vp9enc \
             deadline=1 cpu-used=4 end-usage=cq cq-level=20 target-bitrate=4000000 \
             row-mt=true ! webmmux",
            raw(capture.fps)
        ),
        &target("webm"),
    )?;
    // The poster: one frame, in a file of its own.
    let frame_bytes = u64::from(capture.width) * u64::from(capture.height) * 4;
    let frame = scene.poster_frame.min(capture.frames - 1);
    let poster = read_frame(
        &dir.join(FRAMES),
        u64::from(frame) * frame_bytes,
        frame_bytes,
    )?;
    write(&dir.join("poster.rgba"), &poster)?;
    gst(
        dir,
        &format!(
            "filesrc location=poster.rgba ! {} ! videoconvert ! pngenc",
            raw(1)
        ),
        &target("png"),
    )
}

/// Runs the GStreamer pipeline `pipeline`, ending in a file sink at
/// `file`, in `dir`.
fn gst(dir: &Path, pipeline: &str, file: &Path) -> Result<(), Error> {
    let mut words: Vec<String> = vec!["-q".to_string(), "-e".to_string()];
    words.extend(pipeline.split_whitespace().map(String::from));
    words.extend(["!".to_string(), "filesink".to_string()]);
    words.push(format!("location={}", file.display()));
    run(Command::new("gst-launch-1.0").args(&words).current_dir(dir))
}

/// The `length` bytes at `offset` in the file at `path`.
fn read_frame(path: &Path, offset: u64, length: u64) -> Result<Vec<u8>, Error> {
    use std::io::{Read, Seek, SeekFrom};
    let read = |source| Error::Read {
        path: path.to_path_buf(),
        source,
    };
    let mut file = fs::File::open(path).map_err(read)?;
    file.seek(SeekFrom::Start(offset)).map_err(read)?;
    let mut bytes = Vec::new();
    file.take(length).read_to_end(&mut bytes).map_err(read)?;
    if bytes.len() as u64 != length {
        return Err(read(io::Error::from(io::ErrorKind::UnexpectedEof)));
    }
    Ok(bytes)
}

/// Packs the sandbox's executable and its cooked assets (the [`PACK`] of
/// `target/cooked`, in `assets/`), with a note,
/// `LICENSE` and `NOTICE`, into a `.tar.gz` in `downloads`, by way of
/// `work`.
fn package(
    root: &Path,
    sandbox: &Path,
    build: &Build,
    work: &Path,
    downloads: &Path,
) -> Result<Download, Error> {
    let name = format!("nadir-sandbox-{}-{}", env::consts::OS, env::consts::ARCH);
    let staging = work.join("package");
    let dir = staging.join(&name);
    make_dir(&dir)?;
    let executable = fs::read(sandbox).map_err(|source| Error::Read {
        path: sandbox.to_path_buf(),
        source,
    })?;
    let glibc = glibc_floor(&executable);
    copy(sandbox, &dir.join("sandbox"))?;
    copy_pack(&root.join(COOKED), &dir.join("assets"))?;
    copy(&root.join("LICENSE"), &dir.join("LICENSE"))?;
    copy(&root.join("NOTICE"), &dir.join("NOTICE"))?;
    write(
        &dir.join("README.txt"),
        package_note(build, glibc.as_deref()).as_bytes(),
    )?;
    make_dir(downloads)?;
    let file = format!("{name}.tar.gz");
    let archive = downloads.join(&file);
    run(Command::new("tar")
        .args([
            "--sort=name",
            "--owner=0",
            "--group=0",
            "--numeric-owner",
            "-czf",
        ])
        .arg(&archive)
        .arg("-C")
        .arg(&staging)
        .arg(&name))?;
    let bytes = fs::metadata(&archive)
        .map_err(|source| Error::Read {
            path: archive.clone(),
            source,
        })?
        .len();
    let sums = output(Command::new("sha256sum").arg(&archive))?;
    let sha256 = sums
        .split_whitespace()
        .next()
        .filter(|sum| sum.len() == 64 && sum.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| Error::UnexpectedOutput {
            command: "sha256sum".to_string(),
            detail: format!("expected a SHA-256 first, but got `{}`", sums.trim()),
        })?
        .to_string();
    Ok(Download {
        file,
        bytes,
        sha256,
        glibc,
    })
}

/// The newest glibc symbol version an executable asks for, such as
/// `2.34`: the oldest glibc it runs on. Read from the version names
/// (`GLIBC_2.34`) in its bytes; `None` if it names none.
pub(crate) fn glibc_floor(executable: &[u8]) -> Option<String> {
    const NAME: &[u8] = b"GLIBC_";
    let mut newest: Option<Vec<u32>> = None;
    let mut rest = executable;
    while let Some(at) = rest.windows(NAME.len()).position(|window| window == NAME) {
        rest = &rest[at + NAME.len()..];
        let end = rest
            .iter()
            .position(|&byte| !(byte.is_ascii_digit() || byte == b'.'))
            .unwrap_or(rest.len());
        let version: Option<Vec<u32>> = std::str::from_utf8(&rest[..end])
            .ok()
            .filter(|text| !text.is_empty())
            .and_then(|text| text.split('.').map(|part| part.parse().ok()).collect());
        if let Some(version) = version
            && newest.as_ref().is_none_or(|newest| version > *newest)
        {
            newest = Some(version);
        }
    }
    newest.map(|parts| {
        parts
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".")
    })
}

/// The `README.txt` in the download of `build`, for an executable that
/// needs `glibc` or newer.
fn package_note(build: &Build, glibc: Option<&str>) -> String {
    let glibc = glibc.map_or(String::new(), |version| {
        format!(", and glibc {version} or newer")
    });
    format!(
        "\
Nadir sandbox
=============

The game that drives the Nadir engine's requirements, built by
`cargo xtask site` for the demo page at https://jabbott.dev/Nadir/, from
commit {commit} of {date}:
{repository}/commit/{commit}

Run it from this directory:

    ./sandbox

Its sprites, sounds, prefabs and level are in assets/game.kpak, the pack
`cargo xtask cook` made of the repository's assets/. WASD or the arrow
keys walk. Close the window to quit. The other scenes:

    ./sandbox --scene stress-sprites
    ./sandbox --scene stress-entities

It needs Linux on x86_64 with an X11 server (or XWayland), OpenGL 3.3
through EGL{glibc}. It plays its sound through ALSA
(libasound2), and is silent without it.

Its log is in ~/.local/share/Sandbox/nadir.log.

Licensed under the Apache License 2.0: see LICENSE and NOTICE.
",
        commit = build.commit,
        date = build.date,
        repository = build.repository,
    )
}

/// The commit at `HEAD` and the workspace at it.
fn build_info(root: &Path) -> Result<Build, Error> {
    let git = |args: &[&str]| output(Command::new("git").args(args).current_dir(root));
    let commit = git(&["rev-parse", "HEAD"])?.trim().to_string();
    let shown = git(&["show", "-s", "--format=%cs%n%cI%n%s", "HEAD"])?;
    let mut lines = shown.lines();
    let date = lines.next().unwrap_or_default().to_string();
    let time = lines.next().unwrap_or_default().to_string();
    let subject = lines.next().unwrap_or_default().to_string();
    let toolchain = toolchain(&read_text(&root.join("rust-toolchain.toml"))?)
        .ok_or_else(|| Error::Internal("rust-toolchain.toml names no `channel`".to_string()))?;
    let metadata = metadata::load(root)?;
    let crates = metadata.packages.iter().filter(|p| p.is_member).count();
    let repository = match env::var("GITHUB_REPOSITORY") {
        Ok(repository) if !repository.is_empty() => repository,
        _ => REPOSITORY.to_string(),
    };
    Ok(Build {
        commit,
        date,
        time,
        subject,
        toolchain,
        crates,
        third_party: metadata.packages.len() - crates,
        repository: format!("https://github.com/{repository}"),
    })
}

/// The `channel` that `rust-toolchain.toml` pins.
fn toolchain(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let value = line.trim().strip_prefix("channel")?.trim_start();
        let value = value.strip_prefix('=')?.trim();
        Some(value.trim_matches('"').to_string())
    })
}

/// The roadmap's milestones, its `## M5 — Title` headings, with how many
/// of their tasks are ticked.
fn milestones(roadmap: &str, tasks: &[features::Task]) -> Vec<Milestone> {
    roadmap
        .lines()
        .filter_map(|line| {
            let (id, title) = line.strip_prefix("## ")?.split_once(" — ")?;
            let number = id.strip_prefix('M')?;
            if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let prefix = format!("{id}.");
            let own: Vec<_> = tasks
                .iter()
                .filter(|task| task.id.starts_with(&prefix))
                .collect();
            Some(Milestone {
                id: id.to_string(),
                title: title.to_string(),
                done: own.iter().filter(|task| task.done).count(),
                total: own.len(),
            })
        })
        .collect()
}

/// Runs `command`, which prints to the terminal.
pub(crate) fn run(command: &mut Command) -> Result<(), Error> {
    let line = describe(command);
    let status = command
        .stdin(Stdio::null())
        .status()
        .map_err(|source| Error::Spawn {
            command: line.clone(),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::CommandFailed {
            command: line,
            status,
            stderr: String::new(),
        })
    }
}

/// Runs `command`, and returns what it wrote to standard output.
pub(crate) fn output(command: &mut Command) -> Result<String, Error> {
    let line = describe(command);
    let result = command
        .stdin(Stdio::null())
        .output()
        .map_err(|source| Error::Spawn {
            command: line.clone(),
            source,
        })?;
    if !result.status.success() {
        return Err(Error::CommandFailed {
            command: line,
            status: result.status,
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
        });
    }
    String::from_utf8(result.stdout).map_err(|_| Error::UnexpectedOutput {
        command: line,
        detail: "the output is not valid UTF-8".to_string(),
    })
}

/// `command` as a line, for messages.
fn describe(command: &Command) -> String {
    let mut line = command.get_program().to_string_lossy().into_owned();
    for arg in command.get_args() {
        line.push(' ');
        line.push_str(&arg.to_string_lossy());
    }
    line
}

/// Makes `output` ready for the page: a new or empty directory, or one an
/// earlier run made, without what that run wrote. Refuses any other
/// directory, so that `--out` never deletes what this tool did not write.
fn prepare(output: &Path) -> Result<(), Error> {
    match fs::read_dir(output) {
        Ok(mut entries) => {
            if entries.next().is_some() && !output.join(MARK).is_file() {
                return Err(Error::Usage(format!(
                    "`{}` is not empty, and `cargo xtask site` did not make it: give a new or \
                     empty directory",
                    output.display()
                )));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Read {
                path: output.to_path_buf(),
                source,
            });
        }
    }
    for name in WRITTEN {
        let path = output.join(name);
        let removed = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        match removed {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                return Err(Error::Write {
                    path,
                    source: error,
                });
            }
            _ => {}
        }
    }
    make_dir(output)?;
    write(
        &output.join(MARK),
        b"Made by `cargo xtask site`, which empties this directory of what it wrote \
          when it runs again.\n",
    )
}

pub(crate) fn make_dir(dir: &Path) -> Result<(), Error> {
    fs::create_dir_all(dir).map_err(|source| Error::Write {
        path: dir.to_path_buf(),
        source,
    })
}

pub(crate) fn remove(file: &Path) -> Result<(), Error> {
    fs::remove_file(file).map_err(|source| Error::Write {
        path: file.to_path_buf(),
        source,
    })
}

pub(crate) fn read_text(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    fs::write(path, bytes).map_err(|source| Error::Write {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn copy(from: &Path, to: &Path) -> Result<(), Error> {
    fs::copy(from, to).map(drop).map_err(|source| Error::Write {
        path: to.to_path_buf(),
        source,
    })
}

/// Copies the [`PACK`] in `cooked` into `assets`, as the sandbox reads
/// it from beside its executable: the pack alone, not the loose files the
/// cook wrote beside it, or its cache.
pub(crate) fn copy_pack(cooked: &Path, assets: &Path) -> Result<(), Error> {
    make_dir(assets)?;
    copy(&cooked.join(PACK), &assets.join(PACK))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPTURE: &str = r#"{
  "scene": "field",
  "width": 480,
  "height": 270,
  "fps": 60,
  "frames": 600,
  "ticks": 600,
  "world_hash": "cba9d2bc0882b423",
  "mean_sprites": 57.0,
  "most_sprites": 57,
  "mean_batches": 17.7,
  "mean_tiles": 2210.1,
  "mean_tick_ms": 0.004,
  "longest_tick_ms": 0.045,
  "renderer": "llvmpipe (LLVM 20.1.2, 256 bits)"
}"#;

    #[test]
    fn a_capture_is_read_and_checked() {
        let capture = parse_capture(CAPTURE).expect("valid");
        assert_eq!(
            (capture.width, capture.height, capture.frames),
            (480, 270, 600)
        );
        assert_eq!(capture.world_hash, "cba9d2bc0882b423");
        assert_eq!(capture.mean_batches, 17.7);
        assert_eq!(capture.renderer, "llvmpipe (LLVM 20.1.2, 256 bits)");
        let wrong = [
            ("\"ticks\": 600", "\"ticks\": 1200", "one tick a frame"),
            ("\"fps\": 60", "\"fps\": 30", "one tick a frame"),
            (
                "\"width\": 480",
                "\"width\": 480.5",
                "`width` is not a whole number",
            ),
            ("\"frames\": 600", "\"frames\": 0", "no frames"),
            (
                "\"mean_tiles\": 2210.1",
                "\"mean_tiles\": -1",
                "`mean_tiles` is not a number",
            ),
            (
                "\"world_hash\": \"cba9d2bc0882b423\"",
                "\"world_hash\": 7",
                "`world_hash` is not",
            ),
        ];
        for (from, to, why) in wrong {
            let text = CAPTURE.replace(from, to);
            let error = parse_capture(&text).expect_err(to);
            assert!(error.contains(why), "{to}: {error}");
        }
        assert!(parse_capture("{").is_err());
    }

    #[test]
    fn the_sandbox_is_found_among_cargos_messages() {
        let messages = concat!(
            r#"{"reason":"compiler-artifact","target":{"name":"nadir"},"executable":null}"#,
            "\n",
            r#"{"reason":"compiler-artifact","target":{"name":"sandbox"},"executable":"/t/release/sandbox"}"#,
            "\n",
            r#"{"reason":"build-finished","success":true}"#,
            "\n",
        );
        assert_eq!(
            executable(messages, "sandbox"),
            Some(PathBuf::from("/t/release/sandbox"))
        );
        assert_eq!(executable(messages, "nadir"), None);
        assert_eq!(executable("not json\n", "sandbox"), None);
    }

    #[test]
    fn the_toolchain_is_the_pinned_channel() {
        let text = "[toolchain]\nchannel = \"1.99.0\"\ncomponents = [\"rustfmt\"]\n";
        assert_eq!(toolchain(text), Some("1.99.0".to_string()));
        assert_eq!(toolchain("[toolchain]\n"), None);
        let pinned = include_str!("../../rust-toolchain.toml");
        assert!(toolchain(pinned).is_some_and(|channel| !channel.is_empty()));
    }

    #[test]
    fn milestones_count_their_ticked_tasks() {
        let roadmap = "\
## M0 — Foundation and guardrails

- [x] **M0.1 — Workspace skeleton**
- [x] **M0.2 — core: math and color**

## M1 — A window and events on every desktop

- [x] **M1.1 — sys: dynlib**
- [ ] **M1.5 — sys + platform (Windows)**

## Backlog (M12+, each needs a trigger or a decision)
";
        let tasks = features::tasks(roadmap);
        let found = milestones(roadmap, &tasks);
        assert_eq!(
            found,
            [
                Milestone {
                    id: "M0".to_string(),
                    title: "Foundation and guardrails".to_string(),
                    done: 2,
                    total: 2,
                },
                Milestone {
                    id: "M1".to_string(),
                    title: "A window and events on every desktop".to_string(),
                    done: 1,
                    total: 2,
                },
            ]
        );
    }

    #[test]
    fn every_scene_is_one_the_sandbox_has_and_its_poster_is_captured() {
        let sandbox = include_str!("../../games/sandbox/src/main.rs");
        for scene in SCENES {
            assert!(
                sandbox.contains(&format!("\"{}\" => Some(Scene::", scene.name)),
                "the sandbox has no scene `{}`",
                scene.name
            );
            assert!(scene.poster_frame < 600, "{}", scene.name);
        }
    }

    /// A fresh directory for test `name`, in the system's temporary folder.
    fn scratch(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("nadir-xtask-site-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_output_must_be_new_empty_or_made_by_an_earlier_run() {
        let dir = scratch("prepare");
        // New, then empty: made, and marked.
        let new = dir.join("new");
        prepare(&new).unwrap();
        assert!(new.join(MARK).is_file());
        let empty = dir.join("empty");
        fs::create_dir(&empty).unwrap();
        prepare(&empty).unwrap();
        assert!(empty.join(MARK).is_file());

        // Not empty, and not made by `site`: refused, and left alone.
        let other = dir.join("other");
        fs::create_dir_all(other.join("src")).unwrap();
        fs::write(other.join("index.html"), "mine").unwrap();
        match prepare(&other) {
            Err(Error::Usage(message)) => assert!(message.contains("is not empty"), "{message}"),
            result => panic!("expected a refusal, got {result:?}"),
        }
        assert_eq!(
            fs::read_to_string(other.join("index.html")).unwrap(),
            "mine"
        );
        assert!(other.join("src").is_dir());

        // Made by `site`: what it wrote goes, and nothing else.
        for name in WRITTEN {
            if name.contains('.') && *name != ".work" {
                fs::write(new.join(name), "old").unwrap();
            } else {
                fs::create_dir_all(new.join(name).join("deep")).unwrap();
            }
        }
        fs::write(new.join("notes.txt"), "kept").unwrap();
        prepare(&new).unwrap();
        for name in WRITTEN {
            assert!(!new.join(name).exists(), "{name} is still there");
        }
        assert_eq!(fs::read_to_string(new.join("notes.txt")).unwrap(), "kept");
        assert!(new.join(MARK).is_file());

        // A file is not a directory to write into.
        let file = dir.join("file");
        fs::write(&file, "x").unwrap();
        assert!(matches!(prepare(&file), Err(Error::Read { .. })));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_download_takes_the_pack_alone() {
        let dir = scratch("cooked");
        let cooked = dir.join("cooked");
        for (path, text) in [
            ("game.kpak", "pack"),
            ("atlas/sprites.kbj", "atlas"),
            ("data/prefabs/tree.kbj", "tree"),
            ("levels/field.klvl", "field"),
            (".cache.kbj", "cache"),
        ] {
            let file = cooked.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }
        let assets = dir.join("staged/assets");
        copy_pack(&cooked, &assets).unwrap();
        let found: Vec<_> = fs::read_dir(&assets)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(found, ["game.kpak"]);
        assert_eq!(fs::read(assets.join("game.kpak")).unwrap(), b"pack");
        // A cook that wrote no pack is an error.
        fs::remove_file(cooked.join("game.kpak")).unwrap();
        assert!(copy_pack(&cooked, &dir.join("again")).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    fn build() -> Build {
        Build {
            commit: "0123456789abcdef".to_string(),
            date: "2026-10-31".to_string(),
            time: "2026-10-31T21:28:48-07:00".to_string(),
            subject: "A change".to_string(),
            toolchain: "1.99.0".to_string(),
            crates: 18,
            third_party: 0,
            repository: "https://github.com/jabbott-iii/Nadir".to_string(),
        }
    }

    #[test]
    fn the_glibc_floor_is_the_newest_version_the_executable_names() {
        let bytes = b"\0GLIBC_2.2.5\0GLIBC_2.34\0GLIBC_PRIVATE\0GLIBC_2.3.4\0GLIBC_\0GLIBC_2.18";
        assert_eq!(glibc_floor(bytes), Some("2.34".to_string()));
        assert_eq!(
            glibc_floor(b"GLIBC_2.9\0GLIBC_2.10"),
            Some("2.10".to_string())
        );
        assert_eq!(glibc_floor(b"no versions here"), None);
        assert_eq!(glibc_floor(b"GLIBC_PRIVATE"), None);
        let floored = package_note(&build(), Some("2.34"));
        assert!(
            floored.contains(
                "OpenGL 3.3\nthrough EGL, and glibc 2.34 or newer. It plays its sound \
                 through ALSA\n"
            ),
            "{floored}"
        );
        let note = package_note(&build(), None);
        assert!(
            note.contains(
                "through EGL. It plays its sound through ALSA\n\
                 (libasound2), and is silent without it.\n\nIts log"
            ),
            "{note}"
        );
        assert!(
            note.contains("Its sprites, sounds, prefabs and level are in assets/game.kpak"),
            "{note}"
        );
        assert!(note.ends_with("see LICENSE and NOTICE.\n"), "{note}");
        assert!(
            note.contains(
                "from\ncommit 0123456789abcdef of 2026-10-31:\n\
                 https://github.com/jabbott-iii/Nadir/commit/0123456789abcdef\n"
            ),
            "{note}"
        );
    }

    #[test]
    fn site_takes_only_an_output_directory() {
        let mut out = Vec::new();
        let result = command(Args::new(["--out", "x", "extra"]), Path::new("."), &mut out);
        match result {
            Err(Error::Usage(message)) => {
                assert_eq!(message, "`site` takes only `--out DIR`, but got `extra`");
            }
            other => panic!("expected a usage error, got {other:?}"),
        }
        assert!(out.is_empty());
    }
}
