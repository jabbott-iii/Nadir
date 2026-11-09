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

//! [`Audio`], the main thread's side of the mixer.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nadir_assets::{AssetServer, Handle};
use nadir_core::Vec2;
use nadir_core::log::warn;

use crate::shared::{Command, Retired, Shared, lock};
use crate::{AudioStats, Bus, Music, PlayParams, Sound, SoundData, VoiceChange, VoiceId};

/// The game's handle on the mixer, for the main thread: each call queues a
/// command, which the [`Mixer`](crate::Mixer) carries out at the start of
/// its next callback, a few milliseconds later. Calls never wait on the
/// audio thread, but for the moment it takes to push or pop a command.
#[derive(Debug)]
pub struct Audio {
    pub(crate) shared: Arc<Shared>,
    next_id: AtomicU64,
    /// What [`Audio::update`] swaps for the return queue, of the same
    /// capacity, so that neither ever has to grow.
    spare: Mutex<Vec<Retired>>,
    report: Mutex<Report>,
}

/// What [`Audio::update`] last logged, and when.
#[derive(Debug, Default)]
struct Report {
    at: Option<Instant>,
    underruns: u64,
    dropped: u64,
    music_errors: u64,
}

impl Audio {
    pub(crate) fn new(shared: Arc<Shared>, returns: usize) -> Self {
        Self {
            shared,
            next_id: AtomicU64::new(1),
            spare: Mutex::new(Vec::with_capacity(returns)),
            report: Mutex::new(Report::default()),
        }
    }

    /// Plays the sound under `sound`, in `assets`, as `params` say, and
    /// names the voice: see [`Audio::play_sound`].
    ///
    /// With nothing under the handle, a sound still loading, failed with no
    /// placeholder, or unloaded, it plays nothing, counts it
    /// ([`AudioStats::unplayed`]), and names a voice that never started:
    /// changing or stopping it does nothing. A sound played once, as it
    /// starts, is lost if it has not loaded: load it before
    /// (`AssetServer::load_now`), or play it when its state is loaded. A
    /// handle names an asset of the server it came from: another server's
    /// may name another sound.
    pub fn play(&self, assets: &AssetServer, sound: Handle<Sound>, params: PlayParams) -> VoiceId {
        match assets.get(sound) {
            Some(sound) => self.play_sound(sound, params),
            None => self.unplayed(),
        }
    }

    /// Plays the sound under `sound`, in `assets`, at `position` in the
    /// world: see [`Audio::play_sound_at`], and [`Audio::play`] for a
    /// handle with nothing under it.
    pub fn play_at(
        &self,
        assets: &AssetServer,
        sound: Handle<Sound>,
        position: Vec2,
        params: PlayParams,
    ) -> VoiceId {
        match assets.get(sound) {
            Some(sound) => self.play_sound_at(sound, position, params),
            None => self.unplayed(),
        }
    }

    /// Plays `sound` as `params` say, and names the voice. If the voices
    /// are full, or the sound is playing its
    /// [`max_instances`](PlayParams::max_instances) already, it takes the
    /// place of a voice, which fades out over 5 ms: one fading out already,
    /// whatever its priority, or else the one of the lowest priority no
    /// higher than its own, the oldest of those. At a sound's
    /// `max_instances`, only that sound's voices that are not fading out
    /// are taken: a sound is the one loaded, or made with [`Sound::new`],
    /// and its clones. If there is none to take, it does not play.
    pub fn play_sound(&self, sound: &Sound, params: PlayParams) -> VoiceId {
        self.start(sound.data(), params, None)
    }

    /// Plays `sound` at `position` in the world: its pan and volume follow
    /// from where it is from the listener ([`Audio::set_listener`]).
    pub fn play_sound_at(&self, sound: &Sound, position: Vec2, params: PlayParams) -> VoiceId {
        self.start(sound.data(), params, Some(position))
    }

