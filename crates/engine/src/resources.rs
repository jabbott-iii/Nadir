//! [`Resources`]: the game's and the engine's shared state, by type, until
//! the entity world arrives (roadmap M4.4, ADR-0019).

use std::any::{Any, TypeId, type_name};
use std::cell::{Cell, Ref, RefCell, RefMut};

use nadir_core::FastHashMap;

/// One value of each type, which systems borrow by type: the game's own
/// state, and what the engine shares, such as the input and the frame to
/// draw.
///
/// It has the resource calls of the coming entity world, so that systems
/// keep them when it replaces this (roadmap M4.4).
///
/// ```
/// use nadir::Resources;
///
/// struct Score(u32);
///
/// let mut resources = Resources::new();
/// resources.insert_resource(Score(0));
/// resources.resource_mut::<Score>().0 += 10;
/// assert_eq!(resources.resource::<Score>().0, 10);
/// ```
///
/// A value is borrowed at run time, as a `RefCell` is: borrowing one
/// mutably while it is borrowed panics, and so does asking for a type there
/// is none of. The message names the type and the system that was running.
pub struct Resources {
    map: FastHashMap<TypeId, RefCell<Box<dyn Any>>>,
    /// The system that is running, for messages.
    system: Cell<&'static str>,
}

impl Resources {
    /// No resources.
    pub fn new() -> Self {
        Self {
            map: FastHashMap::default(),
            system: Cell::new("(none)"),
        }
    }

    /// Adds `resource`, or replaces the one of its type.
    pub fn insert_resource<R: 'static>(&mut self, resource: R) {
        self.map
            .insert(TypeId::of::<R>(), RefCell::new(Box::new(resource)));
    }

    /// Whether there is a resource of type `R`.
    pub fn contains<R: 'static>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<R>())
    }

    /// Borrows the resource of type `R`.
    ///
    /// # Panics
    ///
    /// If there is none, or it is borrowed mutably.
    pub fn resource<R: 'static>(&self) -> Ref<'_, R> {
        let cell = self.cell::<R>();
        let Ok(value) = cell.try_borrow() else {
            self.conflict::<R>("borrowed mutably elsewhere");
        };
        Ref::map(value, |value| {
            value
                .downcast_ref::<R>()
                .expect("a resource is stored under its own type")
        })
    }

    /// Borrows the resource of type `R` mutably.
    ///
    /// # Panics
    ///
    /// If there is none, or it is borrowed.
    pub fn resource_mut<R: 'static>(&self) -> RefMut<'_, R> {
        let cell = self.cell::<R>();
        let Ok(value) = cell.try_borrow_mut() else {
            self.conflict::<R>("borrowed elsewhere");
        };
        RefMut::map(value, |value| {
            value
                .downcast_mut::<R>()
                .expect("a resource is stored under its own type")
        })
    }

    /// Names the system that is running, for messages, and returns the one
    /// that was.
    pub(crate) fn set_system(&self, name: &'static str) -> &'static str {
        self.system.replace(name)
    }

    fn cell<R: 'static>(&self) -> &RefCell<Box<dyn Any>> {
        match self.map.get(&TypeId::of::<R>()) {
            Some(cell) => cell,
            None => panic!(
                "there is no resource of type `{}` (system `{}`): insert one with \
                 `App::insert_resource`",
                type_name::<R>(),
                self.system.get()
            ),
        }
    }

    fn conflict<R: 'static>(&self, how: &str) -> ! {
        panic!(
            "the resource `{}` is {how} (system `{}`): end the other borrow first",
            type_name::<R>(),
            self.system.get()
        )
    }
}

impl Default for Resources {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    #[derive(Debug, PartialEq)]
    struct Score(u32);

    /// The message of the panic `f` makes.
    fn panic_message(f: impl FnOnce()) -> String {
        let payload = catch_unwind(AssertUnwindSafe(f)).expect_err("a panic");
        payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn a_resource_is_found_by_its_type_and_replaced_by_another() {
        let mut resources = Resources::new();
        assert!(!resources.contains::<Score>());
        resources.insert_resource(Score(1));
        resources.insert_resource(7u8);
        assert!(resources.contains::<Score>());
        resources.resource_mut::<Score>().0 += 1;
        assert_eq!(*resources.resource::<Score>(), Score(2));
        resources.insert_resource(Score(5));
        assert_eq!(resources.resource::<Score>().0, 5);
        assert_eq!(*resources.resource::<u8>(), 7);
        // Two shared borrows at once are fine.
        let (a, b) = (resources.resource::<Score>(), resources.resource::<Score>());
        assert_eq!(a.0 + b.0, 10);
    }

    #[test]
    fn a_missing_resource_names_its_type_and_the_system() {
        let resources = Resources::new();
        resources.set_system("draw");
        let message = panic_message(|| drop(resources.resource::<Score>()));
        assert!(
            message.contains("Score") && message.contains("`draw`"),
            "{message}"
        );
    }

    #[test]
    fn a_conflicting_borrow_names_its_type_and_the_system() {
        let mut resources = Resources::new();
        resources.insert_resource(Score(0));
        assert_eq!(resources.set_system("move_player"), "(none)");
        let held = resources.resource_mut::<Score>();
        let message = panic_message(|| drop(resources.resource::<Score>()));
        assert!(
            message.contains("Score") && message.contains("`move_player`"),
            "{message}"
        );
        let message = panic_message(|| drop(resources.resource_mut::<Score>()));
        assert!(message.contains("borrowed elsewhere"), "{message}");
        drop(held);
        assert_eq!(resources.resource::<Score>().0, 0);
    }
}
