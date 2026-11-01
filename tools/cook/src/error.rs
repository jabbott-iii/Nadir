//! The error type for `nadir-cook`.

use std::path::PathBuf;
use std::{fmt, io};

use nadir_codec::CodecError;

/// Why a cook failed. Paths are below the source folder, as each field
/// says: as found, or lowercased, as an asset path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CookError {
    /// A file or folder could not be read, written or removed.
    Io {
        /// The file or folder, as given.
        path: PathBuf,
        /// What was being done: "read", "write" and so on.
        action: &'static str,
        /// What went wrong.
        kind: io::ErrorKind,
    },
    /// The output folder is the source folder, or one holds the other.
    Overlap {
        /// The source folder.
        source: PathBuf,
        /// The output folder.
        out: PathBuf,
    },
    /// A file the cook reads has a path that is not an asset path, or a
    /// name that is not UTF-8.
    BadPath {
        /// The path, as found.
        path: String,
        /// Why it is not one.
        reason: &'static str,
    },
    /// A file the cook would read is a symbolic link, which it does not
    /// follow.
    Link {
        /// The link, as found.
        path: String,
    },
    /// Two names in one folder differ only in case, and one of them is
    /// cooked or holds what is.
    CaseCollision {
        /// One of them, as found.
        first: String,
        /// The other, as found.
        second: String,
    },
    /// An image directly in the source folder: an image's folder names its
    /// group.
    ImageAtRoot {
        /// The image, as found.
        path: String,
    },
    /// A `.sprite.jsonc` with no `.png` of its name beside it.
    OrphanSidecar {
        /// The sidecar, as found.
        path: String,
    },
    /// A sidecar or an `atlas.jsonc` that says something wrong.
    Config {
        /// The file, as found.
        path: String,
        /// What is wrong.
        reason: String,
    },
    /// An image that does not fit the sidecar's frames or tiles.
    Layout {
        /// The image, lowercased.
        path: String,
        /// What does not fit.
        reason: String,
    },
    /// A PNG the decoder refused.
    Decode {
        /// The image, lowercased.
        path: String,
        /// Why.
        error: CodecError,
    },
    /// A frame too big for the largest page, with its extrusion.
    TooBig {
        /// The image, lowercased.
        path: String,
        /// The frame, from 0.
        frame: usize,
        /// Its width and height, trimmed.
        size: (u32, u32),
    },
    /// Two outputs, or folders of outputs, that would be one on a system
    /// that ignores case: groups' atlas files, or tile sets' pages.
    OutputCollision {
        /// One, below the output folder.
        first: String,
        /// The other.
        second: String,
    },
    /// An encoder refused what the cook made, or the game's atlas reader
    /// refused the atlas file it encoded: a pivot past what an `f32`
    /// holds, say.
    Encode {
        /// The output, below the output folder.
        path: String,
        /// Why.
        error: CodecError,
    },
}

impl CookError {
    pub(crate) fn io(path: impl Into<PathBuf>, action: &'static str, error: &io::Error) -> Self {
        Self::Io {
            path: path.into(),
            action,
            kind: error.kind(),
        }
    }
}

impl fmt::Display for CookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, action, kind } => {
                write!(f, "could not {action} `{}`: {kind}", path.display())
            }
            Self::Overlap { source, out } => write!(
                f,
                "the output folder `{}` and the source folder `{}` must not hold each other",
                out.display(),
                source.display()
            ),
            Self::BadPath { path, reason } => write!(f, "`{path}` is not an asset path: {reason}"),
            Self::Link { path } => write!(
                f,
                "`{path}` is a symbolic link, which the cook does not follow"
            ),
            Self::CaseCollision { first, second } => write!(
                f,
                "`{first}` and `{second}` differ only in case, which Windows and macOS do not tell apart"
            ),
            Self::ImageAtRoot { path } => write!(
                f,
                "`{path}` is not in a folder: an image's folder names its atlas group"
            ),
            Self::OrphanSidecar { path } => {
                write!(f, "`{path}` has no image of its name beside it")
            }
            Self::Config { path, reason } | Self::Layout { path, reason } => {
                write!(f, "`{path}`: {reason}")
            }
            Self::Decode { path, error } => write!(f, "`{path}`: {error}"),
            Self::TooBig {
                path,
                frame,
                size: (w, h),
            } => write!(
                f,
                "`{path}`: frame {frame}, {w} × {h} once trimmed, does not fit the largest page, 4096 × 4096, with its extrusion"
            ),
            Self::OutputCollision { first, second } => write!(
                f,
                "`{first}` and `{second}` would be one file on Windows and macOS"
            ),
            Self::Encode { path, error } => write!(f, "`{path}`: {error}"),
        }
    }
}

impl std::error::Error for CookError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode { error, .. } | Self::Encode { error, .. } => Some(error),
            _ => None,
        }
    }
}
