//! The mutation engine: makes a new input out of a seed by a few small,
//! random changes.
//!
//! The changes are those listed in `docs/04-quality.md`: bit flips, bytes
//! inserted and deleted, splices from other seeds, small steps on what may
//! be a length field, and integers that sit on a boundary.

use nadir_core::Pcg32;

/// Integers that parsers tend to mishandle: zero, one, and the values on
/// either side of where a signed or unsigned 8, 16 or 32-bit number wraps.
const INTERESTING: &[u32] = &[
    0,
    1,
    0x7f,
    0x80,
    0xff,
    0x100,
    0x7fff,
    0x8000,
    0xffff,
    0x1_0000,
    0x7fff_ffff,
    0x8000_0000,
    u32::MAX,
];

/// [`mutate`] applies between 1 and 2 to this power changes to an input.
const MAX_ROUNDS_LOG2: u32 = 4;

/// Runs of bytes that are inserted, deleted or copied are between 1 and 2
/// to this power bytes long.
const MAX_RUN_LOG2: u32 = 7;

/// The largest step [`Mutation::Arithmetic`] takes, up or down.
const MAX_STEP: u32 = 16;

/// One kind of change to an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mutation {
    /// Flip one bit.
    FlipBit,
    /// Replace one byte with a random one.
    SetByte,
    /// Insert a run of random bytes, or a run of one byte that the input
    /// already uses (more brackets, more digits).
    Insert,
    /// Delete a run of bytes.
    Delete,
    /// Insert a copy of a run of the input's own bytes.
    Duplicate,
    /// Bring in bytes from a seed: either replace the input's tail with the
    /// seed's tail, or insert a run of the seed's bytes.
    Splice,
    /// Overwrite 1, 2 or 4 bytes with one of the [`INTERESTING`] integers,
    /// in either byte order.
    Interesting,
    /// Read 1, 2 or 4 bytes as an integer, in either byte order, and move
    /// it up or down a little. This is what tweaks a length field.
    Arithmetic,
}

impl Mutation {
    /// Every kind of change. [`mutate`] picks among them uniformly.
    pub(crate) const ALL: &[Self] = &[
        Self::FlipBit,
        Self::SetByte,
        Self::Insert,
        Self::Delete,
        Self::Duplicate,
        Self::Splice,
        Self::Interesting,
        Self::Arithmetic,
    ];

    /// Applies this change to `input`. A change that needs more bytes than
    /// `input` has does nothing.
    pub(crate) fn apply(self, rng: &mut Pcg32, input: &mut Vec<u8>, corpus: &[Vec<u8>]) {
        match self {
            Self::FlipBit => {
                if let Some(at) = window(rng, input.len(), 1) {
                    input[at] ^= 1 << rng.range(0..8u32);
                }
            }
            Self::SetByte => {
                if let Some(at) = window(rng, input.len(), 1) {
                    input[at] = random_byte(rng);
                }
            }
            Self::Insert => {
                let at = below(rng, input.len() + 1);
                let count = run_length(rng);
                // A byte the input already uses, half of the time.
                let repeated = match window(rng, input.len(), 1) {
                    Some(from) if coin(rng) => Some(input[from]),
                    _ => None,
                };
                for _ in 0..count {
                    input.push(repeated.unwrap_or_else(|| random_byte(rng)));
                }
                input[at..].rotate_right(count);
            }
            Self::Delete => {
                if let Some(start) = window(rng, input.len(), 1) {
                    let count = run_length(rng).min(input.len() - start);
                    input.drain(start..start + count);
                }
            }
            Self::Duplicate => {
                if let Some(start) = window(rng, input.len(), 1) {
                    let count = run_length(rng).min(input.len() - start);
                    let at = below(rng, input.len() + 1);
                    input.extend_from_within(start..start + count);
                    input[at..].rotate_right(count);
                }
            }
            Self::Splice => {
                let Some(seed) = rng.choose(corpus) else {
                    return;
                };
                let Some(from) = window(rng, seed.len(), 1) else {
                    return;
                };
                if coin(rng) {
                    input.truncate(below(rng, input.len() + 1));
                    input.extend_from_slice(&seed[from..]);
                } else {
                    let count = run_length(rng).min(seed.len() - from);
                    let at = below(rng, input.len() + 1);
                    input.extend_from_slice(&seed[from..from + count]);
                    input[at..].rotate_right(count);
                }
            }
            Self::Interesting => {
                let width = width(rng);
                if let Some(at) = window(rng, input.len(), width) {
                    let value = rng.choose(INTERESTING).copied().unwrap_or(0);
                    write_int(&mut input[at..at + width], value, coin(rng));
                }
            }
            Self::Arithmetic => {
                let width = width(rng);
                if let Some(at) = window(rng, input.len(), width) {
                    let big_endian = coin(rng);
                    let step = 1 + rng.range(0..MAX_STEP);
                    let old = read_int(&input[at..at + width], big_endian);
                    let new = if coin(rng) {
                        old.wrapping_add(step)
                    } else {
                        old.wrapping_sub(step)
                    };
                    write_int(&mut input[at..at + width], new, big_endian);
                }
            }
        }
    }
}

