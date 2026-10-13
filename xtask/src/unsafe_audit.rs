//! `cargo xtask unsafe-audit`: `unsafe` stays in the three crates that may
//! contain it, and each use there says why it is sound.
//!
//! The audit reads source text, so it also covers code that the compiler
//! skips on this OS (other `cfg(target_os)` branches) and code in macros.
//! It knows enough Rust syntax to tell code from comments, strings and
//! character literals, and no more.

use std::fs;
use std::path::{Path, PathBuf};

use crate::check::Report;
use crate::error::Error;

/// The crates that may contain `unsafe` (`AGENTS.md`, rule 3): package name
/// and directory.
const FFI_CRATES: &[(&str, &str)] = &[
    ("nadir-sys", "crates/sys"),
    ("nadir-platform", "crates/platform"),
    ("nadir-gfx", "crates/gfx"),
];

/// The same crates, for messages.
const FFI_NAMES: &str = "nadir-sys, nadir-platform and nadir-gfx";

/// The file in each crate of [`FFI_CRATES`] that may switch the
/// `unsafe_code` lint off, relative to the crate's directory.
const CRATE_ROOT: &str = "/src/lib.rs";

/// How many lines above an `unsafe` block or impl its `SAFETY:` comment may
/// end.
const SAFETY_REACH: usize = 3;

/// Runs the audit on every `.rs` file under `root`.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    let mut files = Vec::new();
    rust_files(root, true, &mut files)?;
    files.sort();

    let mut audit = Audit::default();
    for path in files {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(source) => return Err(Error::Read { path, source }),
        };
        audit.file(&relative(root, &path), &text);
    }
    Ok(audit.report())
}

