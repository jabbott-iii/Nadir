//! The error type for `xtask`.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

/// Why a command could not do its job.
///
/// A check that ran and found problems is not an error: it returns a
/// [`Report`](crate::check::Report) with violations.
#[derive(Debug)]
pub(crate) enum Error {
    /// The command line was not understood.
    Usage(String),
    /// A file or directory could not be read.
    Read {
        /// The file or directory.
        path: PathBuf,
        /// What the OS reported.
        source: io::Error,
    },
    /// A command could not be started.
    Spawn {
        /// The command line.
        command: String,
        /// What the OS reported.
        source: io::Error,
    },
    /// A command ran and reported failure.
    CommandFailed {
        /// The command line.
        command: String,
        /// How the command ended.
        status: ExitStatus,
        /// What the command wrote to standard error, if that was captured.
        stderr: String,
    },
    /// A command printed something this tool does not understand.
    UnexpectedOutput {
        /// The command line.
        command: String,
        /// What is wrong with the output.
        detail: String,
    },
    /// Writing to standard output failed.
    Output(io::Error),
    /// A file could not be written.
    Write {
        /// The file.
        path: PathBuf,
        /// What the OS reported.
        source: io::Error,
    },
    /// A fuzz target has no seed files to start from.
    NoSeeds {
        /// The corpus directory of the target.
        path: PathBuf,
    },
    /// This tool could not do something that should always work.
    Internal(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => f.write_str(message),
            Self::Read { path, source } => {
                write!(f, "cannot read `{}`: {source}", path.display())
            }
            Self::Spawn { command, source } => write!(f, "cannot run `{command}`: {source}"),
            Self::CommandFailed {
                command,
                status,
                stderr,
            } => {
                write!(f, "`{command}` failed ({status})")?;
                match stderr.trim_end() {
                    "" => Ok(()),
                    stderr => write!(f, ":\n{stderr}"),
                }
            }
            Self::UnexpectedOutput { command, detail } => {
                write!(f, "unexpected output from `{command}`: {detail}")
            }
            Self::Output(source) => write!(f, "cannot write to standard output: {source}"),
            Self::Write { path, source } => {
                write!(f, "cannot write `{}`: {source}", path.display())
            }
            Self::NoSeeds { path } => write!(f, "no seed files in `{}`", path.display()),
            Self::Internal(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. }
            | Self::Spawn { source, .. }
            | Self::Write { source, .. }
            | Self::Output(source) => Some(source),
            Self::Usage(_)
            | Self::CommandFailed { .. }
            | Self::UnexpectedOutput { .. }
            | Self::NoSeeds { .. }
            | Self::Internal(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_file_or_command() {
        let read = Error::Read {
            path: PathBuf::from("xtask/deps-allowlist.txt"),
            source: io::Error::from(io::ErrorKind::NotFound),
        };
        let message = read.to_string();
        assert!(
            message.starts_with("cannot read `xtask/deps-allowlist.txt`: "),
            "{message}"
        );

        let unexpected = Error::UnexpectedOutput {
            command: "cargo metadata".to_string(),
            detail: "`packages` is missing".to_string(),
        };
        assert_eq!(
            unexpected.to_string(),
            "unexpected output from `cargo metadata`: `packages` is missing"
        );
    }
}
