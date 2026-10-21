//! `cargo xtask golden [--bless]`: draws the golden scenes and compares each
//! with its reference, `tests/golden/<scene>.qoi` (`docs/04-quality.md`).
//!
//! The scenes are tests of the crates that draw. This tool may depend only
//! on `nadir-core` and `nadir-codec`, so it runs them with `cargo test`,
//! naming a directory in [`DIR_VAR`], and each scene writes what it drew
//! there as a PAM (Netpbm) file. The comparison and the QOI files are this
//! tool's.

use std::collections::{BTreeMap, BTreeSet};
use std::env::{self, VarError};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
use nadir_codec::{CodecError, Limits};
use nadir_core::cli::Args;

use crate::cargo;
use crate::error::Error;

/// The references, relative to the workspace root.
const REFERENCES: &str = "tests/golden";

/// What this tool writes, relative to the workspace root. It is emptied at
/// the start of each run.
const OUTPUT: &str = "target/golden";

/// The directory inside [`OUTPUT`] that the scenes write to.
const RENDERED: &str = "rendered";

/// Names the directory the scenes write to. Unset, they draw but write
/// nothing.
const DIR_VAR: &str = "NADIR_GOLDEN_DIR";

/// Names a part of the renderer's name that every scene must be drawn by,
/// as for the OpenGL tests. Unset or empty, any renderer will do.
const EXPECT_RENDERER: &str = "NADIR_EXPECT_GL_RENDERER";

/// The renderer that draws the references: Mesa's software renderer, which
/// CI uses (`docs/04-quality.md`).
const BLESS_RENDERER: &str = "llvmpipe";

/// How to choose Mesa where libglvnd also has another vendor's EGL.
const CHOOSE_MESA: &str = "With libglvnd, Mesa can be chosen with \
    __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
    LIBGL_ALWAYS_SOFTWARE=1";

/// The largest difference in a channel that still counts as the same.
const TOLERANCE: u8 = 2;

/// One pixel in this many may differ by more than [`TOLERANCE`]: 0.1%.
const OUTLIER_SHARE: usize = 1000;

/// The commands that draw the scenes: each crate's GL test binary, whose
/// `golden` module holds its scenes (ADR-0017).
const SCENES: &[&[&str]] = &[
    &[
        "test",
        "--package",
        "nadir-gfx",
        "--features",
        "headless-test",
        "--test",
        "headless",
        "--",
        "golden::",
    ],
    &[
        "test",
        "--package",
        "nadir-render",
        "--features",
        "headless-test",
        "--test",
        "headless",
        "--",
        "golden::",
    ],
];

/// Runs `golden` with the arguments that follow the command name. Returns
/// whether every scene matched its reference, or, with `--bless`, whether
/// every reference now matches its scene.
pub(crate) fn command(mut args: Args, root: &Path, out: &mut dyn Write) -> Result<bool, Error> {
    let usage = |error: nadir_core::CoreError| Error::Usage(error.to_string());
    let bless = args.flag("--bless");
    if let Some(argument) = args.finish().map_err(usage)?.first() {
        return Err(Error::Usage(format!(
            "`golden` takes only `--bless`, but got `{argument}`"
        )));
    }
    let expected = match env::var(EXPECT_RENDERER) {
        Ok(text) => text,
        Err(VarError::NotPresent) => String::new(),
        Err(VarError::NotUnicode(_)) => {
            return Err(Error::Usage(format!(
                "{EXPECT_RENDERER} is not valid Unicode"
            )));
        }
    };

    run(
        root,
        bless,
        &expected,
        &mut |rendered| draw(root, rendered),
        out,
    )
}

