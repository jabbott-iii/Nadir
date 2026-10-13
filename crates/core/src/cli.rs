//! A minimal command-line argument parser for `xtask`, the cook tool and the
//! game binary.
//!
//! It understands `--flag`, `--key value` (also `--key=value`), positional
//! arguments, and a bare `--` that makes everything after it positional.
//!
//! There is no schema. The caller pulls out what it understands, then calls
//! [`Args::finish`], which rejects whatever options are left:
//!
//! ```
//! use nadir_core::cli::Args;
//!
//! let mut args = Args::new(["fuzz-smoke", "--secs", "60", "--verbose", "json"]);
//! assert_eq!(args.subcommand().as_deref(), Some("fuzz-smoke"));
//! let secs: u64 = args.parsed("--secs")?.unwrap_or(10);
//! let verbose = args.flag("--verbose");
//! let targets = args.finish()?;
//! assert_eq!((secs, verbose, targets), (60, true, vec!["json".to_string()]));
//! # Ok::<(), nadir_core::CoreError>(())
//! ```
//!
//! Because there is no schema, take every option before calling `finish`: an
//! option's value is only recognized as such when its option is asked for.

use std::str::FromStr;

use crate::error::CoreError;

/// The arguments of one command-line invocation, consumed piece by piece.
#[derive(Clone, Debug)]
pub struct Args {
    /// Arguments before any bare `--`, minus those already taken.
    items: Vec<String>,
    /// Arguments after a bare `--`: always positional.
    trailing: Vec<String>,
}

impl Args {
    /// Creates a parser over `args`, which must not include the program
    /// name.
    pub fn new<I>(args: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        let mut items: Vec<String> = args.into_iter().map(Into::into).collect();
        let trailing = match items.iter().position(|item| item == "--") {
            Some(separator) => {
                let trailing = items.split_off(separator + 1);
                items.pop();
                trailing
            }
            None => Vec::new(),
        };
        Self { items, trailing }
    }

    /// Creates a parser over the arguments of the current process, without
    /// the program name.
    ///
    /// An argument that is not valid Unicode has its invalid parts replaced
    /// with `U+FFFD`.
    pub fn from_env() -> Self {
        Self::new(
            std::env::args_os()
                .skip(1)
                .map(|arg| arg.to_string_lossy().into_owned()),
        )
    }

    /// Takes the first argument if it is not an option, for `tool <command>
    /// ...` layouts. Call this before anything else.
    pub fn subcommand(&mut self) -> Option<String> {
        match self.items.first() {
            Some(first) if !is_option(first) => Some(self.items.remove(0)),
            _ => None,
        }
    }

    /// Takes the flag `name` (for example `"--bless"`) and returns whether
    /// it was present.
    pub fn flag(&mut self, name: &str) -> bool {
        match self.items.iter().position(|item| item == name) {
            Some(index) => {
                self.items.remove(index);
                true
            }
            None => false,
        }
    }

    /// Takes the option `name` (for example `"--target"`) and its value,
    /// given as `--target json` or `--target=json`.
    ///
    /// Returns `Ok(None)` if the option is absent, and an error if it is
    /// present without a value. A following argument that starts with `--`
    /// is not taken as the value; use the `=` form for such values.
    pub fn value(&mut self, name: &str) -> Result<Option<String>, CoreError> {
        let is_match = |item: &String| item == name || inline_value(item, name).is_some();
        let Some(index) = self.items.iter().position(is_match) else {
            return Ok(None);
        };
        if let Some(value) = inline_value(&self.items[index], name) {
            let value = value.to_string();
            self.items.remove(index);
            return Ok(Some(value));
        }
        let has_value = self
            .items
            .get(index + 1)
            .is_some_and(|next| !next.starts_with("--"));
        if !has_value {
            return Err(CoreError::MissingOptionValue {
                option: name.to_string(),
            });
        }
        let value = self.items.remove(index + 1);
        self.items.remove(index);
        Ok(Some(value))
    }

    /// Takes the option `name` like [`Args::value`] and parses its value.
    pub fn parsed<T: FromStr>(&mut self, name: &str) -> Result<Option<T>, CoreError> {
        let Some(value) = self.value(name)? else {
            return Ok(None);
        };
        match value.parse() {
            Ok(parsed) => Ok(Some(parsed)),
            Err(_) => Err(CoreError::InvalidOptionValue {
                option: name.to_string(),
                value,
            }),
        }
    }

    /// Finishes parsing and returns the positional arguments in order.
    ///
    /// Fails if an option is left that nothing took: an unknown option, a
    /// repeated one, or a flag that was given a value.
    pub fn finish(self) -> Result<Vec<String>, CoreError> {
        if let Some(unexpected) = self.items.iter().find(|item| is_option(item)) {
            return Err(CoreError::UnexpectedArgument {
                argument: unexpected.clone(),
            });
        }
        let mut positionals = self.items;
        positionals.extend(self.trailing);
        Ok(positionals)
    }
}

/// Returns the value of `item` if it has the form `name=value`.
fn inline_value<'a>(item: &'a str, name: &str) -> Option<&'a str> {
    item.strip_prefix(name)?.strip_prefix('=')
}

