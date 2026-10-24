//! The allocation counter of development builds (feature `dev`). The global
//! allocator counts every heap allocation, for each thread and for the
//! whole process, so that tests and the game can show that a steady frame
//! allocates nothing.
//!
//! With the feature on, every binary that links this crate allocates
//! through it, and no other crate may declare a `#[global_allocator]`: the
//! build fails if one does. Turn `dev` on only from a `[features]` table
//! or a `[dev-dependencies]` entry, never from a normal dependency, so that
//! no release build carries it.
//!
//! It sees only what goes through Rust's global allocator. C libraries that
//! call `malloc` themselves, such as Mesa, Xlib and EGL, are not counted.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

/// Counts of heap use, on one thread or in the whole process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocCounts {
    /// Allocations, reallocations included: a reallocation is how a `Vec`
    /// or a `String` grows.
    pub allocations: u64,
    /// Deallocations. A reallocation is not one.
    pub deallocations: u64,
    /// The bytes the allocations asked for: for a reallocation, its new
    /// size.
    pub bytes: u64,
}

impl AllocCounts {
    /// The counts since `earlier`, an earlier reading of the same counts.
    #[must_use]
    pub fn since(self, earlier: AllocCounts) -> AllocCounts {
        AllocCounts {
            allocations: self.allocations.wrapping_sub(earlier.allocations),
            deallocations: self.deallocations.wrapping_sub(earlier.deallocations),
            bytes: self.bytes.wrapping_sub(earlier.bytes),
        }
    }
}

/// The heap use of this thread since it started.
pub fn thread_alloc_counts() -> AllocCounts {
    THREAD.try_with(Cell::get).unwrap_or_default()
}

/// The heap use of every thread since the process started.
pub fn process_alloc_counts() -> AllocCounts {
    AllocCounts {
        allocations: PROCESS_ALLOCATIONS.load(Ordering::Relaxed),
        deallocations: PROCESS_DEALLOCATIONS.load(Ordering::Relaxed),
        bytes: PROCESS_BYTES.load(Ordering::Relaxed),
    }
}

const ZERO: AllocCounts = AllocCounts {
    allocations: 0,
    deallocations: 0,
    bytes: 0,
};

thread_local! {
    /// This thread's counts. A thread local gets its memory without the
    /// global allocator, and this one has a constant start and no
    /// destructor, so the allocator can read it without recursing.
    static THREAD: Cell<AllocCounts> = const { Cell::new(ZERO) };
}

static PROCESS_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static PROCESS_DEALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static PROCESS_BYTES: AtomicU64 = AtomicU64::new(0);

/// Counts an allocation of `bytes`. It never allocates, locks or panics, as
/// code inside the allocator must not.
fn count_allocation(bytes: usize) {
    let bytes = bytes as u64;
    PROCESS_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    PROCESS_BYTES.fetch_add(bytes, Ordering::Relaxed);
    // Only a thread that is being torn down has no counts left to add to.
    let _ = THREAD.try_with(|counts| {
        let mut thread = counts.get();
        thread.allocations = thread.allocations.wrapping_add(1);
        thread.bytes = thread.bytes.wrapping_add(bytes);
        counts.set(thread);
    });
}

/// Counts a deallocation, as [`count_allocation`] counts an allocation.
fn count_deallocation() {
    PROCESS_DEALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    let _ = THREAD.try_with(|counts| {
        let mut thread = counts.get();
        thread.deallocations = thread.deallocations.wrapping_add(1);
        counts.set(thread);
    });
}

/// The global allocator of development builds: [`System`], counted.
struct Counting;

#[global_allocator]
static ALLOCATOR: Counting = Counting;

// SAFETY: each method passes its arguments, and its caller's guarantees, to
// the same method of `System`, which meets the `GlobalAlloc` contract, and
// returns what `System` returns. The counting never allocates, unwinds or
// touches the memory.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller meets `GlobalAlloc::alloc`'s requirements on
        // `layout`, which are `System::alloc`'s.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            count_allocation(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as for `alloc`.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            count_allocation(layout.size());
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller guarantees that `pointer` came from this
        // allocator, which is `System`'s, with `layout`, and that
        // `new_size` is valid for it, as `System::realloc` requires.
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if !moved.is_null() {
            count_allocation(new_size);
        }
        moved
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller guarantees that `pointer` came from this
        // allocator, which is `System`'s, with `layout`.
        unsafe { System.dealloc(pointer, layout) };
        count_deallocation();
    }
}

#[cfg(test)]
mod tests {
    use std::hint::black_box;
    use std::thread;

    use super::*;

    #[test]
    fn a_box_counts_one_allocation_of_its_size_and_one_deallocation() {
        let start = thread_alloc_counts();
        let boxed = black_box(Box::new([7u8; 100]));
        let allocated = thread_alloc_counts().since(start);
        drop(boxed);
        let freed = thread_alloc_counts().since(start);
        assert_eq!(
            allocated,
            AllocCounts {
                allocations: 1,
                deallocations: 0,
                bytes: 100
            }
        );
        assert_eq!(
            freed,
            AllocCounts {
                allocations: 1,
                deallocations: 1,
                bytes: 100
            }
        );
    }

    #[test]
    fn a_vec_counts_its_growth_and_not_its_reuse() {
        let start = thread_alloc_counts();
        let mut numbers: Vec<u64> = Vec::with_capacity(4);
        for number in 0..1000 {
            numbers.push(black_box(number));
        }
        let grown = thread_alloc_counts().since(start);
        // The first allocation, then a reallocation each time it doubles.
        assert!((2..=12).contains(&grown.allocations), "{grown:?}");
        assert_eq!(
            grown.deallocations, 0,
            "a reallocation is not a deallocation"
        );

        numbers.clear();
        let refilled = thread_alloc_counts();
        for number in 0..1000 {
            numbers.push(black_box(number));
        }
        assert_eq!(
            thread_alloc_counts().since(refilled),
            AllocCounts::default()
        );
    }

    #[test]
    fn another_threads_allocations_count_for_the_process_and_not_here() {
        const MIB: u64 = 1 << 20;
        let (here, process) = (thread_alloc_counts(), process_alloc_counts());
        let theirs = thread::spawn(|| {
            let start = thread_alloc_counts();
            drop(black_box(vec![1u8; MIB as usize]));
            thread_alloc_counts().since(start)
        })
        .join()
        .expect("the thread runs");
        assert_eq!(theirs.allocations, 1);
        assert_eq!(theirs.bytes, MIB);
        // Spawning a thread allocates a little here, far less than theirs.
        let mine = thread_alloc_counts().since(here);
        assert!(mine.bytes < MIB, "{mine:?}");
        // Other tests allocate at the same time, so the process may count
        // more, never less.
        assert!(process_alloc_counts().since(process).bytes >= MIB);
    }
}
