//! The benchmarks. Each one is a name and a function that sets up its
//! input and hands the work to the [`Bencher`].

mod collision;
mod ecs;
mod json;
mod sprite;

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
        name: "sprite_sort_batch",
        run: sprite::sort_batch,
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
