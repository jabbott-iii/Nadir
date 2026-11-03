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

//! `cargo xtask dist [--out DIR]`: builds the release files for the target
//! this runs on, into `target/dist` or `DIR`. The release workflow runs it
//! on each of the six targets and publishes what it writes (ADR-0036).
//!
//! 1. Cooks `assets/` into `target/cooked`, as `cargo xtask cook` does,
//!    and builds `nadir-cook` and the sandbox in release, `--locked`.
//! 2. Lays out `nadir-<version>-<target>/`: the two executables, the
//!    sandbox's cooked assets, the pack `game.kpak`, in `assets/` beside it,
//!    where it reads them (ADR-0033, ADR-0041), a `README.txt`, and the
//!    `LICENSE` and `NOTICE`. On
//!    Windows and macOS the sandbox shows an empty window until the engine
//!    draws there, which its README says ([`Target::drawing_task`]).
//! 3. Runs what it laid out. `nadir-cook --version` must print the
//!    workspace's version; the laid-out cook must cook `assets/` into the
//!    pack the layout holds, byte for byte; and the sandbox must
//!    start, which it shows by refusing a scene that does not exist. It
//!    refuses before it opens a window, so this runs on a machine with no
//!    display.
//! 4. Packs it: into a `.zip` on Windows, with the system's own `tar.exe`,
//!    and a `.tar.gz` elsewhere.
//! 5. On Linux, makes the Debian packages [`COOK_DEB`] and
//!    [`SANDBOX_DEB`] with `dpkg-deb`. `dpkg-shlibdeps` finds the packages
//!    of the libraries each executable links; the sandbox also depends on
//!    those it loads when it runs: X11, EGL and OpenGL.
//!
//! On Windows the executables link the C runtime statically
//! (`.cargo/config.toml`), so that they need no Visual C++
//! redistributable. It signs nothing (that waits for M11.2) and uploads
//! nothing: the release workflow does that.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use nadir_core::cli::Args;

use crate::cargo;
use crate::crates::FACADE;
use crate::error::Error;
use crate::metadata::{self, About};
use crate::site::{self, COOKED};

/// Where the files go, relative to the workspace root, without `--out`.
const OUTPUT: &str = "target/dist";

/// The scratch directory in the output directory, where the layouts are
/// made. It goes when the run is done.
const WORK: &str = ".work";

/// The cargo arguments that build the two programs.
const BUILD: &[&str] = &[
    "build",
    "--release",
    "--locked",
    "--package",
    "nadir-cook",
    "--package",
    "sandbox",
    "--message-format",
    "json-render-diagnostics",
];

/// What the sandbox says when asked for the scene `none`.
const NO_SCENE: &str = "there is no scene `none`";

/// The libraries the sandbox loads when it runs, by their Debian
/// packages: `libX11.so.6`, `libEGL.so.1` and `libOpenGL.so.0`.
const SANDBOX_LOADS: &[&str] = &["libx11-6", "libegl1", "libopengl0"];

/// One Debian package.
struct Deb {
    /// The package's name.
    name: &'static str,
    /// Its Debian section.
    section: &'static str,
    /// The first line of its description.
    summary: &'static str,
    /// The rest of its description, as `dpkg` wants it: each line starting
    /// with a space.
    details: &'static str,
}

/// The cook, as `/usr/bin/nadir-cook`.
const COOK_DEB: Deb = Deb {
    name: "nadir-cook",
    section: "devel",
    summary: "offline asset pipeline of the Nadir game engine",
    details: " Cooks a folder of PNG images, Aseprite exports, BMFont fonts, LDtk\n \
              levels and JSONC data into the atlases, level files and binary\n \
              files the Nadir engine loads.\n",
};

/// The sandbox, in `/usr/lib/games/nadir-sandbox` with its cooked
/// sprites, and `/usr/games/nadir-sandbox`, a link to it.
const SANDBOX_DEB: Deb = Deb {
    name: "nadir-sandbox",
    section: "games",
    summary: "demo game of the Nadir game engine",
    details: " The top-down game that drives the Nadir engine's requirements: a person\n \
              walks a field of tiles, trees and rocks. It needs an X11 server or\n \
              XWayland, and OpenGL 3.3 through EGL.\n",
};

