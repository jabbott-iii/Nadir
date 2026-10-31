//! Adler-32 (RFC 1950): the checksum at the end of a zlib stream.
//!
//! ```
//! use nadir_codec::adler32;
//!
//! assert_eq!(adler32::checksum(b"Wikipedia"), 0x11e6_0398);
//! ```
//!
//! It detects accidental corruption. It is weaker than CRC-32 on short
//! inputs, and no defense against tampering.

/// The largest prime below 2¹⁶.
const MODULUS: u32 = 65_521;

/// The most bytes that can be summed before the sums must be reduced: the
/// largest `n` with `255 n (n + 1) / 2 + (n + 1) (65521 − 1) < 2³²`, as
/// zlib has it. Sums start each run below 65521.
const MOST_BEFORE_REDUCING: usize = 5552;

/// The Adler-32 of `bytes`. That of no bytes is 1.
pub fn checksum(bytes: &[u8]) -> u32 {
    update(1, bytes)
}

/// Continues the running checksum `adler`, whose halves are each below
/// 65521, over `bytes`.
fn update(adler: u32, bytes: &[u8]) -> u32 {
    let mut a = adler & 0xffff;
    let mut b = adler >> 16;
    for part in bytes.chunks(MOST_BEFORE_REDUCING) {
        for &byte in part {
            a += u32::from(byte);
            b += a;
        }
        a %= MODULUS;
        b %= MODULUS;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The definition, one byte at a time.
    fn naive(bytes: &[u8]) -> u32 {
        naive_from(1, bytes)
    }

    /// The definition, continuing from the sums in `adler`.
    fn naive_from(adler: u32, bytes: &[u8]) -> u32 {
        let (mut a, mut b) = (u64::from(adler & 0xffff), u64::from(adler >> 16));
        for &byte in bytes {
            a = (a + u64::from(byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        u32::try_from((b << 16) | a).unwrap()
    }

    #[test]
    fn known_checksums() {
        assert_eq!(checksum(b""), 1);
        assert_eq!(checksum(b"a"), 0x0062_0062);
        assert_eq!(checksum(b"Wikipedia"), 0x11e6_0398);
        assert_eq!(checksum(b"123456789"), 0x091e_01de);
    }

    #[test]
    fn long_runs_of_the_largest_byte_do_not_overflow() {
        // 0xff is the worst case for the unreduced sums.
        for len in [5551, 5552, 5553, 11_104, 100_000] {
            let bytes = vec![0xff; len];
            assert_eq!(checksum(&bytes), naive(&bytes), "{len}");
        }
    }

    #[test]
    fn any_bytes_match_the_definition() {
        let bytes: Vec<u8> = (0..20_000u32).map(|i| (i * 7 + i / 13) as u8).collect();
        assert_eq!(checksum(&bytes), naive(&bytes));
    }

    #[test]
    fn the_largest_sums_do_not_overflow_before_they_are_reduced() {
        // From sums of 65520 each, 5552 bytes of 0xff bring `b` within
        // 277,096 of 2^32, and one more byte would pass it: the bound is
        // tight only from here, not from a checksum's start.
        let bytes = vec![0xff; 3 * 5552 + 17];
        let largest = (65_520 << 16) | 65_520;
        assert_eq!(update(largest, &bytes), naive_from(largest, &bytes));
    }
}
