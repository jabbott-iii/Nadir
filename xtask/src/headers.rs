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

//! `cargo xtask headers`: every source file starts with the Apache-2.0
//! header of `LICENSE`'s appendix, in its comment syntax (ADR-0037).
//!
//! A source file is a Rust, GLSL, Metal, Python, shell, JavaScript, CSS or
//! HTML file, a `Makefile`, a `Dockerfile`, or a workflow in
//! `.github/workflows` ([`kind`]). Its header is [`HEADER`], a comment line
//! each (or one block comment, in CSS and HTML), then a blank line. It
//! starts on line 1, or on line 2 after a GLSL `#version` line, which
//! nadir-gfx requires first, a `#!` line or an HTML doctype.
//!
//! Build output (the top-level `target`) and hidden directories other than
//! `.github` are skipped, and symbolic links are not followed.

use std::fs;
use std::path::{Path, PathBuf};

use crate::check::Report;
use crate::error::Error;
use crate::unsafe_audit::relative;

/// The header's text, a line each, as `LICENSE`'s appendix gives it.
const HEADER: [&str; 13] = [
    "Copyright 2026 Joseph Anthony Abbott III",
    "",
    "Licensed under the Apache License, Version 2.0 (the \"License\");",
    "you may not use this file except in compliance with the License.",
    "You may obtain a copy of the License at",
    "",
    "    http://www.apache.org/licenses/LICENSE-2.0",
    "",
    "Unless required by applicable law or agreed to in writing, software",
    "distributed under the License is distributed on an \"AS IS\" BASIS,",
    "WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.",
    "See the License for the specific language governing permissions and",
    "limitations under the License.",
];

/// How a kind of file writes the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    /// `// ` before each line.
    Slashes,
    /// `# ` before each line.
    Hash,
    /// One `/* */` comment, its lines indented by two spaces.
    Block,
    /// One `<!-- -->` comment, its lines indented by two spaces.
    Markup,
}

/// What may come before the header, on line 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum First {
    /// Nothing.
    Nothing,
    /// Whatever line 1 is: a GLSL source's `#version`.
    Line,
    /// A `#!` line, if the file has one.
    Shebang,
    /// An HTML doctype, if the file has one.
    Doctype,
}

/// Runs the check on the source files under `root`.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    let mut files = Vec::new();
    source_files(root, root, &mut files)?;
    files.sort();
    let mut report = Report::default();
    for (path, file) in &files {
        let Some((style, first)) = kind(path) else {
            continue;
        };
        let bytes = fs::read(file).map_err(|source| Error::Read {
            path: file.clone(),
            source,
        })?;
        match String::from_utf8(bytes) {
            Ok(text) => {
                if let Some(problem) = problem(&text, style, first) {
                    report.violations.push(format!("{path}:{problem}"));
                }
            }
            Err(_) => report.violations.push(format!("{path}: is not UTF-8 text")),
        }
    }
    report.notes.push(format!(
        "source files: {}",
        files
            .iter()
            .filter(|(path, _)| kind(path).is_some())
            .count()
    ));
    Ok(report)
}

/// Collects the files under `dir`, each with its path relative to `root`,
/// skipping what the module's documentation says.
fn source_files(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) -> Result<(), Error> {
    let read_error = |source| Error::Read {
        path: dir.to_path_buf(),
        source,
    };
    for entry in fs::read_dir(dir).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        let file_type = entry.file_type().map_err(read_error)?;
        let path = entry.path();
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let hidden = name.starts_with('.') && name != ".github";
            if !hidden && !(dir == root && name == "target") {
                source_files(root, &path, files)?;
            }
        } else if file_type.is_file() {
            files.push((relative(root, &path), path));
        }
    }
    Ok(())
}

/// How the file at `path`, relative to the workspace with `/` between
/// components, writes the header and what may come before it, or `None`
/// for a file that is not source.
fn kind(path: &str) -> Option<(Style, First)> {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name == "Makefile" || name == "Dockerfile" {
        return Some((Style::Hash, First::Nothing));
    }
    match name.rsplit_once('.').map(|(_, extension)| extension) {
        Some("rs" | "js" | "metal") => Some((Style::Slashes, First::Nothing)),
        Some("glsl") => Some((Style::Slashes, First::Line)),
        Some("py" | "sh") => Some((Style::Hash, First::Shebang)),
        Some("css") => Some((Style::Block, First::Nothing)),
        Some("html") => Some((Style::Markup, First::Doctype)),
        Some("yml" | "yaml") if path.starts_with(".github/workflows/") => {
            Some((Style::Hash, First::Nothing))
        }
        _ => None,
    }
}