/// Runs `cargo xtask dist` with the arguments after `dist`.
pub(crate) fn command(mut args: Args, root: &Path, out: &mut dyn Write) -> Result<bool, Error> {
    let usage = |error: nadir_core::CoreError| Error::Usage(error.to_string());
    let output = args
        .value("--out")
        .map_err(usage)?
        .map_or_else(|| root.join(OUTPUT), PathBuf::from);
    if let Some(argument) = args.finish().map_err(usage)?.first() {
        return Err(Error::Usage(format!(
            "`dist` takes only `--out DIR`, but got `{argument}`"
        )));
    }
    // `dpkg-shlibdeps` runs in a directory of its own.
    let output = std::path::absolute(&output).map_err(|source| Error::Read {
        path: output.clone(),
        source,
    })?;

    let release = release(root)?;
    let say = |out: &mut dyn Write, line: &str| {
        writeln!(out, "dist: {line}")
            .and_then(|()| out.flush())
            .map_err(Error::Output)
    };
    say(
        out,
        &format!("{} for {}", release.version, release.target.triple),
    )?;
    cargo::run(root, &crate::cook_command(false))?;
    let messages = cargo::capture(root, BUILD)?;
    let built = |name: &str| {
        site::executable(&messages, name).ok_or_else(|| Error::UnexpectedOutput {
            command: cargo::display(BUILD),
            detail: format!("no executable named `{name}` among the artifacts"),
        })
    };
    let (cook, sandbox) = (built("nadir-cook")?, built("sandbox")?);

    let work = output.join(WORK);
    remove_dir(&work)?;
    let stem = release.stem();
    let dir = work.join(&stem);
    site::make_dir(&dir)?;
    let target = &release.target;
    site::copy(&cook, &dir.join(target.executable("nadir-cook")))?;
    site::copy(&sandbox, &dir.join(target.executable("sandbox")))?;
    site::copy_pack(&root.join(COOKED), &dir.join("assets"))?;
    site::copy(&root.join("LICENSE"), &dir.join("LICENSE"))?;
    site::copy(&root.join("NOTICE"), &dir.join("NOTICE"))?;
    // The newer of the two programs' floors.
    let glibc = site::glibc_floor(&[read(&cook)?, read(&sandbox)?].concat());
    site::write(
        &dir.join("README.txt"),
        readme(&release, glibc.as_deref()).as_bytes(),
    )?;

    say(out, &format!("running what {stem} holds"))?;
    smoke(root, &dir, &work, &release)?;

    let mut written = vec![pack(&work, &stem, &output, target)?];
    if target.os == Os::Linux {
        for deb in [COOK_DEB, SANDBOX_DEB] {
            written.push(debian(&deb, &dir, &work, &output, &release)?);
        }
    }
    remove_dir(&work)?;
    for file in &written {
        say(out, &format!("wrote {}", file.display()))?;
    }
    Ok(true)
}

/// What a run releases: the workspace's version, what the facade says of
/// itself, and the target.
struct Release {
    version: String,
    about: About,
    target: Target,
}

impl Release {
    /// The name of the layout and the archive, without an extension.
    fn stem(&self) -> String {
        format!("nadir-{}-{}", self.version, self.target.triple)
    }
}

/// Reads the release from the workspace at `root`: the facade's version
/// and metadata, and cargo's host target.
fn release(root: &Path) -> Result<Release, Error> {
    let metadata = metadata::load(root)?;
    let facade = metadata
        .packages
        .into_iter()
        .find(|package| package.is_member && package.name == FACADE)
        .ok_or_else(|| Error::Internal(format!("the workspace has no member `{FACADE}`")))?;
    const VERSION: &[&str] = &["-vV"];
    let text = cargo::capture(root, VERSION)?;
    let triple = text
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or_else(|| Error::UnexpectedOutput {
            command: cargo::display(VERSION),
            detail: "no `host:` line".to_string(),
        })?;
    let target = Target::parse(triple.trim()).map_err(Error::Usage)?;
    Ok(Release {
        version: facade.version,
        about: facade.about,
        target,
    })
}

/// The systems a release is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Os {
    Linux,
    Windows,
    Mac,
}

/// The processors a release is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arch {
    X86_64,
    Aarch64,
}

/// A target triple a release is built for.
#[derive(Debug, PartialEq, Eq)]
struct Target {
    triple: String,
    os: Os,
    arch: Arch,
}

