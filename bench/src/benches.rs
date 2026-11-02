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

//! The benchmarks. Each one is a name and a function that sets up its
//! input and hands the work to the [`Bencher`].

mod collision;
mod ecs;
mod facade;
mod inflate;
mod json;
mod qoi;
mod sprite;
mod tiles;

use crate::harness::Bencher;

/// One benchmark.
pub(crate) struct Benchmark {
    /// The name that filters match and that reports use. It should not
    /// change once a baseline holds it.
    pub(crate) name: &'static str,
    /// Sets up and calls [`Bencher::iter`] once.
    pub(crate) run: fn(&mut Bencher),
}

/// Every benchmark, in the order they run. `docs/04-quality.md` lists the
/// ones the engine must have by the end.
pub(crate) const ALL: &[Benchmark] = &[
    Benchmark {
        name: "json_parse",
        run: json::parse,
    },
    Benchmark {
        name: "qoi_decode_2048",
        run: qoi::decode,
    },
    Benchmark {
        name: "inflate_10mb",
        run: inflate::zlib,
    },
    Benchmark {
        name: "sprite_sort_batch",
        run: sprite::sort_batch,
    },
    Benchmark {
        name: "tile_chunk_rebuild",
        run: tiles::chunk_rebuild,
    },
    Benchmark {
        name: "ecs_join2_5k",
        run: ecs::join2,
    },
    Benchmark {
        name: "ecs_join3_5k",
        run: ecs::join3,
    },
    Benchmark {
        name: "ecs_spawn_despawn",
        run: ecs::spawn_despawn,
    },
    Benchmark {
        name: "collision_hash_5k",
        run: collision::hash,
    },
    Benchmark {
        name: "collision_slide_5k",
        run: collision::slide,
    },
    Benchmark {
        name: "facade_tick_5k",
        run: facade::tick,
    },
    Benchmark {
        name: "facade_frame_10k",
        run: facade::frame,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_distinct_and_easy_to_type() {
        for (index, benchmark) in ALL.iter().enumerate() {
            let name = benchmark.name;
            assert!(
                !name.is_empty()
                    && name.bytes().all(|byte| byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_'),
                "{name:?}"
            );
            assert!(
                ALL[..index].iter().all(|earlier| earlier.name != name),
                "{name} is listed twice"
            );
        }
    }
}
