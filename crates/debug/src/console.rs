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

//! The console: a command line, with history and Tab completion, under a
//! log of what the commands printed and what the game logged
//! (`debug-tools.md`, "Console commands").

use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{self, Write as _};

use nadir_core::{Color, Key};

use crate::Ui;

/// How many lines the console's log keeps: as many as the engine's log
/// ring (`nadir_core::log::RING_CAPACITY`).
pub const CONSOLE_LINES: usize = 1000;

/// How many command lines the console's history keeps.
pub const CONSOLE_HISTORY: usize = 100;

/// The color of a warning: the stats overlay's amber.
const WARNING: Color = Color::rgb(255, 176, 64);

/// The color of an error.
const ERROR: Color = Color::rgb(255, 96, 96);

/// The log's lines a line of the wheel scrolls, as a window's rows.
const LINES_A_WHEEL: f32 = 3.0;

/// The console's own command, and what it says of itself.
const HELP: &str = "help";
const HELP_USAGE: &str = "help [command]";
const HELP_TEXT: &str = "lists the commands, or says what one does";

/// Why the console did not run a line, or a command failed: the console
/// prints it in red.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsoleError {
    /// A command registered with a usage that does not start with its
    /// name: empty, a placeholder, a quote, or `help`, the console's own.
    BadUsage(String),
    /// A line with a quote that is not closed.
    UnclosedQuote,
    /// A line whose first word is no command's.
    UnknownCommand(String),
    /// A line short of an argument its command's usage needs.
    MissingArgument {
        /// The command's usage.
        usage: String,
        /// The argument, as the usage writes it: `<x>`.
        argument: String,
    },
    /// A line with more arguments than its command's usage has.
    TooManyArguments {
        /// The command's usage.
        usage: String,
    },
    /// An argument that is not what its command takes.
    InvalidArgument {
        /// The command's usage.
        usage: String,
        /// The argument, as the usage writes it, or `argument 2`.
        argument: String,
        /// What was typed.
        found: String,
        /// What it takes: `a number`.
        expected: String,
    },
    /// A command that ran and failed, and why.
    Failed(String),
}

impl fmt::Display for ConsoleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadUsage(usage) => write!(
                f,
                "`{usage}` is no usage: it starts with the command's name, which is not `{HELP}`"
            ),
            Self::UnclosedQuote => f.write_str("a quote is not closed"),
            Self::UnknownCommand(name) => write!(f, "no command `{name}`: `{HELP}` lists them"),
            Self::MissingArgument { usage, argument } => {
                write!(f, "{argument} is missing: {usage}")
            }
            Self::TooManyArguments { usage } => write!(f, "too many arguments: {usage}"),
            Self::InvalidArgument {
                usage,
                argument,
                found,
                expected,
            } => write!(f, "{argument}: `{found}` is not {expected}: {usage}"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

impl Error for ConsoleError {}

/// A command's arguments, the words typed after its name, checked against
/// its usage: each word the usage names typed as it is, each choice one of
/// the choices, as the usage spells it, each required argument there, and
/// no more than the usage has.
#[derive(Clone, Copy, Debug)]
pub struct Args<'a> {
    usage: &'a str,
    words: &'a [String],
}

impl<'a> Args<'a> {
    /// The command's usage: `tp <x> <y>`.
    pub fn usage(&self) -> &'a str {
        self.usage
    }

    /// How many arguments were typed.
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// Whether none were.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// The argument at `index`, from 0, if it was typed.
    pub fn get(&self, index: usize) -> Option<&'a str> {
        self.words.get(index).map(String::as_str)
    }

    /// The argument at `index`.
    ///
    /// # Errors
    ///
    /// [`ConsoleError::MissingArgument`] if it was not typed.
    pub fn str(&self, index: usize) -> Result<&'a str, ConsoleError> {
        self.get(index)
            .ok_or_else(|| ConsoleError::MissingArgument {
                usage: self.usage.to_string(),
                argument: self.argument(index),
            })
    }

    /// The argument at `index` as a number, which must be finite.
    ///
    /// # Errors
    ///
    /// [`ConsoleError::MissingArgument`] if it was not typed, and
    /// [`ConsoleError::InvalidArgument`] if it is no finite number.
    pub fn f32(&self, index: usize) -> Result<f32, ConsoleError> {
        let word = self.str(index)?;
        match word.parse::<f32>() {
            Ok(value) if value.is_finite() => Ok(value),
            _ => Err(ConsoleError::InvalidArgument {
                usage: self.usage.to_string(),
                argument: self.argument(index),
                found: word.to_string(),
                expected: "a number".to_string(),
            }),
        }
    }

    /// The argument at `index` as the usage writes it, or its number.
    fn argument(&self, index: usize) -> String {
        match self.usage.split_whitespace().nth(index + 1).map(slot) {
            Some(Slot::Choice { shown, .. } | Slot::Value { shown, .. }) => shown.to_string(),
            _ => format!("argument {}", index + 1),
        }
    }
}

/// Where a command prints: lines added to the console's log, in the
/// text's color unless it says another.
pub struct ConsoleOut<'a> {
    lines: &'a mut Lines,
}

impl ConsoleOut<'_> {
    /// Prints `text`, a line for each of its lines.
    pub fn print(&mut self, text: impl fmt::Display) {
        self.lines.push(Tone::Text, text);
    }

    /// Prints `text` in `color`, a line for each of its lines.
    pub fn print_colored(&mut self, color: Color, text: impl fmt::Display) {
        self.lines.push(Tone::Color(color), text);
    }
}

