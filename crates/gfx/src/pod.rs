//! Plain old data: values that can be uploaded to the GPU as bytes.

/// Plain old data, which can be viewed as bytes.
///
/// It is implemented for `u8`, `u16`, `u32`, `i32` and `f32`, and for arrays
/// of them. Other crates deny `unsafe` and cannot implement it, so they pack
/// their data into slices of these: render's instance data is a `Vec<f32>`.
///
/// ```compile_fail
/// nadir_gfx::bytes_of(&[true]);
/// ```
///
/// ```compile_fail
/// nadir_gfx::bytes_of(&[(1u8, 2u32)]);
/// ```
///
/// # Safety
///
/// Implementors must be `#[repr(C)]` (or a primitive), contain no padding
/// bytes, no pointers or references, and every bit pattern must be valid.
pub unsafe trait Pod: Copy + 'static {}

// SAFETY: an integer has no padding and no pointers, and every bit pattern
// is one of its values.
unsafe impl Pod for u8 {}

// SAFETY: as for `u8`.
unsafe impl Pod for u16 {}

// SAFETY: as for `u8`.
unsafe impl Pod for u32 {}

// SAFETY: as for `u8`.
unsafe impl Pod for i32 {}

// SAFETY: an `f32` has no padding and no pointers, and every bit pattern is
// one of its values, NaNs included.
unsafe impl Pod for f32 {}

// SAFETY: an array is its elements one after another. Each element's size is
// a multiple of its alignment, so there is no padding between them, and the
// elements are `Pod`.
unsafe impl<T: Pod, const N: usize> Pod for [T; N] {}

/// The bytes of `slice`, in memory order.
pub fn bytes_of<T: Pod>(slice: &[T]) -> &[u8] {
    // SAFETY: the pointer and length cover exactly the bytes of `slice`, which
    // stay borrowed for the result's lifetime. `T: Pod` has no padding, so
    // every byte is initialized, and `u8` needs no alignment.
    unsafe { core::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), size_of_val(slice)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_are_their_bytes_in_memory_order() {
        assert_eq!(bytes_of(&[1u8, 2, 255]), [1, 2, 255]);
        assert_eq!(bytes_of(&[0x0102u16]), 0x0102u16.to_ne_bytes());
        assert_eq!(
            bytes_of(&[7u32, 9]),
            [7u32.to_ne_bytes(), 9u32.to_ne_bytes()].concat()
        );
        assert_eq!(bytes_of(&[-1i32]), (-1i32).to_ne_bytes());
        assert_eq!(bytes_of(&[1.5f32]), 1.5f32.to_ne_bytes());
    }

    #[test]
    fn arrays_are_flat() {
        let matrix = [[1.0f32, 2.0, 3.0, 4.0]; 4];
        let bytes = bytes_of(&matrix);
        assert_eq!(bytes.len(), 64);
        assert_eq!(&bytes[4..8], 2.0f32.to_ne_bytes());
        assert_eq!(&bytes[60..], 4.0f32.to_ne_bytes());
    }

    #[test]
    fn an_empty_slice_has_no_bytes() {
        let empty: [f32; 0] = [];
        assert!(bytes_of(&empty).is_empty());
    }
}
