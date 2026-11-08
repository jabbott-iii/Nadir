// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! What the main thread and the audio thread share: the command queue, the
//! return queue and the counters.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

use nadir_core::Vec2;

use crate::{Bus, MusicData, PlayParams, SoundData, VoiceChange, VoiceId};

/// What the main thread asks of the mixer.
#[derive(Debug)]
pub(crate) enum Command {
    Play {
        id: VoiceId,
        sound: Arc<SoundData>,
        params: PlayParams,
        position: Option<Vec2>,
    },
    Stop {
        id: VoiceId,
        fade_ms: u32,
    },
    Set {
        id: VoiceId,
        change: VoiceChange,
    },
    BusVolume {
        bus: Bus,
        volume: f32,
        fade_ms: u32,
    },
    Listener(Vec2),
    PlayMusic {
        track: Arc<MusicData>,
        crossfade_ms: u32,
    },
    StopMusic {
        fade_ms: u32,
    },
}

impl Command {
    /// Whether the command only updates what a later one of its kind sets
    /// again: the first to drop when the queue fills.
    pub(crate) fn is_update(&self) -> bool {
        matches!(self, Self::Set { .. } | Self::Listener(_))
    }

    /// Whether it brings a sound or a track to the mixer.
    pub(crate) fn brings_data(&self) -> bool {
        matches!(self, Self::Play { .. } | Self::PlayMusic { .. })
    }
}

/// What the mixer is done with, for the main thread to drop: the audio
/// thread frees nothing.
#[derive(Debug)]
pub(crate) enum Retired {
    Sound(
        #[allow(dead_code, reason = "held only to be dropped on the main thread")] Arc<SoundData>,
    ),
    Music(
        #[allow(dead_code, reason = "held only to be dropped on the main thread")] Arc<MusicData>,
    ),
}

/// The state both threads hold.
#[derive(Debug)]
pub(crate) struct Shared {
    /// The commands, oldest first. The main thread locks it to push; the
    /// audio thread only tries to, and leaves the commands for its next
    /// callback if it cannot.
    pub(crate) commands: Mutex<VecDeque<Command>>,
    /// The most commands the queue holds: it never grows.
    pub(crate) capacity: usize,
    /// What the mixer is done with. The audio thread pushes only while it
    /// has room, so it never grows from there; `Audio::update` empties it.
    pub(crate) returns: Mutex<Vec<Retired>>,
    pub(crate) active_voices: AtomicU32,
    pub(crate) underruns: AtomicU64,
    pub(crate) callback_ns: AtomicU64,
    pub(crate) rate: AtomicU32,
    /// Commands dropped on the main thread because the queue was full.
    pub(crate) dropped: AtomicU64,
    /// Music frames that failed to decode as they played, which
    /// [`MusicData::new`] rules out: the audio thread counts them, and the
    /// main thread logs them.
    pub(crate) music_errors: AtomicU64,
}

impl Shared {
    pub(crate) fn new(commands: usize, returns: usize) -> Self {
        Self {
            commands: Mutex::new(VecDeque::with_capacity(commands)),
            capacity: commands,
            returns: Mutex::new(Vec::with_capacity(returns)),
            active_voices: AtomicU32::new(0),
            underruns: AtomicU64::new(0),
            callback_ns: AtomicU64::new(0),
            rate: AtomicU32::new(0),
            dropped: AtomicU64::new(0),
            music_errors: AtomicU64::new(0),
        }
    }
}

/// Locks `mutex`, waiting if it must: for the main thread. A lock that a
/// panic poisoned is taken as it is: the queues hold no invariant a panic
/// can break.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Locks `mutex` if no one holds it: for the audio thread, which never
/// waits.
pub(crate) fn try_lock<T>(mutex: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    match mutex.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}
