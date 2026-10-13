//! Scoped timers: [`profile_scope!`](crate::profile_scope) records how long
//! a block ran, and [`take_frame`] collects the records.
//!
//! Recording is behind the `profile` feature. With the feature off, which is
//! the default, `profile_scope!` expands to nothing and [`take_frame`]
//! yields no records, so call sites need no `cfg` of their own.

use std::time::{Duration, Instant};

/// The most records one thread keeps between calls to [`take_frame`].
/// Scopes that finish beyond this are dropped.
///
/// The limit bounds memory on threads that record scopes but never collect
/// them.
pub const MAX_RECORDS_PER_THREAD: usize = 65_536;

/// One finished scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeRecord {
    /// The name given to `profile_scope!`.
    pub name: &'static str,
    /// When the scope was entered.
    pub start: Instant,
    /// How long the scope ran.
    pub duration: Duration,
    /// Nesting depth: 0 for a scope with no enclosing scope on its thread.
    pub depth: u32,
}

/// Times the rest of the enclosing block under `name`, a `&'static str`.
///
/// ```
/// fn update_world() {
///     nadir_core::profile_scope!("update_world");
///     // ... everything to the end of the function is timed ...
/// }
/// # update_world();
/// ```
///
/// Without the `profile` feature of `nadir-core` this expands to nothing,
/// and `name` is not evaluated.
#[macro_export]
macro_rules! profile_scope {
    ($name:expr) => {
        $crate::__profile_scope!($name);
    };
}

#[doc(hidden)]
#[macro_export]
#[cfg(feature = "profile")]
macro_rules! __profile_scope {
    ($name:expr) => {
        let _profile_scope = $crate::profile::Scope::enter($name);
    };
}

#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "profile"))]
macro_rules! __profile_scope {
    ($name:expr) => {};
}

/// Moves the calling thread's finished scopes into `out`, replacing what it
/// held.
///
/// Call this once per frame, and pass the same `Vec` each time: its
/// allocation is handed back to the recorder, so steady-state profiling does
/// not allocate.
///
/// Records are in the order their scopes ended, so a parent follows its
/// children. A scope that is still open is not included; it appears in the
/// frame in which it ends.
pub fn take_frame(out: &mut Vec<ScopeRecord>) {
    out.clear();
    #[cfg(feature = "profile")]
    recorder::swap(out);
}

#[cfg(feature = "profile")]
pub use recorder::Scope;

#[cfg(feature = "profile")]
mod recorder {
    use std::cell::{Cell, RefCell};
    use std::time::Instant;

    use super::{MAX_RECORDS_PER_THREAD, ScopeRecord};

    thread_local! {
        static RECORDS: RefCell<Vec<ScopeRecord>> = const { RefCell::new(Vec::new()) };
        /// The depth the next scope on this thread will have.
        static DEPTH: Cell<u32> = const { Cell::new(0) };
    }

    /// The guard behind `profile_scope!`. It records itself when dropped.
    #[doc(hidden)]
    pub struct Scope {
        name: &'static str,
        start: Instant,
        depth: u32,
    }

    impl Scope {
        /// Starts timing. Use `profile_scope!` instead of calling this.
        pub fn enter(name: &'static str) -> Self {
            // The thread-local is gone only while the thread is shutting
            // down; a scope entered then is simply recorded at depth 0.
            let depth = DEPTH
                .try_with(|depth| depth.replace(depth.get() + 1))
                .unwrap_or(0);
            Self {
                name,
                start: Instant::now(),
                depth,
            }
        }
    }

    impl Drop for Scope {
        fn drop(&mut self) {
            let record = ScopeRecord {
                name: self.name,
                start: self.start,
                duration: self.start.elapsed(),
                depth: self.depth,
            };
            // Both accesses fail only while the thread is shutting down,
            // when there is nobody left to read the record.
            let _ = DEPTH.try_with(|depth| depth.set(self.depth));
            let _ = RECORDS.try_with(|records| {
                if let Ok(mut records) = records.try_borrow_mut()
                    && records.len() < MAX_RECORDS_PER_THREAD
                {
                    records.push(record);
                }
            });
        }
    }

