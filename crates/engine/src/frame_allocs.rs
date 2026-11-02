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

//! [`FrameAllocs`]: what each frame allocated, in development builds.

use nadir_platform::AllocCounts;

/// What the main thread allocated in the last frame: a resource of
/// development builds (feature `dev`).
///
/// A frame counts from the end of the one before it to its own end: the
/// wait for it, its events, its ticks, the frame systems, the drawing and
/// the asset loading. A frame system reads the frame before its own. A
/// steady frame should allocate nothing.
///
/// Only Rust's global allocator is counted, not what C libraries such as
/// the GPU driver allocate themselves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameAllocs(pub AllocCounts);
