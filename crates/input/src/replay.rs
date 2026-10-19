//! Replays: the state of every action, tick after tick.
//!
//! Format version 1, little endian:
//!
//! - The header:
//!   - the magic bytes `NRPL`, and the version as a `u16`
//!   - the number of actions as a `u16`
//!   - a `u64` FNV-1a hash of, for each action in index order, its name,
//!     a 0 byte and its kind's byte
//!   - one byte per action for its kind: 0 for a button, 1 for `Axis1`, 2
//!     for `Axis2`
//! - Then one record per tick, to the end of the file:
//!   - three bits per action, from the least significant bit of the first
//!     byte on: held, just pressed, just released. The unused bits of the
//!     last byte are 0.
//!   - an `i16` per `Axis1` action, and two (x, y) per `Axis2` action, in
//!     index order, in steps of 1/32767. A button's value is 1 while it is
//!     held, so it is not stored.
//!
//! There is no tick count: a replay cut short at the end of a tick plays
//! the ticks before the cut.

use std::fmt;
use std::marker::PhantomData;

use nadir_core::hash::fnv1a64;

use crate::state::{HELD, JUST_PRESSED, JUST_RELEASED, Latched, ONE, longer_than_one};
use crate::{ActionKind, ActionSet, ActionState, InputError, ReplayErrorKind, action};

const MAGIC: [u8; 4] = *b"NRPL";
const VERSION: u16 = 1;
/// The magic, the version, the action count and the hash.
const FIXED_HEADER: usize = 16;
/// The bits each action takes in a record.
const BITS: usize = 3;

/// Where things are in the records of the set `A`.
#[derive(Clone, Copy, Debug)]
struct Layout {
    flag_bytes: usize,
    record: usize,
}

impl Layout {
    fn of<A: ActionSet>() -> Self {
        let flag_bytes = (A::ALL.len() * BITS).div_ceil(8);
        let axes: usize = A::ALL.iter().map(|action| axis_bytes(action.kind())).sum();
        Self {
            flag_bytes,
            record: flag_bytes + axes,
        }
    }
}

/// The bytes an action's value takes in a record.
fn axis_bytes(kind: ActionKind) -> usize {
    match kind {
        ActionKind::Button => 0,
        ActionKind::Axis1 => 2,
        ActionKind::Axis2 => 4,
    }
}

/// The hash of the names and kinds of the set.
fn set_hash<A: ActionSet>() -> u64 {
    let mut bytes = Vec::new();
    for action in A::ALL {
        bytes.extend_from_slice(action.name().as_bytes());
        bytes.extend_from_slice(&[0, action.kind().byte()]);
    }
    fnv1a64(&bytes)
}

/// Writes the state of every tick of a run, for [`InputPlayer`] to play
/// back.
pub struct InputRecorder<A> {
    bytes: Vec<u8>,
    layout: Layout,
    set: PhantomData<A>,
}

