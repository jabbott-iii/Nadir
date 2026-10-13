//! The error type for `nadir-bench`.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Why the benchmarks could not run or report.
#[derive(Debug)]
pub(crate) enum BenchError {
    /// The command line was not understood.
    Usage(String),
    /// A file could not be read.
    Read {
        /// The file.
        path: PathBuf,
        /// What the OS reported.
        source: io::Error,
    },
    /// A file could not be written.
    Write {
        /// The file.
        path: PathBuf,
        /// What the OS reported.
        source: io::Error,
    },
    /// The baseline file is not a benchmark report.
    Baseline {
        /// The file.
        path: PathBuf,
        /// What is wrong with it.
        detail: String,
    },
    /// A benchmark returned without timing anything. The name is the
    /// benchmark's.
    Untimed(String),
    /// Writing to standard output failed.
    Output(io::Error),
}

impl fmt::Display for BenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => f.write_str(message),
            Self::Read { path, source } => {
                write!(f, "cannot read `{}`: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(f, "cannot write `{}`: {source}", path.display())
            }
            Self::Baseline { path, detail } => {
                write!(
                    f,
                    "`{}` is not a benchmark report: {detail}",
                    path.display()
                )
            }
            Self::Untimed(name) => write!(f, "benchmark `{name}` did not time anything"),
            Self::Output(source) => write!(f, "cannot write to standard output: {source}"),
        }
    }
}

impl std::error::Error for BenchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } | Self::Output(source) => {
                Some(source)
            }
            Self::Usage(_) | Self::Baseline { .. } | Self::Untimed(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_file_and_the_problem() {
        let error = BenchError::Baseline {
            path: PathBuf::from("bench/baseline.json"),
            detail: "`benchmarks` is missing".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "`bench/baseline.json` is not a benchmark report: `benchmarks` is missing"
        );
        let error = BenchError::Write {
            path: PathBuf::from("target/bench/latest.json"),
            source: io::Error::from(io::ErrorKind::PermissionDenied),
        };
        assert!(
            error
                .to_string()
                .starts_with("cannot write `target/bench/latest.json`: "),
            "{error}"
        );
    }
}
