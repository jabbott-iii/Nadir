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

//! Size limits that bound what a decoder will accept.

/// Upper bounds a decoder enforces on its input, so that a malformed or
/// hostile file cannot exhaust memory or the stack.
///
/// [`Limits::default`] gives the values the game uses at runtime. Tools may
/// raise them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// How deeply arrays and objects may nest in JSON and KBJ. A document
    /// that is only scalars has depth 0; `[[1]]` has depth 2.
    ///
    /// Parsing uses one stack frame per level, so keep this modest.
    pub max_depth: u32,
    /// The longest string, in bytes of UTF-8 after escapes are decoded.
    /// Applies to object keys as well.
    pub max_string_bytes: usize,
    /// The most elements one array, or members one object, may have; and
    /// the most pages, glyphs or kerning pairs of a bitmap font.
    pub max_entries: usize,
    /// The widest, or tallest, image a decoder accepts, in pixels.
    pub max_texture_dimension: u32,
    /// The most bytes a decoder may produce for one file. An image takes 4
    /// bytes a pixel, a WAV sound 4 bytes a sample (as `f32`), a QOA or KPCM
    /// sound 2 (as `i16`), and a KLZ stream the size it is decoded to; the
    /// resampler's output takes 4 bytes a sample.
    ///
    /// It bounds the output. A decoder may hold more while it works: the
    /// PNG decoder keeps the inflated image data beside the pixels, up to
    /// 2.25 times their size, and a copy of the compressed image data when
    /// it is split over several chunks.
    pub max_decoded_bytes: usize,
}

impl Default for Limits {
    /// Depth 64, strings of 1 MiB, 1,000,000 entries per array or object,
    /// images up to 8192 pixels wide and tall, and 256 MiB decoded from one
    /// file.
    fn default() -> Self {
        Self {
            max_depth: 64,
            max_string_bytes: 1024 * 1024,
            max_entries: 1_000_000,
            max_texture_dimension: 8192,
            max_decoded_bytes: 256 * 1024 * 1024,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documented_runtime_limits() {
        let limits = Limits::default();
        assert_eq!(limits.max_depth, 64);
        assert_eq!(limits.max_string_bytes, 1_048_576);
        assert_eq!(limits.max_entries, 1_000_000);
        assert_eq!(limits.max_texture_dimension, 8192);
        assert_eq!(limits.max_decoded_bytes, 268_435_456);
        // The largest image fits exactly.
        assert_eq!(8192 * 8192 * 4, limits.max_decoded_bytes);
    }
}