/// Collects the `.rs` files under `dir`. Build output (the top-level
/// `target`) and hidden directories are skipped, and symbolic links are not
/// followed.
fn rust_files(dir: &Path, is_root: bool, files: &mut Vec<PathBuf>) -> Result<(), Error> {
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
            let skip = name.to_string_lossy().starts_with('.') || (is_root && name == "target");
            if !skip {
                rust_files(&path, false, files)?;
            }
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

/// `path` relative to `root`, with `/` between components on every OS.
fn relative(root: &Path, path: &Path) -> String {
    let components: Vec<_> = path
        .strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect();
    components.join("/")
}

/// The findings of an audit in progress.
#[derive(Debug, Default)]
struct Audit {
    /// `unsafe` keywords seen in each crate of [`FFI_CRATES`], in that
    /// order.
    uses: [usize; FFI_CRATES.len()],
    /// Which crate roots of [`FFI_CRATES`] have been seen, in that order.
    roots: [bool; FFI_CRATES.len()],
    /// How many files have been audited.
    files: usize,
    violations: Vec<String>,
}

impl Audit {
    /// Audits one file. `path` is relative to the workspace root, with `/`
    /// between components.
    fn file(&mut self, path: &str, text: &str) {
        self.files += 1;
        let in_crate = |(_, dir): &(&str, &str)| {
            path.strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/'))
        };
        let ffi_crate = FFI_CRATES.iter().position(in_crate);
        let root_of = FFI_CRATES
            .iter()
            .position(|(_, dir)| path.strip_prefix(dir) == Some(CRATE_ROOT));
        if let Some(seen) = root_of.and_then(|index| self.roots.get_mut(index)) {
            *seen = true;
        }

        let scan = scan(text);
        for (index, token) in scan.tokens.iter().enumerate() {
            let line = token.line;
            match token.text {
                "unsafe" => {
                    let Some(uses) = ffi_crate.and_then(|index| self.uses.get_mut(index)) else {
                        self.violations.push(format!(
                            "{path}:{line}: `unsafe` is only allowed in {FFI_NAMES}"
                        ));
                        continue;
                    };
                    *uses += 1;
                    let what = match scan.tokens.get(index + 1).map(|next| next.text) {
                        Some("{") => "block",
                        Some("impl") => "impl",
                        // Declarations (`unsafe fn`, `unsafe extern`,
                        // `unsafe trait`) and attributes (`#[unsafe(..)]`).
                        _ => continue,
                    };
                    if !scan.has_safety_comment(token) {
                        self.violations.push(format!(
                            "{path}:{line}: `unsafe` {what} has no `// SAFETY:` comment within \
                             {SAFETY_REACH} lines above it"
                        ));
                    }
                }
                "unsafe_code" if root_of.is_none() => {
                    if let Some(level @ ("allow" | "expect" | "warn")) =
                        lint_level(&scan.tokens, index)
                    {
                        self.violations.push(format!(
                            "{path}:{line}: `{level}(unsafe_code)` is only allowed in the crate \
                             roots of {FFI_NAMES}"
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    /// Finishes the audit.
    fn report(mut self) -> Report {
        // Without this, an audit of the wrong directory would pass by
        // finding nothing.
        for ((_, dir), seen) in FFI_CRATES.iter().zip(self.roots) {
            if !seen {
                self.violations.push(format!(
                    "{dir}{CRATE_ROOT} was not found: is this the workspace root?"
                ));
            }
        }

        let mut notes = vec!["`unsafe` uses per crate (review any growth):".to_string()];
        let width = FFI_CRATES
            .iter()
            .map(|(name, _)| name.len())
            .max()
            .unwrap_or(0);
        for ((name, _), uses) in FFI_CRATES.iter().zip(self.uses) {
            notes.push(format!("  {name:<width$}  {uses:>5}"));
        }
        notes.push(format!("Rust files scanned: {}", self.files));
        Report {
            notes,
            violations: self.violations,
        }
    }
}

/// The lint level that applies to the lint name at `index`: the word before
/// the parenthesis that encloses it, as in `allow(dead_code, unsafe_code)`.
fn lint_level<'a>(tokens: &[Token<'a>], index: usize) -> Option<&'a str> {
    let open = tokens
        .get(..index)?
        .iter()
        .rposition(|token| token.text == "(")?;
    Some(tokens.get(open.checked_sub(1)?)?.text)
}

/// A piece of code: a word (an identifier, keyword or number) or one
/// punctuation character.
#[derive(Debug, PartialEq, Eq)]
struct Token<'a> {
    text: &'a str,
    /// The line the token is on, counted from 1.
    line: usize,
    /// Where the token starts, in bytes.
    offset: usize,
}

/// A comment. `//` comments on consecutive lines form one comment, as long
/// as each line after the first holds nothing else.
#[derive(Debug)]
struct Comment {
    /// Where the comment starts, in bytes.
    offset: usize,
    /// The line the comment ends on.
    last_line: usize,
    /// Whether it is written with `//`, not `/* */`.
    is_line_comment: bool,
    /// Whether it contains `SAFETY:`.
    has_safety: bool,
}

/// What the audit needs from one source file.
#[derive(Debug, Default)]
struct Scan<'a> {
    tokens: Vec<Token<'a>>,
    comments: Vec<Comment>,
}

impl Scan<'_> {
    /// Whether a `SAFETY:` comment starts before `token` and ends on its
    /// line or at most [`SAFETY_REACH`] lines above it.
    fn has_safety_comment(&self, token: &Token<'_>) -> bool {
        self.comments.iter().any(|comment| {
            comment.has_safety
                && comment.offset < token.offset
                && comment.last_line + SAFETY_REACH >= token.line
        })
    }
}

/// Splits Rust source into code tokens and comments. String and character
/// literals are dropped.
///
/// This never fails: text that is not valid Rust gives some answer, and
/// the compiler is the one to complain about it.
fn scan(text: &str) -> Scan<'_> {
    let mut scanner = Scanner {
        text,
        bytes: text.as_bytes(),
        pos: 0,
        line: 1,
        code_on_line: false,
        scan: Scan::default(),
    };
    while let Some(byte) = scanner.peek(0) {
        match byte {
            b'\n' => scanner.newline(),
            b'/' if scanner.peek(1) == Some(b'/') => scanner.line_comment(),
            b'/' if scanner.peek(1) == Some(b'*') => scanner.block_comment(),
            b'"' => {
                scanner.pos += 1;
                scanner.string();
            }
            b'\'' => scanner.quote(),
            _ if byte.is_ascii_whitespace() => scanner.pos += 1,
            _ => match scanner.peek_char() {
                Some(character) if is_word_char(character) => scanner.word(),
                Some(character) if character.is_ascii() => {
                    scanner.token(scanner.pos, scanner.pos + 1);
                    scanner.pos += 1;
                }
                // Anything else separates tokens, as white space does.
                Some(character) => scanner.pos += character.len_utf8(),
                None => scanner.pos += 1,
            },
        }
    }
    scanner.scan
}

/// Whether `byte` can start the name of a raw identifier (`r#name`). Names
/// that start outside ASCII are not recognized; they then read as an
/// ordinary word, which errs towards reporting.
fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

/// Whether `character` can be part of an identifier, keyword or number.
///
/// Outside ASCII only letters and digits count. A byte-order mark or an
/// exotic space next to `unsafe` therefore cannot hide the keyword by
/// becoming part of the word.
fn is_word_char(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

struct Scanner<'a> {
    text: &'a str,
    bytes: &'a [u8],
    /// The next byte to look at.
    pos: usize,
    /// The line `pos` is on, counted from 1.
    line: usize,
    /// Whether the current line has a token or literal on it so far.
    code_on_line: bool,
    scan: Scan<'a>,
}

impl Scanner<'_> {
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
    }

    /// The character at `pos`, if `pos` is at the start of one.
    fn peek_char(&self) -> Option<char> {
        self.text.get(self.pos..)?.chars().next()
    }

    /// Steps over the rest of a word.
    fn skip_word(&mut self) {
        while let Some(character) = self.peek_char().filter(|c| is_word_char(*c)) {
            self.pos += character.len_utf8();
        }
    }

    /// Steps over the line break at `pos`.
    fn newline(&mut self) {
        self.pos += 1;
        self.line += 1;
        self.code_on_line = false;
    }

    fn token(&mut self, start: usize, end: usize) {
        if let Some(text) = self.text.get(start..end) {
            self.scan.tokens.push(Token {
                text,
                line: self.line,
                offset: start,
            });
        }
        self.code_on_line = true;
    }

    /// Records the comment from `start` to `pos`.
    fn comment(&mut self, start: usize, is_line_comment: bool) {
        let has_safety = self
            .text
            .get(start..self.pos)
            .is_some_and(|comment| comment.contains("SAFETY:"));
        // A `//` line directly below another one continues it.
        let continued = match self.scan.comments.last_mut() {
            Some(previous)
                if is_line_comment
                    && previous.is_line_comment
                    && !self.code_on_line
                    && previous.last_line + 1 == self.line =>
            {
                previous.last_line = self.line;
                previous.has_safety |= has_safety;
                true
            }
            _ => false,
        };
        if !continued {
            self.scan.comments.push(Comment {
                offset: start,
                last_line: self.line,
                is_line_comment,
                has_safety,
            });
        }
    }

    /// Reads a `//` comment, up to the end of the line.
    fn line_comment(&mut self) {
        let start = self.pos;
        while self.peek(0).is_some_and(|byte| byte != b'\n') {
            self.pos += 1;
        }
        self.comment(start, true);
    }

    /// Reads a `/* */` comment. These nest.
    fn block_comment(&mut self) {
        let start = self.pos;
        let mut depth = 0_usize;
        loop {
            match (self.peek(0), self.peek(1)) {
                (Some(b'/'), Some(b'*')) => {
                    self.pos += 2;
                    depth += 1;
                }
                (Some(b'*'), Some(b'/')) => {
                    self.pos += 2;
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                (Some(b'\n'), _) => self.newline(),
                (Some(_), _) => self.pos += 1,
                (None, _) => break,
            }
        }
        self.comment(start, false);
    }

    /// Skips the rest of a string literal. The opening quote is behind
    /// `pos`.
    fn string(&mut self) {
        while let Some(byte) = self.peek(0) {
            match byte {
                b'"' => {
                    self.pos += 1;
                    break;
                }
                // An escape: whatever comes next does not end the string.
                b'\\' => {
                    self.pos += 1;
                    match self.peek(0) {
                        Some(b'\n') => self.newline(),
                        Some(_) => self.pos += 1,
                        None => {}
                    }
                }
                b'\n' => self.newline(),
                _ => self.pos += 1,
            }
        }
        self.code_on_line = true;
    }

    /// Skips the rest of a raw string literal, which ends at a quote
    /// followed by `hashes` `#` characters. The opening quote is behind
    /// `pos`.
    fn raw_string(&mut self, hashes: usize) {
        while let Some(byte) = self.peek(0) {
            match byte {
                b'"' => {
                    self.pos += 1;
                    if (0..hashes).all(|ahead| self.peek(ahead) == Some(b'#')) {
                        self.pos += hashes;
                        break;
                    }
                }
                b'\n' => self.newline(),
                _ => self.pos += 1,
            }
        }
        self.code_on_line = true;
    }

    /// Handles the `'` at `pos`: skips a character literal, or steps over
    /// the quote of a lifetime or loop label, whose name is then read as an
    /// ordinary word.
    fn quote(&mut self) {
        if self.peek(1) == Some(b'\\') {
            // `'\n'`, `'\''`, `'\u{1F600}'`: the quote, the backslash and
            // the escaped byte (which may be a quote), then everything up
            // to the closing quote. A literal cannot span lines.
            self.pos += 2;
            if self.peek(0).is_some_and(|byte| byte != b'\n') {
                self.pos += 1;
            }
            while let Some(byte) = self.peek(0) {
                if byte == b'\n' {
                    break;
                }
                self.pos += 1;
                if byte == b'\'' {
                    break;
                }
            }
            self.code_on_line = true;
            return;
        }
        let mut after = self.text.get(self.pos + 1..).unwrap_or("").chars();
        match (after.next(), after.next()) {
            // `'x'`: one character and the closing quote.
            (Some(character), Some('\'')) if !matches!(character, '\'' | '\n') => {
                self.pos += 2 + character.len_utf8();
                self.code_on_line = true;
            }
            _ => self.pos += 1,
        }
    }

    /// Reads a word. A word directly before a quote can be the prefix of a
    /// literal instead (`b"..."`, `r#"..."#`, `b'x'`).
    fn word(&mut self) {
        let start = self.pos;
        self.skip_word();
        let word = self.text.get(start..self.pos).unwrap_or("");
        match (word, self.peek(0)) {
            ("b" | "c", Some(b'"')) => {
                self.pos += 1;
                self.string();
            }
            ("b", Some(b'\'')) => self.quote(),
            ("r" | "br" | "cr", Some(b'"' | b'#')) => self.raw(start, word == "r"),
            _ => self.token(start, self.pos),
        }
    }

    /// Handles what follows the word `r`, `br` or `cr` (which started at
    /// `start`) when that is `"` or `#`: a raw string, or after `r#` a raw
    /// identifier.
    fn raw(&mut self, start: usize, may_be_identifier: bool) {
        let mut hashes = 0;
        while self.peek(hashes) == Some(b'#') {
            hashes += 1;
        }
        if self.peek(hashes) == Some(b'"') {
            self.pos += hashes + 1;
            self.raw_string(hashes);
        } else if may_be_identifier && hashes == 1 && self.peek(1).is_some_and(is_word_start) {
            // A raw identifier such as `r#type` is a name, never a keyword,
            // so it is not a token the audit looks at.
            self.pos += 1;
            self.skip_word();
            self.code_on_line = true;
        } else {
            self.token(start, self.pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use nadir_core::testing;

    use super::*;

    /// The words and punctuation of `text`, as text.
    fn tokens(text: &str) -> Vec<&str> {
        scan(text).tokens.iter().map(|token| token.text).collect()
    }

    /// How many `unsafe` keywords `text` has in code.
    fn unsafe_count(text: &str) -> usize {
        tokens(text)
            .iter()
            .filter(|text| **text == "unsafe")
            .count()
    }

    /// The lines of the `unsafe` keywords in `text`.
    fn unsafe_lines(text: &str) -> Vec<usize> {
        scan(text)
            .tokens
            .iter()
            .filter(|token| token.text == "unsafe")
            .map(|token| token.line)
            .collect()
    }

    /// Audits `files` (path and text), plus the three crate roots so that
    /// the audit does not complain about those, and returns the violations.
    fn violations(files: &[(&str, &str)]) -> Vec<String> {
        audit(files).violations
    }

    fn audit(files: &[(&str, &str)]) -> Report {
        let mut audit = Audit::default();
        for (_, dir) in FFI_CRATES {
            let root = format!("{dir}{CRATE_ROOT}");
            if !files.iter().any(|(path, _)| *path == root) {
                audit.file(&root, "#![allow(unsafe_code)]\n");
            }
        }
        for (path, text) in files {
            audit.file(path, text);
        }
        audit.report()
    }

    // --- The scanner ---

    #[test]
    fn code_becomes_words_and_punctuation() {
        assert_eq!(
            tokens("let x: u32 = unsafe { *ptr.add(1) };"),
            [
                "let", "x", ":", "u32", "=", "unsafe", "{", "*", "ptr", ".", "add", "(", "1", ")",
                "}", ";"
            ]
        );
        assert_eq!(tokens(""), Vec::<&str>::new());
        assert_eq!(tokens(" \t\r\n\n"), Vec::<&str>::new());
    }

    #[test]
    fn tokens_know_their_line_and_offset() {
        let scan = scan("fn f() {\n    unsafe { g() }\n}\n");
        let found = scan.tokens.iter().find(|token| token.text == "unsafe");
        assert_eq!(
            found,
            Some(&Token {
                text: "unsafe",
                line: 2,
                offset: 13
            })
        );
        assert_eq!(scan.tokens.last().map(|token| token.line), Some(3));
    }

    #[test]
    fn unsafe_is_matched_as_a_whole_word() {
        assert_eq!(unsafe_count("unsafe"), 1);
        assert_eq!(unsafe_count("(unsafe)"), 1);
        assert_eq!(unsafe_count("unsafe{unsafe}"), 2);
        for text in [
            "unsafe_code",
            "is_unsafe",
            "unsafely",
            "Unsafe",
            "UNSAFE",
            "unsafe1",
            "_unsafe",
            "unsafeé",
            "éunsafe",
        ] {
            assert_eq!(unsafe_count(text), 0, "{text}");
        }
    }

    #[test]
    fn characters_that_are_not_letters_do_not_hide_a_keyword() {
        // A byte-order mark, a no-break space, a left-to-right mark, a line
        // separator, an emoji.
        for stray in ['\u{feff}', '\u{a0}', '\u{200e}', '\u{2028}', '\u{1F600}'] {
            assert_eq!(
                tokens(&format!("{stray}unsafe{stray}{{{stray}}}")),
                ["unsafe", "{", "}"],
                "{stray:?}"
            );
            assert_eq!(tokens(&format!("r#type{stray}unsafe")), ["unsafe"]);
        }
        // Letters do make a different word.
        assert_eq!(tokens("unsafeé { }"), ["unsafeé", "{", "}"]);
    }

    #[test]
    fn comments_are_not_code() {
        for text in [
            "// unsafe { }\nafter",
            "/// unsafe { }\nafter",
            "//! unsafe { }\nafter",
            "/* unsafe { } */ after",
            "/* a\n unsafe\n */ after",
            "/* /* unsafe */ unsafe */ after",
            "/* /* /* */ unsafe */ unsafe */ after",
            "/** unsafe */ after",
            "/*/ unsafe */ after",
            "/* unsafe **/ after",
            "x /* unsafe */.after",
        ] {
            assert_eq!(unsafe_count(text), 0, "{text}");
            // The comment ended where it should: the code after it is seen.
            assert_eq!(tokens(text).last(), Some(&"after"), "{text}");
        }
        // A comment marker inside a string is not a comment.
        assert_eq!(unsafe_count(r#"let url = "http://x"; unsafe { }"#), 1);
        assert_eq!(unsafe_count(r#"let s = "/*"; unsafe { } /* "*/"#), 1);
    }

    #[test]
    fn string_literals_are_not_code() {
        for text in [
            r#""unsafe { }" after"#,
            r#""a \" unsafe \" b" after"#,
            r#""a \\" after"#,
            r#""a \\\" unsafe" after"#,
            r#"b"unsafe" after"#,
            r#"c"unsafe" after"#,
            r#"b"\"unsafe" after"#,
            "\"line one\nunsafe\nline three\" after",
            "\"continued \\\n   unsafe\" after",
            r#""" after"#,
            r#""'" after"#,
        ] {
            assert_eq!(unsafe_count(text), 0, "{text}");
            assert_eq!(tokens(text), ["after"], "{text}");
        }
    }

    #[test]
    fn raw_string_literals_are_not_code() {
        for text in [
            r###"r"unsafe \" after"###,
            r###"r#"unsafe " unsafe"# after"###,
            r###"r##"unsafe "# unsafe"## after"###,
            r###"br"unsafe" after"###,
            r###"br#"unsafe " unsafe"# after"###,
            r###"cr#"unsafe " unsafe"# after"###,
            "r#\"line one\nunsafe\"# after",
            r###"r#""# after"###,
        ] {
            assert_eq!(unsafe_count(text), 0, "{text}");
            assert_eq!(tokens(text), ["after"], "{text}");
        }
        // In a raw string a backslash is an ordinary character.
        assert_eq!(tokens(r#"r"\" after r"\" last"#), ["after", "last"]);
    }

    #[test]
    fn words_that_only_look_like_literal_prefixes_are_words() {
        assert_eq!(tokens("r + b - c"), ["r", "+", "b", "-", "c"]);
        assert_eq!(tokens("br"), ["br"]);
        assert_eq!(tokens(r#"attr "unsafe" x"#), ["attr", "x"]);
        // `#` after `r` without a string or a name: punctuation.
        assert_eq!(tokens("r # x"), ["r", "#", "x"]);
        assert_eq!(tokens("br#x"), ["br", "#", "x"]);
    }

    #[test]
    fn raw_identifiers_are_not_keywords() {
        assert_eq!(tokens("let r#type = r#unsafe;"), ["let", "=", ";"]);
        assert_eq!(unsafe_count("r#unsafe { }"), 0);
    }

    #[test]
    fn character_literals_are_not_code() {
        for text in [
            "'a' after",
            "'\"' after",
            "'{' after",
            "'é' after",
            "'\u{1F600}' after",
            r"'\'' after",
            r"'\\' after",
            r"'\n' after",
            r"'\x41' after",
            r"'\u{1F600}' after",
            r"'\u{22}' after",
            "b'a' after",
            "b'\"' after",
            r"b'\'' after",
            r"b'\\' after",
        ] {
            assert_eq!(tokens(text), ["after"], "{text}");
        }
        // A quote character literal must not open a string, and the string
        // after it must still be skipped.
        assert_eq!(
            tokens(r#"let q = '"'; unsafe { } "unsafe""#),
            ["let", "q", "=", ";", "unsafe", "{", "}"]
        );
        assert_eq!(
            unsafe_count(r#"match c { b'"' => unsafe { }, b'\\' => 1 }"#),
            1
        );
    }

    #[test]
    fn a_character_literal_ends_exactly_at_its_closing_quote() {
        // Stopping a byte early would leave a quote behind, which pairs up
        // with the next literal's opening quote and so opens a string at
        // the `"`.
        for text in [
            r#"['a','"'] after"#,
            r#"['é','"'] after"#,
            "['\u{1F600}','\"'] after",
            r#"['\'','"'] after"#,
            r#"['\\','"'] after"#,
            r#"['\n','"'] after"#,
            r#"['\u{e9}','"'] after"#,
            r#"[b'a',b'"'] after"#,
            r#"[b'\'',b'"'] after"#,
        ] {
            let found = tokens(text);
            assert_eq!(found.first(), Some(&"["), "{text}");
            assert_eq!(found.last(), Some(&"after"), "{text}");
        }
        assert_eq!(tokens(r#"['é','"'] after"#), ["[", ",", "]", "after"]);
    }

    #[test]
    fn lifetimes_and_labels_are_not_character_literals() {
        assert_eq!(
            tokens("fn f<'a>(x: &'a str) -> &'a str { unsafe { x } }"),
            [
                "fn", "f", "<", "a", ">", "(", "x", ":", "&", "a", "str", ")", "-", ">", "&", "a",
                "str", "{", "unsafe", "{", "x", "}", "}"
            ]
        );
        assert_eq!(
            unsafe_count("'outer: loop { unsafe { }; break 'outer; }"),
            1
        );
        assert_eq!(
            unsafe_count("impl<'a, 'b> X<'a, 'b> { unsafe fn f() {} }"),
            1
        );
        assert_eq!(unsafe_count("&'static str; unsafe { }"), 1);
        // A lifetime followed, later on the line, by a character literal.
        assert_eq!(
            unsafe_count("fn f<'a>(c: &'a u8) -> bool { *c == b'\"' } unsafe"),
            1
        );
        assert_eq!(unsafe_count("x: &'a T, y: 'b', unsafe"), 1);
    }

    #[test]
    fn lines_are_counted_through_comments_and_literals() {
        let text = "/* one\ntwo */ \"three\nfour\" r#\"five\nsix\"# \"seven \\\neight\" // nine\n\
                    unsafe\n/*\n*/ unsafe\n'x' unsafe\n\n\nunsafe";
        assert_eq!(unsafe_lines(text), [6, 8, 9, 12]);
        assert_eq!(unsafe_lines("a\r\nunsafe\r\n\r\nunsafe"), [2, 4]);
    }

    #[test]
    fn text_that_ends_early_does_not_hide_what_came_before() {
        for text in [
            "unsafe { } \"never closed",
            "unsafe { } /* never closed",
            "unsafe { } r#\"never closed\"",
            "unsafe { } '",
            "unsafe { } '\\",
            "unsafe { } b'",
            "unsafe { } \"ends with a backslash \\",
            "unsafe { } /",
            "unsafe { } r#",
        ] {
            assert_eq!(unsafe_count(text), 1, "{text}");
        }
    }

    #[test]
    fn line_comments_on_consecutive_lines_are_one_comment() {
        let scan = scan(
            "// one\n// two\n   // three\n\n// four\ncode(); // five\n// six\ncode(); // seven\n\
             /* eight */\n// nine\n",
        );
        let lines: Vec<(usize, bool)> = scan
            .comments
            .iter()
            .map(|comment| (comment.last_line, comment.is_line_comment))
            .collect();
        // one-three; four; five-six (a comment may start after code);
        // seven; eight; nine.
        assert_eq!(
            lines,
            [
                (3, true),
                (5, true),
                (7, true),
                (8, true),
                (9, false),
                (10, true)
            ]
        );
    }

    #[test]
    fn scanning_never_panics_and_keeps_its_bookkeeping_consistent() {
        const PIECES: &[&str] = &[
            "unsafe",
            "unsafe_code",
            "allow",
            "impl",
            "r",
            "b",
            "br",
            "c",
            "x",
            "é",
            "\u{1F600}",
            "\"",
            "'",
            "#",
            "\\",
            "/",
            "*",
            "//",
            "/*",
            "*/",
            "{",
            "}",
            "(",
            ")",
            "\n",
            " ",
            "!",
            "SAFETY:",
            "r#\"",
            "\"#",
            "b'",
            "'\\",
            "\\\n",
            "\r\n",
        ];
        testing::check_sized(
            "unsafe_audit_scan",
            2000,
            |rng, size| {
                let mut text = String::new();
                for _ in 0..rng.range(0..size + 1) {
                    text.push_str(rng.choose(PIECES).copied().unwrap_or(""));
                }
                text
            },
            |text| {
                let scan = scan(text);
                let line_count = text.matches('\n').count() + 1;
                let (mut previous_end, mut previous_line) = (0, 1);
                for token in &scan.tokens {
                    if text.get(token.offset..token.offset + token.text.len()) != Some(token.text) {
                        return Err(format!("{token:?} is not at its offset"));
                    }
                    if token.text.is_empty() {
                        return Err(format!("{token:?} is empty"));
                    }
                    let expected_line = text[..token.offset].matches('\n').count() + 1;
                    if token.line != expected_line {
                        return Err(format!("{token:?} is on line {expected_line}"));
                    }
                    if token.offset < previous_end || token.line < previous_line {
                        return Err(format!("{token:?} is out of order"));
                    }
                    (previous_end, previous_line) = (token.offset + token.text.len(), token.line);
                }
                for comment in &scan.comments {
                    if comment.offset >= text.len() || comment.last_line > line_count {
                        return Err(format!("{comment:?} is outside the text"));
                    }
                }
                // The audit on top of the scanner must hold up as well.
                let mut audit = Audit::default();
                audit.file("crates/sys/src/lib.rs", text);
                audit.file("crates/ecs/src/lib.rs", text);
                audit.report();
                Ok(())
            },
        );
    }

    // --- Where `unsafe` may appear ---

    #[test]
    fn unsafe_outside_the_ffi_crates_is_a_violation() {
        assert_eq!(
            violations(&[(
                "crates/ecs/src/world.rs",
                "fn f(p: *const u8) -> u8 {\n    // SAFETY: documented, but still not allowed.\n    \
                 unsafe { *p }\n}\n"
            )]),
            [
                "crates/ecs/src/world.rs:3: `unsafe` is only allowed in nadir-sys, nadir-platform \
              and nadir-gfx"
            ]
        );
    }

    #[test]
    fn every_form_of_unsafe_outside_the_ffi_crates_is_a_violation() {
        for text in [
            "unsafe fn f() {}",
            "unsafe impl Send for X {}",
            "unsafe trait Pod {}",
            "unsafe extern \"C\" { fn f(); }",
            "#[unsafe(no_mangle)] fn f() {}",
            "macro_rules! m { () => { unsafe { f() } } }",
            "#[cfg(target_os = \"windows\")] fn f() { unsafe { g() } }",
        ] {
            assert_eq!(
                violations(&[("crates/ecs/src/lib.rs", text)]).len(),
                1,
                "{text}"
            );
        }
    }

    #[test]
    fn every_directory_but_the_three_ffi_crates_is_outside() {
        for path in [
            "crates/core/src/lib.rs",
            "crates/ecs/tests/world.rs",
            "crates/engine/src/lib.rs",
            "xtask/src/main.rs",
            "games/sandbox/src/main.rs",
            "tools/cook/src/main.rs",
            "bench/src/main.rs",
            "tests/golden/scene.rs",
            "build.rs",
            // Names that only start like an FFI crate's directory.
            "crates/system/src/lib.rs",
            "crates/sys.rs",
            "crates/gfx-tools/src/lib.rs",
            "vendor/crates/sys/src/lib.rs",
        ] {
            assert_eq!(
                violations(&[(path, "// SAFETY: no.\nunsafe { f() }")]),
                [format!(
                    "{path}:2: `unsafe` is only allowed in nadir-sys, nadir-platform and \
                     nadir-gfx"
                )]
            );
        }
        for path in [
            "crates/sys/src/lib.rs",
            "crates/sys/src/x11/window.rs",
            "crates/sys/tests/layout.rs",
            "crates/platform/src/linux.rs",
            "crates/gfx/src/gl/device.rs",
            "crates/gfx/build.rs",
        ] {
            assert_eq!(
                violations(&[(path, "// SAFETY: fine.\nunsafe { f() }")]),
                Vec::<String>::new(),
                "{path}"
            );
        }
    }

    #[test]
    fn unsafe_in_comments_and_strings_is_not_a_violation() {
        let text = "//! Never use `unsafe { }` here.\n\
                    /// ```\n/// unsafe { f() }\n/// ```\n\
                    const HELP: &str = \"unsafe { } is denied\";\n\
                    const RAW: &str = r#\"unsafe impl Send for X {}\"#;\n\
                    fn is_unsafe(unsafe_code: bool) -> bool { unsafe_code }\n";
        assert_eq!(
            violations(&[("crates/ecs/src/lib.rs", text)]),
            Vec::<String>::new()
        );
    }

    // --- `SAFETY:` comments ---

    /// The violations for `text` as a file of `nadir-sys`.
    fn sys_violations(text: &str) -> Vec<String> {
        violations(&[("crates/sys/src/x11.rs", text)])
    }

    fn undocumented(line: usize, what: &str) -> String {
        format!(
            "crates/sys/src/x11.rs:{line}: `unsafe` {what} has no `// SAFETY:` comment within 3 \
             lines above it"
        )
    }

    #[test]
    fn a_documented_block_passes() {
        for text in [
            "// SAFETY: `p` is valid.\nunsafe { *p }",
            "    // SAFETY: `p` is valid.\n    let x = unsafe { *p };",
            "/* SAFETY: `p` is valid. */\nunsafe { *p }",
            "/* SAFETY: `p` is valid. */ unsafe { *p }",
            "let x = /* SAFETY: `p` is valid. */ unsafe { *p };",
            "f(); // SAFETY: `p` is valid.\nunsafe { *p }",
            "// SAFETY: `p` is valid.\nunsafe{ *p }",
            "// SAFETY: `p` is valid.\nunsafe\n{ *p }",
            "// Context first.\n//\n// SAFETY: `p` is valid.\nunsafe { *p }",
        ] {
            assert_eq!(sys_violations(text), Vec::<String>::new(), "{text}");
        }
    }

    #[test]
    fn an_undocumented_block_is_a_violation() {
        assert_eq!(sys_violations("unsafe { *p }"), [undocumented(1, "block")]);
        assert_eq!(
            sys_violations("fn f() {\n    let x = unsafe { *p };\n}"),
            [undocumented(2, "block")]
        );
        for text in [
            // An ordinary comment.
            "// `p` is valid.\nunsafe { *p }",
            // The wrong spelling.
            "// safety: `p` is valid.\nunsafe { *p }",
            "// SAFETY `p` is valid.\nunsafe { *p }",
            "// Safety: `p` is valid.\nunsafe { *p }",
            // Not a comment.
            "let s = \"// SAFETY: no\";\nunsafe { *p }",
            // After the keyword.
            "unsafe { *p } // SAFETY: too late.",
            "unsafe {\n    // SAFETY: too late.\n    *p\n}",
            "unsafe /* SAFETY: too late. */ { *p }",
        ] {
            assert_eq!(sys_violations(text).len(), 1, "{text}");
        }
    }

    #[test]
    fn the_comment_may_end_up_to_three_lines_above() {
        let block = |gap: usize| format!("// SAFETY: fine.\n{}unsafe {{ *p }}", "\n".repeat(gap));
        for gap in 0..3 {
            assert_eq!(sys_violations(&block(gap)), Vec::<String>::new(), "{gap}");
        }
        assert_eq!(sys_violations(&block(3)), [undocumented(5, "block")]);

        // Attributes and the start of a statement fit in the gap.
        let text =
            "// SAFETY: fine.\n#[allow(clippy::let_and_return)]\nlet x =\n    unsafe { *p };";
        assert_eq!(sys_violations(text), Vec::<String>::new());
        let text = "// SAFETY: fine.\na();\nb();\nc();\nunsafe { *p };";
        assert_eq!(sys_violations(text), [undocumented(5, "block")]);
    }

    #[test]
    fn a_long_comment_counts_from_its_last_line() {
        let text = "// SAFETY: there is a lot to say:\n// - one\n// - two\n// - three\n// - four\n\
                    // - five\nunsafe { *p }";
        assert_eq!(sys_violations(text), Vec::<String>::new());
        let text = "/* SAFETY: there is a lot to say:\n   - one\n   - two\n   - three\n   - four\n\
                    */\nunsafe { *p }";
        assert_eq!(sys_violations(text), Vec::<String>::new());

        // Code between two comments separates them: the first one is too
        // far away by then.
        let text = "// SAFETY: for `a`.\nunsafe { a() };\n// Now `b`.\n// More on `b`.\n\
                    // Still `b`.\nunsafe { b() };";
        assert_eq!(sys_violations(text), [undocumented(6, "block")]);
    }

    #[test]
    fn each_block_needs_a_comment_in_reach() {
        let text = "// SAFETY: one.\nunsafe { a() };\n\n\n\nunsafe { b() };\n// SAFETY: three.\n\
                    unsafe { c() };";
        assert_eq!(sys_violations(text), [undocumented(6, "block")]);
    }

    #[test]
    fn an_unsafe_impl_needs_a_comment() {
        assert_eq!(
            sys_violations(
                "// SAFETY: the handle is only used on one thread.\nunsafe impl Send for X {}"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            sys_violations("struct X;\n\nunsafe impl<T: Send> Send for X<T> {}"),
            [undocumented(3, "impl")]
        );
    }

    #[test]
    fn declarations_and_attributes_need_no_safety_comment() {
        let text = "pub unsafe fn f() {}\nunsafe extern \"C\" { fn g(); }\npub unsafe trait Pod {}\n\
                    #[unsafe(no_mangle)]\nextern \"C\" fn h() {}\n";
        assert_eq!(sys_violations(text), Vec::<String>::new());
    }

    // --- The count ---

    #[test]
    fn uses_are_counted_per_ffi_crate() {
        let report = audit(&[
            (
                "crates/sys/src/x11.rs",
                "unsafe extern \"C\" { fn a(); }\npub unsafe fn b() {}\n// unsafe in a comment\n",
            ),
            ("crates/sys/src/egl.rs", "unsafe extern \"C\" { fn c(); }\n"),
            (
                "crates/gfx/src/gl.rs",
                "// SAFETY: fine.\nlet x = unsafe { a() };\n// SAFETY: fine.\nunsafe impl Pod for X {}\n\
                 pub unsafe trait Pod {}\n#[unsafe(no_mangle)]\nfn f() {}\n",
            ),
            ("crates/ecs/src/lib.rs", "pub fn safe() {}\n"),
        ]);
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes,
            [
                "`unsafe` uses per crate (review any growth):",
                "  nadir-sys           3",
                "  nadir-platform      0",
                "  nadir-gfx           4",
                "Rust files scanned: 7",
            ]
        );
    }

    #[test]
    fn uses_outside_the_ffi_crates_are_violations_and_not_counted() {
        let report = audit(&[("crates/ecs/src/lib.rs", "unsafe fn f() {}\n")]);
        assert_eq!(report.violations.len(), 1);
        assert_eq!(
            report.notes[1..4],
            [
                "  nadir-sys           0",
                "  nadir-platform      0",
                "  nadir-gfx           0"
            ]
        );
    }

    // --- The `unsafe_code` lint ---

    #[test]
    fn the_ffi_crate_roots_may_allow_unsafe_code() {
        let report = audit(&[
            (
                "crates/sys/src/lib.rs",
                "//! Docs.\n#![allow(unsafe_code)]\n",
            ),
            ("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n"),
            ("crates/gfx/src/lib.rs", "#![allow(unsafe_code)]\n"),
        ]);
        assert_eq!(report.violations, Vec::<String>::new());
    }

    #[test]
    fn nothing_else_may_allow_unsafe_code() {
        let not_allowed = |path: &str, line: usize, level: &str| {
            format!(
                "{path}:{line}: `{level}(unsafe_code)` is only allowed in the crate roots of \
                 nadir-sys, nadir-platform and nadir-gfx"
            )
        };
        for path in [
            "crates/ecs/src/lib.rs",
            "crates/engine/src/lib.rs",
            "xtask/src/main.rs",
            // Not the crate root, although it is an FFI crate.
            "crates/sys/src/x11.rs",
            "crates/sys/src/x11/lib.rs",
            "crates/sys/tests/lib.rs",
        ] {
            assert_eq!(
                violations(&[(path, "//! Docs.\n#![allow(unsafe_code)]\n")]),
                [not_allowed(path, 2, "allow")]
            );
        }

        let path = "crates/ecs/src/lib.rs";
        for (text, level) in [
            ("#[allow(unsafe_code)] mod ffi;", "allow"),
            ("#![allow(dead_code, unsafe_code)]", "allow"),
            ("#![allow(unsafe_code, dead_code)]", "allow"),
            ("#![allow(clippy::all, unsafe_code)]", "allow"),
            ("#![allow(unsafe_code, reason = \"FFI\")]", "allow"),
            ("#![allow( unsafe_code )]", "allow"),
            ("#![cfg_attr(not(test), allow(unsafe_code))]", "allow"),
            ("#[expect(unsafe_code)] mod ffi;", "expect"),
            ("#![warn(unsafe_code)]", "warn"),
        ] {
            assert_eq!(
                violations(&[(path, text)]),
                [not_allowed(path, 1, level)],
                "{text}"
            );
        }
    }

    #[test]
    fn stricter_levels_and_mere_mentions_of_the_lint_are_fine() {
        for text in [
            "#![deny(unsafe_code)]",
            "#![forbid(unsafe_code)]",
            "#![deny(missing_docs, unsafe_code)]",
            "// #![allow(unsafe_code)]",
            "const HELP: &str = \"#![allow(unsafe_code)]\";",
            "let unsafe_code = 1;",
            "check(unsafe_code);",
            "unsafe_code",
            "(unsafe_code)",
            "#![allow(dead_code)]",
        ] {
            assert_eq!(
                violations(&[("crates/ecs/src/lib.rs", text)]),
                Vec::<String>::new(),
                "{text}"
            );
        }
    }

    // --- The whole audit ---

    #[test]
    fn a_missing_crate_root_is_a_violation() {
        let mut audit = Audit::default();
        audit.file("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n");
        audit.file("crates/gfx/src/lib.rs", "");
        assert_eq!(
            audit.report().violations,
            ["crates/platform/src/lib.rs was not found: is this the workspace root?"]
        );
        assert_eq!(Audit::default().report().violations.len(), 3);
    }

    /// Writes `files` (path relative to the returned directory, and text)
    /// into a fresh directory under `target/tmp`.
    fn tree(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/tmp")
            .join(format!("xtask-unsafe-audit-{name}"));
        // The directory may be left over from an earlier run.
        let _ = fs::remove_dir_all(&root);
        for (path, text) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        root
    }

    const ROOTS: [(&str, &str); 3] = [
        ("crates/sys/src/lib.rs", "#![allow(unsafe_code)]\n"),
        ("crates/platform/src/lib.rs", "#![allow(unsafe_code)]\n"),
        ("crates/gfx/src/lib.rs", "#![allow(unsafe_code)]\n"),
    ];

    #[test]
    fn run_audits_every_rust_file_under_the_root() {
        let mut files = ROOTS.to_vec();
        files.extend([
            ("crates/sys/src/x11/window.rs", "pub unsafe fn f() {}\n"),
            (
                "crates/ecs/src/lib.rs",
                "pub fn f() {\n    unsafe { g() }\n}\n",
            ),
            (
                "crates/ecs/src/deep/er/mod.rs",
                "unsafe impl Send for X {}\n",
            ),
            (
                "games/sandbox/src/main.rs",
                "#![allow(unsafe_code)]\nfn main() {}\n",
            ),
            ("build.rs", "fn main() { unsafe { f() } }\n"),
        ]);
        let root = tree("run", &files);
        let report = run(&root).unwrap();
        // In path order, whatever order the OS lists directories in.
        assert_eq!(
            report.violations,
            [
                "build.rs:1: `unsafe` is only allowed in nadir-sys, nadir-platform and nadir-gfx",
                "crates/ecs/src/deep/er/mod.rs:1: `unsafe` is only allowed in nadir-sys, \
                 nadir-platform and nadir-gfx",
                "crates/ecs/src/lib.rs:2: `unsafe` is only allowed in nadir-sys, nadir-platform \
                 and nadir-gfx",
                "games/sandbox/src/main.rs:1: `allow(unsafe_code)` is only allowed in the crate \
                 roots of nadir-sys, nadir-platform and nadir-gfx",
            ]
        );
        assert_eq!(report.notes[1], "  nadir-sys           1");
        assert_eq!(report.notes[4], "Rust files scanned: 8");
    }

    #[test]
    fn run_skips_build_output_hidden_directories_and_other_file_types() {
        let mut files = ROOTS.to_vec();
        files.extend([
            ("target/debug/build/out.rs", "unsafe { f() }\n"),
            (".git/hooks/sample.rs", "unsafe { f() }\n"),
            (".cache/x/y.rs", "unsafe { f() }\n"),
            ("docs/notes.md", "unsafe { f() }\n"),
            ("crates/ecs/src/lib.rs.orig", "unsafe { f() }\n"),
            ("crates/ecs/src/rs", "unsafe { f() }\n"),
            // Only the top-level `target` is build output.
            ("crates/render/src/target/mod.rs", "pub fn f() {}\n"),
        ]);
        let root = tree("skip", &files);
        let report = run(&root).unwrap();
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(report.notes[4], "Rust files scanned: 4");
    }

    #[test]
    fn run_does_not_skip_a_nested_directory_named_target() {
        let mut files = ROOTS.to_vec();
        files.push(("crates/render/src/target/mod.rs", "unsafe { f() }\n"));
        let root = tree("nested-target", &files);
        assert_eq!(run(&root).unwrap().violations.len(), 1);
    }

    #[test]
    fn run_reports_a_directory_it_cannot_read() {
        let root = tree("missing", &[]);
        let error = run(&root).unwrap_err();
        assert!(
            matches!(&error, Error::Read { path, .. } if *path == root),
            "{error:?}"
        );
    }

    #[test]
    fn run_reports_a_file_that_is_not_text() {
        let root = tree("not-text", &ROOTS);
        let path = root.join("crates/ecs/src/lib.rs");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        let error = run(&root).unwrap_err();
        assert!(
            matches!(&error, Error::Read { path: failed, .. } if *failed == path),
            "{error:?}"
        );
    }
}