/// How a line is colored.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tone {
    Text,
    /// A line typed, as the console echoes it.
    Echo,
    Warning,
    Error,
    Color(Color),
}

/// A line of the log.
#[derive(Clone, Debug, PartialEq)]
struct Line {
    text: String,
    tone: Tone,
}

/// The log's lines, the newest last, and how far it is scrolled.
#[derive(Debug, Default)]
struct Lines {
    lines: VecDeque<Line>,
    /// How many lines up from the newest the view is: 0 sticks to it.
    scroll: usize,
    /// What a push formats into.
    scratch: String,
}

impl Lines {
    /// Adds `text`, a line for each of its lines, the oldest lines going
    /// past [`CONSOLE_LINES`], their strings reused. A view scrolled up
    /// stays on the lines it shows.
    fn push(&mut self, tone: Tone, text: impl fmt::Display) {
        self.scratch.clear();
        // Writing to a `String` fails only if a `Display` impl does; what
        // it wrote is printed.
        let _ = write!(self.scratch, "{text}");
        for part in self.scratch.split('\n') {
            let part = part.strip_suffix('\r').unwrap_or(part);
            let mut text = if self.lines.len() >= CONSOLE_LINES {
                self.lines
                    .pop_front()
                    .map(|line| line.text)
                    .unwrap_or_default()
            } else {
                String::new()
            };
            text.clear();
            text.push_str(part);
            self.lines.push_back(Line { text, tone });
            if self.scroll > 0 {
                self.scroll += 1;
            }
        }
    }
}

/// A command registered: its name, the first word of its usage.
#[derive(Clone, Debug)]
struct Command<C> {
    name: String,
    usage: String,
    help: String,
    command: C,
}

/// A word of a usage after the command's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot<'a> {
    /// A word typed as it is: `dump` of `profile dump [path]`.
    Word(&'a str),
    /// One of words: `<error|warn>`, or `[on|off]` if it may be left out.
    Choice {
        shown: &'a str,
        choices: &'a str,
        optional: bool,
    },
    /// A value: `<x>`, or `[path]` if it may be left out.
    Value { shown: &'a str, optional: bool },
}

/// The slot a usage's word is: a placeholder starts with `<` or `[`, or
/// ends with `>` or `]` (`[x y]` is two), and is a choice if it holds `|`.
fn slot(word: &str) -> Slot<'_> {
    let open = word.starts_with(['<', '[']);
    let close = word.ends_with(['>', ']']);
    if !open && !close {
        return Slot::Word(word);
    }
    let optional = word.starts_with('[') || word.ends_with(']');
    let inner = word
        .trim_start_matches(['<', '['])
        .trim_end_matches(['>', ']']);
    if inner.contains('|') {
        Slot::Choice {
            shown: word,
            choices: inner,
            optional,
        }
    } else {
        Slot::Value {
            shown: word,
            optional,
        }
    }
}

/// The choices of a choice's slot.
fn choices(choices: &str) -> impl Iterator<Item = &str> {
    choices.split('|').filter(|choice| !choice.is_empty())
}

/// Splits `line` into its words, into `words`: whitespace between words;
/// a double quote groups what is up to the next, spaces too, and in it
/// `\"` is a quote and `\\` a backslash, any other backslash kept as it is
/// (`"C:\My Traces\t.json"`). Outside quotes a backslash is itself. `""`
/// is an empty word.
///
/// # Errors
///
/// [`ConsoleError::UnclosedQuote`] for a quote not closed.
fn split(line: &str, words: &mut Vec<String>) -> Result<(), ConsoleError> {
    words.clear();
    let mut word = String::new();
    let mut in_word = false;
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' => quoted = false,
                '\\' if matches!(chars.peek(), Some('"' | '\\')) => {
                    if let Some(escaped) = chars.next() {
                        word.push(escaped);
                    }
                }
                _ => word.push(c),
            }
        } else if c == '"' {
            quoted = true;
            in_word = true;
        } else if c.is_whitespace() {
            if in_word {
                words.push(std::mem::take(&mut word));
                in_word = false;
            }
        } else {
            word.push(c);
            in_word = true;
        }
    }
    if quoted {
        return Err(ConsoleError::UnclosedQuote);
    }
    if in_word {
        words.push(word);
    }
    Ok(())
}

/// Checks `words`, a line's arguments, against `usage`, and spells each
/// choice as the usage does.
///
/// # Errors
///
/// A word that is not the usage's, a choice that is none of its choices,
/// a required argument missing, or more arguments than the usage has.
fn check(usage: &str, words: &mut [String]) -> Result<(), ConsoleError> {
    let mut slots = 0;
    for (index, word) in usage.split_whitespace().skip(1).enumerate() {
        slots += 1;
        let invalid = |found: &str, expected: String| ConsoleError::InvalidArgument {
            usage: usage.to_string(),
            argument: match slot(word) {
                Slot::Word(_) => format!("argument {}", index + 1),
                Slot::Choice { shown, .. } | Slot::Value { shown, .. } => shown.to_string(),
            },
            found: found.to_string(),
            expected,
        };
        match (words.get_mut(index), slot(word)) {
            (None, Slot::Word(word)) => {
                return Err(ConsoleError::MissingArgument {
                    usage: usage.to_string(),
                    argument: format!("`{word}`"),
                });
            }
            (
                None,
                Slot::Choice {
                    shown,
                    optional: false,
                    ..
                }
                | Slot::Value {
                    shown,
                    optional: false,
                },
            ) => {
                return Err(ConsoleError::MissingArgument {
                    usage: usage.to_string(),
                    argument: shown.to_string(),
                });
            }
            (None, _) | (Some(_), Slot::Value { .. }) => {}
            (Some(typed), Slot::Word(word)) => {
                if typed.as_str() != word {
                    return Err(invalid(typed, format!("`{word}`")));
                }
            }
            (Some(typed), Slot::Choice { choices: all, .. }) => {
                let Some(choice) = choices(all).find(|choice| choice.eq_ignore_ascii_case(typed))
                else {
                    let list = choices(all).collect::<Vec<_>>().join(", ");
                    return Err(invalid(typed, format!("one of {list}")));
                };
                if typed != choice {
                    *typed = choice.to_string();
                }
            }
        }
    }
    if words.len() > slots {
        return Err(ConsoleError::TooManyArguments {
            usage: usage.to_string(),
        });
    }
    Ok(())
}

