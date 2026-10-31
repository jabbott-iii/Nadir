//! Which strings are asset paths, and the type that holds one.

use std::fmt;

use crate::AssetError;
use crate::assets::lowercase;

/// An asset's path below the root of its [`Source`](crate::Source), such as
/// `sprites/hero.qoi`: the only form a source is given.
///
/// It is normalized: its ASCII letters are lowercased, so that
/// `Sprites/Hero.qoi` is the same path, read from the same file, on every
/// system. It is then checked as one byte grammar, the same everywhere: `/`
/// between its parts, none of them empty, `.` or `..`, ending in a dot or a
/// space, or named like a Windows device, and no control character, `\`,
/// `:` or `*?"<>|`. Nothing else is rewritten: a path that fails is refused,
/// not repaired.
///
/// ```
/// use nadir_assets::AssetPath;
///
/// let path = AssetPath::new("Sprites/Hero.qoi").expect("an asset path");
/// assert_eq!(path.as_str(), "sprites/hero.qoi");
/// assert_eq!(path.extension(), Some("qoi"));
/// assert!(AssetPath::new("../hero.qoi").is_err());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetPath(String);

impl AssetPath {
    /// `path`, lowercased, if it is then an asset path.
    ///
    /// # Errors
    ///
    /// [`AssetError::BadPath`], saying why it is not one.
    pub fn new(path: &str) -> Result<Self, AssetError> {
        let path = lowercase(path);
        check(&path).map_err(AssetError::BadPath)?;
        Ok(Self(path.into_owned()))
    }

    /// The path, lowercased.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// What its last part has after its last dot, if it has a dot: `qoi`
    /// for `sprites/hero.qoi`.
    pub fn extension(&self) -> Option<&str> {
        let name = self.0.rsplit('/').next()?;
        name.rsplit_once('.').map(|(_, extension)| extension)
    }
}

impl fmt::Display for AssetPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The longest path accepted, in bytes.
const MAX_LEN: usize = 1024;

/// Windows opens a device for a file with one of these names, whatever its
/// extension, and with spaces after it.
const DEVICE_NAMES: [&str; 24] = [
    "con", "prn", "aux", "nul", "conin$", "conout$", "com1", "com2", "com3", "com4", "com5",
    "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8",
    "lpt9",
];

/// Whether Windows opens a device for a file whose name starts `stem` (up
/// to the first dot).
fn is_device_name(stem: &str) -> bool {
    // Every device name starts with one of these: most names are known at
    // once, which matters for a storm of loads.
    if !matches!(
        stem.as_bytes().first(),
        Some(b'a' | b'c' | b'l' | b'n' | b'p' | b'A' | b'C' | b'L' | b'N' | b'P')
    ) {
        return false;
    }
    let stem = stem.trim_end_matches(' ');
    if DEVICE_NAMES
        .iter()
        .any(|name| stem.eq_ignore_ascii_case(name))
    {
        return true;
    }
    // COM and LPT ports numbered with a superscript 1, 2 or 3.
    match (stem.get(..3), stem.get(3..)) {
        (Some(port), Some(digit)) => {
            (port.eq_ignore_ascii_case("com") || port.eq_ignore_ascii_case("lpt"))
                && ["\u{b9}", "\u{b2}", "\u{b3}"].contains(&digit)
        }
        _ => false,
    }
}