impl Target {
    /// Reads `triple`, which must be one of the six release targets.
    fn parse(triple: &str) -> Result<Self, String> {
        let unknown = || {
            format!(
                "a release is built for x86_64 and aarch64 Linux (gnu), Windows (msvc) and \
                 macOS, not `{triple}`"
            )
        };
        let (arch, system) = triple.split_once('-').ok_or_else(unknown)?;
        let arch = match arch {
            "x86_64" => Arch::X86_64,
            "aarch64" => Arch::Aarch64,
            _ => return Err(unknown()),
        };
        let os = match system {
            "unknown-linux-gnu" => Os::Linux,
            "pc-windows-msvc" => Os::Windows,
            "apple-darwin" => Os::Mac,
            _ => return Err(unknown()),
        };
        Ok(Self {
            triple: triple.to_string(),
            os,
            arch,
        })
    }

    /// The file name of the executable `name`.
    fn executable(&self, name: &str) -> String {
        match self.os {
            Os::Windows => format!("{name}.exe"),
            Os::Linux | Os::Mac => name.to_string(),
        }
    }

    /// The system and processor, in words.
    fn platform(&self) -> &'static str {
        match (self.os, self.arch) {
            (Os::Linux, Arch::X86_64) => "Linux on x86_64",
            (Os::Linux, Arch::Aarch64) => "Linux on arm64",
            (Os::Windows, Arch::X86_64) => "Windows on x64",
            (Os::Windows, Arch::Aarch64) => "Windows on Arm",
            (Os::Mac, Arch::X86_64) => "macOS on Intel",
            (Os::Mac, Arch::Aarch64) => "macOS on Apple silicon",
        }
    }

    /// The archive's file extension.
    fn archive(&self) -> &'static str {
        match self.os {
            Os::Windows => "zip",
            Os::Linux | Os::Mac => "tar.gz",
        }
    }

    /// The roadmap task that makes the engine draw on this target, if it
    /// does not yet: it needs an OpenGL context, which Linux has, and
    /// Windows gets with M2.6 and macOS with M2.7. Until then the sandbox
    /// opens its window and runs undrawn, an empty window (ADR-0019).
    fn drawing_task(&self) -> Option<&'static str> {
        match self.os {
            Os::Linux => None,
            Os::Windows => Some("M2.6"),
            Os::Mac => Some("M2.7"),
        }
    }

    /// The Debian architecture.
    fn debian_arch(&self) -> &'static str {
        match self.arch {
            Arch::X86_64 => "amd64",
            Arch::Aarch64 => "arm64",
        }
    }
}