/// Anything that starts with `-` is an option, except a lone `-` (by
/// convention, standard input) and negative numbers.
fn is_option(arg: &str) -> bool {
    arg.len() > 1 && arg.starts_with('-') && arg.parse::<f64>().is_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unexpected(argument: &str) -> CoreError {
        CoreError::UnexpectedArgument {
            argument: argument.to_string(),
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn empty_command_line() {
        let mut args = Args::new(Vec::<String>::new());
        assert_eq!(args.subcommand(), None);
        assert!(!args.flag("--bless"));
        assert_eq!(args.value("--target"), Ok(None));
        assert_eq!(args.parsed::<u32>("--secs"), Ok(None));
        assert_eq!(args.finish(), Ok(Vec::new()));
    }

    #[test]
    fn flags_values_and_positionals_in_any_order() {
        let mut args = Args::new([
            "golden",
            "scene_a",
            "--backend",
            "metal",
            "--bless",
            "scene_b",
        ]);
        assert_eq!(args.subcommand().as_deref(), Some("golden"));
        assert!(args.flag("--bless"));
        assert!(!args.flag("--bless"));
        assert_eq!(args.value("--backend"), Ok(Some("metal".to_string())));
        assert_eq!(args.value("--backend"), Ok(None));
        assert_eq!(args.finish(), Ok(strings(&["scene_a", "scene_b"])));
    }

    #[test]
    fn equals_form() {
        let mut args = Args::new(["--target=json", "--secs=60", "--name=", "--expr=--x"]);
        assert_eq!(args.value("--target"), Ok(Some("json".to_string())));
        assert_eq!(args.parsed::<u64>("--secs"), Ok(Some(60)));
        assert_eq!(args.value("--name"), Ok(Some(String::new())));
        assert_eq!(args.value("--expr"), Ok(Some("--x".to_string())));
        assert_eq!(args.finish(), Ok(Vec::new()));
    }

    #[test]
    fn an_option_name_does_not_match_a_longer_option() {
        let mut args = Args::new(["--target-dir", "out", "--targets=3"]);
        assert_eq!(args.value("--target"), Ok(None));
        assert!(!args.flag("--target"));
        assert_eq!(args.value("--target-dir"), Ok(Some("out".to_string())));
        assert_eq!(args.finish(), Err(unexpected("--targets=3")));
    }

    #[test]
    fn subcommand_is_only_the_first_argument() {
        let mut args = Args::new(["--verbose", "bench", "json"]);
        assert_eq!(args.subcommand(), None);
        assert!(args.flag("--verbose"));
        assert_eq!(args.subcommand().as_deref(), Some("bench"));
        assert_eq!(args.finish(), Ok(strings(&["json"])));
    }

    #[test]
    fn missing_values_are_errors() {
        let missing = || CoreError::MissingOptionValue {
            option: "--secs".to_string(),
        };
        // At the end of the line.
        assert_eq!(Args::new(["--secs"]).value("--secs"), Err(missing()));
        // Followed by another option.
        assert_eq!(
            Args::new(["--secs", "--target", "json"]).value("--secs"),
            Err(missing())
        );
        assert_eq!(
            Args::new(["--secs"]).parsed::<u64>("--secs"),
            Err(missing())
        );
    }

    #[test]
    fn unparsable_values_are_errors() {
        let mut args = Args::new(["--secs", "soon", "--scale", "1.5"]);
        let expected = CoreError::InvalidOptionValue {
            option: "--secs".to_string(),
            value: "soon".to_string(),
        };
        assert_eq!(args.parsed::<u64>("--secs"), Err(expected));
        assert_eq!(args.parsed::<f32>("--scale"), Ok(Some(1.5)));
    }

    #[test]
    fn negative_numbers_are_values_and_positionals() {
        let mut args = Args::new(["tp", "--offset", "-5", "-12.5", "-", "-3"]);
        assert_eq!(args.subcommand().as_deref(), Some("tp"));
        assert_eq!(args.parsed::<i32>("--offset"), Ok(Some(-5)));
        assert_eq!(args.finish(), Ok(strings(&["-12.5", "-", "-3"])));
    }

    #[test]
    fn leftover_options_are_rejected() {
        // Unknown option.
        assert_eq!(
            Args::new(["--frobnicate"]).finish(),
            Err(unexpected("--frobnicate"))
        );
        assert_eq!(Args::new(["-x", "file"]).finish(), Err(unexpected("-x")));

        // Given twice.
        let mut twice = Args::new(["--secs", "1", "--secs", "2"]);
        assert_eq!(twice.parsed::<u64>("--secs"), Ok(Some(1)));
        assert_eq!(twice.finish(), Err(unexpected("--secs")));

        // A flag with a value it does not take.
        let mut valued_flag = Args::new(["--bless=yes"]);
        assert!(!valued_flag.flag("--bless"));
        assert_eq!(valued_flag.finish(), Err(unexpected("--bless=yes")));
    }

    #[test]
    fn everything_after_a_bare_double_dash_is_positional() {
        let mut args = Args::new(["run", "--verbose", "--", "--bless", "--secs", "5", "--"]);
        assert_eq!(args.subcommand().as_deref(), Some("run"));
        assert!(args.flag("--verbose"));
        // Options after the separator are not options.
        assert!(!args.flag("--bless"));
        assert_eq!(args.value("--secs"), Ok(None));
        assert_eq!(
            args.finish(),
            Ok(strings(&["--bless", "--secs", "5", "--"]))
        );
    }

    #[test]
    fn a_value_cannot_be_taken_from_across_the_separator() {
        let mut args = Args::new(["--secs", "--", "5"]);
        assert_eq!(
            args.value("--secs"),
            Err(CoreError::MissingOptionValue {
                option: "--secs".to_string()
            })
        );
    }

    #[test]
    fn from_env_does_not_include_the_program_name() {
        // The test binary's own path must not show up as an argument.
        let program = std::env::args().next().expect("the test binary has a path");
        let args = Args::from_env();
        assert!(!args.items.contains(&program));
    }
}