/// Checks that `path` is an asset path, the same way on every system: a
/// relative path, below the asset root, that each of Windows, macOS and
/// Linux opens as the same file name.
///
/// Its parts are separated by `/`. None is empty, `.` or `..`, ends in a dot
/// or a space, or is a Windows device name. No byte is a control character,
/// `\`, `:` or one of `*?"<>|`. The server lowercases a path before it
/// checks it.
pub(crate) fn check(path: &str) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("it is empty");
    }
    if path.len() > MAX_LEN {
        return Err("it is longer than 1024 bytes");
    }
    for byte in path.bytes() {
        match byte {
            0..0x20 | 0x7f => return Err("it has a control character"),
            b'\\' => return Err("it has a `\\`: parts are separated by `/`"),
            b':' => return Err("it has a `:`"),
            b'*' | b'?' | b'"' | b'<' | b'>' | b'|' => {
                return Err("it has a character Windows does not allow in names");
            }
            _ => {}
        }
    }
    for part in path.split('/') {
        if part.is_empty() {
            return Err("it is absolute, or has an empty part");
        }
        if part == "." || part == ".." {
            return Err("it has a `.` or `..` part");
        }
        if part.ends_with('.') || part.ends_with(' ') {
            return Err("a part ends in a dot or a space");
        }
        if is_device_name(part.split('.').next().unwrap_or(part)) {
            return Err("a part is a Windows device name");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Component, Path};

    use nadir_core::testing::check as property;

    use super::*;

    #[test]
    fn ordinary_paths_pass() {
        for path in [
            "hero.qoi",
            "sprites/hero.qoi",
            "a/b/c/d.e.qoi",
            "Sprites/Hero.qoi",
            "levels/level 1.klvl",
            "music/théâtre.qoa",
            "console.qoi",
            "com10.qoi",
            "com\u{b9}x.qoi",
            "conin.qoi",
        ] {
            assert_eq!(check(path), Ok(()), "{path}");
        }
    }

    #[test]
    fn paths_that_leave_the_root_or_differ_by_system_fail() {
        for path in [
            "",
            "/hero.qoi",
            "//server/share/hero.qoi",
            "C:hero.qoi",
            "C:/hero.qoi",
            "a:b.qoi",
            "a\\b.qoi",
            "./hero.qoi",
            "a/../b.qoi",
            "..",
            "a//b.qoi",
            "a/",
            "a/b.",
            "a /b.qoi",
            "con.qoi",
            "a/NUL",
            "lpt1.tar.qoi",
            "con .qoi",
            "a/nul  .txt",
            "COM\u{b9}.qoi",
            "lpt\u{b3}",
            "CONIN$",
            "conout$.txt",
            "a\0.qoi",
            "a\n.qoi",
            "what?.qoi",
            "a|b.qoi",
        ] {
            assert!(check(path).is_err(), "{path:?} passed");
        }
        assert!(check(&"a".repeat(MAX_LEN + 1)).is_err());
        assert_eq!(check(&"a".repeat(MAX_LEN)), Ok(()));
    }

    #[test]
    fn an_asset_path_is_lowercased_then_checked() {
        let path = AssetPath::new("Notes/HELLO.Txt").expect("an asset path");
        assert_eq!(path.as_str(), "notes/hello.txt");
        assert_eq!(path.to_string(), "notes/hello.txt");
        assert_eq!(path, AssetPath::new("notes/hello.txt").expect("the same"));
        // Only ASCII letters change.
        assert_eq!(
            AssetPath::new("Théâtre.QOA")
                .expect("an asset path")
                .as_str(),
            "théâtre.qoa"
        );
        // Lowercased, `CON` is still a device name.
        assert_eq!(
            AssetPath::new("CON.qoi"),
            Err(AssetError::BadPath("a part is a Windows device name"))
        );
        assert_eq!(
            AssetPath::new("a\\b.qoi"),
            Err(AssetError::BadPath(
                "it has a `\\`: parts are separated by `/`"
            ))
        );
    }

    #[test]
    fn the_extension_is_the_last_parts_after_its_last_dot() {
        let extension = |path: &str| {
            let path = AssetPath::new(path).expect("an asset path");
            path.extension().map(str::to_string)
        };
        assert_eq!(extension("a/b.tar.qoi").as_deref(), Some("qoi"));
        assert_eq!(extension("hero.QOI").as_deref(), Some("qoi"));
        assert_eq!(extension("a.b/hero").as_deref(), None);
        assert_eq!(extension("notes").as_deref(), None);
        assert_eq!(extension(".hidden").as_deref(), Some("hidden"));
    }

    #[test]
    fn a_path_that_passes_stays_below_the_root() {
        const ALPHABET: &[u8] = b"ab./\\: \0C";
        property(
            "asset paths stay below the root",
            20_000,
            |rng| {
                let len = rng.range(0..12u32) as usize;
                (0..len)
                    .map(|_| char::from(*rng.choose(ALPHABET).expect("not empty")))
                    .collect::<String>()
            },
            |path| {
                if check(path).is_err() {
                    return Ok(());
                }
                let joined = Path::new("root").join(path);
                let below = joined.strip_prefix("root").is_ok_and(|rest| {
                    rest.components()
                        .all(|part| matches!(part, Component::Normal(_)))
                });
                if below {
                    Ok(())
                } else {
                    Err(format!("{path:?} joins as {}", joined.display()))
                }
            },
        );
    }
}