/// What Tab does to a line.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Completion {
    /// Nothing matches, or the word cannot be completed (it is quoted).
    Nothing,
    /// The line, its last word completed: in full, with a space after it,
    /// or as far as its matches agree.
    Line(String),
    /// The matches, which agree no further than what was typed.
    List(Vec<String>),
}

/// Completes the last word of `line`, or the next if it ends with
/// whitespace: the first word from the commands' names; another from its
/// slot in the command's usage (a word, or a choice's choices; a value has
/// none); `help`'s from the names.
fn complete<C>(commands: &[Command<C>], line: &str) -> Completion {
    let start = line
        .char_indices()
        .rfind(|(_, c)| c.is_whitespace())
        .map_or(0, |(at, c)| at + c.len_utf8());
    let (head, typed) = line.split_at(start);
    if typed.contains('"') {
        return Completion::Nothing;
    }
    let mut words = Vec::new();
    if split(head, &mut words).is_err() {
        return Completion::Nothing;
    }
    let names =
        || std::iter::once(HELP).chain(commands.iter().map(|command| command.name.as_str()));
    let mut matches: Vec<&str> = match words.split_first() {
        None => names().collect(),
        Some((name, before)) if name == HELP => {
            if before.is_empty() {
                names().collect()
            } else {
                Vec::new()
            }
        }
        Some((name, before)) => commands
            .iter()
            .find(|command| command.name == *name)
            .and_then(|command| command.usage.split_whitespace().nth(before.len() + 1))
            .map_or_else(Vec::new, |word| match slot(word) {
                Slot::Word(word) => vec![word],
                Slot::Choice { choices: all, .. } => choices(all).collect(),
                Slot::Value { .. } => Vec::new(),
            }),
    };
    matches.retain(|candidate| candidate.starts_with(typed));
    matches.sort_unstable();
    matches.dedup();
    match matches.as_slice() {
        [] => Completion::Nothing,
        [only] => Completion::Line(format!("{head}{only} ")),
        [first, rest @ ..] => {
            let common = common_prefix(first, rest);
            if common.len() > typed.len() {
                Completion::Line(format!("{head}{common}"))
            } else {
                Completion::List(matches.iter().map(|m| (*m).to_string()).collect())
            }
        }
    }
}

/// How much of `first` every one of `rest` starts with.
fn common_prefix<'a>(first: &'a str, rest: &[&str]) -> &'a str {
    let mut end = first.len();
    for other in rest {
        let same = first
            .char_indices()
            .zip(other.chars())
            .take_while(|((_, a), b)| a == b)
            .last()
            .map_or(0, |((at, c), _)| at + c.len_utf8());
        end = end.min(same);
    }
    &first[..end]
}

/// A console: a command line under a log, in a window of its own. A
/// command is registered with its usage, which names it and its arguments,
/// a line of help, and `command`, the caller's value for it (an enum of the
/// caller's commands, or a function), which the console hands back with the
/// arguments when a line names it, for the caller to run.
///
/// - **Lines** are words split by whitespace, a double quote grouping
///   words (`spawn "big rock"`), `\"` and `\\` in it a quote and a
///   backslash. The first word names the command; `help` is the console's
///   own, which lists the commands or says what one does.
/// - **Usages** write each argument after the name: a word to type as it
///   is (`profile dump [path]`), a value (`<x>`, or `[path]` if it may be
///   left out), or a choice (`<error|warn|info>`). The console checks a
///   line against its command's usage before the command runs: each word
///   typed, each choice one of its own (ignoring case, then spelled as the
///   usage spells it), each required argument there, no more arguments than
///   the usage has. [`Args`] gives them, and reads a value as a number.
/// - **Completion:** Tab completes the line's last word, or the next after
///   a space: the first from the commands' names, the others from their
///   slots in the usage, a word or a choice's choices. One match is put in
///   whole, a space after it; several as far as they agree, or, if they
///   agree no further, listed in the log, a match a line. A value has no
///   completions.
/// - **History:** each line run is kept, the last [`CONSOLE_HISTORY`] of
///   them, one the same as the line before it once; Up goes back through
///   them, and Down forward, to the line being typed.
/// - **The log** keeps the last [`CONSOLE_LINES`] lines: each line run,
///   after `>`, what the commands printed ([`ConsoleOut`]), their errors in
///   red, and what the caller adds ([`Console::log_record`]): the engine's
///   log records, warnings in amber and errors in red. A line too long for
///   the window is cut at its edge.
pub struct Console<C> {
    commands: Vec<Command<C>>,
    line: String,
    history: VecDeque<String>,
    /// The line of the history shown, while Up and Down go through it.
    browsing: Option<usize>,
    /// The line being typed, kept while the history is shown.
    draft: String,
    lines: Lines,
    /// The wheel's lines not yet scrolled: a fraction of one.
    wheel: f32,
    /// The UI's frame the console was last shown in.
    shown_in: Option<u64>,
    /// Whether the command line had the keyboard then.
    keyboard: bool,
    words: Vec<String>,
}

