//! Non-cryptographic hashing: [`fnv1a64`] for stable keys, [`crc32`] for
//! integrity checks, and [`FxLikeHasher`] for fast, deterministic hash maps.
//!
//! None of these resist deliberate collisions. Do not use them on input an
//! attacker controls where that would matter.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// Hashes `bytes` with 64-bit FNV-1a.
///
/// The result is stable across runs, platforms and engine versions, so it is
/// safe to store on disk. It is the hash behind asset keys.
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET_BASIS;
    let mut i = 0;
    while i < bytes.len() {
        hash = (hash ^ bytes[i] as u64).wrapping_mul(PRIME);
        i += 1;
    }
    hash
}

/// One table entry per byte value, for the reflected IEEE polynomial.
const CRC32_TABLE: [u32; 256] = {
    const POLYNOMIAL: u32 = 0xedb8_8320;

    let mut table = [0; 256];
    let mut byte = 0;
    while byte < 256 {
        let mut crc = byte as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ POLYNOMIAL
            };
            bit += 1;
        }
        table[byte] = crc;
        byte += 1;
    }
    table
};

/// Computes the CRC-32 of `bytes`: the IEEE 802.3 checksum used by PNG, zlib
/// and gzip.
///
/// It detects accidental corruption. It is not a defense against tampering.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        let entry = CRC32_TABLE[usize::from(crc.to_le_bytes()[0] ^ byte)];
        crc = entry ^ (crc >> 8);
    }
    !crc
}

/// A fast, deterministic hasher for in-memory hash maps, in the style of Fx
/// hashing: rotate, xor, multiply, one word at a time.
///
/// It has no per-process random state, so a map that uses it iterates in an
/// order that depends only on the history of operations on that map. That
/// keeps simulation replays deterministic.
///
/// The output is not stable across engine versions. Do not store it on disk;
/// use [`fnv1a64`] for that.
#[derive(Clone, Copy, Debug, Default)]
pub struct FxLikeHasher {
    hash: u64,
}

impl FxLikeHasher {
    /// An odd constant with well-spread bits: 2⁶⁴ / π.
    const MULTIPLIER: u64 = 0x517c_c1b7_2722_0a95;

    fn add_word(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(Self::MULTIPLIER);
    }
}

impl Hasher for FxLikeHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (words, tail) = bytes.as_chunks::<8>();
        for &word in words {
            self.add_word(u64::from_le_bytes(word));
        }
        if !tail.is_empty() {
            let mut last = [0; 8];
            last[..tail.len()].copy_from_slice(tail);
            self.add_word(u64::from_le_bytes(last));
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.add_word(u64::from(value));
    }

    fn write_u16(&mut self, value: u16) {
        self.add_word(u64::from(value));
    }

    fn write_u32(&mut self, value: u32) {
        self.add_word(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.add_word(value);
    }

    fn write_usize(&mut self, value: usize) {
        self.add_word(value as u64);
    }

    fn finish(&self) -> u64 {
        // The multiply mixes upward, so the high bits are the best mixed.
        // Hash tables pick buckets from the low bits: rotate the good bits
        // down, or keys that differ only in their high bits would collide.
        self.hash.rotate_left(26)
    }
}

/// The [`BuildHasher`](std::hash::BuildHasher) behind [`FastHashMap`] and
/// [`FastHashSet`].
pub type FastBuildHasher = BuildHasherDefault<FxLikeHasher>;

/// A [`HashMap`] that uses [`FxLikeHasher`]. Create one with
/// `FastHashMap::default()`.
///
/// Use this instead of the standard `HashMap` in simulation code: the
/// standard hasher is seeded randomly per process, which makes iteration
/// order differ from run to run.
pub type FastHashMap<K, V> = HashMap<K, V, FastBuildHasher>;