/// The header's lines as `style` writes them.
fn header(style: Style) -> Vec<String> {
    let prefixed = |prefix: &str| -> Vec<String> {
        HEADER
            .iter()
            .map(|line| {
                if line.is_empty() {
                    prefix.to_string()
                } else {
                    format!("{prefix} {line}")
                }
            })
            .collect()
    };
    let enclosed = |open: &str, close: &str| -> Vec<String> {
        let mut lines = vec![open.to_string()];
        lines.extend(HEADER.iter().map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("  {line}")
            }
        }));
        lines.push(close.to_string());
        lines
    };
    match style {
        Style::Slashes => prefixed("//"),
        Style::Hash => prefixed("#"),
        Style::Block => enclosed("/*", "*/"),
        Style::Markup => enclosed("<!--", "-->"),
    }
}

/// Where `text`'s header is wrong, as `line: what`, or `None` if it is
/// right.
fn problem(text: &str, style: Style, first: First) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let line_1 = lines.first().copied().unwrap_or_default();
    let start = match first {
        First::Nothing => 0,
        First::Line => 1,
        First::Shebang => usize::from(line_1.starts_with("#!")),
        First::Doctype => usize::from(line_1.to_ascii_lowercase().starts_with("<!doctype")),
    };
    let expected = header(style);
    // CSS and HTML open and close their comment on lines of their own.
    let opening = usize::from(matches!(style, Style::Block | Style::Markup));
    for (index, wanted) in expected.iter().enumerate() {
        let line = start + index;
        if lines.get(line) != Some(&wanted.as_str()) {
            let which = match index.checked_sub(opening) {
                None => "the comment that holds the Apache-2.0 header".to_string(),
                Some(appendix) if appendix < HEADER.len() => format!(
                    "line {} of the Apache-2.0 header in `LICENSE`'s appendix",
                    appendix + 1
                ),
                Some(_) => "the end of the comment that holds the Apache-2.0 header".to_string(),
            };
            return Some(format!("{}: expected `{wanted}`, {which}", line + 1));
        }
    }
    let after = start + expected.len();
    match lines.get(after) {
        Some(line) if !line.is_empty() => Some(format!(
            "{}: expected a blank line after the Apache-2.0 header",
            after + 1
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `body` after the header in `style`, and after `first` if any.
    fn with_header(style: Style, first: Option<&str>, body: &str) -> String {
        let mut lines: Vec<String> = first.map(str::to_string).into_iter().collect();
        lines.extend(header(style));
        lines.push(String::new());
        lines.push(body.to_string());
        lines.join("\n")
    }

    #[test]
    fn the_header_is_the_one_in_license_s_appendix() {
        let license = include_str!("../../LICENSE");
        let (_, appendix) = license
            .split_once("APPENDIX: How to apply the Apache License to your work.")
            .expect("LICENSE has its appendix");
        let lines: Vec<&str> = appendix.lines().collect();
        let start = lines
            .iter()
            .position(|line| line.trim_start().starts_with("Copyright "))
            .expect("the appendix has a copyright line");
        let boilerplate: Vec<&str> = lines[start..start + HEADER.len()]
            .iter()
            .map(|line| line.strip_prefix("   ").unwrap_or(line))
            .collect();
        assert_eq!(boilerplate, HEADER);
    }

    #[test]
    fn each_kind_of_source_has_its_comments_and_place() {
        for (path, kind_) in [
            (
                "crates/core/src/lib.rs",
                Some((Style::Slashes, First::Nothing)),
            ),
            ("site/nadir.js", Some((Style::Slashes, First::Nothing))),
            (
                "crates/render/shaders/blit/blit.metal",
                Some((Style::Slashes, First::Nothing)),
            ),
            (
                "crates/render/shaders/blit/blit.frag.glsl",
                Some((Style::Slashes, First::Line)),
            ),
            (
                "crates/codec/tests/png/make_vectors.py",
                Some((Style::Hash, First::Shebang)),
            ),
            (
                ".github/release/apt-repo.sh",
                Some((Style::Hash, First::Shebang)),
            ),
            ("site/nadir.css", Some((Style::Block, First::Nothing))),
            ("site/index.html", Some((Style::Markup, First::Doctype))),
            ("Makefile", Some((Style::Hash, First::Nothing))),
            ("tools/cook/Dockerfile", Some((Style::Hash, First::Nothing))),
            (
                ".github/workflows/ci.yml",
                Some((Style::Hash, First::Nothing)),
            ),
            // Configuration and data carry none.
            (".github/ISSUE_TEMPLATE/issue.yml", None),
            (".github/dependabot.yml", None),
            ("Cargo.toml", None),
            ("README.md", None),
            ("docs/features.jsonc", None),
            ("crates/codec/tests/png/vectors.txt", None),
            ("assets/sprites/person.png", None),
            ("LICENSE", None),
            (".gitignore", None),
        ] {
            assert_eq!(kind(path), kind_, "{path}");
        }
    }

    #[test]
    fn a_file_with_its_header_in_place_passes() {
        for (style, first, line_1) in [
            (Style::Slashes, First::Nothing, None),
            (Style::Slashes, First::Line, Some("#version 330 core")),
            (Style::Hash, First::Shebang, Some("#!/bin/sh")),
            (Style::Hash, First::Shebang, None),
            (Style::Hash, First::Nothing, None),
            (Style::Block, First::Nothing, None),
            (Style::Markup, First::Doctype, Some("<!doctype html>")),
            (Style::Markup, First::Doctype, Some("<!DOCTYPE html>")),
            (Style::Markup, First::Doctype, None),
        ] {
            let text = with_header(style, line_1, "rest");
            assert_eq!(problem(&text, style, first), None, "{text}");
            // As a Windows checkout may have it.
            let crlf = text.replace('\n', "\r\n");
            assert_eq!(problem(&crlf, style, first), None, "{crlf}");
        }
        // Nothing after it is fine too.
        let text = header(Style::Slashes).join("\n");
        assert_eq!(problem(&text, Style::Slashes, First::Nothing), None);
    }

    #[test]
    fn each_style_writes_the_header_in_its_comments() {
        assert_eq!(
            header(Style::Slashes)[..3],
            [
                "// Copyright 2026 Joseph Anthony Abbott III",
                "//",
                "// Licensed under the Apache License, Version 2.0 (the \"License\");",
            ]
        );
        assert_eq!(
            header(Style::Hash)[6],
            "#     http://www.apache.org/licenses/LICENSE-2.0"
        );
        let block = header(Style::Block);
        assert_eq!(
            [block[0].as_str(), block[1].as_str(), block[2].as_str()],
            ["/*", "  Copyright 2026 Joseph Anthony Abbott III", ""]
        );
        assert_eq!(block.last().map(String::as_str), Some("*/"));
        let markup = header(Style::Markup);
        assert_eq!(markup.first().map(String::as_str), Some("<!--"));
        assert_eq!(markup.last().map(String::as_str), Some("-->"));
        assert_eq!(markup.len(), HEADER.len() + 2);
    }

    #[test]
    fn a_file_without_its_header_fails_on_the_line_it_should_start() {
        assert_eq!(
            problem("//! A crate.\n", Style::Slashes, First::Nothing).as_deref(),
            Some(
                "1: expected `// Copyright 2026 Joseph Anthony Abbott III`, line 1 of the \
                 Apache-2.0 header in `LICENSE`'s appendix"
            )
        );
        assert_eq!(
            problem("", Style::Hash, First::Nothing).as_deref(),
            Some(
                "1: expected `# Copyright 2026 Joseph Anthony Abbott III`, line 1 of the \
                 Apache-2.0 header in `LICENSE`'s appendix"
            )
        );
        // After the shebang, the doctype or the `#version` line.
        let shebang = "#!/usr/bin/env python3\n\"\"\"A script.\"\"\"\n";
        assert!(
            problem(shebang, Style::Hash, First::Shebang)
                .is_some_and(|problem| problem.starts_with("2: expected `# Copyright"))
        );
        let page = "<!doctype html>\n<html>\n";
        assert!(
            problem(page, Style::Markup, First::Doctype)
                .is_some_and(|problem| problem.starts_with("2: expected `<!--`"))
        );
    }

    #[test]
    fn a_header_out_of_place_or_altered_fails() {
        // Above GLSL's `#version`, which nadir-gfx requires on line 1.
        let shader = format!("{}\n#version 330 core\n", header(Style::Slashes).join("\n"));
        assert!(
            problem(&shader, Style::Slashes, First::Line)
                .is_some_and(|problem| problem.starts_with("2: expected `// Copyright"))
        );
        // Another year or holder, a reworded line, the wrong comments.
        let altered = with_header(Style::Slashes, None, "fn f() {}")
            .replace("Copyright 2026", "Copyright 2027");
        assert!(
            problem(&altered, Style::Slashes, First::Nothing)
                .is_some_and(|problem| problem.starts_with("1: expected"))
        );
        let reworded = with_header(Style::Hash, None, "x").replace("AS IS", "as is");
        assert!(
            problem(&reworded, Style::Hash, First::Nothing).is_some_and(|problem| problem
                .starts_with("10: expected `# distributed under the License is distributed"))
        );
        let hashes = with_header(Style::Hash, None, "fn f() {}");
        assert!(problem(&hashes, Style::Slashes, First::Nothing).is_some());
        // In CSS and HTML, the appendix's lines are counted inside the comment.
        let css = with_header(Style::Block, None, "body {}").replace("2026", "2027");
        assert_eq!(
            problem(&css, Style::Block, First::Nothing).as_deref(),
            Some(
                "2: expected `  Copyright 2026 Joseph Anthony Abbott III`, line 1 of the \
                 Apache-2.0 header in `LICENSE`'s appendix"
            )
        );
        let unclosed =
            with_header(Style::Markup, Some("<!doctype html>"), "<html>").replace("-->\n", "");
        assert_eq!(
            problem(&unclosed, Style::Markup, First::Doctype).as_deref(),
            Some("16: expected `-->`, the end of the comment that holds the Apache-2.0 header")
        );
        assert_eq!(
            problem("<!doctype html>\n<html>\n", Style::Markup, First::Doctype).as_deref(),
            Some("2: expected `<!--`, the comment that holds the Apache-2.0 header")
        );
        // Code right after it.
        let crowded = format!("{}\nfn f() {{}}\n", header(Style::Slashes).join("\n"));
        assert_eq!(
            problem(&crowded, Style::Slashes, First::Nothing).as_deref(),
            Some("14: expected a blank line after the Apache-2.0 header")
        );
    }

    #[test]
    fn the_walk_skips_build_output_and_hidden_folders_but_github() {
        let dir = std::env::temp_dir().join(format!("nadir-xtask-headers-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for folder in [
            "src",
            "target/debug",
            ".git",
            ".claude/worktrees/w/src",
            ".github/workflows",
        ] {
            fs::create_dir_all(dir.join(folder)).unwrap();
        }
        let good = with_header(Style::Slashes, None, "fn f() {}");
        fs::write(dir.join("src/lib.rs"), &good).unwrap();
        fs::write(dir.join("src/bad.rs"), "fn f() {}\n").unwrap();
        fs::write(dir.join("src/notes.txt"), "no header\n").unwrap();
        fs::write(dir.join("src/latin1.rs"), [0xE9, b'\n']).unwrap();
        fs::write(dir.join("target/debug/build.rs"), "fn f() {}\n").unwrap();
        fs::write(dir.join(".git/hook.sh"), "echo\n").unwrap();
        fs::write(dir.join(".claude/worktrees/w/src/lib.rs"), "fn f() {}\n").unwrap();
        fs::write(dir.join(".github/workflows/ci.yml"), "on: push\n").unwrap();
        let report = run(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(report.notes, ["source files: 4"]);
        assert_eq!(report.violations.len(), 3, "{:?}", report.violations);
        assert!(report.violations[0].starts_with(".github/workflows/ci.yml:1: expected `# "));
        assert!(report.violations[1].starts_with("src/bad.rs:1: expected `// "));
        assert_eq!(report.violations[2], "src/latin1.rs: is not UTF-8 text");
    }

    #[test]
    fn every_source_file_in_the_workspace_has_it() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let report = run(&root).unwrap();
        assert_eq!(report.violations, Vec::<String>::new());
    }
}