impl<C> Default for Console<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C> Console<C> {
    /// A console with no commands but `help`, an empty line and log.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            line: String::new(),
            history: VecDeque::new(),
            browsing: None,
            draft: String::new(),
            lines: Lines::default(),
            wheel: 0.0,
            shown_in: None,
            keyboard: false,
            words: Vec::new(),
        }
    }

    /// Registers `command` under `usage`, whose first word is its name, with
    /// `help`, a line of what it does; it replaces a command of that name.
    /// `help` lists the commands in the order they were first registered.
    ///
    /// # Errors
    ///
    /// [`ConsoleError::BadUsage`] if the usage's first word is not a name:
    /// there is none, it is a placeholder or a choice, it has a quote, or
    /// it is `help`.
    pub fn register(&mut self, usage: &str, help: &str, command: C) -> Result<(), ConsoleError> {
        let usage = usage.trim();
        let name = usage.split_whitespace().next().unwrap_or_default();
        let bad = name.is_empty() || name == HELP || name.contains(['"', '<', '>', '[', ']', '|']);
        if bad {
            return Err(ConsoleError::BadUsage(usage.to_string()));
        }
        let registered = Command {
            name: name.to_string(),
            usage: usage.to_string(),
            help: help.trim().to_string(),
            command,
        };
        match self.commands.iter_mut().find(|known| known.name == name) {
            Some(known) => *known = registered,
            None => self.commands.push(registered),
        }
        Ok(())
    }

    /// The line being typed.
    pub fn line(&self) -> &str {
        &self.line
    }

    /// Sets the line being typed, as if typed: its caret goes to its end.
    pub fn set_line(&mut self, line: &str) {
        self.line.clear();
        self.line.push_str(line);
        self.browsing = None;
    }

    /// The log's lines, the oldest first.
    pub fn lines(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.lines.lines.iter().map(|line| line.text.as_str())
    }

    /// Prints `text` in the log, a line for each of its lines.
    pub fn print(&mut self, text: impl fmt::Display) {
        self.lines.push(Tone::Text, text);
    }

    /// Prints `text` in red, a line for each of its lines.
    pub fn print_error(&mut self, text: impl fmt::Display) {
        self.lines.push(Tone::Error, text);
    }

    /// Adds a record of the engine's log, as `nadir_core::log::since` gives
    /// it (`<time> <LEVEL> <target>: <message>`), a line for each of its
    /// lines: in amber for `WARN`, in red for `ERROR`.
    pub fn log_record(&mut self, record: &str) {
        let tone = match record.split(' ').nth(1) {
            Some("ERROR") => Tone::Error,
            Some("WARN") => Tone::Warning,
            _ => Tone::Text,
        };
        self.lines.push(tone, record);
    }

    /// Up: shows the line run before the one shown, keeping the line being
    /// typed for [`Console::newer`] to come back to.
    pub fn older(&mut self) {
        let index = match self.browsing {
            Some(index) => index.saturating_sub(1),
            None if self.history.is_empty() => return,
            None => {
                self.draft.clear();
                self.draft.push_str(&self.line);
                self.history.len() - 1
            }
        };
        self.browsing = Some(index);
        self.line.clear();
        self.line.push_str(&self.history[index]);
    }

    /// Down: shows the line run after the one shown, or past the last, the
    /// line that was being typed.
    pub fn newer(&mut self) {
        let Some(index) = self.browsing else {
            return;
        };
        self.line.clear();
        if index + 1 < self.history.len() {
            self.browsing = Some(index + 1);
            self.line.push_str(&self.history[index + 1]);
        } else {
            self.browsing = None;
            self.line.push_str(&self.draft);
        }
    }

    /// Tab: completes the line's last word, or lists what it could be, as
    /// [`Console`] says.
    pub fn complete(&mut self) {
        match complete(&self.commands, &self.line) {
            Completion::Nothing => {}
            Completion::Line(line) => {
                self.line = line;
                self.browsing = None;
            }
            Completion::List(matches) => self.lines.push(Tone::Text, matches.join("\n")),
        }
    }
}

impl<C: Copy> Console<C> {
    /// Runs `line` as if it were typed and Enter pressed, but leaves the
    /// line being typed as it is: brings the log to its newest lines, so
    /// that what follows is in view, echoes the line in the log, after `>`,
    /// keeps it in the history, and, if it names a command and its
    /// arguments fit the command's usage, calls `run` with the command, its
    /// arguments and where to print. An error, the line's or the one `run`
    /// returns, is printed in red. A line of only whitespace does nothing.
    pub fn run_line<F>(&mut self, line: &str, mut run: F)
    where
        F: FnMut(C, &Args<'_>, &mut ConsoleOut<'_>) -> Result<(), ConsoleError>,
    {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        self.lines.scroll = 0;
        self.lines.push(Tone::Echo, format_args!("> {line}"));
        if self.history.back().is_none_or(|last| last != line) {
            if self.history.len() >= CONSOLE_HISTORY {
                self.history.pop_front();
            }
            self.history.push_back(line.to_string());
        }
        self.browsing = None;
        self.draft.clear();
        if let Err(error) = self.dispatch(line, &mut run) {
            self.lines.push(Tone::Error, &error);
        }
    }