    /// Swaps the thread's records with the (empty) `out`.
    pub(super) fn swap(out: &mut Vec<ScopeRecord>) {
        // Fails only while the thread is shutting down; `out` stays empty.
        let _ = RECORDS.try_with(|records| {
            if let Ok(mut records) = records.try_borrow_mut() {
                std::mem::swap(&mut *records, out);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(feature = "profile"))]
    mod disabled {
        use super::super::*;
        use std::cell::Cell;

        #[test]
        fn without_the_feature_nothing_is_recorded_or_evaluated() {
            let evaluated = Cell::new(false);
            {
                crate::profile_scope!("outer");
                crate::profile_scope!({
                    evaluated.set(true);
                    "inner"
                });
            }
            assert!(!evaluated.get(), "profile_scope! evaluated its argument");

            let stale = ScopeRecord {
                name: "stale",
                start: Instant::now(),
                duration: Duration::ZERO,
                depth: 0,
            };
            let mut frame = vec![stale];
            take_frame(&mut frame);
            assert!(frame.is_empty());
        }
    }

    #[cfg(feature = "profile")]
    mod recording {
        use super::super::*;
        use std::thread;

        fn names(frame: &[ScopeRecord]) -> Vec<(&'static str, u32)> {
            frame
                .iter()
                .map(|record| (record.name, record.depth))
                .collect()
        }

        // Each test runs on its own thread, so each has its own records.

        #[test]
        fn scopes_last_to_the_end_of_their_block_and_nest() {
            let mut frame = Vec::new();
            {
                crate::profile_scope!("frame");
                {
                    crate::profile_scope!("update");
                    crate::profile_scope!("physics");
                    thread::sleep(Duration::from_millis(2));
                }
                crate::profile_scope!("render");
            }
            take_frame(&mut frame);
            // In the order they ended; depth counts enclosing scopes.
            assert_eq!(
                names(&frame),
                [("physics", 2), ("update", 1), ("render", 1), ("frame", 0)]
            );

            let [physics, update, render, whole] = frame[..] else {
                panic!("expected four records");
            };
            assert!(physics.duration >= Duration::from_millis(2));
            assert!(update.duration >= physics.duration);
            assert!(whole.duration >= update.duration);
            assert!(whole.start <= update.start && update.start <= physics.start);
            assert!(render.start >= update.start + update.duration);
            assert!(whole.start + whole.duration >= render.start + render.duration);
        }

        #[test]
        fn take_frame_empties_the_recorder_and_replaces_the_output() {
            let mut frame = Vec::new();
            {
                crate::profile_scope!("first");
            }
            take_frame(&mut frame);
            assert_eq!(names(&frame), [("first", 0)]);

            // The old contents of `frame` are replaced, not appended to.
            {
                crate::profile_scope!("second");
            }
            take_frame(&mut frame);
            assert_eq!(names(&frame), [("second", 0)]);

            take_frame(&mut frame);
            assert!(frame.is_empty());
        }

        #[test]
        fn steady_state_collection_reuses_both_buffers() {
            let mut frame = Vec::new();
            let mut pointers = Vec::new();
            for _ in 0..6 {
                for _ in 0..10 {
                    crate::profile_scope!("work");
                }
                take_frame(&mut frame);
                assert_eq!(frame.len(), 10);
                pointers.push((frame.as_ptr(), frame.capacity()));
            }
            // After warm-up the same two allocations alternate.
            assert_eq!(pointers[2], pointers[4]);
            assert_eq!(pointers[3], pointers[5]);
        }

        #[test]
        fn a_scope_open_across_take_frame_lands_in_the_next_frame() {
            let mut frame = Vec::new();
            {
                crate::profile_scope!("long");
                {
                    crate::profile_scope!("short");
                }
                take_frame(&mut frame);
                assert_eq!(names(&frame), [("short", 1)]);
            }
            take_frame(&mut frame);
            assert_eq!(names(&frame), [("long", 0)]);
        }

        #[test]
        fn threads_record_separately() {
            let mut frame = Vec::new();
            {
                crate::profile_scope!("main");
            }
            let other = thread::spawn(|| {
                let mut frame = Vec::new();
                {
                    crate::profile_scope!("worker");
                }
                take_frame(&mut frame);
                names(&frame)
            })
            .join()
            .expect("worker thread panicked");
            assert_eq!(other, [("worker", 0)]);
            take_frame(&mut frame);
            assert_eq!(names(&frame), [("main", 0)]);
        }

        #[test]
        fn records_beyond_the_limit_are_dropped() {
            let mut frame = Vec::new();
            for _ in 0..MAX_RECORDS_PER_THREAD + 100 {
                crate::profile_scope!("flood");
            }
            take_frame(&mut frame);
            assert_eq!(frame.len(), MAX_RECORDS_PER_THREAD);
            // Recording resumes once the records are collected.
            {
                crate::profile_scope!("after");
            }
            take_frame(&mut frame);
            assert_eq!(names(&frame), [("after", 0)]);
        }

        #[test]
        fn the_name_can_be_any_static_str_expression() {
            struct System {
                name: &'static str,
            }
            let system = System {
                name: "player_move",
            };
            let mut frame = Vec::new();
            {
                crate::profile_scope!(system.name);
            }
            take_frame(&mut frame);
            assert_eq!(names(&frame), [("player_move", 0)]);
        }
    }
}
