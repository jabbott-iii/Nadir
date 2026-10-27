//! [`TickStats`]: how long each frame's ticks took.

use std::time::Duration;

/// How many ticks the frame ran, and how long they took: a resource.
///
/// The loop writes it after the frame's ticks and before its frame stages,
/// so frame systems read their own frame's. A tick counts from its latch to
/// the end of its stages, with its commands applied. A frame with no tick
/// holds zeros.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickStats {
    /// How many ticks ran.
    pub ticks: u32,
    /// How long they took, together.
    pub time: Duration,
    /// How long the longest of them took.
    pub longest: Duration,
}
