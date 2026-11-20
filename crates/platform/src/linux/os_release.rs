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

//! The distribution's name, from `os-release`, and the kernel's version.

use std::env::consts::ARCH;
use std::fs::File;
use std::io::{self, Read};

/// Where the distribution describes itself, in the order `os-release(5)`
/// says to look: the second only when the first is not there.
const OS_RELEASE: [&str; 2] = ["/etc/os-release", "/usr/lib/os-release"];

/// The kernel's version, as `uname -r` gives it.
const KERNEL_RELEASE: &str = "/proc/sys/kernel/osrelease";

/// The most read of either file: real ones are under a kilobyte.
const MAX_BYTES: u64 = 64 * 1024;

/// The most characters kept of a name or a version.
const MAX_CHARS: usize = 200;

/// Describes the distribution, the kernel and the architecture, as
/// [`crate::os_description`] documents.
pub(crate) fn os_description() -> String {
    let pretty = os_release().and_then(|text| pretty_name(&text));
    let kernel = read_small(KERNEL_RELEASE).ok();
    describe(pretty.as_deref(), kernel.as_deref(), ARCH)
}

/// The text of the first of [`OS_RELEASE`] that is there, or `None` if
/// neither is, or the one there cannot be read.
fn os_release() -> Option<String> {
    for path in OS_RELEASE {
        match read_small(path) {
            Ok(text) => return Some(text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
    }
    None
}

/// The text of the file at `path`, its first [`MAX_BYTES`] only, with
/// what is not UTF-8 replaced.
fn read_small(path: &str) -> io::Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?.take(MAX_BYTES).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// One line: `<pretty> (Linux <kernel>, <arch>)`. `os-release(5)` says a
/// distribution without a pretty name is `Linux`.
fn describe(pretty: Option<&str>, kernel: Option<&str>, arch: &str) -> String {
    let pretty = pretty.map(clean).filter(|name| !name.is_empty());
    let kernel = kernel.map(clean).filter(|version| !version.is_empty());
    format!(
        "{} (Linux {}, {arch})",
        pretty.as_deref().unwrap_or("Linux"),
        kernel.as_deref().unwrap_or("version unknown")
    )
}

/// The value of `PRETTY_NAME` in the text of an `os-release` file, the last
/// if it is set more than once, as a shell would read it.
fn pretty_name(text: &str) -> Option<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("PRETTY_NAME="))
        .next_back()
        .map(unquote)
}

/// A value of `os-release(5)`, read as a shell reads a word: in double
/// quotes a backslash escapes only `"`, `\`, `$` and `` ` ``; in single
/// quotes nothing is escaped; outside quotes a backslash escapes any
/// character. Quotes open and close as often as they come (`"a"'b'c` is
/// `abc`), and one left open runs to the end.
fn unquote(value: &str) -> String {
    let mut word = String::with_capacity(value.len());
    let mut chars = value.trim().chars().peekable();
    let mut quote = None;
    while let Some(c) = chars.next() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), c) if c == open => quote = None,
            (Some('\''), c) => word.push(c),
            (Some(_), '\\') => match chars.next_if(|next| matches!(next, '"' | '\\' | '$' | '`')) {
                Some(escaped) => word.push(escaped),
                None => word.push('\\'),
            },
            (None, '\\') => word.extend(chars.next()),
            (_, c) => word.push(c),
        }
    }
    word
}

/// `text` on one line, its control characters made spaces, trimmed, and
/// [`MAX_CHARS`] long at most.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pretty_name_is_read_as_a_shell_would() {
        let ubuntu = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\n\
                      PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nID=ubuntu\n";
        assert_eq!(pretty_name(ubuntu).as_deref(), Some("Ubuntu 24.04.1 LTS"));
        // Unquoted, single quotes, escapes, and the last of two.
        assert_eq!(pretty_name("PRETTY_NAME=Arch").as_deref(), Some("Arch"));
        assert_eq!(
            pretty_name("PRETTY_NAME='Fedora Linux 41'").as_deref(),
            Some("Fedora Linux 41")
        );
        assert_eq!(
            pretty_name(r#"PRETTY_NAME="A \"quoted\" \\ \$name""#).as_deref(),
            Some(r#"A "quoted" \ $name"#)
        );
        assert_eq!(
            pretty_name("PRETTY_NAME=\"Old\"\n  PRETTY_NAME=\"New\"  \n").as_deref(),
            Some("New")
        );
        // A quote left open, and a backslash at the end.
        assert_eq!(pretty_name("PRETTY_NAME=\"Open").as_deref(), Some("Open"));
        assert_eq!(pretty_name("PRETTY_NAME=end\\").as_deref(), Some("end"));
        // Backslashes as a shell keeps them: all in single quotes, and in
        // double quotes those before other characters; quotes that open and
        // close again.
        assert_eq!(
            pretty_name(r"PRETTY_NAME='C:\path \$x'").as_deref(),
            Some(r"C:\path \$x")
        );
        assert_eq!(
            pretty_name(r#"PRETTY_NAME="a\b \`c\`""#).as_deref(),
            Some(r"a\b `c`")
        );
        assert_eq!(
            pretty_name(r#"PRETTY_NAME=un\ quoted"double"'single'"#).as_deref(),
            Some("un quoteddoublesingle")
        );
        // Not there, or only in a comment or another variable's name.
        assert_eq!(pretty_name(""), None);
        assert_eq!(pretty_name("# PRETTY_NAME=\"Commented\"\nNAME=x"), None);
        assert_eq!(pretty_name("MY_PRETTY_NAME=\"Other\""), None);
    }

    #[test]
    fn the_description_has_the_distribution_the_kernel_and_the_architecture() {
        assert_eq!(
            describe(
                Some("Pop!_OS 24.04 LTS"),
                Some("7.2.15-generic\n"),
                "x86_64"
            ),
            "Pop!_OS 24.04 LTS (Linux 7.2.15-generic, x86_64)"
        );
        // What is missing, empty or blank is said so.
        assert_eq!(
            describe(None, None, "aarch64"),
            "Linux (Linux version unknown, aarch64)"
        );
        assert_eq!(
            describe(Some(" \t"), Some(""), "x86_64"),
            "Linux (Linux version unknown, x86_64)"
        );
        // A name stays on one line, and short.
        assert_eq!(
            describe(Some("Two\nlines"), Some("6.1"), "x86_64"),
            "Two lines (Linux 6.1, x86_64)"
        );
        let long = "x".repeat(1000);
        let described = describe(Some(&long), Some("6.1"), "x86_64");
        assert_eq!(described.len(), MAX_CHARS + " (Linux 6.1, x86_64)".len());
    }

    #[test]
    fn this_system_is_described() {
        let description = os_description();
        // Every Linux has the kernel's version; most have an os-release.
        let kernel = read_small(KERNEL_RELEASE).expect("the kernel's version");
        // A version, such as `6.8.0-49-generic`, not another of the kernel's
        // files.
        assert!(
            kernel.starts_with(|c: char| c.is_ascii_digit()) && kernel.contains('.'),
            "{kernel}"
        );
        assert!(
            description.ends_with(&format!(" (Linux {}, {ARCH})", clean(&kernel))),
            "{description}"
        );
        assert!(!description.contains("version unknown"), "{description}");
        let pretty = os_release().and_then(|text| pretty_name(&text));
        if let Some(pretty) = pretty.as_deref().map(clean).filter(|name| !name.is_empty()) {
            assert!(description.starts_with(&pretty), "{description}");
        }
        // A missing file is no os-release.
        let missing = read_small("/nonexistent/os-release").expect_err("no such file");
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
    }
}
