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

//! [`Mixer`], the audio thread's side: the voices, the music, the buses and
//! the master.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use nadir_core::Vec2;

use crate::dsp::{Ramp, limit};
use crate::music::{BUFFER_LEN, Track};
use crate::shared::{Command, Retired, Shared, lock, try_lock};
use crate::voice::{self, Rendered, Space, Voice};
use crate::{AudioConfig, Bus, MusicData, PlayParams, SoundData, VoiceId};

/// The most frames mixed at once: a larger callback is mixed in parts of
/// this many.
pub(crate) const CHUNK: usize = 512;

/// The shortest fade, in milliseconds: every start and stop, and every
/// change of a bus's volume, takes at least this long.
pub(crate) const MIN_FADE_MS: u32 = 2;

/// How long a stolen voice takes to fade out, in milliseconds.
pub(crate) const STEAL_MS: u32 = 5;

/// How long a voice's gains take to reach a change of its volume, pan or
/// position, in milliseconds: about a tick, so that a sound moving with
/// its entity changes smoothly from one frame's position to the next.
pub(crate) const SMOOTH_MS: u32 = 16;

/// The samples a voice or a track takes to reach 0 when it is stolen or
/// handed off: `most`, or what is left of a fade out already shorter.
fn fade_out(fade: &Ramp, most: u32) -> u32 {
    if fade.target() == 0.0 {
        fade.remaining().clamp(1, most)
    } else {
        most
    }
}

/// Fades `fade` out over `samples`, or sooner if it is fading out already
/// and would be silent sooner.
fn stop_fade(fade: &mut Ramp, samples: u32) {
    if fade.target() != 0.0 || fade.remaining() > samples {
        fade.to(0.0, samples);
    }
}

/// What a voice slot holds.
#[derive(Debug)]
enum Slot {
    Free,
    /// A voice, which may be fading out after a stop.
    Live(Voice),
    /// A stolen voice fading out, and the voice that took its place, which
    /// plays from the start: the slot is the new voice's once the old one
    /// is silent.
    Handoff {
        old: Voice,
        new: Voice,
    },
}

/// What a music slot holds.
#[derive(Debug)]
enum MusicState {
    Free,
    /// A track, which may be fading out.
    Live(Track),
    /// A track fading out over 2 ms, and the one that starts in its place
    /// once it is silent, fading in over `fade_in` samples: the two would
    /// need the one buffer at once.
    Handoff {
        old: Track,
        next: Arc<MusicData>,
        fade_in: u32,
    },
}

/// A music voice: its state and the buffer it decodes into.
#[derive(Debug)]
struct MusicSlot {
    state: MusicState,
    buffer: Box<[i16]>,
}

impl MusicSlot {
    /// The track sounding in it, if any: fading in, playing or fading out.
    fn sounding(&self) -> Option<&Track> {
        match &self.state {
            MusicState::Free => None,
            MusicState::Live(track) | MusicState::Handoff { old: track, .. } => Some(track),
        }
    }
}

/// The audio thread's side of the mixer. The platform's audio thread
/// calls [`Mixer::render`] for each buffer the device wants; it never
/// blocks, allocates, frees or logs.
///
/// Dropping it drops the sounds it holds on the thread that drops it.
#[derive(Debug)]
pub struct Mixer {
    shared: Arc<Shared>,
    slots: Box<[Slot]>,
    music: [MusicSlot; 2],
    /// The music slot whose track is playing, not fading out.
    active_music: Option<usize>,
    buses: [Ramp; 4],
    space: Space,
    /// What the mixer is done with, for the main thread to drop. Its
    /// capacity is fixed: the sounds and tracks held and those retired
    /// together never pass it, so it never grows.
    retired: Vec<Retired>,
    /// Four buses and the master, each [`CHUNK`] stereo frames.
    scratch: Box<[f32]>,
    /// The next voice's place in the order voices start in.
    order: u64,
    /// The rate of the callback being mixed.
    rate: u32,
}

impl Mixer {
    pub(crate) fn new(shared: Arc<Shared>, config: &AudioConfig, returns: usize) -> Self {
        // A first lock may allocate where a mutex is made lazily: do it
        // here, not on the audio thread.
        drop(lock(&shared.commands));
        drop(lock(&shared.returns));
        let music = || MusicSlot {
            state: MusicState::Free,
            buffer: vec![0; BUFFER_LEN].into_boxed_slice(),
        };
        Self {
            shared,
            slots: (0..config.voices).map(|_| Slot::Free).collect(),
            music: [music(), music()],
            active_music: None,
            buses: [Ramp::at(1.0); 4],
            space: Space {
                listener: Vec2::ZERO,
                pan_width: config.pan_width,
                min_dist: config.min_dist,
                max_dist: config.max_dist,
            },
            retired: Vec::with_capacity(returns),
            scratch: vec![0.0; CHUNK * 2 * 5].into_boxed_slice(),
            order: 0,
            rate: 0,
        }
    }