/// Runs the commands in [`SCENES`], which write into `rendered`. Returns
/// the first that failed, if one did: a scene whose own checks fail still
/// saves what it drew, so a failed command is reported after the
/// comparison, not instead of it.
fn draw(root: &Path, rendered: &Path) -> Result<Option<String>, Error> {
    let mut failed = None;
    for scenes in SCENES {
        match cargo::run_with_env(root, scenes, &[(DIR_VAR, rendered.as_os_str())]) {
            Ok(()) => {}
            Err(Error::CommandFailed { command, .. }) => {
                failed = failed.or(Some(command));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(failed)
}

/// Empties [`OUTPUT`], has `draw` draw the scenes into its [`RENDERED`]
/// directory, and finishes the run. `draw` returns the command that
/// failed, if one did.
fn run(
    root: &Path,
    bless: bool,
    expected: &str,
    draw: &mut dyn FnMut(&Path) -> Result<Option<String>, Error>,
    out: &mut dyn Write,
) -> Result<bool, Error> {
    let output = root.join(OUTPUT);
    match fs::remove_dir_all(&output) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(Error::Write {
                path: output,
                source: error,
            });
        }
        _ => {}
    }
    let rendered = output.join(RENDERED);
    fs::create_dir_all(&rendered).map_err(|source| Error::Write {
        path: rendered.clone(),
        source,
    })?;
    // The scenes' test output goes to the terminal directly.
    out.flush().map_err(Error::Output)?;
    let failed = draw(&rendered)?;
    finish(root, bless, expected, failed.as_deref(), out)
}

/// Everything after the scenes have drawn: reads what they wrote and the
/// references, compares them, writes the images and references, and
/// prints the results. Returns whether the run passed.
///
/// `failed_command` is the command that drew the scenes, if it failed.
/// Then nothing is blessed, and the run fails, whatever the comparison
/// finds.
fn finish(
    root: &Path,
    bless: bool,
    expected: &str,
    failed_command: Option<&str>,
    out: &mut dyn Write,
) -> Result<bool, Error> {
    let output = Error::Output;
    let bless = bless && failed_command.is_none();
    if let Some(command) = failed_command {
        writeln!(
            out,
            "golden: `{command}` failed, as it says above; comparing what the scenes drew, and \
             blessing nothing"
        )
        .map_err(output)?;
    }
    let rendered = read_rendered(&root.join(OUTPUT).join(RENDERED))?;
    let References {
        scenes: references,
        overrides,
        strays,
    } = read_references(&root.join(REFERENCES))?;

    let renderers: BTreeSet<&str> = rendered
        .values()
        .map(|scene| scene.renderer.as_str())
        .collect();
    let listed = renderers
        .iter()
        .map(|renderer| format!("`{renderer}`"))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(
        out,
        "golden: {} drawn by {}",
        plural(rendered.len(), "scene"),
        if listed.is_empty() {
            "nothing"
        } else {
            &listed
        }
    )
    .map_err(output)?;
    let refusal = match renderers.iter().collect::<Vec<_>>()[..] {
        [] => None,
        [renderer] if !renderer.contains(expected) => Some(format!(
            "{EXPECT_RENDERER} asks for `{expected}`. {CHOOSE_MESA}"
        )),
        [renderer] if bless && !renderer.contains(BLESS_RENDERER) => Some(format!(
            "references are drawn by {BLESS_RENDERER}, as CI draws them. {CHOOSE_MESA}"
        )),
        [_] => None,
        _ => Some("one run must draw every scene with one renderer".to_string()),
    };
    if let Some(reason) = refusal {
        writeln!(out, "golden: FAILED: {reason}").map_err(output)?;
        return Ok(false);
    }

    let names: BTreeSet<&str> = rendered
        .keys()
        .chain(references.keys())
        .map(String::as_str)
        .collect();
    if names.is_empty() && strays.is_empty() {
        writeln!(
            out,
            "golden: FAILED: no scene drew anything, and there are no references"
        )
        .map_err(output)?;
        return Ok(false);
    }
    let width = names
        .iter()
        .copied()
        .chain(overrides.iter().chain(&strays).map(String::as_str))
        .map(str::len)
        .max()
        .unwrap_or(0);
    let mut failed = 0;
    let mut tally = Tally::default();
    for name in &names {
        let scene = rendered.get(*name);
        let verdict = judge(scene, references.get(*name).map(Vec::as_slice));
        let line = match (scene, bless) {
            (Some(scene), true) if !verdict.passed() => {
                write_file(
                    &root.join(REFERENCES).join(format!("{name}.qoi")),
                    &encode(scene)?,
                )?;
                tally.count(&verdict);
                blessed(&verdict)
            }
            (Some(scene), false) if !verdict.passed() => {
                failed += 1;
                save_images(root, name, scene, &verdict)?;
                failure(name, &verdict, failed_command.is_some())
            }
            _ if verdict.passed() => {
                tally.kept += 1;
                success(&verdict)
            }
            _ => {
                failed += 1;
                failure(name, &verdict, failed_command.is_some())
            }
        };
        writeln!(out, "  {name:<width$}  {line}").map_err(output)?;
    }
    for name in &overrides {
        writeln!(
            out,
            "  {name:<width$}  skipped: a backend's own reference, which nothing checks before M10.4"
        )
        .map_err(output)?;
    }
    for name in &strays {
        failed += 1;
        writeln!(
            out,
            "  {name:<width$}  FAILED: not the name of a reference; rename {REFERENCES}/{name}.qoi \
             to <scene>.qoi, or remove it"
        )
        .map_err(output)?;
    }

    let total = names.len() + strays.len();
    if failed > 0 {
        writeln!(out, "golden: FAILED ({failed} of {total} scenes)").map_err(output)?;
    } else if let Some(command) = failed_command {
        writeln!(out, "golden: FAILED: `{command}` failed").map_err(output)?;
    } else if bless {
        writeln!(
            out,
            "golden: blessed ({} new, {} updated, {} kept)",
            tally.new, tally.updated, tally.kept
        )
        .map_err(output)?;
    } else {
        writeln!(out, "golden: ok ({})", plural(total, "scene")).map_err(output)?;
    }
    Ok(failed == 0 && failed_command.is_none())
}

/// What one scene drew: RGBA8 pixels, from the top row.
#[derive(Debug, PartialEq, Eq)]
struct Rendered {
    width: u32,
    height: u32,
    /// The renderer that drew it, as the scene wrote it.
    renderer: String,
    pixels: Vec<u8>,
}

/// How a scene compares with its reference.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// Within tolerance, with this many pixels that differ by more than
    /// [`TOLERANCE`].
    Same { over: usize },
    /// Too many pixels differ by more than [`TOLERANCE`].
    Differs {
        over: usize,
        total: usize,
        /// The largest difference in any channel.
        largest: u8,
    },
    /// The reference has another size.
    OtherSize { width: u32, height: u32 },
    /// The reference cannot be decoded.
    BadReference(CodecError),
    /// There is no reference for the scene.
    NoReference,
    /// There is a reference, but no scene drew it.
    NotRendered,
}

impl Verdict {
    fn passed(&self) -> bool {
        matches!(self, Self::Same { .. })
    }
}

/// Compares what a scene drew with its reference, when there are both.
fn judge(scene: Option<&Rendered>, reference: Option<&[u8]>) -> Verdict {
    let (Some(scene), Some(reference)) = (scene, reference) else {
        return match scene {
            Some(_) => Verdict::NoReference,
            None => Verdict::NotRendered,
        };
    };
    let image = match qoi::decode(reference, Limits::default()) {
        Ok(image) => image,
        Err(error) => return Verdict::BadReference(error),
    };
    let Header { width, height, .. } = image.header;
    if (width, height) != (scene.width, scene.height) {
        return Verdict::OtherSize { width, height };
    }
    let (over, largest) = count_over(&image.pixels, &scene.pixels);
    let total = scene.pixels.len() / 4;
    if over <= total / OUTLIER_SHARE {
        Verdict::Same { over }
    } else {
        Verdict::Differs {
            over,
            total,
            largest,
        }
    }
}

