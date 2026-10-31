//! File-format codecs for the Nadir engine: Adler-32, inflate, PNG, QOI, QOA,
//! WAV, KLZ block compression, JSON(C), binary JSON (KBJ) and BMFont.
//!
//! Every decoder treats its input as untrusted: it returns a [`CodecError`]
//! for anything malformed, checks sizes against [`Limits`], and never
//! panics.

pub mod adler32;
mod error;
pub mod inflate;
pub mod json;
mod limits;
#[cfg(feature = "png")]
pub mod png;
pub mod qoi;

pub use error::CodecError;
pub use limits::Limits;