/// Changes `input` in place: between 1 and 16 random [`Mutation`]s, one
/// after another. `corpus` is where splices take their bytes from. The
/// result is at most `max_len` bytes long.
pub(crate) fn mutate(rng: &mut Pcg32, input: &mut Vec<u8>, corpus: &[Vec<u8>], max_len: usize) {
    for _ in 0..rounds(rng) {
        if let Some(mutation) = rng.choose(Mutation::ALL) {
            mutation.apply(rng, input, corpus);
        }
        input.truncate(max_len);
    }
}

/// How many changes to make to one input: 1, 2, 4, 8 or 16.
fn rounds(rng: &mut Pcg32) -> u32 {
    1 << rng.range(0..MAX_ROUNDS_LOG2 + 1)
}

/// A uniform number below `bound`, or 0 if `bound` is 0. Inputs are far
/// shorter than `u32::MAX` bytes; a longer one is only changed near its
/// start.
fn below(rng: &mut Pcg32, bound: usize) -> usize {
    rng.range(0..u32::try_from(bound).unwrap_or(u32::MAX)) as usize
}

/// Where a run of `width` bytes can start in `len` bytes: a uniform choice
/// among the positions where it fits, or `None` if it does not fit.
fn window(rng: &mut Pcg32, len: usize, width: usize) -> Option<usize> {
    let positions = (len + 1)
        .checked_sub(width)
        .filter(|&positions| positions > 0)?;
    Some(below(rng, positions))
}

/// How long a run to insert, delete or copy: a power of two, so that short
/// runs are as likely as long ones.
fn run_length(rng: &mut Pcg32) -> usize {
    1 << rng.range(0..MAX_RUN_LOG2 + 1)
}

/// How many bytes an integer takes: 1, 2 or 4.
fn width(rng: &mut Pcg32) -> usize {
    1 << rng.range(0..3u32)
}

fn coin(rng: &mut Pcg32) -> bool {
    rng.range(0..2u32) == 0
}

fn random_byte(rng: &mut Pcg32) -> u8 {
    rng.next_u32().to_le_bytes()[0]
}

/// Reads `bytes` (at most 4) as an unsigned integer.
fn read_int(bytes: &[u8], big_endian: bool) -> u32 {
    let mut value = 0;
    for index in 0..bytes.len() {
        let byte = if big_endian {
            bytes[index]
        } else {
            bytes[bytes.len() - 1 - index]
        };
        value = (value << 8) | u32::from(byte);
    }
    value
}

/// Writes the low bytes of `value` over all of `bytes` (at most 4).
fn write_int(bytes: &mut [u8], value: u32, big_endian: bool) {
    let width = bytes.len();
    let little = value.to_le_bytes();
    for (index, byte) in bytes.iter_mut().enumerate() {
        let from = if big_endian { width - 1 - index } else { index };
        *byte = little[from];
    }
}

#[cfg(test)]
mod tests {
    use nadir_core::testing;

    use super::*;

    /// Applies `mutation` to a copy of `input` with each of 500 random
    /// streams, and hands every result to `check`.
    fn each_result(
        mutation: Mutation,
        input: &[u8],
        corpus: &[Vec<u8>],
        mut check: impl FnMut(&[u8]),
    ) {
        for stream in 0..500 {
            let mut rng = Pcg32::new(7, stream);
            let mut output = input.to_vec();
            mutation.apply(&mut rng, &mut output, corpus);
            check(&output);
        }
    }