/// The number of pixels with a channel that differs by more than
/// [`TOLERANCE`], and the largest difference in any channel.
fn count_over(expected: &[u8], actual: &[u8]) -> (usize, u8) {
    let mut over = 0;
    let mut largest = 0;
    for (expected, actual) in expected
        .as_chunks::<4>()
        .0
        .iter()
        .zip(actual.as_chunks::<4>().0)
    {
        let delta = delta(expected, actual);
        largest = largest.max(delta);
        over += usize::from(delta > TOLERANCE);
    }
    (over, largest)
}

/// The largest difference in any channel of two pixels.
fn delta(expected: &[u8; 4], actual: &[u8; 4]) -> u8 {
    expected
        .iter()
        .zip(actual)
        .map(|(&expected, &actual)| expected.abs_diff(actual))
        .max()
        .unwrap_or(0)
}

/// An image of where two images differ: magenta where a pixel differs by
/// more than [`TOLERANCE`], and elsewhere the expected pixel in a dim gray.
/// It is opaque everywhere, so that viewers show it whatever the scene's
/// alpha.
fn diff_image(expected: &[u8], actual: &[u8]) -> Vec<u8> {
    let expected = expected.as_chunks::<4>().0;
    let actual = actual.as_chunks::<4>().0;
    let mut image = Vec::with_capacity(expected.len() * 4);
    for (expected, actual) in expected.iter().zip(actual) {
        if delta(expected, actual) > TOLERANCE {
            image.extend_from_slice(&[255, 0, 255, 255]);
        } else {
            let [r, g, b, _] = expected.map(u16::from);
            // At most 765 / 12, which fits a byte.
            let gray = ((r + g + b) / 12) as u8;
            image.extend_from_slice(&[gray, gray, gray, 255]);
        }
    }
    image
}

/// Writes `actual.qoi` for a scene that failed, and `diff.qoi` if it could
/// be compared.
fn save_images(root: &Path, name: &str, scene: &Rendered, verdict: &Verdict) -> Result<(), Error> {
    let output = root.join(OUTPUT);
    write_file(&output.join(format!("{name}.actual.qoi")), &encode(scene)?)?;
    if let Verdict::Differs { .. } = verdict {
        let reference = root.join(REFERENCES).join(format!("{name}.qoi"));
        let bytes = fs::read(&reference).map_err(|source| Error::Read {
            path: reference,
            source,
        })?;
        let expected = qoi::decode(&bytes, Limits::default()).map_err(|error| {
            Error::Internal(format!("a reference changed during the run: {error}"))
        })?;
        let diff = Rendered {
            pixels: diff_image(&expected.pixels, &scene.pixels),
            renderer: String::new(),
            ..*scene
        };
        write_file(&output.join(format!("{name}.diff.qoi")), &encode(&diff)?)?;
    }
    Ok(())
}

/// What a passing scene reports.
fn success(verdict: &Verdict) -> String {
    match verdict {
        Verdict::Same { over: 0 } => "ok".to_string(),
        Verdict::Same { over } => format!("ok ({} over the tolerance)", plural(*over, "pixel")),
        _ => String::new(),
    }
}

/// What a failing scene reports. `scenes_failed` says whether the command
/// that drew the scenes failed.
fn failure(name: &str, verdict: &Verdict, scenes_failed: bool) -> String {
    let actual = format!("{OUTPUT}/{name}.actual.qoi");
    let mut line = String::from("FAILED: ");
    // Writing to a `String` cannot fail.
    let _ = match verdict {
        Verdict::Differs {
            over,
            total,
            largest,
        } => write!(
            line,
            "{over} of {total} pixels differ by more than {TOLERANCE}, where at most {} may, \
             by up to {largest}; see {OUTPUT}/{name}.diff.qoi",
            total / OUTLIER_SHARE
        ),
        Verdict::OtherSize { width, height } => {
            write!(line, "the reference is {width}×{height}; see {actual}")
        }
        Verdict::BadReference(error) => write!(line, "the reference is unreadable: {error}"),
        Verdict::NoReference => write!(
            line,
            "no reference; look at {actual}, then run `cargo xtask golden --bless`"
        ),
        Verdict::NotRendered if scenes_failed => write!(
            line,
            "not drawn: its test failed before it saved what it drew, as it says above"
        ),
        Verdict::NotRendered => write!(
            line,
            "not drawn: its scene is gone (then remove {REFERENCES}/{name}.qoi), or the \
             tests that draw skipped for want of an offscreen OpenGL context"
        ),
        Verdict::Same { .. } => Ok(()),
    };
    line
}

/// What `--bless` reports for a scene whose reference it wrote.
fn blessed(verdict: &Verdict) -> String {
    match verdict {
        Verdict::NoReference => "new".to_string(),
        Verdict::Differs { over, .. } => {
            format!(
                "updated: {} were over the tolerance",
                plural(*over, "pixel")
            )
        }
        Verdict::OtherSize { width, height } => format!("updated: it was {width}×{height}"),
        Verdict::BadReference(_) => "updated: the old one was unreadable".to_string(),
        Verdict::Same { .. } | Verdict::NotRendered => String::new(),
    }
}

/// How many references `--bless` wrote, and kept.
#[derive(Default)]
struct Tally {
    new: usize,
    updated: usize,
    kept: usize,
}

impl Tally {
    fn count(&mut self, verdict: &Verdict) {
        match verdict {
            Verdict::NoReference => self.new += 1,
            _ => self.updated += 1,
        }
    }
}

/// Every scene in `dir`, by name: the files `<scene>.pam`.
fn read_rendered(dir: &Path) -> Result<BTreeMap<String, Rendered>, Error> {
    let mut scenes = BTreeMap::new();
    for (name, path) in files_with_extension(dir, "pam")? {
        let bytes = fs::read(&path).map_err(|source| Error::Read { path, source })?;
        let scene = parse_pam(&bytes).map_err(|detail| Error::UnexpectedOutput {
            command: format!("the golden scene `{name}`"),
            detail,
        })?;
        scenes.insert(name, scene);
    }
    Ok(scenes)
}