    /// Fills `out`, interleaved frames of `channels` channels at `rate`
    /// frames a second, with the next part of the mix, after carrying out
    /// the commands the main thread has queued (unless it holds the queue
    /// just then: they wait for the next call). The mix is stereo: one
    /// channel takes the mean of the two, and a third and more are silent.
    /// The samples past the last whole frame, a channel count of 0 and a
    /// rate of 0 are silent and advance nothing.
    pub fn render(&mut self, out: &mut [f32], channels: usize, rate: u32) {
        let started = Instant::now();
        if channels == 0 || rate == 0 {
            out.fill(0.0);
            return;
        }
        self.rate = rate;
        self.flush();
        self.run_commands();
        let frames = out.len() / channels;
        let mut done = 0;
        while done < frames {
            let len = CHUNK.min(frames - done);
            self.mix(len);
            let master = &self.scratch[CHUNK * 8..][..len * 2];
            let out = &mut out[done * channels..(done + len) * channels];
            for (frame, mixed) in out
                .chunks_exact_mut(channels)
                .zip(master.as_chunks::<2>().0)
            {
                if channels == 1 {
                    frame[0] = 0.5 * (mixed[0] + mixed[1]);
                } else {
                    frame[0] = mixed[0];
                    frame[1] = mixed[1];
                    frame[2..].fill(0.0);
                }
            }
            done += len;
        }
        out[frames * channels..].fill(0.0);
        self.flush();
        self.record(started);
    }

    /// The room it has for what it is done with, which never changes.
    #[cfg(test)]
    pub(crate) fn retired_capacity(&self) -> usize {
        self.retired.capacity()
    }

    /// Counts an underrun the device reported: for the platform's backend.
    pub fn count_underrun(&self) {
        self.shared.underruns.fetch_add(1, Ordering::Relaxed);
    }

    /// The samples in `ms` milliseconds at the current rate, rounded up, so
    /// that a fade is never shorter than it says: at least 1.
    fn samples(&self, ms: u32) -> u32 {
        let samples = (u64::from(self.rate) * u64::from(ms)).div_ceil(1000);
        samples.clamp(1, u64::from(u32::MAX)) as u32
    }

    /// A fade's samples: of `ms` milliseconds, at least [`MIN_FADE_MS`].
    fn fade(&self, ms: u32) -> u32 {
        self.samples(ms.max(MIN_FADE_MS))
    }

    /// The sounds and tracks the slots hold.
    fn held(&self) -> usize {
        let sounds: usize = self
            .slots
            .iter()
            .map(|slot| match slot {
                Slot::Free => 0,
                Slot::Live(_) => 1,
                Slot::Handoff { .. } => 2,
            })
            .sum();
        let music: usize = self
            .music
            .iter()
            .map(|slot| match slot.state {
                MusicState::Free => 0,
                MusicState::Live(_) => 1,
                MusicState::Handoff { .. } => 2,
            })
            .sum();
        sounds + music
    }

    fn retire(&mut self, retired: Retired) {
        debug_assert!(self.retired.len() < self.retired.capacity());
        self.retired.push(retired);
    }

    /// Hands what the mixer is done with to the main thread, as much as the
    /// return queue has room for, unless the main thread holds it.
    fn flush(&mut self) {
        if self.retired.is_empty() {
            return;
        }
        if let Some(mut returns) = try_lock(&self.shared.returns) {
            let room = returns.capacity() - returns.len();
            let start = self.retired.len() - room.min(self.retired.len());
            returns.extend(self.retired.drain(start..));
        }
    }

    /// Carries out the queued commands, unless the main thread holds the
    /// queue. A command that brings a sound or a track waits, with those
    /// after it, while the sounds held and retired would fill
    /// [`Mixer::retired`], so that whatever the mixer gives back has room.
    /// The queue is locked only to take each command, so the main thread
    /// never waits for one to be carried out; at most a queue's worth are
    /// taken in a callback.
    fn run_commands(&mut self) {
        let mut total = self.held() + self.retired.len();
        let room = self.retired.capacity();
        for _ in 0..self.shared.capacity {
            let command = {
                let Some(mut queue) = try_lock(&self.shared.commands) else {
                    return;
                };
                match queue.front() {
                    Some(command) if command.brings_data() && total >= room => None,
                    Some(_) => queue.pop_front(),
                    None => None,
                }
            };
            let Some(command) = command else {
                return;
            };
            total += usize::from(command.brings_data());
            self.run(command);
        }
    }