    /// Splits, checks and runs `line`.
    fn dispatch<F>(&mut self, line: &str, run: &mut F) -> Result<(), ConsoleError>
    where
        F: FnMut(C, &Args<'_>, &mut ConsoleOut<'_>) -> Result<(), ConsoleError>,
    {
        let mut words = std::mem::take(&mut self.words);
        let result = split(line, &mut words).and_then(|()| {
            let Some((name, rest)) = words.split_first_mut() else {
                return Ok(());
            };
            if name == HELP {
                check(HELP_USAGE, rest)?;
                return self.help(rest.first().map(String::as_str));
            }
            let Some(command) = self.commands.iter().find(|command| command.name == *name) else {
                return Err(ConsoleError::UnknownCommand(name.clone()));
            };
            check(&command.usage, rest)?;
            let args = Args {
                usage: &command.usage,
                words: rest,
            };
            let mut out = ConsoleOut {
                lines: &mut self.lines,
            };
            run(command.command, &args, &mut out)
        });
        self.words = words;
        result
    }

    /// `help`: every command's usage and help, in columns, or `name`'s.
    fn help(&mut self, name: Option<&str>) -> Result<(), ConsoleError> {
        let all = std::iter::once((HELP_USAGE, HELP_TEXT)).chain(
            self.commands
                .iter()
                .map(|command| (command.usage.as_str(), command.help.as_str())),
        );
        let Some(name) = name else {
            let width = all
                .clone()
                .map(|(usage, _)| usage.chars().count())
                .max()
                .unwrap_or(0);
            for (usage, help) in all {
                self.lines
                    .push(Tone::Text, format_args!("{usage:<width$}  {help}"));
            }
            return Ok(());
        };
        let (usage, help) = all
            .clone()
            .find(|(usage, _)| usage.split_whitespace().next() == Some(name))
            .ok_or_else(|| ConsoleError::UnknownCommand(name.to_string()))?;
        self.lines.push(Tone::Text, format_args!("{usage}  {help}"));
        Ok(())
    }