/// The `README.txt` of the archive, whose programs need `glibc` or newer,
/// on Linux.
fn readme(release: &Release, glibc: Option<&str>) -> String {
    let target = &release.target;
    let run = |name: &str| match target.os {
        Os::Windows => format!(".\\{}", target.executable(name)),
        Os::Linux | Os::Mac => format!("./{name}"),
    };
    let cook = run("nadir-cook");
    let mut programs = String::from(
        "  nadir-cook   the offline asset pipeline. It cooks a folder of PNG\n               \
         images, Aseprite exports, BMFont fonts, LDtk levels and\n               \
         JSONC data into the atlases, level files and binary\n               \
         files the engine loads.\n",
    );
    programs.push_str(
        "  sandbox      the game that drives the engine's requirements, with its\n               \
         cooked sprites, prefabs and levels in assets/game.kpak.\n",
    );
    let sandbox = run("sandbox");
    let commands = format!("    {cook} --help\n    {cook} SOURCE OUT\n    {sandbox}\n");
    let draws = match target.drawing_task() {
        None => "The sandbox needs an X11 server (or XWayland) and OpenGL 3.3 through\n\
                 EGL."
            .to_string(),
        Some(task) => format!(
            "On this system the sandbox opens its window and runs, but shows an\n\
             empty window: the engine draws here once roadmap task {task} gives it\n\
             an OpenGL context."
        ),
    };
    let log = match target.os {
        Os::Linux => "~/.local/share/Sandbox/nadir.log",
        Os::Windows => "%LOCALAPPDATA%\\Nadir\\Sandbox\\nadir.log",
        Os::Mac => "~/Library/Logs/Sandbox/nadir.log",
    };
    let sandbox = format!(
        "In the sandbox, WASD or the arrow keys walk; close the window to quit. The\n\
         other scenes:\n\n    \
         {sandbox} --scene stress-sprites\n    \
         {sandbox} --scene stress-entities\n\n\
         {draws} Its log is {log}.\n"
    );
    let system = match target.os {
        Os::Linux => glibc.map_or(String::new(), |version| {
            format!("\nThe programs need glibc {version} or newer.\n")
        }),
        Os::Windows => "\nThe programs need Windows 10 or newer. They are not signed yet, so\n\
                        SmartScreen may warn before the first run: choose More info, then\n\
                        Run anyway.\n"
            .to_string(),
        Os::Mac => "\nThe programs need macOS 12 or newer. They are not signed or notarized\n\
                    yet, so macOS refuses them when a browser downloaded the archive.\n\
                    Clear the quarantine first:\n\n    \
                    xattr -dr com.apple.quarantine .\n"
            .to_string(),
    };
    format!(
        "\
Nadir {version}
{underline}

Nadir is a 2D top-down game engine for desktops, written in Rust from the
ground up: {homepage}

This archive holds its programs for {platform}:

{programs}
Run them from this directory:

{commands}
{sandbox}{system}
The engine is a library on crates.io, https://crates.io/crates/nadir_engine,
and its source is at {repository}.

Licensed under the Apache License 2.0: see LICENSE and NOTICE.
",
        version = release.version,
        underline = "=".repeat("Nadir ".len() + release.version.len()),
        homepage = release.about.homepage.as_deref().unwrap_or_default(),
        platform = target.platform(),
        repository = release.about.repository.as_deref().unwrap_or_default(),
    )
}

/// Runs the programs laid out in `dir` ([module docs](self), step 3), with
/// `work` for the cook's output.
fn smoke(root: &Path, dir: &Path, work: &Path, release: &Release) -> Result<(), Error> {
    let target = &release.target;
    let cook = dir.join(target.executable("nadir-cook"));
    let printed = site::output(Command::new(&cook).arg("--version"))?;
    let wanted = format!("nadir-cook {}", release.version);
    if printed.trim_end() != wanted {
        return Err(Error::UnexpectedOutput {
            command: "nadir-cook --version".to_string(),
            detail: format!("expected `{wanted}`, but got `{}`", printed.trim_end()),
        });
    }

    let cooked = work.join("cooked");
    site::run(Command::new(&cook).arg(root.join("assets")).arg(&cooked))?;
    let shipped = dir.join("assets").join(site::PACK);
    if read(&shipped)? != read(&cooked.join(site::PACK))? {
        return Err(Error::UnexpectedOutput {
            command: format!("nadir-cook assets {}", cooked.display()),
            detail: format!(
                "the cook laid out did not pack what `{}` holds",
                shipped.display()
            ),
        });
    }

    let sandbox = dir.join(target.executable("sandbox"));
    let ran = Command::new(&sandbox)
        .args(["--scene", "none"])
        .output()
        .map_err(|source| Error::Spawn {
            command: format!("{} --scene none", sandbox.display()),
            source,
        })?;
    let said = String::from_utf8_lossy(&ran.stderr);
    if ran.status.success() || !said.contains(NO_SCENE) {
        return Err(Error::UnexpectedOutput {
            command: "sandbox --scene none".to_string(),
            detail: format!(
                "expected it to refuse the scene, saying \"{NO_SCENE}\", but it ended with \
                 {} and said `{}`",
                ran.status,
                said.trim_end()
            ),
        });
    }
    Ok(())
}

/// The files below `dir`, by their paths from it with `/` between
/// folders, sorted, with their bytes. Links are left out.
fn files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, Error> {
    fn walk(dir: &Path, prefix: &str, files: &mut Vec<(String, Vec<u8>)>) -> Result<(), Error> {
        let read_dir = |source| Error::Read {
            path: dir.to_path_buf(),
            source,
        };
        for entry in fs::read_dir(dir).map_err(read_dir)? {
            let entry = entry.map_err(read_dir)?;
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            let path = entry.path();
            if entry.file_type().map_err(read_dir)?.is_symlink() {
                continue;
            } else if path.is_dir() {
                walk(&path, &format!("{name}/"), files)?;
            } else {
                files.push((name, read(&path)?));
            }
        }
        Ok(())
    }
    let mut found = Vec::new();
    walk(dir, "", &mut found)?;
    found.sort();
    Ok(found)
}

