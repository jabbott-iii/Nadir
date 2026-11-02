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

//! File-format codecs for the Nadir engine: Adler-32, inflate, PNG, QOI, WAV,
//! KLZ block compression, JSON(C), binary JSON (KBJ), BMFont, Aseprite's
//! sprite sheet data, LDtk's projects, and the cook's atlas, level and
//! prefab files. QOA comes with the audio (roadmap M6.1).
//!
//! Every decoder treats its input as untrusted: it returns a [`CodecError`]
//! for anything malformed, checks sizes against [`Limits`], and never
//! panics.

pub mod adler32;
pub mod aseprite;
pub mod atlas;
pub mod bmfont;
mod error;
pub mod inflate;
pub mod json;
pub mod kbj;
pub mod klz;
pub mod ldtk;
pub mod level;
mod limits;
#[cfg(feature = "png")]
pub mod png;
pub mod prefab;
pub mod qoi;
pub mod wav;

pub use error::CodecError;
pub use limits::Limits;
