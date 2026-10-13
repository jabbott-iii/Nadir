//! File-format codecs for the Nadir engine: Adler-32, inflate, PNG, QOI, QOA,
//! WAV, KLZ block compression, JSON(C), binary JSON (KBJ) and BMFont.
//!
//! Every decoder treats its input as untrusted: it returns a [`CodecError`]
//! for anything malformed, checks sizes against [`Limits`], and never
//! panics.

mod error;
pub mod json;
mod limits;

pub use error::CodecError;
pub use limits::Limits;
