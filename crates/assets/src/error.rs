//! The error type for `nadir-assets`.

use std::{fmt, io};

use nadir_codec::CodecError;

/// Why an asset could not be loaded.
///
/// It does not hold the asset's path: the server logs every failure with
/// its path, and [`AssetServer::path`](crate::AssetServer::path) gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetError {
    /// The path is not one an asset can have.
    BadPath(&'static str),
    /// The path does not end in an extension of the asset type.
    Extension {
        /// The extensions the type takes.
        expected: &'static [&'static str],
    },
    /// There is no file at the path.
    NotFound,
    /// The file could not be read.
    Read(io::ErrorKind),
    /// A codec refused the file.
    Decode(CodecError),
    /// The asset type refused the asset, or could not make it, for a reason
    /// of its own: data that does not fit it, say, or a GPU error.
    Invalid(String),
    /// Making the asset needs a resource that the
    /// [`FinalizeCtx`](crate::FinalizeCtx) does not hold, such as the GPU
    /// device.
    MissingContext(&'static str),
    /// The asset type has no placeholder.
    NoPlaceholder,
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadPath(reason) => write!(f, "not a valid asset path: {reason}"),
            Self::Extension { expected } => {
                write!(f, "the path must end in .{}", expected.join(" or ."))
            }
            Self::NotFound => f.write_str("no such file"),
            Self::Read(kind) => write!(f, "the file could not be read: {kind}"),
            Self::Decode(error) => write!(f, "{error}"),
            Self::Invalid(reason) => f.write_str(reason),
            Self::MissingContext(resource) => {
                write!(
                    f,
                    "making it needs a `{resource}`, which the context does not hold"
                )
            }
            Self::NoPlaceholder => f.write_str("the asset type has no placeholder"),
        }
    }
}

impl std::error::Error for AssetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_say_what_went_wrong() {
        let cases = [
            (
                AssetError::BadPath("a `..` component"),
                "not a valid asset path: a `..` component",
            ),
            (
                AssetError::Extension {
                    expected: &["qoi", "png"],
                },
                "the path must end in .qoi or .png",
            ),
            (AssetError::NotFound, "no such file"),
            (
                AssetError::Read(io::ErrorKind::PermissionDenied),
                "the file could not be read: permission denied",
            ),
            (AssetError::Invalid("not UTF-8".to_string()), "not UTF-8"),
            (
                AssetError::MissingContext("nadir_gfx::Device"),
                "making it needs a `nadir_gfx::Device`, which the context does not hold",
            ),
            (
                AssetError::NoPlaceholder,
                "the asset type has no placeholder",
            ),
        ];
        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
        }
    }
}