    fn run(&mut self, command: Command) {
        match command {
            Command::Play {
                id,
                sound,
                params,
                position,
            } => self.play(id, sound, &params, position),
            Command::Stop { id, fade_ms } => {
                let fade = self.fade(fade_ms);
                for slot in self.slots.iter_mut() {
                    match slot {
                        Slot::Free => {}
                        Slot::Live(voice) => {
                            if voice.id == id {
                                stop_fade(&mut voice.fade, fade);
                            }
                        }
                        Slot::Handoff { old, new } => {
                            for voice in [old, new] {
                                if voice.id == id {
                                    stop_fade(&mut voice.fade, fade);
                                }
                            }
                        }
                    }
                }
            }
            Command::Set { id, change } => {
                for slot in self.slots.iter_mut() {
                    match slot {
                        Slot::Live(voice) | Slot::Handoff { new: voice, .. } if voice.id == id => {
                            voice.change(change);
                        }
                        _ => {}
                    }
                }
            }
            Command::BusVolume {
                bus,
                volume,
                fade_ms,
            } => {
                let fade = self.fade(fade_ms);
                self.buses[bus.index()].to(voice::volume(volume), fade);
            }
            Command::Listener(position) => {
                if position.x.is_finite() && position.y.is_finite() {
                    self.space.listener = position;
                }
            }
            Command::PlayMusic {
                track,
                crossfade_ms,
            } => self.play_music(track, crossfade_ms),
            Command::StopMusic { fade_ms } => {
                let fade = self.fade(fade_ms);
                if let Some(active) = self.active_music.take() {
                    self.fade_out_music(active, fade);
                }
                // A track still fading out from a crossfade is silent as
                // soon as this one, if it was to take longer.
                for slot in &mut self.music {
                    if let MusicState::Live(track) = &mut slot.state {
                        stop_fade(&mut track.fade, fade);
                    }
                }
            }
        }
    }

    /// Starts a voice, stealing one if it must: see [`Audio::play`](crate::Audio::play).
    fn play(
        &mut self,
        id: VoiceId,
        sound: Arc<SoundData>,
        params: &PlayParams,
        position: Option<Vec2>,
    ) {
        let instance =
            |voice: &Voice| Arc::ptr_eq(&voice.sound, &sound) && voice.fade.target() != 0.0;
        let instances = self
            .slots
            .iter()
            .filter(|slot| match slot {
                Slot::Live(voice) | Slot::Handoff { new: voice, .. } => instance(voice),
                Slot::Free => false,
            })
            .count();
        let index = if instances >= usize::from(params.max_instances.max(1)) {
            self.victim(instance, params.priority)
        } else if let Some(free) = self
            .slots
            .iter()
            .position(|slot| matches!(slot, Slot::Free))
        {
            Some(free)
        } else {
            self.victim(|_| true, params.priority)
        };
        let Some(index) = index else {
            self.retire(Retired::Sound(sound));
            return;
        };
        let new = Voice::new(
            id,
            sound,
            params,
            position,
            self.order,
            self.samples(MIN_FADE_MS),
        );
        self.order += 1;
        let steal = self.samples(STEAL_MS);
        self.slots[index] = match std::mem::replace(&mut self.slots[index], Slot::Free) {
            Slot::Free => Slot::Live(new),
            Slot::Live(mut old) => {
                old.fade.to(0.0, fade_out(&old.fade, steal));
                Slot::Handoff { old, new }
            }
            handoff @ Slot::Handoff { .. } => {
                // `victim` passes over these: never reached.
                self.retire(Retired::Sound(new.sound));
                handoff
            }
        };
    }