    /// The ways `longer` can be `shorter` with one run of bytes added: the
    /// position of the run in `longer`, for every position that works.
    fn insertions(shorter: &[u8], longer: &[u8]) -> Vec<usize> {
        let count = longer.len() - shorter.len();
        (0..=shorter.len())
            .filter(|&at| longer[..at] == shorter[..at] && longer[at + count..] == shorter[at..])
            .collect()
    }

    const TEXT: &[u8] = b"{\"size\": [480, 270], \"title\": \"Sandbox\"}";

    #[test]
    fn every_mutation_is_listed_once() {
        assert_eq!(Mutation::ALL.len(), 8);
        for (index, mutation) in Mutation::ALL.iter().enumerate() {
            assert!(!Mutation::ALL[..index].contains(mutation), "{mutation:?}");
        }
    }

    #[test]
    fn the_interesting_integers_sit_on_every_wrapping_point() {
        // The ones `docs/04-quality.md` names.
        for value in [0, 1, 255, 65535, u32::MAX] {
            assert!(INTERESTING.contains(&value), "{value}");
        }
        // Either side of where a signed or an unsigned number of each
        // width wraps around.
        for bits in [8, 16, 32] {
            let unsigned_max = u32::MAX >> (32 - bits);
            let signed_max = unsigned_max >> 1;
            for value in [signed_max, signed_max + 1, unsigned_max] {
                assert!(INTERESTING.contains(&value), "{value:#x}");
            }
            if let Some(past_the_end) = unsigned_max.checked_add(1) {
                assert!(INTERESTING.contains(&past_the_end), "{past_the_end:#x}");
            }
        }
        for (index, value) in INTERESTING.iter().enumerate() {
            assert!(!INTERESTING[..index].contains(value), "{value:#x} twice");
        }
    }

    #[test]
    fn flip_bit_changes_exactly_one_bit() {
        let mut positions = Vec::new();
        each_result(Mutation::FlipBit, TEXT, &[], |output| {
            assert_eq!(output.len(), TEXT.len());
            let changed: Vec<(usize, u8)> = (0..TEXT.len())
                .filter(|&at| output[at] != TEXT[at])
                .map(|at| (at, output[at] ^ TEXT[at]))
                .collect();
            assert_eq!(changed.len(), 1);
            assert_eq!(changed[0].1.count_ones(), 1);
            positions.push((changed[0].0, changed[0].1));
        });
        // Every byte and every bit gets its turn.
        assert!(positions.iter().any(|&(at, _)| at == 0));
        assert!(positions.iter().any(|&(at, _)| at == TEXT.len() - 1));
        for bit in 0..8 {
            assert!(positions.iter().any(|&(_, mask)| mask == 1 << bit), "{bit}");
        }
    }

    #[test]
    fn set_byte_changes_at_most_one_byte() {
        let mut values = Vec::new();
        each_result(Mutation::SetByte, TEXT, &[], |output| {
            assert_eq!(output.len(), TEXT.len());
            let changed: Vec<usize> = (0..TEXT.len())
                .filter(|&at| output[at] != TEXT[at])
                .collect();
            assert!(changed.len() <= 1);
            values.extend(changed.iter().map(|&at| output[at]));
        });
        // Not only text: bytes from all over the range.
        assert!(values.iter().any(|&byte| byte < 0x20));
        assert!(values.iter().any(|&byte| byte >= 0x80));
    }

    #[test]
    fn insert_adds_one_run_and_keeps_the_rest_in_order() {
        let (mut lengths, mut repeated, mut mixed) = (Vec::new(), 0, 0);
        let (mut at_start, mut at_end) = (false, false);
        each_result(Mutation::Insert, TEXT, &[], |output| {
            let count = output.len() - TEXT.len();
            assert!((1..=128).contains(&count) && count.is_power_of_two());
            let places = insertions(TEXT, output);
            assert!(!places.is_empty(), "{output:?}");
            lengths.push(count);
            at_start |= places.contains(&0);
            at_end |= places.contains(&TEXT.len());
            let run = &output[places[0]..places[0] + count];
            if count >= 8 && run.iter().all(|&byte| byte == run[0]) {
                assert!(
                    TEXT.contains(&run[0]),
                    "a repeated byte comes from the input"
                );
                repeated += 1;
            } else if count >= 8 {
                mixed += 1;
            }
        });
        assert!(lengths.contains(&1) && lengths.contains(&128));
        assert!(at_start && at_end);
        assert!(repeated > 20 && mixed > 20, "{repeated} {mixed}");
    }

