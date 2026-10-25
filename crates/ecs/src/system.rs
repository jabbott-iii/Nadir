//! The name of the system that is running, for panic messages.

use std::cell::Cell;

/// What [`current_system`] says outside any system.
const NONE: &str = "(none)";

thread_local! {
    /// The running system's name. A constant start and no destructor, so
    /// reading it never fails or allocates.
    static CURRENT: Cell<&'static str> = const { Cell::new(NONE) };
}

/// The name of the system running on this thread, or `(none)`.
pub(crate) fn current_system() -> &'static str {
    CURRENT.with(Cell::get)
}

/// Names the running system until it is dropped. The name before it comes
/// back then, also when the system panics.
pub(crate) struct SystemScope {
    previous: &'static str,
}

impl SystemScope {
    pub(crate) fn enter(name: &'static str) -> Self {
        Self {
            previous: CURRENT.with(|current| current.replace(name)),
        }
    }
}

impl Drop for SystemScope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.set(self.previous));
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    #[test]
    fn a_scope_names_the_system_until_it_ends_even_in_a_panic() {
        assert_eq!(current_system(), NONE);
        {
            let _outer = SystemScope::enter("outer");
            assert_eq!(current_system(), "outer");
            let caught = catch_unwind(AssertUnwindSafe(|| {
                let _inner = SystemScope::enter("inner");
                assert_eq!(current_system(), "inner");
                panic!("the system fails");
            }));
            assert!(caught.is_err());
            assert_eq!(current_system(), "outer");
        }
        assert_eq!(current_system(), NONE);
    }
}