    /// The slot of the voice to steal for one of `priority`, among those
    /// `eligible`: one fading out already, whatever its priority, or else
    /// the one of the lowest priority, no higher than `priority`, and of
    /// those the oldest. A slot in a handoff holds two voices already, and
    /// is passed over for the 5 ms it lasts.
    fn victim(&self, eligible: impl Fn(&Voice) -> bool, priority: u8) -> Option<usize> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| match slot {
                Slot::Live(voice) if eligible(voice) => Some((index, voice)),
                _ => None,
            })
            .filter(|(_, voice)| voice.fade.target() == 0.0 || voice.priority <= priority)
            .min_by_key(|(_, voice)| (voice.fade.target() != 0.0, voice.priority, voice.order))
            .map(|(index, _)| index)
    }

    /// Crossfades to `track`: see [`Audio::play_music`](crate::Audio::play_music).
    fn play_music(&mut self, track: Arc<MusicData>, crossfade_ms: u32) {
        let fade = self.fade(crossfade_ms);
        // The track waiting to start in the active slot: nothing changes.
        let waiting = self.active_music.is_some_and(|active| {
            matches!(&self.music[active].state,
                MusicState::Handoff { next, .. } if next.same(&track))
        });
        if waiting {
            self.retire(Retired::Music(track));
            return;
        }
        // The track sounding in a slot, playing or fading out: it goes on
        // from where it is, fading back in if it must.
        let sounding = (0..2).find(|&index| {
            self.music[index]
                .sounding()
                .is_some_and(|playing| playing.music.same(&track))
        });
        if let Some(index) = sounding {
            if self.active_music != Some(index)
                && let Some(active) = self.active_music
            {
                self.fade_out_music(active, fade);
            }
            self.resume_music(index, fade);
            self.active_music = Some(index);
            self.retire(Retired::Music(track));
            return;
        }
        if let Some(active) = self.active_music.take() {
            self.fade_out_music(active, fade);
        }
        // A new track: in a free slot, or else in place of the quieter of
        // the two fading out.
        let level = |slot: &MusicSlot| slot.sounding().map_or(-1.0, |track| track.fade.value());
        let index = usize::from(level(&self.music[1]) < level(&self.music[0]));
        self.active_music = Some(index);
        let handoff = self.samples(MIN_FADE_MS);
        match std::mem::replace(&mut self.music[index].state, MusicState::Free) {
            MusicState::Free => self.start_track(index, track, fade),
            MusicState::Live(mut old) => {
                old.fade.to(0.0, fade_out(&old.fade, handoff));
                self.music[index].state = MusicState::Handoff {
                    old,
                    next: track,
                    fade_in: fade,
                };
            }
            MusicState::Handoff { old, next, .. } => {
                // A handoff is only ever in the active slot, which has just
                // faded out without one: never reached.
                self.retire(Retired::Music(next));
                self.music[index].state = MusicState::Handoff {
                    old,
                    next: track,
                    fade_in: fade,
                };
            }
        }
    }

    /// Fades the track sounding in a slot back to full over `fade`
    /// samples, from where its fade is, and cancels a handoff away from it.
    fn resume_music(&mut self, index: usize, fade: u32) {
        let mut track = match std::mem::replace(&mut self.music[index].state, MusicState::Free) {
            MusicState::Free => return,
            MusicState::Live(track) => track,
            MusicState::Handoff { old, next, .. } => {
                self.retire(Retired::Music(next));
                old
            }
        };
        if track.fade.target() != 1.0 {
            track.fade.to(1.0, fade);
        }
        self.music[index].state = MusicState::Live(track);
    }

    /// Fades out the track in a slot over `fade` samples; one waiting to
    /// start in it never does, and the one fading out for it goes on.
    fn fade_out_music(&mut self, index: usize, fade: u32) {
        match std::mem::replace(&mut self.music[index].state, MusicState::Free) {
            MusicState::Free => {}
            MusicState::Live(mut track) => {
                stop_fade(&mut track.fade, fade);
                self.music[index].state = MusicState::Live(track);
            }
            MusicState::Handoff { old, next, .. } => {
                self.retire(Retired::Music(next));
                self.music[index].state = MusicState::Live(old);
            }
        }
    }

    fn start_track(&mut self, index: usize, track: Arc<MusicData>, fade_in: u32) {
        let slot = &mut self.music[index];
        match Track::start(track, &mut slot.buffer, fade_in) {
            Ok(track) => slot.state = MusicState::Live(track),
            Err(track) => {
                self.shared.music_errors.fetch_add(1, Ordering::Relaxed);
                self.retire(Retired::Music(track));
                if self.active_music == Some(index) {
                    self.active_music = None;
                }
            }
        }
    }

    /// Mixes `len` frames, at most [`CHUNK`], into the master's buffer.
    fn mix(&mut self, len: usize) {
        let rate = self.rate;
        let space = self.space;
        let smooth = self.samples(SMOOTH_MS);
        let (buses, rest) = self.scratch.split_at_mut(CHUNK * 8);
        let master = &mut rest[..len * 2];
        for bus in buses.as_chunks_mut::<{ CHUNK * 2 }>().0 {
            bus[..len * 2].fill(0.0);
        }
        let retired = &mut self.retired;
        let retire = |retired: &mut Vec<Retired>, item: Retired| {
            debug_assert!(retired.len() < retired.capacity());
            retired.push(item);
        };
        let mut render = |voice: &mut Voice| {
            let bus = &mut buses[voice.bus.index() * CHUNK * 2..][..len * 2];
            voice.render(bus, rate, &space, smooth) != Rendered::Playing
        };
        for slot in self.slots.iter_mut() {
            *slot = match std::mem::replace(slot, Slot::Free) {
                Slot::Free => Slot::Free,
                Slot::Live(mut voice) => {
                    if render(&mut voice) {
                        retire(retired, Retired::Sound(voice.sound));
                        Slot::Free
                    } else {
                        Slot::Live(voice)
                    }
                }
                Slot::Handoff { mut old, mut new } => match (render(&mut old), render(&mut new)) {
                    (false, false) => Slot::Handoff { old, new },
                    (true, false) => {
                        retire(retired, Retired::Sound(old.sound));
                        Slot::Live(new)
                    }
                    (false, true) => {
                        retire(retired, Retired::Sound(new.sound));
                        Slot::Live(old)
                    }
                    (true, true) => {
                        retire(retired, Retired::Sound(old.sound));
                        retire(retired, Retired::Sound(new.sound));
                        Slot::Free
                    }
                },
            };
        }
        let music_bus = &mut buses[Bus::Music.index() * CHUNK * 2..][..len * 2];
        let errors = &self.shared.music_errors;
        let active = &mut self.active_music;
        for (index, slot) in self.music.iter_mut().enumerate() {
            let MusicSlot { state, buffer } = slot;
            let mut end = |retired: &mut Vec<Retired>, track: Arc<MusicData>, failed: bool| {
                if failed {
                    errors.fetch_add(1, Ordering::Relaxed);
                }
                retire(retired, Retired::Music(track));
                if *active == Some(index) {
                    *active = None;
                }
            };
            *state = match std::mem::replace(state, MusicState::Free) {
                MusicState::Free => MusicState::Free,
                MusicState::Live(mut track) => match track.render(music_bus, rate, buffer) {
                    Rendered::Playing => MusicState::Live(track),
                    Rendered::Ended(_) => {
                        end(retired, track.music, track.failed);
                        MusicState::Free
                    }
                },
                MusicState::Handoff {
                    mut old,
                    next,
                    fade_in,
                } => match old.render(music_bus, rate, buffer) {
                    Rendered::Playing => MusicState::Handoff { old, next, fade_in },
                    Rendered::Ended(frames) => {
                        // The track that was waiting starts where the old
                        // one fell silent; the slot stays the active one.
                        if old.failed {
                            errors.fetch_add(1, Ordering::Relaxed);
                        }
                        retire(retired, Retired::Music(old.music));
                        match Track::start(next, buffer, fade_in) {
                            Err(next) => {
                                end(retired, next, true);
                                MusicState::Free
                            }
                            Ok(mut track) => {
                                let rest = &mut music_bus[frames * 2..];
                                match track.render(rest, rate, buffer) {
                                    Rendered::Playing => MusicState::Live(track),
                                    Rendered::Ended(_) => {
                                        end(retired, track.music, track.failed);
                                        MusicState::Free
                                    }
                                }
                            }
                        }
                    }
                },
            };
        }
        master.fill(0.0);
        for (bus, gain) in buses
            .as_chunks::<{ CHUNK * 2 }>()
            .0
            .iter()
            .zip(&mut self.buses)
        {
            let (master, _) = master.as_chunks_mut::<2>();
            for (out, mixed) in master.iter_mut().zip(bus.as_chunks::<2>().0) {
                let gain = gain.next();
                out[0] += mixed[0] * gain;
                out[1] += mixed[1] * gain;
            }
        }
        for sample in master.iter_mut() {
            *sample = limit(*sample);
        }
    }

    /// Publishes the voices sounding, the rate and the time the callback
    /// took.
    fn record(&self, started: Instant) {
        let shared = &self.shared;
        let voices: usize = self
            .slots
            .iter()
            .map(|slot| match slot {
                Slot::Free => 0,
                Slot::Live(_) => 1,
                Slot::Handoff { .. } => 2,
            })
            .sum();
        shared.active_voices.store(voices as u32, Ordering::Relaxed);
        shared.rate.store(self.rate, Ordering::Relaxed);
        let ns = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        shared.callback_ns.store(ns, Ordering::Relaxed);
    }
}