/// The backends that may keep references of their own, as
/// `<scene>.<backend>.qoi` (`docs/04-quality.md`).
const BACKENDS: &[&str] = &["metal"];

/// The `.qoi` files in [`REFERENCES`], by name: the stem of each file.
#[derive(Debug, Default)]
struct References {
    /// The references, by scene.
    scenes: BTreeMap<String, Vec<u8>>,
    /// The references of one backend, `<scene>.<backend>`, which nothing
    /// checks before M10.4.
    overrides: Vec<String>,
    /// Other names with a dot, which are no reference's.
    strays: Vec<String>,
}

/// Reads [`References`] from `dir`. There are none until the first
/// `--bless`.
fn read_references(dir: &Path) -> Result<References, Error> {
    let mut references = References::default();
    if !dir.exists() {
        return Ok(references);
    }
    for (name, path) in files_with_extension(dir, "qoi")? {
        match name.split_once('.') {
            None => {
                let bytes = fs::read(&path).map_err(|source| Error::Read { path, source })?;
                references.scenes.insert(name, bytes);
            }
            Some((scene, backend)) if !scene.is_empty() && BACKENDS.contains(&backend) => {
                references.overrides.push(name);
            }
            Some(_) => references.strays.push(name),
        }
    }
    Ok(references)
}