    #[test]
    fn delete_removes_one_run() {
        let mut lengths = Vec::new();
        let (mut from_the_start, mut from_the_middle, mut to_the_end) = (false, false, false);
        each_result(Mutation::Delete, TEXT, &[], |output| {
            assert!(output.len() < TEXT.len());
            let places = insertions(output, TEXT);
            assert!(!places.is_empty(), "{output:?}");
            let count = TEXT.len() - output.len();
            lengths.push(count);
            from_the_start |= places.contains(&0);
            to_the_end |= places.contains(&output.len());
            from_the_middle |= places.iter().all(|&at| at > 0 && at < output.len());
        });
        assert!(from_the_start && from_the_middle && to_the_end);
        assert!(lengths.contains(&1));
        // A run that would go past the end stops at the end.
        assert!(lengths.iter().any(|&count| !count.is_power_of_two()));
        assert!(lengths.iter().all(|&count| count <= TEXT.len()));
    }

    #[test]
    fn duplicate_inserts_a_copy_of_part_of_the_input() {
        let mut lengths = Vec::new();
        let (mut from_the_middle, mut into_the_middle) = (false, false);
        each_result(Mutation::Duplicate, TEXT, &[], |output| {
            let count = output.len() - TEXT.len();
            assert!((1..=TEXT.len()).contains(&count));
            let places = insertions(TEXT, output);
            assert!(!places.is_empty(), "{output:?}");
            let copied = places.iter().any(|&at| {
                let run = &output[at..at + count];
                TEXT.windows(count).any(|window| window == run)
            });
            assert!(copied, "{output:?}");
            lengths.push(count);
            // Long runs are unambiguous: there is one place the copy can
            // be, and one place it can have come from.
            if count >= 8 && places.len() == 1 {
                let at = places[0];
                from_the_middle |= output[at..at + count] != TEXT[..count];
                into_the_middle |= at > 0 && at < TEXT.len();
            }
        });
        assert!(from_the_middle && into_the_middle);
        assert!(lengths.contains(&1) && lengths.iter().any(|&count| count >= 16));
    }

    #[test]
    fn splice_brings_in_bytes_from_a_seed() {
        let input = [b'a'; 40];
        let corpus = [vec![b'b'; 300], Vec::new()];
        let (mut crossed, mut inserted, mut untouched) = (0, 0, 0);
        let (mut kept, mut tails) = (Vec::new(), Vec::new());
        each_result(Mutation::Splice, &input, &corpus, |output| {
            assert!(output.iter().all(|&byte| byte == b'a' || byte == b'b'));
            let first_b = output.iter().position(|&byte| byte == b'b');
            let Some(first_b) = first_b else {
                // The empty seed was picked.
                assert_eq!(output, input);
                untouched += 1;
                return;
            };
            let run = output[first_b..]
                .iter()
                .take_while(|&&byte| byte == b'b')
                .count();
            let tail = &output[first_b + run..];
            assert!(tail.iter().all(|&byte| byte == b'a'));
            assert!(first_b <= input.len());
            if !tail.is_empty() {
                // A run of the seed's bytes, inserted: nothing is lost.
                assert_eq!(output.len(), input.len() + run);
                assert!((1..=128).contains(&run));
                inserted += 1;
            } else if first_b < input.len() {
                // The start of the input, then the end of the seed.
                assert!((1..=300).contains(&run));
                crossed += 1;
                kept.push(first_b);
                tails.push(run);
            } else {
                // Either of the two, at the very end of the input.
                assert!((1..=300).contains(&run));
            }
        });
        assert!(crossed > 50 && inserted > 50 && untouched > 50);
        // A cross-over keeps a start of any length, and takes an end of
        // any length.
        assert!(kept.contains(&0) && kept.iter().any(|&len| len > 20));
        assert!(tails.iter().any(|&len| len < 100) && tails.iter().any(|&len| len > 200));
    }

    #[test]
    fn splice_without_seeds_does_nothing() {
        each_result(Mutation::Splice, TEXT, &[], |output| {
            assert_eq!(output, TEXT)
        });
        each_result(Mutation::Splice, TEXT, &[Vec::new()], |output| {
            assert_eq!(output, TEXT);
        });
    }

