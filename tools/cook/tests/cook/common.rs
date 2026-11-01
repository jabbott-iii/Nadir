//! What the cook's tests share: PNG files written from pixels, source trees
//! in fresh folders, and reading back what the cook wrote.

use std::fs;
use std::path::{Path, PathBuf};

use nadir_codec::adler32;
use nadir_codec::json::Value;
use nadir_codec::qoi::{self, Image};
use nadir_codec::{Limits, kbj};
use nadir_cook::{CookError, Options, Report};
use nadir_core::hash::crc32;

/// A PNG of `width` × `height` RGBA8 pixels, rows from the top, with each
/// of `extra` (a chunk's type and data) after its header. The image data is
/// one zlib stream of stored blocks, every row filtered with None.
pub(crate) fn png(width: u32, height: u32, pixels: &[u8], extra: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    assert_eq!(
        pixels.len(),
        (width * height * 4) as usize,
        "the pixels' count"
    );
    let mut raw = Vec::new();
    for row in pixels.chunks(width as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (index, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(index + 1 == blocks.len()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32::checksum(&raw).to_be_bytes());

    let mut file = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut file, b"IHDR", &header);
    for (kind, data) in extra {
        chunk(&mut file, kind, data);
    }
    chunk(&mut file, b"IDAT", &zlib);
    chunk(&mut file, b"IEND", &[]);
    file
}

fn chunk(file: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    file.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = file.len();
    file.extend_from_slice(kind);
    file.extend_from_slice(data);
    let crc = crc32(&file[start..]);
    file.extend_from_slice(&crc.to_be_bytes());
}

/// `width` × `height` pixels of one color.
pub(crate) fn filled(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    rgba.repeat((width * height) as usize)
}

/// A PNG of one color.
pub(crate) fn plain(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    png(width, height, &filled(width, height, rgba), &[])
}

/// A fresh, empty folder for one test, under `target/tmp`, named for the
/// test and the process, so that runs at once do not share it.
pub(crate) fn dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("nadir-cook")
        .join(format!("{test}-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("remove an earlier run's folder");
    }
    fs::create_dir_all(&dir).expect("create the test folder");
    dir
}

/// Writes `bytes` at `path` below `root`, making its folders.
pub(crate) fn write(root: &Path, path: &str, bytes: impl AsRef<[u8]>) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("a file in a folder")).expect("make the folders");
    fs::write(path, bytes).expect("write the file");
}

/// Every file below `root`, with its bytes, by its path below `root`, in
/// order.
pub(crate) fn files(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).expect("read the folder") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let name = path
                    .strip_prefix(root)
                    .expect("below the root")
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push((name, fs::read(&path).expect("read the file")));
            }
        }
    }
    found.sort();
    found
}

/// The paths of [`files`].
pub(crate) fn names(root: &Path) -> Vec<String> {
    files(root).into_iter().map(|(name, _)| name).collect()
}

/// Cooks `source` into `out` as the cooker `test`.
pub(crate) fn cook(source: &Path, out: &Path) -> Result<Report, CookError> {
    cook_with(source, out, false, "test")
}

/// Cooks with the options given.
pub(crate) fn cook_with(
    source: &Path,
    out: &Path,
    force: bool,
    cooker: &str,
) -> Result<Report, CookError> {
    nadir_cook::cook(
        source,
        out,
        &Options {
            force,
            cooker: cooker.to_string(),
        },
    )
}

/// Each job's name, and whether it was cooked.
pub(crate) fn jobs(report: &Report) -> Vec<(String, bool)> {
    report
        .jobs
        .iter()
        .map(|job| (job.name.clone(), job.cooked))
        .collect()
}

/// The KBJ file at `path` below `out`.
pub(crate) fn kbj(out: &Path, path: &str) -> Value {
    let bytes = fs::read(out.join(path)).expect("read the file");
    kbj::decode(&bytes, Limits::default()).expect("a KBJ file")
}

/// The QOI file at `path` below `out`.
pub(crate) fn qoi(out: &Path, path: &str) -> Image {
    let bytes = fs::read(out.join(path)).expect("read the file");
    qoi::decode(&bytes, Limits::default()).expect("a QOI file")
}

/// A whole number from a KBJ value.
pub(crate) fn int(value: &Value, key: &str) -> u32 {
    let number = value.get(key).and_then(Value::as_i64).expect("a number");
    u32::try_from(number).expect("not negative")
}

/// The sprite named `name` in an atlas file.
pub(crate) fn sprite<'a>(atlas: &'a Value, name: &str) -> &'a Value {
    atlas
        .get("sprites")
        .and_then(Value::as_array)
        .expect("sprites")
        .iter()
        .find(|sprite| sprite.get("name").and_then(Value::as_str) == Some(name))
        .unwrap_or_else(|| panic!("no sprite `{name}`"))
}

/// A sprite's frames.
pub(crate) fn frames(sprite: &Value) -> &[Value] {
    sprite
        .get("frames")
        .and_then(Value::as_array)
        .expect("frames")
}

/// The pixel at `(x, y)` of an image.
pub(crate) fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * image.header.width + x) * 4) as usize;
    image.pixels[at..at + 4].try_into().expect("4 bytes")
}

/// The pixels of a frame of an atlas, read from its page.
pub(crate) fn frame_pixels(out: &Path, atlas: &Value, frame: &Value) -> (u32, u32, Vec<u8>) {
    let pages = atlas.get("pages").and_then(Value::as_array).expect("pages");
    let page = &pages[int(frame, "page") as usize];
    let image = qoi(
        out,
        page.get("path").and_then(Value::as_str).expect("a path"),
    );
    let (x, y, w, h) = (
        int(frame, "x"),
        int(frame, "y"),
        int(frame, "w"),
        int(frame, "h"),
    );
    let mut pixels = Vec::new();
    for row in y..y + h {
        for column in x..x + w {
            pixels.extend_from_slice(&pixel(&image, column, row));
        }
    }
    (w, h, pixels)
}

/// Whether this system tells file names apart by case in `dir`.
pub(crate) fn case_sensitive(dir: &Path) -> bool {
    fs::write(dir.join("probe"), b"").expect("write a probe");
    let sensitive = !dir.join("PROBE").exists();
    fs::remove_file(dir.join("probe")).expect("remove the probe");
    sensitive
}