/// Packs the layout `stem` in `work` into an archive in `output`, and
/// returns its path.
fn pack(work: &Path, stem: &str, output: &Path, target: &Target) -> Result<PathBuf, Error> {
    let archive = output.join(format!("{stem}.{}", target.archive()));
    remove_file(&archive)?;
    let mut command = match target.os {
        // Not the `tar` first on the path, which can be Git's GNU tar, which
        // writes no zip: Windows' own, bsdtar, picks the format by the
        // name's extension (`-a`).
        Os::Windows => {
            let system = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            let mut command = Command::new(Path::new(&system).join("System32").join("tar.exe"));
            command.args(["-a", "-c", "-f"]);
            command
        }
        Os::Linux => {
            let mut command = Command::new("tar");
            command.args([
                "--sort=name",
                "--owner=0",
                "--group=0",
                "--numeric-owner",
                "--mode=go-w",
                "-czf",
            ]);
            command
        }
        Os::Mac => {
            let mut command = Command::new("tar");
            command.arg("-czf");
            command
        }
    };
    site::run(command.arg(&archive).arg("-C").arg(work).arg(stem))?;
    Ok(archive)
}

/// Makes the Debian package `deb` from the layout in `dir`, by way of
/// `work`, into `output`, and returns its path.
fn debian(
    deb: &Deb,
    dir: &Path,
    work: &Path,
    output: &Path,
    release: &Release,
) -> Result<PathBuf, Error> {
    let root = work.join("deb").join(deb.name);
    let doc = root.join("usr/share/doc").join(deb.name);
    for folder in [&doc, &root.join("DEBIAN")] {
        site::make_dir(folder)?;
    }
    // The executable `dpkg-shlibdeps` reads, and the packages of the
    // libraries it loads itself.
    let (executable, mut depends) = if deb.name == COOK_DEB.name {
        let bin = root.join("usr/bin");
        site::make_dir(&bin)?;
        let executable = bin.join("nadir-cook");
        site::copy(&dir.join("nadir-cook"), &executable)?;
        (executable, Vec::new())
    } else {
        let lib = root.join("usr/lib/games").join(deb.name);
        let games = root.join("usr/games");
        for folder in [&lib, &games] {
            site::make_dir(folder)?;
        }
        let executable = lib.join("sandbox");
        site::copy(&dir.join("sandbox"), &executable)?;
        site::copy_pack(&dir.join("assets"), &lib.join("assets"))?;
        // Run through the link, the sandbox still finds `assets` beside
        // itself: on Linux its own path is the one the link names.
        site::run(
            Command::new("ln")
                .arg("-s")
                .arg(format!("../lib/games/{}/sandbox", deb.name))
                .arg(games.join(deb.name)),
        )?;
        (executable, SANDBOX_LOADS.to_vec())
    };
    let linked = linked_packages(&executable, work)?;
    depends.insert(0, &linked);
    site::write(
        &doc.join("copyright"),
        copyright(&release.about)?.as_bytes(),
    )?;
    // Debian keeps the license's text, but not the release's NOTICE.
    site::copy(&dir.join("NOTICE"), &doc.join("NOTICE"))?;
    // Whatever the umask made: folders and executables 0755, files 0644.
    site::run(Command::new("chmod").args(["-R", "u=rwX,go=rX"]).arg(&root))?;
    let control = control(deb, release, &depends.join(", "), installed_kib(&root)?)?;
    site::write(&root.join("DEBIAN/control"), control.as_bytes())?;
    // The release's own version in the file's name, not Debian's: GitHub
    // turns the `~` of `0.2.0~rc.1` into `.` in an asset's name, and the
    // repository's index must name the file as GitHub keeps it.
    let file = output.join(deb_file(deb, release));
    remove_file(&file)?;
    site::run(
        Command::new("dpkg-deb")
            .args(["--root-owner-group", "-Zxz", "--build"])
            .arg(&root)
            .arg(&file),
    )?;
    Ok(file)
}

