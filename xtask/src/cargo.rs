//! Running `cargo` from the workspace root.

use std::env;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::Error;

/// Runs `cargo <args>` in `root`. The command prints straight to the
/// terminal.
pub(crate) fn run(root: &Path, args: &[&str]) -> Result<(), Error> {
    run_with_env(root, args, &[])
}

/// Runs `cargo <args>` in `root`, with `env` added to the environment it
/// inherits. The command prints straight to the terminal.
pub(crate) fn run_with_env(
    root: &Path,
    args: &[&str],
    env: &[(&str, &OsStr)],
) -> Result<(), Error> {
    let status = command(root, args)
        .envs(env.iter().copied())
        .status()
        .map_err(|source| Error::Spawn {
            command: display(args),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::CommandFailed {
            command: display(args),
            status,
            stderr: String::new(),
        })
    }
}

/// Runs `cargo <args>` in `root` and returns what it wrote to standard
/// output.
pub(crate) fn capture(root: &Path, args: &[&str]) -> Result<String, Error> {
    let output = command(root, args)
        .output()
        .map_err(|source| Error::Spawn {
            command: display(args),
            source,
        })?;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            command: display(args),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    String::from_utf8(output.stdout).map_err(|_| Error::UnexpectedOutput {
        command: display(args),
        detail: "the output is not valid UTF-8".to_string(),
    })
}

/// The command line as the user would type it, for messages.
pub(crate) fn display(args: &[&str]) -> String {
    format!("cargo {}", args.join(" "))
}

fn command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(program());
    command.args(args).current_dir(root).stdin(Stdio::null());
    command
}

/// The cargo that is running this tool, which sets `CARGO` for it. Falls
/// back to whatever `cargo` is on the path.
fn program() -> OsString {
    env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_the_command_line() {
        assert_eq!(
            display(&["fmt", "--all", "--", "--check"]),
            "cargo fmt --all -- --check"
        );
    }
}