/// The files in `dir` named `<stem>.<extension>`, as `(stem, path)`, sorted
/// by stem.
fn files_with_extension(dir: &Path, extension: &str) -> Result<Vec<(String, PathBuf)>, Error> {
    let read = |source| Error::Read {
        path: dir.to_path_buf(),
        source,
    };
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).map_err(read)? {
        let path = entry.map_err(read)?.path();
        if path.extension().is_some_and(|found| found == extension) && path.is_file() {
            let stem = path.file_stem().and_then(|stem| stem.to_str());
            if let Some(stem) = stem {
                files.push((stem.to_string(), path));
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Reads a PAM file as the scenes write it: `P7`, then `WIDTH`, `HEIGHT`,
/// `DEPTH 4`, `MAXVAL 255` and `TUPLTYPE RGB_ALPHA`, in any order, with `#`
/// comments, one of which may be `# renderer <name>`, then `ENDHDR` and
/// exactly the pixels.
fn parse_pam(bytes: &[u8]) -> Result<Rendered, String> {
    const END: &[u8] = b"\nENDHDR\n";
    let split = bytes
        .windows(END.len())
        .position(|window| window == END)
        .ok_or("there is no `ENDHDR` line")?;
    let (head, rest) = bytes.split_at(split);
    let pixels = &rest[END.len()..];
    let head = std::str::from_utf8(head).map_err(|_| "the header is not text")?;
    let mut lines = head.split('\n');
    if lines.next() != Some("P7") {
        return Err("it does not start with `P7`".to_string());
    }
    let (mut width, mut height, mut depth, mut maxval, mut tupltype) =
        (None, None, None, None, None);
    let mut renderer = String::new();
    for line in lines {
        if let Some(comment) = line.strip_prefix('#') {
            if let Some(name) = comment.strip_prefix(" renderer ") {
                renderer = name.to_string();
            }
            continue;
        }
        let (key, value) = line
            .split_once(' ')
            .ok_or_else(|| format!("the header line `{line}` has no value"))?;
        // Digits only: `parse` would also take a sign.
        let number = || {
            Some(value)
                .filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
                .and_then(|value| value.parse::<u32>().ok())
                .ok_or_else(|| format!("`{key}` is `{value}`, not a number"))
        };
        let field = match key {
            "WIDTH" => &mut width,
            "HEIGHT" => &mut height,
            "DEPTH" => &mut depth,
            "MAXVAL" => &mut maxval,
            "TUPLTYPE" => {
                if tupltype.replace(value).is_some() {
                    return Err(format!("`{key}` is given twice"));
                }
                continue;
            }
            _ => return Err(format!("unknown header line `{line}`")),
        };
        if field.replace(number()?).is_some() {
            return Err(format!("`{key}` is given twice"));
        }
    }
    let (Some(width), Some(height)) = (width, height) else {
        return Err("`WIDTH` or `HEIGHT` is missing".to_string());
    };
    if (depth, maxval, tupltype) != (Some(4), Some(255), Some("RGB_ALPHA")) {
        return Err(
            "it is not 8-bit RGBA: `DEPTH 4`, `MAXVAL 255`, `TUPLTYPE RGB_ALPHA`".to_string(),
        );
    }
    if width == 0 || height == 0 {
        return Err(format!("it is {width}×{height}"));
    }
    let len = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|len| len.checked_mul(4))
        .ok_or_else(|| format!("it is {width}×{height}, too large to hold"))?;
    if u64::try_from(pixels.len()) != Ok(len) {
        return Err(format!(
            "it has {} bytes of pixels, where {width}×{height} takes {len}",
            pixels.len()
        ));
    }
    Ok(Rendered {
        width,
        height,
        renderer,
        pixels: pixels.to_vec(),
    })
}

/// Encodes a scene as QOI. Its pixels are those the scene read back from an
/// sRGB target, or from a linear one, which the header cannot tell apart;
/// the header says sRGB.
fn encode(scene: &Rendered) -> Result<Vec<u8>, Error> {
    let header = Header {
        width: scene.width,
        height: scene.height,
        channels: Channels::Rgba,
        color_space: ColorSpace::Srgb,
    };
    // `parse_pam` checked the size, so this cannot fail.
    qoi::encode(&header, &scene.pixels).map_err(|error| Error::Internal(error.to_string()))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let write = |source| Error::Write {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(write)?;
    }
    fs::write(path, bytes).map_err(write)
}

fn plural(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        _ => format!("{count} {noun}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PAM file as the scenes write it.
    fn pam(width: u32, height: u32, renderer: &str, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = format!(
            "P7\nWIDTH {width}\nHEIGHT {height}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\n\
             # renderer {renderer}\nENDHDR\n"
        )
        .into_bytes();
        bytes.extend_from_slice(pixels);
        bytes
    }

    fn scene(width: u32, height: u32, pixels: Vec<u8>) -> Rendered {
        Rendered {
            width,
            height,
            renderer: "test".to_string(),
            pixels,
        }
    }

    fn qoi_of(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        encode(&scene(width, height, pixels.to_vec())).unwrap()
    }

    /// A 320×180 gray image, with `changed` pixels moved by `by` in one
    /// channel.
    fn gray(changed: usize, channel: usize, by: u8) -> Vec<u8> {
        let mut pixels = [100u8, 100, 100, 255].repeat(320 * 180);
        for pixel in pixels.as_chunks_mut::<4>().0.iter_mut().take(changed) {
            pixel[channel] = pixel[channel].wrapping_add(by);
        }
        pixels
    }

    fn verdict(expected: &[u8], actual: Vec<u8>) -> Verdict {
        judge(
            Some(&scene(320, 180, actual)),
            Some(&qoi_of(320, 180, expected)),
        )
    }

    #[test]
    fn a_difference_of_two_is_the_same_and_three_is_not() {
        let base = gray(0, 0, 0);
        assert_eq!(verdict(&base, base.clone()), Verdict::Same { over: 0 });
        assert_eq!(
            verdict(&base, gray(57_600, 1, 2)),
            Verdict::Same { over: 0 }
        );
        assert_eq!(
            verdict(&base, gray(57_600, 1, 254)),
            Verdict::Same { over: 0 },
            "-2 is as near as +2"
        );
        assert_eq!(verdict(&base, gray(1, 2, 3)), Verdict::Same { over: 1 });
    }

    #[test]
    fn at_most_one_pixel_in_a_thousand_may_differ_by_more() {
        let base = gray(0, 0, 0);
        // 57 600 pixels: 57 may differ, 58 may not.
        assert_eq!(verdict(&base, gray(57, 0, 3)), Verdict::Same { over: 57 });
        assert_eq!(
            verdict(&base, gray(58, 0, 3)),
            Verdict::Differs {
                over: 58,
                total: 57_600,
                largest: 3
            }
        );
        // Alpha counts as much as color.
        assert_eq!(
            verdict(&base, gray(100, 3, 253)),
            Verdict::Differs {
                over: 100,
                total: 57_600,
                largest: 3
            }
        );
    }

    #[test]
    fn a_scene_without_a_readable_reference_of_its_size_fails() {
        let pixels = gray(0, 0, 0);
        let drawn = scene(320, 180, pixels.clone());
        assert_eq!(judge(Some(&drawn), None), Verdict::NoReference);
        assert_eq!(
            judge(None, Some(&qoi_of(320, 180, &pixels))),
            Verdict::NotRendered
        );
        assert_eq!(
            judge(Some(&drawn), Some(&qoi_of(1, 1, &[0, 0, 0, 255]))),
            Verdict::OtherSize {
                width: 1,
                height: 1
            }
        );
        // One side the same is not enough, either way round.
        for (width, height) in [(320, 90), (160, 180), (320, 360)] {
            let pixels = [100u8, 100, 100, 255].repeat((width * height) as usize);
            assert_eq!(
                judge(Some(&drawn), Some(&qoi_of(width, height, &pixels))),
                Verdict::OtherSize { width, height }
            );
        }
        assert!(matches!(
            judge(Some(&drawn), Some(b"not a qoi file")),
            Verdict::BadReference(CodecError::Qoi { offset: 0, .. })
        ));
    }

    #[test]
    fn the_diff_image_is_magenta_where_pixels_differ_and_opaque() {
        let expected = [12, 24, 36, 0, 12, 24, 36, 0, 0, 0, 0, 0];
        let actual = [14, 22, 36, 0, 15, 24, 36, 0, 0, 0, 0, 3];
        assert_eq!(
            diff_image(&expected, &actual),
            [6, 6, 6, 255, 255, 0, 255, 255, 255, 0, 255, 255]
        );
        assert_eq!(diff_image(&[255; 4], &[255; 4]), [63, 63, 63, 255]);
    }

    #[test]
    fn a_pam_file_is_read_as_the_scenes_write_it() {
        let pixels = [1, 2, 3, 4, 5, 6, 7, 8];
        let read = parse_pam(&pam(2, 1, "llvmpipe (LLVM 20.1.2, 256 bits)", &pixels)).unwrap();
        assert_eq!(
            read,
            Rendered {
                width: 2,
                height: 1,
                renderer: "llvmpipe (LLVM 20.1.2, 256 bits)".to_string(),
                pixels: pixels.to_vec(),
            }
        );
        // Lines may come in any order, other comments are skipped, and the
        // renderer may be missing.
        let reordered = b"P7\n# made by hand\nTUPLTYPE RGB_ALPHA\nMAXVAL 255\nDEPTH 4\nHEIGHT 1\n\
                          WIDTH 1\nENDHDR\n\x01\x02\x03\x04";
        let read = parse_pam(reordered).unwrap();
        assert_eq!(
            (read.width, read.height, read.renderer.as_str()),
            (1, 1, "")
        );
    }

    #[test]
    fn a_pam_file_that_is_not_what_the_scenes_write_is_rejected() {
        let good = pam(1, 1, "r", &[0; 4]);
        let text = String::from_utf8(good.clone()).unwrap();
        let bad = [
            text.replace("P7", "P6"),
            text.replace("DEPTH 4", "DEPTH 3"),
            text.replace("MAXVAL 255", "MAXVAL 65535"),
            text.replace("RGB_ALPHA", "RGB"),
            text.replace("WIDTH 1", "WIDTH one"),
            text.replace("WIDTH 1", "WIDTH +1"),
            text.replace("WIDTH 1", "WIDTH 1\nWIDTH 1"),
            text.replace("DEPTH 4", "DEPTH 4\nTUPLTYPE RGB_ALPHA"),
            text.replace("WIDTH 1\n", ""),
            text.replace("WIDTH 1", "BREADTH 1"),
            text.replace("ENDHDR", "END"),
        ];
        for bad in bad {
            assert!(parse_pam(bad.as_bytes()).is_err(), "{bad:?}");
        }
        assert!(parse_pam(&good[..good.len() - 1]).is_err());
        assert_eq!(
            parse_pam(&pam(0, 1, "r", &[])),
            Err("it is 0×1".to_string())
        );
        // A size too large for any file is refused, not overflowed.
        let huge = format!("{u}", u = u32::MAX);
        let huge = text.replace("WIDTH 1", &format!("WIDTH {huge}"));
        let huge = huge.replace("HEIGHT 1", &format!("HEIGHT {}", u32::MAX));
        assert!(parse_pam(huge.as_bytes()).is_err());
        assert!(parse_pam(&[&good[..], &[0]].concat()).is_err());
        assert!(parse_pam(b"").is_err());
    }

    // ---------------------------------------------------------- whole runs

    /// A workspace in `target/tmp` with `files` in it, and the scene output
    /// directory made.
    fn root(test: &str, files: &[(&str, Vec<u8>)]) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/tmp")
            .join(format!("xtask-golden-{test}"));
        // It may be left over from an earlier run.
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(OUTPUT).join(RENDERED)).unwrap();
        for (path, contents) in files {
            write_file(&root.join(path), contents).unwrap();
        }
        root
    }

    fn rendered(name: &str, pixels: &[u8]) -> (String, Vec<u8>) {
        (
            format!("{OUTPUT}/{RENDERED}/{name}.pam"),
            pam(2, 1, "llvmpipe", pixels),
        )
    }

    fn reference(name: &str, pixels: &[u8]) -> (String, Vec<u8>) {
        (format!("{REFERENCES}/{name}.qoi"), qoi_of(2, 1, pixels))
    }

    fn finished(root: &Path, bless: bool, expected: &str) -> (bool, String) {
        let mut out = Vec::new();
        let passed = finish(root, bless, expected, None, &mut out).unwrap();
        (passed, String::from_utf8(out).unwrap())
    }

    fn files(pairs: &[(String, Vec<u8>)]) -> Vec<(&str, Vec<u8>)> {
        pairs
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.clone()))
            .collect()
    }

    const RED: [u8; 8] = [255, 0, 0, 255, 255, 0, 0, 255];
    const BLUE: [u8; 8] = [0, 0, 255, 255, 0, 0, 255, 255];

    #[test]
    fn matching_scenes_pass() {
        let root = root(
            "pass",
            &files(&[
                rendered("a", &RED),
                reference("a", &RED),
                rendered("b", &BLUE),
                reference("b", &[1, 0, 254, 255, 0, 2, 255, 255]),
            ]),
        );
        let (passed, out) = finished(&root, false, "llvm");
        assert!(passed, "{out}");
        assert_eq!(
            out,
            "golden: 2 scenes drawn by `llvmpipe`\n  a  ok\n  b  ok\ngolden: ok (2 scenes)\n"
        );
        assert!(!root.join(OUTPUT).join("a.actual.qoi").exists());
    }

    #[test]
    fn a_scene_that_differs_fails_and_leaves_its_images() {
        let root = root(
            "differs",
            &files(&[rendered("scene", &RED), reference("scene", &BLUE)]),
        );
        let (passed, out) = finished(&root, false, "");
        assert!(!passed, "{out}");
        assert!(
            out.contains(
                "  scene  FAILED: 2 of 2 pixels differ by more than 2, where at most 0 may, by \
                 up to 255; see target/golden/scene.diff.qoi\n"
            ),
            "{out}"
        );
        assert!(out.ends_with("golden: FAILED (1 of 1 scenes)\n"), "{out}");
        let output = root.join(OUTPUT);
        let actual = fs::read(output.join("scene.actual.qoi")).unwrap();
        assert_eq!(actual, qoi_of(2, 1, &RED));
        let diff = qoi::decode(
            &fs::read(output.join("scene.diff.qoi")).unwrap(),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(diff.pixels, [255, 0, 255, 255].repeat(2));
        // The reference is left alone.
        let kept = fs::read(root.join(REFERENCES).join("scene.qoi")).unwrap();
        assert_eq!(kept, qoi_of(2, 1, &BLUE));
    }

    #[test]
    fn a_scene_without_a_reference_and_a_reference_without_a_scene_fail() {
        let root = root(
            "unpaired",
            &files(&[rendered("new", &RED), reference("gone", &RED)]),
        );
        let (passed, out) = finished(&root, false, "");
        assert!(!passed, "{out}");
        assert!(
            out.contains("  gone  FAILED: not drawn: its scene is gone"),
            "{out}"
        );
        assert!(
            out.contains(
                "  new   FAILED: no reference; look at target/golden/new.actual.qoi, then run \
                 `cargo xtask golden --bless`\n"
            ),
            "{out}"
        );
        assert!(out.ends_with("golden: FAILED (2 of 2 scenes)\n"), "{out}");
        assert!(root.join(OUTPUT).join("new.actual.qoi").exists());
    }

    #[test]
    fn bless_writes_the_references_that_differ_and_keeps_the_rest() {
        let near = [254, 1, 0, 255, 255, 0, 2, 255];
        let pairs = [
            rendered("differs", &RED),
            reference("differs", &BLUE),
            rendered("near", &RED),
            reference("near", &near),
            rendered("new", &BLUE),
        ];
        let root = root("bless", &files(&pairs));
        let (passed, out) = finished(&root, true, "");
        assert!(passed, "{out}");
        assert!(
            out.contains("  differs  updated: 2 pixels were over the tolerance\n"),
            "{out}"
        );
        assert!(out.contains("  near     ok\n"), "{out}");
        assert!(out.contains("  new      new\n"), "{out}");
        assert!(
            out.ends_with("golden: blessed (1 new, 1 updated, 1 kept)\n"),
            "{out}"
        );
        let references = root.join(REFERENCES);
        assert_eq!(
            fs::read(references.join("differs.qoi")).unwrap(),
            qoi_of(2, 1, &RED)
        );
        assert_eq!(
            fs::read(references.join("new.qoi")).unwrap(),
            qoi_of(2, 1, &BLUE)
        );
        // A reference within tolerance keeps its bytes.
        assert_eq!(
            fs::read(references.join("near.qoi")).unwrap(),
            qoi_of(2, 1, &near)
        );
        // And the next run passes.
        let (passed, out) = finished(&root, false, "");
        assert!(passed, "{out}");
    }

    #[test]
    fn bless_cannot_mend_a_reference_without_a_scene() {
        let root = root(
            "bless-gone",
            &files(&[
                rendered("kept", &RED),
                reference("kept", &RED),
                reference("gone", &RED),
            ]),
        );
        let (passed, out) = finished(&root, true, "");
        assert!(!passed, "{out}");
        assert!(out.ends_with("golden: FAILED (1 of 2 scenes)\n"), "{out}");
    }

    #[test]
    fn backend_references_are_listed_but_not_checked() {
        let root = root(
            "override",
            &files(&[
                rendered("a", &RED),
                reference("a", &RED),
                reference("a.metal", &BLUE),
            ]),
        );
        let (passed, out) = finished(&root, false, "");
        assert!(passed, "{out}");
        assert!(
            out.contains("  a.metal  skipped: a backend's own reference"),
            "{out}"
        );
        // A copied `actual` image is no reference, and is not skipped.
        let stray = (format!("{REFERENCES}/a.actual.qoi"), qoi_of(2, 1, &BLUE));
        write_file(&root.join(&stray.0), &stray.1).unwrap();
        let (passed, out) = finished(&root, false, "");
        assert!(!passed, "{out}");
        assert!(
            out.contains(
                "  a.actual  FAILED: not the name of a reference; rename \
                 tests/golden/a.actual.qoi to <scene>.qoi, or remove it\n"
            ),
            "{out}"
        );
        assert!(out.ends_with("golden: FAILED (1 of 2 scenes)\n"), "{out}");
    }

    #[test]
    fn scenes_drawn_by_another_renderer_than_asked_for_fail() {
        let root = root(
            "renderer",
            &files(&[rendered("a", &RED), reference("a", &BLUE)]),
        );
        let (passed, out) = finished(&root, true, "NVIDIA");
        assert!(!passed, "{out}");
        assert!(
            out.starts_with(
                "golden: 1 scene drawn by `llvmpipe`\n\
                 golden: FAILED: NADIR_EXPECT_GL_RENDERER asks for `NVIDIA`. With libglvnd"
            ),
            "{out}"
        );
        // Nothing is blessed.
        let kept = fs::read(root.join(REFERENCES).join("a.qoi")).unwrap();
        assert_eq!(kept, qoi_of(2, 1, &BLUE));
    }

    #[test]
    fn only_llvmpipe_may_bless() {
        let nvidia = (
            format!("{OUTPUT}/{RENDERED}/a.pam"),
            pam(2, 1, "NVIDIA GeForce RTX 3070", &RED),
        );
        let root = root("bless-renderer", &files(&[nvidia, reference("a", &BLUE)]));
        let (passed, out) = finished(&root, true, "");
        assert!(!passed, "{out}");
        assert!(
            out.contains("FAILED: references are drawn by llvmpipe, as CI draws them."),
            "{out}"
        );
        let kept = fs::read(root.join(REFERENCES).join("a.qoi")).unwrap();
        assert_eq!(kept, qoi_of(2, 1, &BLUE));
        // Checking against them is fine.
        let (passed, out) = finished(&root, false, "");
        assert!(!passed, "{out}");
        assert!(out.contains("  a  FAILED: 2 of 2 pixels"), "{out}");
    }

    #[test]
    fn every_scene_must_come_from_one_renderer() {
        let other = (
            format!("{OUTPUT}/{RENDERED}/b.pam"),
            pam(2, 1, "llvmpipe (other)", &RED),
        );
        let root = root(
            "renderers",
            &files(&[rendered("a", &RED), reference("a", &RED), other]),
        );
        let (passed, out) = finished(&root, false, "");
        assert!(!passed, "{out}");
        assert_eq!(
            out,
            "golden: 2 scenes drawn by `llvmpipe`, `llvmpipe (other)`\n\
             golden: FAILED: one run must draw every scene with one renderer\n"
        );
    }

    #[test]
    fn when_the_scenes_fail_their_own_checks_they_are_compared_and_not_blessed() {
        for bless in [false, true] {
            let root = root(
                &format!("checks-{bless}"),
                &files(&[
                    rendered("same", &RED),
                    reference("same", &RED),
                    rendered("differs", &RED),
                    reference("differs", &BLUE),
                    rendered("drawn", &RED),
                    reference("undrawn", &RED),
                ]),
            );
            let mut out = Vec::new();
            let passed = finish(&root, bless, "", Some("cargo test"), &mut out).unwrap();
            let out = String::from_utf8(out).unwrap();
            assert!(!passed, "{out}");
            assert!(
                out.starts_with("golden: `cargo test` failed, as it says above;"),
                "{out}"
            );
            assert!(out.contains("  differs  FAILED: 2 of 2 pixels"), "{out}");
            // A scene that saved nothing failed before it could.
            assert!(
                out.contains(
                    "  undrawn  FAILED: not drawn: its test failed before it saved what it \
                     drew, as it says above\n"
                ),
                "{out}"
            );
            assert!(out.ends_with("golden: FAILED (3 of 4 scenes)\n"), "{out}");
            assert!(root.join(OUTPUT).join("differs.actual.qoi").exists());
            assert!(root.join(OUTPUT).join("differs.diff.qoi").exists());
            assert!(!root.join(REFERENCES).join("drawn.qoi").exists());
            let kept = fs::read(root.join(REFERENCES).join("differs.qoi")).unwrap();
            assert_eq!(kept, qoi_of(2, 1, &BLUE));
        }
        // With every scene as its reference, the failed command still fails
        // the run.
        let only = self::root(
            "checks-only",
            &files(&[rendered("a", &RED), reference("a", &RED)]),
        );
        let mut out = Vec::new();
        let passed = finish(&only, false, "", Some("cargo test"), &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(!passed, "{out}");
        assert!(
            out.ends_with("  a  ok\ngolden: FAILED: `cargo test` failed\n"),
            "{out}"
        );
    }

    #[test]
    fn a_reference_of_another_size_or_unreadable_fails_and_bless_mends_it() {
        let pairs = [
            rendered("sized", &RED),
            (format!("{REFERENCES}/sized.qoi"), qoi_of(1, 1, &RED[..4])),
            rendered("broken", &RED),
            (format!("{REFERENCES}/broken.qoi"), b"qoif".to_vec()),
        ];
        let root = root("mend", &files(&pairs));
        let (passed, out) = finished(&root, false, "");
        assert!(!passed, "{out}");
        assert!(
            out.contains("  broken  FAILED: the reference is unreadable: QOI error at byte 4"),
            "{out}"
        );
        assert!(
            out.contains(
                "  sized   FAILED: the reference is 1×1; see target/golden/sized.actual.qoi\n"
            ),
            "{out}"
        );
        let output = root.join(OUTPUT);
        for name in ["sized", "broken"] {
            assert!(output.join(format!("{name}.actual.qoi")).exists(), "{name}");
            assert!(!output.join(format!("{name}.diff.qoi")).exists(), "{name}");
        }

        let (passed, out) = finished(&root, true, "");
        assert!(passed, "{out}");
        assert!(
            out.contains("  broken  updated: the old one was unreadable\n"),
            "{out}"
        );
        assert!(out.contains("  sized   updated: it was 1×1\n"), "{out}");
        for name in ["sized", "broken"] {
            let written = fs::read(root.join(REFERENCES).join(format!("{name}.qoi"))).unwrap();
            assert_eq!(written, qoi_of(2, 1, &RED), "{name}");
        }
    }

    #[test]
    fn a_run_starts_from_an_empty_output_and_names_it_to_the_scenes() {
        let root = root(
            "wiring",
            &files(&[
                reference("a", &RED),
                (
                    format!("{OUTPUT}/{RENDERED}/stale.pam"),
                    pam(2, 1, "llvmpipe", &RED),
                ),
                (format!("{OUTPUT}/old.diff.qoi"), Vec::new()),
            ]),
        );
        let mut seen = None;
        let mut draw = |dir: &Path| {
            seen = Some(dir.to_path_buf());
            // Only what this run draws is there.
            assert_eq!(fs::read_dir(dir).unwrap().count(), 0);
            assert!(!dir.join("../old.diff.qoi").exists());
            write_file(&dir.join("a.pam"), &pam(2, 1, "llvmpipe", &RED))?;
            Ok(None)
        };
        let mut out = Vec::new();
        let passed = super::run(&root, false, "", &mut draw, &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(passed, "{out}");
        assert_eq!(seen, Some(root.join(OUTPUT).join(RENDERED)));
        assert!(!out.contains("stale"), "{out}");
    }

    #[test]
    fn a_failed_scene_command_fails_the_run_and_blesses_nothing() {
        let root = root("wiring-failed", &files(&[reference("a", &BLUE)]));
        for bless in [false, true] {
            let mut draw = |dir: &Path| {
                write_file(&dir.join("a.pam"), &pam(2, 1, "llvmpipe", &RED))?;
                Ok(Some("cargo test".to_string()))
            };
            let mut out = Vec::new();
            let passed = super::run(&root, bless, "", &mut draw, &mut out).unwrap();
            let out = String::from_utf8(out).unwrap();
            assert!(!passed, "{out}");
            assert!(out.contains("  a  FAILED: 2 of 2 pixels"), "{out}");
            let kept = fs::read(root.join(REFERENCES).join("a.qoi")).unwrap();
            assert_eq!(kept, qoi_of(2, 1, &BLUE));
        }
        // An error that is not a failed command ends the run with it.
        let mut draw = |_: &Path| Err(Error::Internal("no cargo".to_string()));
        let result = super::run(&root, false, "", &mut draw, &mut Vec::new());
        assert!(matches!(result, Err(Error::Internal(_))), "{result:?}");
    }

    #[test]
    fn no_scenes_and_no_references_fail() {
        let root = root("empty", &[]);
        let (passed, out) = finished(&root, false, "");
        assert!(!passed, "{out}");
        assert_eq!(
            out,
            "golden: 0 scenes drawn by nothing\n\
             golden: FAILED: no scene drew anything, and there are no references\n"
        );
    }

    #[test]
    fn golden_takes_only_bless() {
        for args in [&["--fast"][..], &["scene"], &["--bless", "x"]] {
            let mut out = Vec::new();
            let result = command(Args::new(args.iter().copied()), Path::new("."), &mut out);
            assert!(
                matches!(result, Err(Error::Usage(_))),
                "{args:?}: {result:?}"
            );
            assert!(out.is_empty());
        }
    }
}