    #[test]
    fn interesting_writes_a_boundary_value_in_either_byte_order() {
        let input = [0x55_u8; 12];
        let mut seen = Vec::new();
        each_result(Mutation::Interesting, &input, &[], |output| {
            assert_eq!(output.len(), input.len());
            let changed: Vec<usize> = (0..input.len()).filter(|&at| output[at] != 0x55).collect();
            let (Some(&first), Some(&last)) = (changed.first(), changed.last()) else {
                panic!("no boundary value contains the byte 0x55");
            };
            assert!(last - first < 4);
            seen.push(output[first..=last].to_vec());
        });
        for expected in [
            vec![0x00],
            vec![0x01],
            vec![0x7f],
            vec![0x80],
            vec![0xff],
            vec![0xff, 0xff],
            vec![0x7f, 0xff],
            vec![0xff, 0x7f],
            vec![0x00, 0x80],
            vec![0x80, 0x00],
            vec![0x00, 0x01],
            vec![0x01, 0x00],
            vec![0xff, 0xff, 0xff, 0xff],
            vec![0x7f, 0xff, 0xff, 0xff],
            vec![0xff, 0xff, 0xff, 0x7f],
            vec![0x80, 0x00, 0x00, 0x00],
            vec![0x00, 0x00, 0x00, 0x80],
            vec![0x00, 0x01, 0x00, 0x00],
            vec![0x00, 0x00, 0x01, 0x00],
            vec![0x00, 0x00, 0xff, 0xff],
            vec![0xff, 0xff, 0x00, 0x00],
        ] {
            assert!(seen.contains(&expected), "{expected:02x?}");
        }
    }

    #[test]
    fn arithmetic_steps_a_number_by_at_most_16() {
        // Every byte differs from its neighbours, so that the changed bytes
        // show which number moved.
        let input: Vec<u8> = (0..12).map(|index| 0x40 + 0x0b * index).collect();
        let mut steps = Vec::new();
        each_result(Mutation::Arithmetic, &input, &[], |output| {
            assert_eq!(output.len(), input.len());
            // Find a width, a place and a byte order that explain it.
            let mut explained = None;
            for width in [1, 2, 4] {
                for at in 0..=input.len() - width {
                    let outside_same =
                        output[..at] == input[..at] && output[at + width..] == input[at + width..];
                    if !outside_same {
                        continue;
                    }
                    for big_endian in [false, true] {
                        let number = |bytes: &[u8]| {
                            let mut padded = [0_u8; 4];
                            if big_endian {
                                padded[4 - width..].copy_from_slice(bytes);
                                u32::from_be_bytes(padded)
                            } else {
                                padded[..width].copy_from_slice(bytes);
                                u32::from_le_bytes(padded)
                            }
                        };
                        let modulus = 1_i64 << (8 * width);
                        let old = i64::from(number(&input[at..at + width]));
                        let new = i64::from(number(&output[at..at + width]));
                        let up = (new - old).rem_euclid(modulus);
                        let step = if up <= 16 { up } else { up - modulus };
                        if (1..=16).contains(&step.abs()) {
                            explained = Some(step);
                        }
                    }
                }
            }
            steps.push(explained.unwrap_or_else(|| panic!("{output:02x?}")));
        });
        for expected in [-16, -1, 1, 16] {
            assert!(steps.contains(&expected), "{expected}");
        }
    }

    #[test]
    fn arithmetic_wraps_around_within_its_width() {
        let mut wrapped = false;
        each_result(
            Mutation::Arithmetic,
            &[0xff, 0xff, 0xff, 0xff],
            &[],
            |output| {
                wrapped |= output.iter().any(|&byte| byte < 0x10);
            },
        );
        assert!(wrapped);
    }