/// The packages of the libraries `executable` links, with the oldest
/// version of each it runs with, as `dpkg-shlibdeps` reads them from its
/// symbols: `libc6 (>= 2.34), libgcc-s1 (>= 4.2)`. It runs in a folder of
/// its own in `work`, as it wants a `debian/control` there.
fn linked_packages(executable: &Path, work: &Path) -> Result<String, Error> {
    let dir = work.join("shlibdeps");
    site::make_dir(&dir.join("debian"))?;
    site::write(
        &dir.join("debian/control"),
        b"Source: nadir\n\nPackage: nadir\nArchitecture: any\n",
    )?;
    let mut command = Command::new("dpkg-shlibdeps");
    command
        .arg("-O")
        .arg(format!("-e{}", executable.display()))
        .current_dir(&dir);
    let text = site::output(&mut command)?;
    text.lines()
        .find_map(|line| line.strip_prefix("shlibs:Depends="))
        .map(|depends| depends.trim().to_string())
        .filter(|depends| !depends.is_empty())
        .ok_or_else(|| Error::UnexpectedOutput {
            command: format!("dpkg-shlibdeps -O -e{}", executable.display()),
            detail: format!("no `shlibs:Depends=` line in `{}`", text.trim_end()),
        })
}

/// The file name of the package `deb` of `release`.
fn deb_file(deb: &Deb, release: &Release) -> String {
    format!(
        "{}_{}_{}.deb",
        deb.name,
        release.version,
        release.target.debian_arch()
    )
}

/// The package's `DEBIAN/control`.
fn control(deb: &Deb, release: &Release, depends: &str, kib: u64) -> Result<String, Error> {
    let about = &release.about;
    let maintainer = about
        .authors
        .first()
        .ok_or_else(|| Error::Internal(format!("`{FACADE}` names no author to maintain it")))?;
    let homepage = about
        .homepage
        .as_ref()
        .or(about.repository.as_ref())
        .ok_or_else(|| Error::Internal(format!("`{FACADE}` has no homepage or repository")))?;
    Ok(format!(
        "\
Package: {name}
Version: {version}
Architecture: {arch}
Maintainer: {maintainer}
Installed-Size: {kib}
Depends: {depends}
Section: {section}
Priority: optional
Homepage: {homepage}
Description: {summary}
{details}",
        name = deb.name,
        version = debian_version(&release.version),
        arch = release.target.debian_arch(),
        section = deb.section,
        summary = deb.summary,
        details = deb.details,
    ))
}

/// The package's `copyright`, in Debian's machine-readable format. The
/// license's text is the one Debian keeps in `/usr/share/common-licenses`.
fn copyright(about: &About) -> Result<String, Error> {
    let license = about.license.as_deref().unwrap_or_default();
    if license != "Apache-2.0" {
        return Err(Error::Internal(format!(
            "the Debian copyright file points to Debian's copy of the Apache-2.0 license, but \
             the workspace's license is `{license}`"
        )));
    }
    let source = about
        .repository
        .as_deref()
        .ok_or_else(|| Error::Internal(format!("`{FACADE}` has no repository")))?;
    Ok(format!(
        "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: Nadir
Source: {source}

Files: *
Copyright: {authors}
License: Apache-2.0
 On Debian systems, the full text of the Apache License, Version 2.0, is
 in /usr/share/common-licenses/Apache-2.0.
",
        authors = about.authors.join(", "),
    ))
}

/// The space the files below `root`, but `DEBIAN`, take once installed,
/// in KiB as `dpkg` counts it: each file rounded up to a whole KiB.
fn installed_kib(root: &Path) -> Result<u64, Error> {
    let files = files(root)?;
    Ok(files
        .iter()
        .filter(|(name, _)| !name.starts_with("DEBIAN/"))
        .map(|(_, bytes)| (bytes.len() as u64).div_ceil(1024))
        .sum())
}

/// The Debian version of the semantic version `version`. A pre-release
/// sorts before its release in both, which Debian writes with `~`:
/// `0.2.0-rc.1` is `0.2.0~rc.1`.
fn debian_version(version: &str) -> String {
    version.replacen('-', "~", 1)
}