/// A [`HashSet`] that uses [`FxLikeHasher`]. Create one with
/// `FastHashSet::default()`.
pub type FastHashSet<T> = HashSet<T, FastBuildHasher>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::for_cases;
    use std::hash::{BuildHasher, Hash};

    fn fx(feed: impl FnOnce(&mut FxLikeHasher)) -> u64 {
        let mut hasher = FxLikeHasher::default();
        feed(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn fnv1a64_matches_reference_vectors() {
        // From the FNV reference test suite.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
        assert_eq!(fnv1a64(b"sprites/player.qoi"), 0x42ce_5f3f_73e0_aa16);
    }

    #[test]
    fn fnv1a64_works_in_const_context() {
        const KEY: u64 = fnv1a64(b"foobar");
        assert_eq!(KEY, 0x8594_4171_f739_67e8);
    }

    #[test]
    fn crc32_matches_reference_vectors() {
        // The standard CRC-32 check value.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"a"), 0xe8b7_be43);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414f_a339
        );
        let every_byte: Vec<u8> = (0..=u8::MAX).collect();
        assert_eq!(crc32(&every_byte), 0x2905_8c73);
    }

    #[test]
    fn crc32_detects_any_single_bit_flip() {
        for_cases("crc32 single bit flip", 200, |rng| {
            let len = 1 + rng.next_u32() as usize % 64;
            let mut data: Vec<u8> = (0..len).map(|_| rng.next_u32() as u8).collect();
            let original = crc32(&data);
            let bit = rng.next_u32() as usize % (len * 8);
            data[bit / 8] ^= 1 << (bit % 8);
            if crc32(&data) == original {
                return Err(format!(
                    "flipping bit {bit} of {len} bytes kept CRC {original:#010x}"
                ));
            }
            Ok(())
        });
    }

    #[test]
    fn fx_like_hasher_output_is_pinned() {
        // Expected values come from an independent implementation of the
        // algorithm. They pin the function across platforms and processes;
        // changing them changes every FastHashMap's iteration order.
        assert_eq!(fx(|_| {}), 0);
        assert_eq!(fx(|h| h.write_u64(42)), 0x31ae_56f1_c979_df20);
        assert_eq!(fx(|h| h.write_u64(65536)), 0x882a_5400_0306_dc9c);
        assert_eq!(
            fx(|h| {
                h.write_u32(1);
                h.write_u32(2);
            }),
            0xffe6_3eaf_21a9_2f99
        );
        assert_eq!(fx(|h| h.write(b"hello")), 0x1a9b_7456_2122_95a9);
        assert_eq!(fx(|h| h.write(b"12345678")), 0xea0d_1082_17a1_c4b9);
        let twenty: Vec<u8> = (0..20).collect();
        assert_eq!(fx(|h| h.write(&twenty)), 0x68e7_75ec_0bbd_abcc);
    }

    #[test]
    fn integer_writes_agree_across_widths() {
        let expected = fx(|h| h.write_u64(200));
        assert_eq!(fx(|h| h.write_u8(200)), expected);
        assert_eq!(fx(|h| h.write_u16(200)), expected);
        assert_eq!(fx(|h| h.write_u32(200)), expected);
        assert_eq!(fx(|h| h.write_usize(200)), expected);
    }

    #[test]
    fn keys_that_differ_only_in_high_bits_spread_over_low_bits() {
        // Without the final rotate, every one of these would land in the
        // same bucket of a small table.
        let mut buckets = FastHashSet::default();
        for n in 0..256u64 {
            buckets.insert(fx(|h| h.write_u64(n << 32)) & 0xff);
        }
        assert!(
            buckets.len() > 128,
            "only {} of 256 buckets used",
            buckets.len()
        );
    }

    #[test]
    fn fast_hash_map_and_set_work() {
        let mut map = FastHashMap::default();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("a", 3);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("a"), Some(&3));
        assert_eq!(map.get("c"), None);

        let mut set = FastHashSet::default();
        assert!(set.insert((1, 2)));
        assert!(!set.insert((1, 2)));
        assert!(set.contains(&(1, 2)));
    }

    #[test]
    fn hashing_is_the_same_for_every_hasher_instance() {
        fn hash_of(value: impl Hash) -> u64 {
            FastBuildHasher::default().hash_one(value)
        }
        assert_eq!(hash_of("player"), hash_of("player"));
        assert_eq!(hash_of((3u32, -7i32)), hash_of((3u32, -7i32)));
        assert_ne!(hash_of("player"), hash_of("enemy"));
    }

    #[test]
    fn iteration_order_depends_only_on_operation_history() {
        for_cases("fast hash map iteration order", 50, |rng| {
            let ops: Vec<(bool, u32)> = (0..300)
                .map(|_| (rng.next_u32() % 4 != 0, rng.next_u32() % 128))
                .collect();
            let build = || {
                let mut map = FastHashMap::default();
                for &(insert, key) in &ops {
                    if insert {
                        map.insert(key, key.wrapping_mul(3));
                    } else {
                        map.remove(&key);
                    }
                }
                map.into_iter().collect::<Vec<_>>()
            };
            let (first, second) = (build(), build());
            if first != second {
                return Err(format!(
                    "same operations, different order:\n{first:?}\n{second:?}"
                ));
            }
            Ok(())
        });
    }
}
