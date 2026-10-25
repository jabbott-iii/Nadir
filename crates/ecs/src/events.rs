//! [`Events`]: messages from one system to others, by tick and by frame.

use std::any::TypeId;

use crate::World;

/// The events of type `T`: a resource that writers send to, and readers
/// read, after [`World::add_event`].
///
/// Each event goes into two buffers:
///
/// - the tick's, for fixed-stage readers: cleared as each tick starts, so a
///   reader sees what its tick sent, from writers that run before it;
/// - the frame's, for frame-stage readers: everything sent since the last
///   frame, in all its ticks, cleared after the frame stages.
///
/// The schedule clears both ([`Schedule`](crate::Schedule)). The buffers
/// are reused, so steady sending allocates nothing.
pub struct Events<T> {
    tick: Vec<T>,
    frame: Vec<T>,
}

impl<T> Events<T> {
    /// No events.
    pub const fn new() -> Self {
        Self {
            tick: Vec::new(),
            frame: Vec::new(),
        }
    }

    /// What this tick sent so far: for readers in fixed stages.
    pub fn tick_events(&self) -> &[T] {
        &self.tick
    }

    /// What was sent since the last frame: for readers in frame stages.
    pub fn frame_events(&self) -> &[T] {
        &self.frame
    }
}

impl<T: Clone> Events<T> {
    /// Sends `event` to this tick's readers and the frame's.
    pub fn send(&mut self, event: T) {
        self.tick.push(event.clone());
        self.frame.push(event);
    }
}

impl<T> Default for Events<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// How to clear one event type's buffers, without naming the type.
pub(crate) struct EventClears {
    /// The type of `Events<T>`.
    type_id: TypeId,
    tick: fn(&mut World),
    frame: fn(&mut World),
    /// The buffers' capacities, for tests.
    #[cfg(test)]
    pub(crate) capacities: fn(&World) -> [usize; 2],
}

impl World {
    /// Adds the event type `T`: the resource [`Events<T>`], which the
    /// schedule clears. An `Events<T>` already inserted is kept. Adding the
    /// type again does nothing.
    pub fn add_event<T: Clone + 'static>(&mut self) {
        let type_id = TypeId::of::<Events<T>>();
        if self.events.iter().any(|clears| clears.type_id == type_id) {
            return;
        }
        if !self.contains_resource::<Events<T>>() {
            self.insert_resource(Events::<T>::new());
        }
        self.events.push(EventClears {
            type_id,
            tick: |world| {
                if let Some(events) = world.resource_get_mut::<Events<T>>() {
                    events.tick.clear();
                }
            },
            frame: |world| {
                if let Some(events) = world.resource_get_mut::<Events<T>>() {
                    events.frame.clear();
                }
            },
            #[cfg(test)]
            capacities: |world| {
                let events = world.resource::<Events<T>>();
                [events.tick.capacity(), events.frame.capacity()]
            },
        });
    }

    /// Clears every event type's tick buffer, as a tick starts.
    pub(crate) fn clear_tick_events(&mut self) {
        for at in 0..self.events.len() {
            (self.events[at].tick)(self);
        }
    }

    /// Clears every event type's frame buffer, after the frame stages.
    pub(crate) fn clear_frame_events(&mut self) {
        for at in 0..self.events.len() {
            (self.events[at].frame)(self);
        }
    }
}