impl<A: ActionSet> InputRecorder<A> {
    /// A recording of no ticks yet.
    ///
    /// # Errors
    ///
    /// Fails with [`InputError::ActionSet`] if the set breaks a rule of
    /// [`ActionSet`].
    pub fn new() -> Result<Self, InputError> {
        action::validate::<A>()?;
        let mut bytes = Vec::with_capacity(FIXED_HEADER + A::ALL.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        // `validate` checked that the count fits.
        bytes.extend_from_slice(&(A::ALL.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&set_hash::<A>().to_le_bytes());
        bytes.extend(A::ALL.iter().map(|action| action.kind().byte()));
        Ok(Self {
            bytes,
            layout: Layout::of::<A>(),
            set: PhantomData,
        })
    }

    /// Adds the state of the next tick.
    pub fn record(&mut self, state: &ActionState<A>) {
        let start = self.bytes.len();
        self.bytes.resize(start + self.layout.flag_bytes, 0);
        for (i, &action) in A::ALL.iter().enumerate() {
            let flags = state.get(action).flags;
            for (bit, flag) in [HELD, JUST_PRESSED, JUST_RELEASED].into_iter().enumerate() {
                if flags & flag != 0 {
                    let n = i * BITS + bit;
                    self.bytes[start + n / 8] |= 1 << (n % 8);
                }
            }
        }
        for &action in A::ALL {
            let [x, y] = state.get(action).axes;
            match action.kind() {
                ActionKind::Button => {}
                ActionKind::Axis1 => self.bytes.extend_from_slice(&x.to_le_bytes()),
                ActionKind::Axis2 => {
                    self.bytes.extend_from_slice(&x.to_le_bytes());
                    self.bytes.extend_from_slice(&y.to_le_bytes());
                }
            }
        }
    }

    /// The replay so far.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The replay.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// Plays back a replay that [`InputRecorder`] wrote, in place of live
/// latching.
///
/// It reads strictly. These are errors, each with the byte offset of the
/// problem: a header or tick cut short; another magic, version, action
/// count, kind or names; flag bits that no action uses; flags that
/// [`Input::latch`](crate::Input::latch) never makes (just pressed but not
/// held, or held and just released but not just pressed); and an `Axis1`
/// below 0, an `i16::MIN` or a direction longer than 1. Values are not
/// checked against the flags, since analog input (roadmap M8.4) makes
/// values of any size.
pub struct InputPlayer<'a, A> {
    bytes: &'a [u8],
    at: usize,
    layout: Layout,
    set: PhantomData<A>,
}

impl<'a, A: ActionSet> InputPlayer<'a, A> {
    /// Reads the header of a replay, and checks that it was recorded with
    /// the set `A`.
    ///
    /// # Errors
    ///
    /// Fails with [`InputError::ActionSet`] if the set breaks a rule of
    /// [`ActionSet`], or with [`InputError::Replay`] if the header is not
    /// one of a replay of version 1 recorded with this set.
    pub fn new(bytes: &'a [u8]) -> Result<Self, InputError> {
        action::validate::<A>()?;
        let error = |offset, kind| Err(replay_error(offset, kind));

        let start = &bytes[..bytes.len().min(MAGIC.len())];
        if start != &MAGIC[..start.len()] {
            return error(0, ReplayErrorKind::NotAReplay);
        }
        let mut at = 0;
        take::<4>(bytes, &mut at, "the magic bytes")?;
        let version = u16::from_le_bytes(take(bytes, &mut at, "the format version")?);
        if version != VERSION {
            return error(4, ReplayErrorKind::UnknownVersion { found: version });
        }
        let count = u16::from_le_bytes(take(bytes, &mut at, "the action count")?);
        if usize::from(count) != A::ALL.len() {
            return error(
                6,
                ReplayErrorKind::ActionCount {
                    expected: A::ALL.len(),
                    found: count,
                },
            );
        }
        let hash = u64::from_le_bytes(take(bytes, &mut at, "the action set's hash")?);
        for &action in A::ALL {
            let offset = at;
            let [kind] = take(bytes, &mut at, "the action kinds")?;
            if kind != action.kind().byte() {
                return error(
                    offset,
                    ReplayErrorKind::ActionKind {
                        action: action.name(),
                        expected: action.kind(),
                        found: kind,
                    },
                );
            }
        }
        if hash != set_hash::<A>() {
            return error(8, ReplayErrorKind::OtherActionSet);
        }
        Ok(Self {
            bytes,
            at,
            layout: Layout::of::<A>(),
            set: PhantomData,
        })
    }

    /// Fills `state` with the next tick's, and returns whether there was
    /// one: `Ok(false)` at the end of the replay.
    ///
    /// # Errors
    ///
    /// Fails with [`InputError::Replay`] if the tick is cut short or breaks
    /// a rule listed on [`InputPlayer`]. `state` is then left as it was,
    /// and the player stays at that tick.
    pub fn next_tick(&mut self, state: &mut ActionState<A>) -> Result<bool, InputError> {
        if self.at == self.bytes.len() {
            return Ok(false);
        }
        let Some(record) = self.bytes.get(self.at..self.at + self.layout.record) else {
            return Err(replay_error(
                self.bytes.len(),
                ReplayErrorKind::UnexpectedEnd {
                    expected: "the rest of a tick",
                },
            ));
        };
        self.check(record)?;
        let mut values = &record[self.layout.flag_bytes..];
        for (i, &action) in A::ALL.iter().enumerate() {
            let flags = flags_of(record, i);
            let mut next = || {
                let (value, rest) = values.split_at(2);
                values = rest;
                i16::from_le_bytes([value[0], value[1]])
            };
            let held = if flags & HELD != 0 { ONE } else { 0 };
            let axes = match action.kind() {
                ActionKind::Button => [held, 0],
                ActionKind::Axis1 => [next(), 0],
                ActionKind::Axis2 => [next(), next()],
            };
            state.set(action, Latched { flags, axes });
        }
        self.at += self.layout.record;
        Ok(true)
    }

    /// Checks a whole record, before anything is filled from it.
    fn check(&self, record: &[u8]) -> Result<(), InputError> {
        let error =
            |offset_in_record: usize, kind| Err(replay_error(self.at + offset_in_record, kind));
        for (i, &action) in A::ALL.iter().enumerate() {
            let flags = flags_of(record, i);
            let byte = i * BITS / 8;
            if flags & JUST_PRESSED != 0 && flags & HELD == 0 {
                return error(
                    byte,
                    ReplayErrorKind::PressedButNotHeld {
                        action: action.name(),
                    },
                );
            }
            if flags == HELD | JUST_RELEASED {
                return error(
                    byte,
                    ReplayErrorKind::ReleasedButHeld {
                        action: action.name(),
                    },
                );
            }
        }
        let used = A::ALL.len() * BITS;
        if !used.is_multiple_of(8) && record[self.layout.flag_bytes - 1] >> (used % 8) != 0 {
            return error(self.layout.flag_bytes - 1, ReplayErrorKind::UnusedBitsSet);
        }

        let mut at = self.layout.flag_bytes;
        let mut next = || {
            let offset = at;
            at += 2;
            (
                offset,
                i16::from_le_bytes([record[offset], record[offset + 1]]),
            )
        };
        for &action in A::ALL {
            let out_of_range = |offset, found| {
                error(
                    offset,
                    ReplayErrorKind::AxisOutOfRange {
                        action: action.name(),
                        found,
                    },
                )
            };
            match action.kind() {
                ActionKind::Button => {}
                ActionKind::Axis1 => {
                    let (offset, value) = next();
                    if value < 0 {
                        return out_of_range(offset, value);
                    }
                }
                ActionKind::Axis2 => {
                    let (x_offset, x) = next();
                    let (y_offset, y) = next();
                    for (offset, value) in [(x_offset, x), (y_offset, y)] {
                        if value < -ONE {
                            return out_of_range(offset, value);
                        }
                    }
                    if longer_than_one(x, y) {
                        return error(
                            x_offset,
                            ReplayErrorKind::DirectionTooLong {
                                action: action.name(),
                            },
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

impl<A> fmt::Debug for InputRecorder<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputRecorder")
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

impl<A> fmt::Debug for InputPlayer<'_, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputPlayer")
            .field("at", &self.at)
            .field("len", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

/// The three flags of action `i` in a record.
fn flags_of(record: &[u8], i: usize) -> u8 {
    let mut flags = 0;
    for (bit, flag) in [HELD, JUST_PRESSED, JUST_RELEASED].into_iter().enumerate() {
        let n = i * BITS + bit;
        if record[n / 8] & (1 << (n % 8)) != 0 {
            flags |= flag;
        }
    }
    flags
}

/// The next `N` bytes, or an error at the end of the replay.
fn take<const N: usize>(
    bytes: &[u8],
    at: &mut usize,
    expected: &'static str,
) -> Result<[u8; N], InputError> {
    let Some(chunk) = bytes.get(*at..).and_then(<[u8]>::first_chunk::<N>) else {
        return Err(replay_error(
            bytes.len(),
            ReplayErrorKind::UnexpectedEnd { expected },
        ));
    };
    *at += N;
    Ok(*chunk)
}

fn replay_error(offset: usize, kind: ReplayErrorKind) -> InputError {
    InputError::Replay { offset, kind }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Act;

    const HEADER: usize = FIXED_HEADER + 4;

    /// Move held and just pressed, up and right; Jump tapped; Pause up;
    /// Throttle held at 1.
    fn state() -> ActionState<Act> {
        let mut state = ActionState::new();
        let latched = |flags, axes| Latched { flags, axes };
        state.set(Act::Move, latched(HELD | JUST_PRESSED, [23169, -23169]));
        state.set(
            Act::Jump,
            latched(HELD | JUST_PRESSED | JUST_RELEASED, [ONE, 0]),
        );
        state.set(Act::Throttle, latched(HELD, [ONE, 0]));
        state
    }

    /// The record of [`state`].
    const RECORD: [u8; 8] = [
        // Move: bits 0 and 1; Jump: bits 3, 4 and 5; Throttle: bit 9.
        0b0011_1011,
        0b0000_0010,
        // 23169 and -23169.
        0x81,
        0x5a,
        0x7f,
        0xa5,
        // 32767.
        0xff,
        0x7f,
    ];

    fn replay() -> Vec<u8> {
        let mut recorder = InputRecorder::<Act>::new().expect("a valid set");
        recorder.record(&state());
        recorder.into_bytes()
    }

    /// The replay with the byte at `at` replaced.
    fn with(at: usize, byte: u8) -> Vec<u8> {
        let mut bytes = replay();
        bytes[at] = byte;
        bytes
    }

    fn header_error(bytes: &[u8]) -> Option<(usize, ReplayErrorKind)> {
        match InputPlayer::<Act>::new(bytes) {
            Err(InputError::Replay { offset, kind }) => Some((offset, kind)),
            _ => None,
        }
    }

    fn tick_error(bytes: &[u8]) -> Option<(usize, ReplayErrorKind)> {
        let mut player = InputPlayer::<Act>::new(bytes).expect("a good header");
        let mut state = ActionState::new();
        let error = match player.next_tick(&mut state) {
            Err(InputError::Replay { offset, kind }) => Some((offset, kind)),
            _ => None,
        };
        assert_eq!(state, ActionState::new(), "nothing filled from a bad tick");
        error
    }

    #[test]
    fn a_replay_is_written_byte_for_byte() {
        let bytes = replay();
        let mut header = b"NRPL\x01\x00\x04\x00".to_vec();
        header.extend_from_slice(&set_hash::<Act>().to_le_bytes());
        header.extend_from_slice(&[2, 0, 0, 1]);
        assert_eq!(bytes[..HEADER], header);
        assert_eq!(bytes[HEADER..], RECORD);
    }

    #[test]
    fn a_replay_plays_back_what_was_recorded() {
        let bytes = replay();
        let mut player = InputPlayer::<Act>::new(&bytes).expect("a good header");
        let mut state = ActionState::new();
        assert_eq!(player.next_tick(&mut state), Ok(true));
        assert_eq!(state, self::state());
        assert_eq!(state.value(Act::Jump), 1.0, "derived from held");
        assert_eq!(player.next_tick(&mut state), Ok(false));
        assert_eq!(player.next_tick(&mut state), Ok(false));
    }

    #[test]
    fn a_header_from_another_file_or_set_is_refused() {
        use ReplayErrorKind as K;
        assert_eq!(header_error(b"PNG\x89"), Some((0, K::NotAReplay)));
        assert_eq!(header_error(b"NRX"), Some((0, K::NotAReplay)));
        let expected = |expected| K::UnexpectedEnd { expected };
        assert_eq!(header_error(b""), Some((0, expected("the magic bytes"))));
        assert_eq!(header_error(b"NR"), Some((2, expected("the magic bytes"))));
        assert_eq!(
            header_error(b"NRPL\x01"),
            Some((5, expected("the format version")))
        );
        assert_eq!(
            header_error(&with(4, 2)),
            Some((4, K::UnknownVersion { found: 2 }))
        );
        assert_eq!(
            header_error(&with(6, 3)),
            Some((
                6,
                K::ActionCount {
                    expected: 4,
                    found: 3
                }
            ))
        );
        assert_eq!(
            header_error(&with(FIXED_HEADER + 3, 2)),
            Some((
                FIXED_HEADER + 3,
                K::ActionKind {
                    action: "throttle",
                    expected: ActionKind::Axis1,
                    found: 2
                }
            ))
        );
        // Same kinds, other names.
        assert_eq!(header_error(&with(8, 0)), Some((8, K::OtherActionSet)));
        assert_eq!(
            header_error(&replay()[..HEADER - 1]),
            Some((HEADER - 1, expected("the action kinds")))
        );
    }

    #[test]
    fn a_tick_that_latching_cannot_make_is_refused() {
        use ReplayErrorKind as K;
        let record = HEADER;
        assert_eq!(
            tick_error(&replay()[..record + 7]),
            Some((
                record + 7,
                K::UnexpectedEnd {
                    expected: "the rest of a tick"
                }
            ))
        );
        // Bits 12 to 15 belong to no action.
        assert_eq!(
            tick_error(&with(record + 1, 0b0001_0010)),
            Some((record + 1, K::UnusedBitsSet))
        );
        // Jump just pressed, not held: bit 4 without bit 3.
        assert_eq!(
            tick_error(&with(record, 0b0001_0011)),
            Some((record, K::PressedButNotHeld { action: "jump" }))
        );
        // Pause held and just released: bits 6 and 8.
        let mut bytes = with(record, 0b0111_1011);
        bytes[record + 1] = 0b0000_0011;
        assert_eq!(
            tick_error(&bytes),
            Some((record, K::ReleasedButHeld { action: "pause" }))
        );
        // Throttle below 0.
        let mut bytes = with(record + 6, 0xff);
        bytes[record + 7] = 0xff;
        assert_eq!(
            tick_error(&bytes),
            Some((
                record + 6,
                K::AxisOutOfRange {
                    action: "throttle",
                    found: -1
                }
            ))
        );
        // Move's y at -32768.
        let mut bytes = with(record + 4, 0x00);
        bytes[record + 5] = 0x80;
        assert_eq!(
            tick_error(&bytes),
            Some((
                record + 4,
                K::AxisOutOfRange {
                    action: "move",
                    found: i16::MIN
                }
            ))
        );
        // Move at (32767, 1).
        let mut bytes = replay();
        bytes[record + 2..record + 6].copy_from_slice(&[0xff, 0x7f, 0x01, 0x00]);
        assert_eq!(
            tick_error(&bytes),
            Some((record + 2, K::DirectionTooLong { action: "move" }))
        );
    }

    #[test]
    fn a_player_stays_at_a_bad_tick() {
        let bytes = with(HEADER, 0b0001_0011);
        let mut player = InputPlayer::<Act>::new(&bytes).expect("a good header");
        let mut state = ActionState::new();
        let first = player.next_tick(&mut state);
        assert!(first.is_err());
        assert_eq!(player.next_tick(&mut state), first);
    }
}