    #[test]
    fn integers_are_read_and_written_in_both_byte_orders() {
        assert_eq!(read_int(&[0x12], false), 0x12);
        assert_eq!(read_int(&[0x12], true), 0x12);
        assert_eq!(read_int(&[0x34, 0x12], false), 0x1234);
        assert_eq!(read_int(&[0x12, 0x34], true), 0x1234);
        assert_eq!(read_int(&[0x78, 0x56, 0x34, 0x12], false), 0x1234_5678);
        assert_eq!(read_int(&[0x12, 0x34, 0x56, 0x78], true), 0x1234_5678);

        let mut bytes = [0; 4];
        write_int(&mut bytes, 0x1234_5678, false);
        assert_eq!(bytes, [0x78, 0x56, 0x34, 0x12]);
        write_int(&mut bytes, 0x1234_5678, true);
        assert_eq!(bytes, [0x12, 0x34, 0x56, 0x78]);
        // A narrower window takes the low bytes.
        let mut bytes = [0; 2];
        write_int(&mut bytes, 0x1234_5678, false);
        assert_eq!(bytes, [0x78, 0x56]);
        write_int(&mut bytes, 0x1234_5678, true);
        assert_eq!(bytes, [0x56, 0x78]);
        let mut bytes = [0; 1];
        write_int(&mut bytes, 0x1234_5678, true);
        assert_eq!(bytes, [0x78]);
    }

    #[test]
    fn changes_that_need_bytes_leave_a_short_input_alone() {
        for &mutation in Mutation::ALL {
            each_result(mutation, &[], &[], |output| {
                if mutation == Mutation::Insert {
                    assert!(!output.is_empty());
                } else {
                    assert_eq!(output, b"", "{mutation:?}");
                }
            });
        }
        // One byte is too short for a 2 or 4-byte integer, but not for a
        // 1-byte one.
        for mutation in [Mutation::Interesting, Mutation::Arithmetic] {
            let mut changed = 0;
            each_result(mutation, &[0x55], &[], |output| {
                assert_eq!(output.len(), 1);
                changed += usize::from(output != [0x55]);
            });
            assert!((50..450).contains(&changed), "{mutation:?}: {changed}");
        }
    }

    #[test]
    fn mutate_makes_1_2_4_8_or_16_changes() {
        let mut seen = [0_u32; 17];
        for stream in 0..500 {
            let count = rounds(&mut Pcg32::new(5, stream));
            assert!([1, 2, 4, 8, 16].contains(&count), "{count}");
            seen[count as usize] += 1;
        }
        for count in [1, 2, 4, 8, 16] {
            assert!(seen[count] > 50, "{count}: {}", seen[count]);
        }
    }

    #[test]
    fn mutate_stacks_changes_and_nearly_always_changes_something() {
        let mut longest = 0;
        let mut changed = 0;
        for stream in 0..2000 {
            let mut rng = Pcg32::new(11, stream);
            let mut input = TEXT.to_vec();
            mutate(&mut rng, &mut input, &[], usize::MAX);
            changed += usize::from(input != TEXT);
            longest = longest.max(input.len());
        }
        // No single change adds more than 128 bytes.
        assert!(longest > TEXT.len() + 128, "{longest}");
        assert!(changed > 1900, "{changed}");
    }

    #[test]
    fn mutate_gives_the_same_result_for_the_same_random_stream() {
        let corpus = [TEXT.to_vec(), b"[1, 2, 3]".to_vec()];
        let run = |seed| {
            let mut rng = Pcg32::new(seed, 3);
            let mut input = TEXT.to_vec();
            mutate(&mut rng, &mut input, &corpus, 4096);
            input
        };
        assert_eq!(run(1), run(1));
        assert_ne!(run(1), run(2));
    }

    #[test]
    fn mutate_never_exceeds_the_length_limit_and_never_panics() {
        testing::check_sized(
            "fuzz_mutate",
            3000,
            |rng, size| {
                let bytes = |rng: &mut Pcg32, len: u32| -> Vec<u8> {
                    (0..len).map(|_| random_byte(rng)).collect()
                };
                let len = rng.range(0..size * 3 + 1);
                let input = bytes(rng, len);
                let seeds = rng.range(0..4u32);
                let corpus: Vec<Vec<u8>> = (0..seeds)
                    .map(|_| {
                        let len = rng.range(0..size * 3 + 1);
                        bytes(rng, len)
                    })
                    .collect();
                let max_len = rng.range(0..400u32) as usize;
                (input, corpus, max_len, rng.next_u32())
            },
            |(input, corpus, max_len, seed)| {
                let mut rng = Pcg32::new(u64::from(*seed), 0);
                let mut output = input.clone();
                for _ in 0..8 {
                    mutate(&mut rng, &mut output, corpus, *max_len);
                    if output.len() > *max_len {
                        return Err(format!("{} bytes", output.len()));
                    }
                }
                Ok(())
            },
        );
    }
}
