//! Work that must not allocate, counted by the allocation counter of the
//! `dev` feature. The engine's own steady frame is checked in `app.rs`.
#![cfg(feature = "dev")]

use std::hint::black_box;

use nadir::thread_alloc_counts;
use nadir_codec::qoi;
use nadir_codec::{CodecError, Limits};

/// A QOI file: the header, `body`, and the end marker.
fn qoi_file(width: u32, height: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = b"qoif".to_vec();
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[4, 0]);
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
    bytes
}

#[test]
fn a_qoi_file_too_short_for_its_size_is_refused_before_anything_is_allocated() {
    // 23 bytes that claim 8192 × 8192 pixels, 256 MiB: the largest image
    // the limits allow, with room for one run.
    let huge = qoi_file(8192, 8192, &[0xfd]);
    assert_eq!(huge.len(), 23);
    let start = thread_alloc_counts();
    let result = qoi::decode(black_box(&huge), Limits::default());
    let allocated = thread_alloc_counts().since(start);
    assert!(matches!(result, Err(CodecError::Qoi { .. })), "{result:?}");
    assert_eq!(allocated.allocations, 0, "{allocated:?}");

    // The same count sees a file that does decode allocate its pixels.
    let small = qoi_file(8, 8, &[0xfd, 0xc1]);
    let start = thread_alloc_counts();
    let image = qoi::decode(black_box(&small), Limits::default()).expect("64 pixels");
    assert!(thread_alloc_counts().since(start).bytes >= 8 * 8 * 4);
    drop(image);
}
