//! Which strings are asset paths.

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