    /// Fills a window with the console: the log, as many of its lines as
    /// fit, the newest at the bottom; and the command line under it, across
    /// the window. The command line takes the keyboard when the console is
    /// shown after a frame it was not, none of that frame's typing or keys
    /// reaching it: a key that showed the console, typing its character as
    /// it is pressed, is not typed. With it, Enter runs the line, as
    /// [`Console::run_line`] does, and empties it; Tab completes; Up and
    /// Down go through the history; PageUp and PageDown scroll the log a
    /// page, as the wheel over it does by three lines. Each is handled after
    /// the typing before it and before what follows, as the input ends a
    /// frame at each ([`DebugInput`](crate::DebugInput)). A log scrolled up
    /// stays on its lines as new ones come, until a line is run, and one at
    /// its bottom shows the newest. The rows fit the window, which does not
    /// scroll once its size is settled.
    pub fn show<F>(&mut self, ui: &mut Ui<'_>, run: F)
    where
        F: FnMut(C, &Args<'_>, &mut ConsoleOut<'_>) -> Result<(), ConsoleError>,
    {
        // Shown after a frame it was not: the keys of this frame were
        // pressed before the line had the keyboard, and are not its.
        let frame = ui.frame_number();
        let opened = self.shown_in.is_none_or(|shown| shown + 1 != frame);
        if opened {
            ui.focus_next();
        }
        self.shown_in = Some(frame);

        // As many rows as fit in the window, the command line one of them,
        // with no slack: a row that does not fit whole would scroll it.
        let step = ui.row_step();
        let rows = if step > 0.0 {
            (ui.room() / step).floor().max(0.0) as usize
        } else {
            0
        };
        let page = rows.saturating_sub(1);
        let count = self.lines.lines.len();
        // The page keys, before the log is drawn, while the command line
        // has had the keyboard.
        if self.keyboard && !opened {
            let by = page.max(1);
            if ui.key_pressed(Key::PageUp) {
                self.lines.scroll += by;
            }
            if ui.key_pressed(Key::PageDown) {
                self.lines.scroll = self.lines.scroll.saturating_sub(by);
            }
        }
        let wheel = ui.wheel_over(page as f32 * step);
        self.wheel += wheel * LINES_A_WHEEL;
        let lines = self.wheel.trunc();
        self.wheel -= lines;
        // A turn that is not finite scrolls this frame at most, and leaves
        // no remainder to spoil the next.
        if !self.wheel.is_finite() {
            self.wheel = 0.0;
        }
        let scroll = self.lines.scroll as f32 + lines;
        let top = count.saturating_sub(page);
        self.lines.scroll = (scroll.max(0.0) as usize).min(top);

        let end = count - self.lines.scroll;
        let start = end.saturating_sub(page);
        ui.skip_rows(page - (end - start));
        let style = ui.style();
        for line in self.lines.lines.range(start..end) {
            let color = match line.tone {
                Tone::Text => style.text,
                Tone::Echo => style.accent,
                Tone::Warning => WARNING,
                Tone::Error => ERROR,
                Tone::Color(color) => color,
            };
            ui.label_colored(color, &line.text);
        }

        let (response, keyboard) = ui.text_line("##nadir::console::line", &mut self.line);
        self.keyboard = keyboard;
        if response.changed {
            self.browsing = None;
        }
        if response.submitted {
            // The line's buffer is kept for the next line.
            let mut line = std::mem::take(&mut self.line);
            self.run_line(&line, run);
            line.clear();
            self.line = line;
        }
        // After the line's edits, which came before them: the input ends a
        // frame at each of these keys.
        if keyboard && !opened {
            if ui.key_pressed(Key::Tab) {
                self.complete();
            }
            if ui.key_pressed(Key::Up) {
                self.older();
            }
            if ui.key_pressed(Key::Down) {
                self.newer();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Result<Vec<String>, ConsoleError> {
        let mut words = vec!["left over".to_string()];
        split(line, &mut words).map(|()| words)
    }

    #[test]
    fn a_line_splits_at_whitespace_and_a_quote_groups_words() {
        assert_eq!(words("").unwrap(), Vec::<String>::new());
        assert_eq!(words("  \t ").unwrap(), Vec::<String>::new());
        assert_eq!(words("tp 10 -2.5").unwrap(), ["tp", "10", "-2.5"]);
        assert_eq!(words("  tp\t10   20  ").unwrap(), ["tp", "10", "20"]);
        assert_eq!(
            words(r#"spawn "big rock" 1"#).unwrap(),
            ["spawn", "big rock", "1"]
        );
        // A quote groups inside a word, and `""` is an empty word.
        assert_eq!(words(r#"a"b c"d "" e"#).unwrap(), ["ab cd", "", "e"]);
        // In quotes, `\"` is a quote and `\\` a backslash; other
        // backslashes are kept, as outside quotes.
        assert_eq!(
            words(r#"say "a \"b\" c\\" C:\x\y "C:\My Traces\t.json""#).unwrap(),
            ["say", r#"a "b" c\"#, r"C:\x\y", r"C:\My Traces\t.json"]
        );
        assert_eq!(words("é ü").unwrap(), ["é", "ü"]);
    }

    #[test]
    fn a_quote_not_closed_is_an_error() {
        assert_eq!(words(r#"say "hello"#), Err(ConsoleError::UnclosedQuote));
        assert_eq!(words(r#"say "a\""#), Err(ConsoleError::UnclosedQuote));
        assert_eq!(words(r#"""#), Err(ConsoleError::UnclosedQuote));
    }

    #[test]
    fn a_usage_s_words_are_words_values_and_choices() {
        assert_eq!(slot("dump"), Slot::Word("dump"));
        assert_eq!(
            slot("<x>"),
            Slot::Value {
                shown: "<x>",
                optional: false
            }
        );
        assert_eq!(
            slot("[path]"),
            Slot::Value {
                shown: "[path]",
                optional: true
            }
        );
        // `[x y]` is two optional values.
        assert!(matches!(slot("[x"), Slot::Value { optional: true, .. }));
        assert!(matches!(slot("y]"), Slot::Value { optional: true, .. }));
        assert_eq!(
            slot("<error|warn>"),
            Slot::Choice {
                shown: "<error|warn>",
                choices: "error|warn",
                optional: false
            }
        );
        assert_eq!(choices("a||b|").collect::<Vec<_>>(), ["a", "b"]);
    }

    fn checked(usage: &str, line: &str) -> Result<Vec<String>, ConsoleError> {
        let mut words = words(line).expect("the line splits");
        check(usage, &mut words).map(|()| words)
    }

    #[test]
    fn arguments_are_checked_against_the_usage() {
        let tp = "tp <x> <y>";
        assert_eq!(checked(tp, "1 2").unwrap(), ["1", "2"]);
        assert_eq!(
            checked(tp, "1"),
            Err(ConsoleError::MissingArgument {
                usage: tp.into(),
                argument: "<y>".into()
            })
        );
        assert_eq!(
            checked(tp, "1 2 3"),
            Err(ConsoleError::TooManyArguments { usage: tp.into() })
        );
        let dump = "profile dump [path]";
        assert_eq!(checked(dump, "dump").unwrap(), ["dump"]);
        assert_eq!(checked(dump, "dump t.json").unwrap(), ["dump", "t.json"]);
        assert_eq!(
            checked(dump, ""),
            Err(ConsoleError::MissingArgument {
                usage: dump.into(),
                argument: "`dump`".into()
            })
        );
        assert_eq!(
            checked(dump, "dupm").unwrap_err().to_string(),
            "argument 1: `dupm` is not `dump`: profile dump [path]"
        );
        // A choice, ignoring case, spelt as the usage spells it.
        let level = "log level <target> <error|warn|info>";
        assert_eq!(
            checked(level, "level nadir_render WARN").unwrap(),
            ["level", "nadir_render", "warn"]
        );
        assert_eq!(
            checked(level, "level nadir_render loud")
                .unwrap_err()
                .to_string(),
            "<error|warn|info>: `loud` is not one of error, warn, info: \
             log level <target> <error|warn|info>"
        );
        // Optional values may be left out, from the end.
        let spawn = "spawn <prefab> [x] [y]";
        assert_eq!(checked(spawn, "rock").unwrap(), ["rock"]);
        assert_eq!(checked(spawn, "rock 1 2").unwrap(), ["rock", "1", "2"]);
        assert!(checked("quit", "now").is_err());
        assert_eq!(checked("quit", "").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn args_read_values_and_name_them_by_the_usage() {
        let words = ["1.5".to_string(), "x".to_string(), "inf".to_string()];
        let args = Args {
            usage: "move <dx> <dy> [dz]",
            words: &words,
        };
        assert_eq!((args.len(), args.is_empty()), (3, false));
        assert_eq!(args.f32(0), Ok(1.5));
        assert_eq!(args.str(1), Ok("x"));
        assert_eq!(
            args.f32(1).unwrap_err().to_string(),
            "<dy>: `x` is not a number: move <dx> <dy> [dz]"
        );
        assert!(args.f32(2).is_err(), "a number that is not finite");
        assert_eq!(args.get(3), None);
        assert_eq!(
            args.str(3).unwrap_err().to_string(),
            "argument 4 is missing: move <dx> <dy> [dz]"
        );
    }

    fn console() -> Console<u8> {
        let mut console = Console::new();
        for (usage, id) in [
            ("tp <x> <y>", 1),
            ("timescale <scale>", 2),
            ("profile dump [path]", 3),
            ("log level <target> <error|warn|info|debug|trace>", 4),
            ("level <name>", 5),
            ("quit", 6),
        ] {
            console
                .register(usage, "does a thing", id)
                .expect("a usage");
        }
        console
    }

    fn completed(line: &str) -> Completion {
        complete(&console().commands, line)
    }

    fn list(words: &[&str]) -> Completion {
        Completion::List(words.iter().map(|word| (*word).to_string()).collect())
    }

    #[test]
    fn tab_completes_a_command_s_name() {
        // One match: whole, and a space after it.
        assert_eq!(completed("q"), Completion::Line("quit ".into()));
        assert_eq!(completed("  ti"), Completion::Line("  timescale ".into()));
        assert_eq!(completed("he"), Completion::Line("help ".into()));
        assert_eq!(completed("le"), Completion::Line("level ".into()));
        assert_eq!(completed("lo"), Completion::Line("log ".into()));
        // Several: as far as they agree, then listed.
        assert_eq!(completed("p"), Completion::Line("profile ".into()));
        assert_eq!(completed("ti"), Completion::Line("timescale ".into()));
        assert_eq!(completed("l"), list(&["level", "log"]));
        assert_eq!(
            completed(""),
            list(&["help", "level", "log", "profile", "quit", "timescale", "tp"])
        );
        assert_eq!(completed("t"), list(&["timescale", "tp"]));
        assert_eq!(completed("x"), Completion::Nothing);
        // A name is matched as typed.
        assert_eq!(completed("Q"), Completion::Nothing);
    }

    #[test]
    fn tab_completes_an_argument_from_the_usage() {
        // A word of the usage, after the name.
        assert_eq!(
            completed("profile "),
            Completion::Line("profile dump ".into())
        );
        assert_eq!(
            completed("profile d"),
            Completion::Line("profile dump ".into())
        );
        assert_eq!(completed("profile x"), Completion::Nothing);
        // A value has no completions.
        assert_eq!(completed("profile dump "), Completion::Nothing);
        assert_eq!(completed("tp "), Completion::Nothing);
        // A choice's choices.
        assert_eq!(
            completed("log level nadir_render "),
            list(&["debug", "error", "info", "trace", "warn"])
        );
        assert_eq!(
            completed("log level nadir_render w"),
            Completion::Line("log level nadir_render warn ".into())
        );
        // Past the usage, or after an unknown command, nothing.
        assert_eq!(completed("quit "), Completion::Nothing);
        assert_eq!(completed("nope "), Completion::Nothing);
        // `help`'s argument is a command's name.
        assert_eq!(completed("help q"), Completion::Line("help quit ".into()));
        assert_eq!(completed("help quit "), Completion::Nothing);
        // A word in quotes, or after a quote not closed, is not completed.
        assert_eq!(completed(r#"profile "d"#), Completion::Nothing);
        assert_eq!(completed(r#"level "a b"#), Completion::Nothing);
        assert_eq!(
            completed(r#"spawn "a b" "#),
            Completion::Nothing,
            "no command spawn here"
        );
    }

    #[test]
    fn the_common_prefix_ends_on_a_character() {
        assert_eq!(common_prefix("abc", &["abd", "abx"]), "ab");
        assert_eq!(common_prefix("abc", &[]), "abc");
        assert_eq!(common_prefix("éa", &["éb"]), "é");
        assert_eq!(common_prefix("éa", &["eb"]), "");
    }

    #[test]
    fn a_usage_must_start_with_a_name() {
        let mut console = console();
        for usage in [
            "", "   ", "<x>", "[x]", "\"say\"", "a|b", "help", "help [x]",
        ] {
            assert_eq!(
                console.register(usage, "", 9),
                Err(ConsoleError::BadUsage(usage.trim().into())),
                "{usage:?}"
            );
        }
        // One of a name already registered replaces it, in its place.
        console
            .register("  quit  now? ", " ends ", 7)
            .expect("a usage");
        let quit = console.commands.iter().find(|c| c.name == "quit").unwrap();
        assert_eq!(
            (quit.usage.as_str(), quit.help.as_str(), quit.command),
            ("quit  now?", "ends", 7)
        );
        assert_eq!(console.commands.len(), 6);
    }

    /// Runs `line`, recording what ran.
    fn run(console: &mut Console<u8>, line: &str) -> Vec<(u8, Vec<String>)> {
        let mut ran = Vec::new();
        console.run_line(line, |command, args, out| {
            ran.push((
                command,
                (0..args.len())
                    .filter_map(|i| args.get(i))
                    .map(String::from)
                    .collect(),
            ));
            if command == 2 {
                let scale = args.f32(0)?;
                out.print(format_args!("scale {scale}\nok"));
            }
            if command == 6 {
                return Err(ConsoleError::Failed("not now".into()));
            }
            Ok(())
        });
        ran
    }

    #[test]
    fn a_line_run_is_echoed_and_its_command_called_with_its_arguments() {
        let mut console = console();
        assert_eq!(
            run(&mut console, "  tp 1 2 "),
            [(1, vec!["1".into(), "2".into()])]
        );
        assert_eq!(
            run(&mut console, "log level nadir_render DEBUG"),
            [(
                4,
                vec!["level".into(), "nadir_render".into(), "debug".into()]
            )]
        );
        assert_eq!(run(&mut console, "timescale 0.5").len(), 1);
        assert_eq!(run(&mut console, "timescale fast").len(), 1);
        // Not run: a line that does not fit, an unknown command, a quote
        // not closed, whitespace.
        assert!(run(&mut console, "tp 1").is_empty());
        assert!(run(&mut console, "warp 1 2").is_empty());
        assert!(run(&mut console, "level \"x").is_empty());
        assert!(run(&mut console, "   ").is_empty());
        assert_eq!(run(&mut console, "quit").len(), 1);
        assert_eq!(
            console.lines().collect::<Vec<_>>(),
            [
                "> tp 1 2",
                "> log level nadir_render DEBUG",
                "> timescale 0.5",
                "scale 0.5",
                "ok",
                "> timescale fast",
                "<scale>: `fast` is not a number: timescale <scale>",
                "> tp 1",
                "<y> is missing: tp <x> <y>",
                "> warp 1 2",
                "no command `warp`: `help` lists them",
                "> level \"x",
                "a quote is not closed",
                "> quit",
                "not now",
            ]
        );
        let tones: Vec<Tone> = console.lines.lines.iter().map(|line| line.tone).collect();
        assert_eq!(tones[0], Tone::Echo);
        assert_eq!(tones[3], Tone::Text);
        assert_eq!(tones[6], Tone::Error);
        assert_eq!(tones[14], Tone::Error);
    }

    #[test]
    fn help_lists_the_commands_in_columns_or_says_what_one_does() {
        let mut console = Console::new();
        console
            .register("tp <x> <y>", "moves the player", 1u8)
            .unwrap();
        console.register("quit", "ends the game", 2).unwrap();
        assert!(run(&mut console, "help").is_empty());
        assert!(run(&mut console, "help tp").is_empty());
        assert!(run(&mut console, "help help").is_empty());
        assert!(run(&mut console, "help nope").is_empty());
        assert!(run(&mut console, "help tp quit").is_empty());
        assert_eq!(
            console.lines().collect::<Vec<_>>(),
            [
                "> help",
                "help [command]  lists the commands, or says what one does",
                "tp <x> <y>      moves the player",
                "quit            ends the game",
                "> help tp",
                "tp <x> <y>  moves the player",
                "> help help",
                "help [command]  lists the commands, or says what one does",
                "> help nope",
                "no command `nope`: `help` lists them",
                "> help tp quit",
                "too many arguments: help [command]",
            ]
        );
    }

    #[test]
    fn up_and_down_go_through_the_history_back_to_the_line_typed() {
        let mut console = console();
        console.older();
        assert_eq!(console.line(), "", "no history yet");
        for line in ["tp 1 2", "quit", "quit", "  tp 1 2", "level a"] {
            run(&mut console, line);
        }
        // `quit` twice in a row is kept once.
        assert_eq!(console.history, ["tp 1 2", "quit", "tp 1 2", "level a"]);
        console.set_line("half typ");
        console.older();
        assert_eq!(console.line(), "level a");
        console.older();
        console.older();
        console.older();
        assert_eq!(console.line(), "tp 1 2");
        console.older();
        assert_eq!(console.line(), "tp 1 2", "the oldest stays");
        console.newer();
        assert_eq!(console.line(), "quit");
        console.newer();
        console.newer();
        console.newer();
        assert_eq!(console.line(), "half typ", "back to what was typed");
        console.newer();
        assert_eq!(console.line(), "half typ");
        // The history keeps the last lines only.
        for n in 0..CONSOLE_HISTORY + 5 {
            run(&mut console, &format!("level {n}"));
        }
        assert_eq!(console.history.len(), CONSOLE_HISTORY);
        assert_eq!(console.history[0], "level 5");
    }

    #[test]
    fn tab_completes_the_console_s_line_or_lists_the_matches() {
        let mut console = console();
        console.set_line("t");
        console.complete();
        assert_eq!(console.line(), "t");
        let listed: Vec<&str> = console.lines().collect();
        assert_eq!(listed, ["timescale", "tp"], "a match a line");
        console.set_line("prof");
        console.complete();
        assert_eq!(console.line(), "profile ");
        console.complete();
        assert_eq!(console.line(), "profile dump ");
    }

    #[test]
    fn the_log_keeps_its_last_lines_colors_records_and_reuses_its_strings() {
        let mut console: Console<u8> = Console::new();
        console.log_record("2026-11-14T10:00:00.000Z WARN  game: low\nlower");
        console.log_record("2026-11-14T10:00:00.000Z ERROR game: boom");
        console.log_record("2026-11-14T10:00:00.000Z INFO  game: fine");
        console.print_error("bad\r\nworse");
        let tones: Vec<Tone> = console.lines.lines.iter().map(|line| line.tone).collect();
        assert_eq!(
            tones,
            [
                Tone::Warning,
                Tone::Warning,
                Tone::Error,
                Tone::Text,
                Tone::Error,
                Tone::Error
            ]
        );
        assert_eq!(console.lines().nth(1), Some("lower"));
        assert_eq!(console.lines().nth(5), Some("worse"));
        for n in 0..CONSOLE_LINES {
            console.print(format_args!("line {n}, long enough to keep"));
        }
        assert_eq!(console.lines().len(), CONSOLE_LINES);
        assert_eq!(console.lines().next(), Some("line 0, long enough to keep"));
        let oldest = console.lines.lines[0].text.as_ptr();
        console.print("short");
        assert_eq!(console.lines().last(), Some("short"));
        assert_eq!(console.lines.lines[CONSOLE_LINES - 1].text.as_ptr(), oldest);
    }
}