fn read(path: &Path) -> Result<Vec<u8>, Error> {
    fs::read(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// Removes the directory `dir`, if there is one.
fn remove_dir(dir: &Path) -> Result<(), Error> {
    match fs::remove_dir_all(dir) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(Error::Write {
            path: dir.to_path_buf(),
            source: error,
        }),
        _ => Ok(()),
    }
}

/// Removes the file `file`, if there is one.
fn remove_file(file: &Path) -> Result<(), Error> {
    match fs::remove_file(file) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(Error::Write {
            path: file.to_path_buf(),
            source: error,
        }),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(triple: &str, version: &str) -> Release {
        Release {
            version: version.to_string(),
            about: About {
                description: Some("A lightweight, cross-platform Rust engine.".to_string()),
                license: Some("Apache-2.0".to_string()),
                repository: Some("https://github.com/jabbott-iii/Nadir".to_string()),
                homepage: Some("https://jabbott.dev/Nadir/".to_string()),
                authors: vec!["A. Author <a@example.com>".to_string()],
            },
            target: Target::parse(triple).unwrap(),
        }
    }

    #[test]
    fn the_six_release_targets_are_read_and_no_others() {
        let read: Vec<_> = [
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
        ]
        .into_iter()
        .map(|triple| {
            let target = Target::parse(triple).unwrap();
            (
                target.platform(),
                target.executable("sandbox"),
                target.archive(),
                target.debian_arch(),
            )
        })
        .collect();
        assert_eq!(
            read,
            [
                ("Linux on x86_64", "sandbox".to_string(), "tar.gz", "amd64"),
                ("Linux on arm64", "sandbox".to_string(), "tar.gz", "arm64"),
                ("Windows on x64", "sandbox.exe".to_string(), "zip", "amd64"),
                ("Windows on Arm", "sandbox.exe".to_string(), "zip", "arm64"),
                ("macOS on Intel", "sandbox".to_string(), "tar.gz", "amd64"),
                (
                    "macOS on Apple silicon",
                    "sandbox".to_string(),
                    "tar.gz",
                    "arm64"
                ),
            ]
        );
        for triple in [
            "x86_64-unknown-linux-musl",
            "x86_64-pc-windows-gnu",
            "i686-pc-windows-msvc",
            "riscv64gc-unknown-linux-gnu",
            "x86_64",
        ] {
            assert_eq!(
                Target::parse(triple),
                Err(format!(
                    "a release is built for x86_64 and aarch64 Linux (gnu), Windows (msvc) and \
                     macOS, not `{triple}`"
                ))
            );
        }
    }

    #[test]
    fn the_archive_is_named_for_the_version_and_target() {
        let release = release("aarch64-pc-windows-msvc", "0.2.0-rc.1");
        assert_eq!(release.stem(), "nadir-0.2.0-rc.1-aarch64-pc-windows-msvc");
    }

    #[test]
    fn the_readme_says_how_to_run_each_program_and_what_it_needs() {
        let linux = readme(&release("x86_64-unknown-linux-gnu", "0.1.0"), Some("2.34"));
        assert!(linux.starts_with("Nadir 0.1.0\n===========\n"), "{linux}");
        for line in [
            "This archive holds its programs for Linux on x86_64:",
            "  sandbox      the game that drives the engine's requirements, with its",
            "    ./nadir-cook SOURCE OUT\n    ./sandbox\n",
            "    ./sandbox --scene stress-sprites",
            "EGL. Its log is ~/.local/share/Sandbox/nadir.log.",
            "The programs need glibc 2.34 or newer.",
            "https://crates.io/crates/nadir_engine,\nand its source is at https://github.com/jabbott-iii/Nadir.",
        ] {
            assert!(linux.contains(line), "`{line}` is not in:\n{linux}");
        }
        assert!(!linux.contains("signed"), "{linux}");
        let windows = readme(&release("aarch64-pc-windows-msvc", "0.1.0"), None);
        for line in [
            "programs for Windows on Arm:",
            "    .\\nadir-cook.exe --help",
            "    .\\sandbox.exe\n",
            "    .\\sandbox.exe --scene stress-sprites",
            "shows an\nempty window: the engine draws here once roadmap task M2.6",
            "%LOCALAPPDATA%\\Nadir\\Sandbox\\nadir.log",
            "Windows 10 or newer",
            "SmartScreen",
        ] {
            assert!(windows.contains(line), "`{line}` is not in:\n{windows}");
        }
        let mac = readme(&release("aarch64-apple-darwin", "0.1.0"), None);
        for line in [
            "macOS on Apple silicon",
            "    ./sandbox --scene stress-entities",
            "roadmap task M2.7",
            "~/Library/Logs/Sandbox/nadir.log",
            "xattr -dr com.apple.quarantine .",
        ] {
            assert!(mac.contains(line), "`{line}` is not in:\n{mac}");
        }
        assert!(!mac.contains("X11"), "{mac}");
    }

    #[test]
    fn a_debian_version_puts_a_pre_release_before_its_release() {
        // In the file's name, the release's own version, with no `~`.
        let release = release("aarch64-unknown-linux-gnu", "0.2.0-rc.1");
        assert_eq!(
            deb_file(&COOK_DEB, &release),
            "nadir-cook_0.2.0-rc.1_arm64.deb"
        );
        assert_eq!(debian_version("0.1.0"), "0.1.0");
        assert_eq!(debian_version("0.2.0-rc.1"), "0.2.0~rc.1");
        assert_eq!(debian_version("1.0.0-beta-2+b7"), "1.0.0~beta-2+b7");
    }

    #[test]
    fn the_control_file_names_the_package_and_what_it_depends_on() {
        let release = release("aarch64-unknown-linux-gnu", "0.2.0-rc.1");
        let text = control(&SANDBOX_DEB, &release, "libc6 (>= 2.34), libx11-6", 812).unwrap();
        assert_eq!(
            text,
            "\
Package: nadir-sandbox
Version: 0.2.0~rc.1
Architecture: arm64
Maintainer: A. Author <a@example.com>
Installed-Size: 812
Depends: libc6 (>= 2.34), libx11-6
Section: games
Priority: optional
Homepage: https://jabbott.dev/Nadir/
Description: demo game of the Nadir game engine
 The top-down game that drives the Nadir engine's requirements: a person
 walks a field of tiles, trees and rocks. It needs an X11 server or
 XWayland, and OpenGL 3.3 through EGL.
"
        );
        // Each line of a description after the first starts with a space,
        // as dpkg wants.
        for deb in [COOK_DEB, SANDBOX_DEB] {
            assert!(deb.details.ends_with('\n'));
            assert!(deb.details.lines().all(|line| line.starts_with(' ')));
        }
        let mut nobody = release;
        nobody.about.authors.clear();
        assert!(control(&COOK_DEB, &nobody, "libc6", 1).is_err());
    }

    #[test]
    fn the_copyright_file_points_to_debian_s_copy_of_the_license() {
        let about = release("x86_64-unknown-linux-gnu", "0.1.0").about;
        let text = copyright(&about).unwrap();
        assert!(
            text.starts_with("Format: https://www.debian.org/"),
            "{text}"
        );
        assert!(
            text.contains("Copyright: A. Author <a@example.com>\n"),
            "{text}"
        );
        assert!(
            text.contains("/usr/share/common-licenses/Apache-2.0"),
            "{text}"
        );
        let other = About {
            license: Some("MIT".to_string()),
            ..about
        };
        assert!(copyright(&other).is_err());
    }

    /// A fresh directory for test `name`, in the system's temporary folder.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nadir-xtask-dist-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_tree_s_files_are_sorted_and_counted_once_installed() {
        let dir = scratch("files");
        for (path, bytes) in [("a/one.kbj", "1"), ("a/b/two.qoi", "2"), ("three", "")] {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let tree = files(&dir).unwrap();
        let names: Vec<&str> = tree.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["a/b/two.qoi", "a/one.kbj", "three"]);
        // Files of the package's control do not count as installed.
        fs::create_dir_all(dir.join("DEBIAN")).unwrap();
        fs::write(dir.join("DEBIAN/control"), vec![b'x'; 5000]).unwrap();
        fs::write(dir.join("big"), vec![b'x'; 1025]).unwrap();
        // 1, 1, 0 and 2 KiB.
        assert_eq!(installed_kib(&dir).unwrap(), 4);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dist_takes_only_an_output_directory() {
        let mut out = Vec::new();
        let result = command(Args::new(["--out", "x", "extra"]), Path::new("."), &mut out);
        match result {
            Err(Error::Usage(message)) => {
                assert_eq!(message, "`dist` takes only `--out DIR`, but got `extra`");
            }
            other => panic!("expected a usage error, got {other:?}"),
        }
        assert!(out.is_empty());
    }
}