    /// Counts a play of a handle with nothing under it, and names a voice
    /// that never starts.
    fn unplayed(&self) -> VoiceId {
        self.shared.unplayed.fetch_add(1, Ordering::Relaxed);
        VoiceId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    fn start(&self, sound: &Arc<SoundData>, params: PlayParams, position: Option<Vec2>) -> VoiceId {
        let id = VoiceId(self.next_id.fetch_add(1, Ordering::Relaxed));
        self.send(Command::Play {
            id,
            sound: Arc::clone(sound),
            params,
            position,
        });
        id
    }

    /// Fades the voice out over `fade_ms` milliseconds, at least 2, and
    /// ends it.
    pub fn stop(&self, voice: VoiceId, fade_ms: u32) {
        self.send(Command::Stop { id: voice, fade_ms });
    }

    /// Changes a voice that is playing.
    pub fn set_voice(&self, voice: VoiceId, change: VoiceChange) {
        self.send(Command::Set { id: voice, change });
    }

    /// Moves a bus's volume to `volume` over `fade_ms` milliseconds, at
    /// least 2. Volumes multiply: a voice at 0.5 on a bus at 0.5 plays at
    /// 0.25.
    pub fn set_bus_volume(&self, bus: Bus, volume: f32, fade_ms: u32) {
        self.send(Command::BusVolume {
            bus,
            volume,
            fade_ms,
        });
    }

    /// Puts the listener at `position` in the world: where voices played at
    /// a position are heard from.
    pub fn set_listener(&self, position: Vec2) {
        self.send(Command::Listener(position));
    }

    /// Plays the track under `track`, in `assets`: see
    /// [`Audio::play_track`]. With nothing under the handle, a track still
    /// loading, failed with no placeholder, or unloaded, the music goes on
    /// as it is, and the play is counted ([`AudioStats::unplayed`]).
    pub fn play_music(&self, assets: &AssetServer, track: Handle<Music>, crossfade_ms: u32) {
        match assets.get(track) {
            Some(track) => self.play_track(track, crossfade_ms),
            None => {
                self.unplayed();
            }
        }
    }

    /// Plays `track` on the music bus, looped, crossfading from the track
    /// playing over `crossfade_ms` milliseconds, at least 2. The track
    /// playing already, or the same file's bytes loaded again, goes on as
    /// it is.
    pub fn play_track(&self, track: &Music, crossfade_ms: u32) {
        self.send(Command::PlayMusic {
            track: Arc::clone(track.data()),
            crossfade_ms,
        });
    }

    /// Fades the music out over `fade_ms` milliseconds, at least 2.
    pub fn stop_music(&self, fade_ms: u32) {
        self.send(Command::StopMusic { fade_ms });
    }

    /// Drops the sounds and tracks the mixer is done with, here on the main
    /// thread, and logs what went wrong since the last report, once a
    /// second at most: underruns, dropped commands, music that failed to
    /// decode. Call it once a frame: until it runs, what the mixer is done
    /// with waits, and a mixer that holds too much takes no new sound.
    pub fn update(&self) {
        if Arc::strong_count(&self.shared) == 1 {
            // The mixer is gone: what it never took is dropped here.
            drop(std::mem::take(&mut *lock(&self.shared.commands)));
        }
        let mut spare = lock(&self.spare);
        std::mem::swap(&mut *spare, &mut *lock(&self.shared.returns));
        spare.clear();
        drop(spare);
        let shared = &self.shared;
        let counts = [
            shared.underruns.load(Ordering::Relaxed),
            shared.dropped.load(Ordering::Relaxed),
            shared.music_errors.load(Ordering::Relaxed),
        ];
        let mut report = lock(&self.report);
        let logged = [report.underruns, report.dropped, report.music_errors];
        let due = report
            .at
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(1));
        if counts != logged && due {
            warn!(
                "audio since the last report: {} underruns, {} commands dropped (the queue was full), \
                 {} music frames that would not decode",
                counts[0] - logged[0],
                counts[1] - logged[1],
                counts[2] - logged[2],
            );
            [report.underruns, report.dropped, report.music_errors] = counts;
            report.at = Some(Instant::now());
        }
    }

    /// What the mixer has been doing.
    pub fn stats(&self) -> AudioStats {
        let shared = &self.shared;
        AudioStats {
            active_voices: shared.active_voices.load(Ordering::Relaxed),
            underruns: shared.underruns.load(Ordering::Relaxed),
            callback_time: Duration::from_nanos(shared.callback_ns.load(Ordering::Relaxed)),
            rate: shared.rate.load(Ordering::Relaxed),
            dropped: shared.dropped.load(Ordering::Relaxed),
            unplayed: shared.unplayed.load(Ordering::Relaxed),
        }
    }

    /// Queues `command`, unless the queue is full: then it is dropped, here,
    /// and counted. A change to a voice or to the listener is dropped once
    /// the queue is half full, as the next frame sends it again, so that
    /// the rest keep their room. With the mixer gone, nothing is queued.
    fn send(&self, command: Command) {
        let shared = &*self.shared;
        if Arc::strong_count(&self.shared) == 1 {
            return;
        }
        let limit = if command.is_update() {
            shared.capacity / 2
        } else {
            shared.capacity
        };
        let mut queue = lock(&shared.commands);
        if queue.len() < limit {
            queue.push_back(command);
        } else {
            drop(queue);
            shared.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}
