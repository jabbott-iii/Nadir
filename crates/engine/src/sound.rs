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

//! Sound in the world: the mixer as the platform's audio thread renders
//! it, and `nadir::sound`, which each frame puts the listener where it is,
//! plays the [`AudioEmitter`]s at their entities, and crossfades to the
//! track of the [`MusicArea`] the listener is in (ADR-0014).

use nadir_assets::{AssetServer, Handle, LoadState};
use nadir_audio::{Audio, AudioConfig, Mixer, Music, VoiceChange, VoiceId};
use nadir_core::Vec2;
use nadir_core::log::warn;
use nadir_core::time::Frame;
use nadir_ecs::{Component, Entity, World};
use nadir_platform::AudioRender;
use nadir_render::RenderFrame;

use crate::{AudioEmitter, AudioListener, MusicArea, PrevTransform, Transform};

/// How long an emitter's voice fades out as it stops, in milliseconds.
const STOP_MS: u32 = 100;

/// How far past the distance it starts within an emitter goes before it
/// stops, in pixels: so that one at the edge does not stop and start again
/// frame after frame.
const MARGIN: f32 = 64.0;

/// The emitters the table has room for before it grows.
const EMITTERS: usize = 64;

/// The mixer, as the platform's audio thread renders it.
pub(crate) struct MixerOutput(pub(crate) Mixer);

impl AudioRender for MixerOutput {
    fn render(&mut self, out: &mut [f32], channels: u16, sample_rate: u32) {
        self.0.render(out, usize::from(channels), sample_rate);
    }

    fn underrun(&mut self) {
        self.0.count_underrun();
    }
}

/// What the engine's `nadir::sound` keeps of the game's sound from frame
/// to frame: a resource of every [`App`](crate::App), for the game to
/// read.
#[derive(Debug)]
pub struct AudioState {
    /// The emitters it started, or warned of, in no order.
    emitters: Vec<Entry>,
    /// The area whose track it sent last, and that track.
    area: Option<(Entity, Handle<Music>)>,
    /// The areas whose tracks did not load, as they were when it warned of
    /// them.
    failed: Vec<(Entity, MusicArea)>,
    /// Where the listener is this frame.
    listener: Vec2,
    /// Where the queue last took the listener to be: `None` before it
    /// first did.
    placed: Option<Vec2>,
    /// How far from the listener an emitter starts: where the mixer makes
    /// it silent.
    range: f32,
}

/// An emitter that `nadir::sound` started, or warned of.
#[derive(Debug)]
struct Entry {
    entity: Entity,
    /// The emitter as it was sent, to compare with each frame.
    emitter: AudioEmitter,
    /// Its voice, or `None` for one whose sound did not load.
    voice: Option<VoiceId>,
    /// Where the queue last took its voice to be.
    position: Vec2,
}

/// On each entity that has an entry in the table, so that a lookup tells
/// the emitters `nadir::sound` started from those still to start.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tracked;

impl Component for Tracked {}

impl AudioState {
    /// Nothing started yet, for a mixer of `config`.
    pub(crate) fn new(config: &AudioConfig) -> Self {
        Self {
            emitters: Vec::with_capacity(EMITTERS),
            area: None,
            failed: Vec::new(),
            listener: Vec2::ZERO,
            placed: None,
            range: config.max_dist,
        }
    }

    /// The entity of the [`MusicArea`] whose track was sent last. `None`
    /// before the first is, and from when the listener is in no area, or
    /// the area's entity is despawned or loses its `MusicArea`, until the
    /// next is.
    pub fn music_area(&self) -> Option<Entity> {
        self.area.map(|(entity, _)| entity)
    }

    /// Moves, changes and stops the emitters it started, then starts those
    /// that can start.
    fn emitters(
        &mut self,
        world: &World,
        audio: &Audio,
        assets: &AssetServer,
        at: &impl Fn(Entity) -> Option<Vec2>,
    ) {
        let emitters = world.read::<AudioEmitter>();
        let mut tracked = world.write::<Tracked>();
        let mut index = 0;
        while let Some(entry) = self.emitters.get_mut(index) {
            let emitter = emitters.get(entry.entity);
            let keep = match entry.voice {
                // Warned of: tried again once it goes or changes, whatever
                // its `Transform`.
                None => emitter.is_some_and(|emitter| same(emitter, &entry.emitter)),
                Some(voice) => match emitter.zip(at(entry.entity)) {
                    Some((emitter, position)) => {
                        let far = position.distance(self.listener) > self.range + MARGIN;
                        if far || restarts(emitter, &entry.emitter) {
                            sent(audio, |audio| audio.stop(voice, STOP_MS)).is_none()
                        } else {
                            entry.follow(emitter, position, voice, audio);
                            true
                        }
                    }
                    // Despawned, or without its emitter or its `Transform`.
                    None => sent(audio, |audio| audio.stop(voice, STOP_MS)).is_none(),
                },
            };
            if keep {
                index += 1;
            } else {
                tracked.remove(entry.entity);
                self.emitters.swap_remove(index);
            }
        }
        for (entity, emitter) in emitters.iter() {
            if tracked.contains(entity) {
                continue;
            }
            let Some(position) = at(entity) else {
                continue;
            };
            let near = position.distance(self.listener) <= self.range;
            if !near {
                continue;
            }
            let voice = match assets.get(emitter.sound) {
                Some(sound) => {
                    let play = |audio: &Audio| audio.play_sound_at(sound, position, emitter.params);
                    match sent(audio, play) {
                        Some(voice) => Some(voice),
                        // The queue was full: the next frame plays it.
                        None => continue,
                    }
                }
                None if assets.state(emitter.sound) == Some(LoadState::Loading) => continue,
                None => {
                    match assets.path(emitter.sound) {
                        Some(path) => warn!(
                            "{entity:?}'s AudioEmitter plays `{path}`, which did not load: it \
                             is silent until the emitter changes"
                        ),
                        None => warn!(
                            "{entity:?}'s AudioEmitter plays a sound the asset server does not \
                             hold: it is silent until the emitter changes"
                        ),
                    }
                    None
                }
            };
            self.emitters.push(Entry {
                entity,
                emitter: *emitter,
                voice,
                position,
            });
            tracked.insert(entity, Tracked);
        }
    }

    /// Sends the track of the area the listener is in, if it is not the
    /// one sent last.
    fn music(
        &mut self,
        world: &World,
        audio: &Audio,
        assets: &AssetServer,
        at: &impl Fn(Entity) -> Option<Vec2>,
    ) {
        let areas = world.read::<MusicArea>();
        if self.area.is_some_and(|(entity, _)| !areas.contains(entity)) {
            self.area = None;
        }
        self.failed
            .retain(|&(entity, area)| areas.get(entity).is_some_and(|now| same_area(now, &area)));
        let listener = self.listener;
        let failed = &self.failed;
        let warned = |entity: Entity| failed.iter().any(|&(warned, _)| warned == entity);
        let holds = |entity: Entity, area: &MusicArea| {
            at(entity).is_some_and(|centre| {
                let offset = (listener - centre).abs();
                offset.x <= area.half.x && offset.y <= area.half.y
            })
        };
        // The area sent last while it holds the listener, so that where
        // areas overlap the music does not change back and forth: even
        // one whose new track did not load.
        let current = self.area.and_then(|(entity, _)| {
            let area = areas.get(entity).filter(|area| holds(entity, area))?;
            Some((entity, *area))
        });
        let first = || {
            let (entity, area) = areas
                .iter()
                .find(|&(entity, area)| !warned(entity) && holds(entity, area))?;
            Some((entity, *area))
        };
        let Some((entity, area)) = current.or_else(first) else {
            // In no area: the music plays on, and coming back sends it
            // again, which the mixer carries on with.
            self.area = None;
            return;
        };
        // Its track sent, or warned of: the music plays on as it is.
        if self.area == Some((entity, area.music)) || warned(entity) {
            return;
        }
        match assets.get(area.music) {
            Some(track) => {
                if sent(audio, |audio| audio.play_track(track, area.crossfade_ms)).is_some() {
                    self.area = Some((entity, area.music));
                }
            }
            None if assets.state(area.music) == Some(LoadState::Loading) => {}
            None => {
                match assets.path(area.music) {
                    Some(path) => warn!(
                        "{entity:?}'s MusicArea plays `{path}`, which did not load: the music does \
                         not change for it until the area changes"
                    ),
                    None => warn!(
                        "{entity:?}'s MusicArea plays a track the asset server does not hold: the \
                         music does not change for it until the area changes"
                    ),
                }
                self.failed.push((entity, area));
            }
        }
    }
}

impl Entry {
    /// Sends the voice `position`, and `emitter`'s volume and pitch, where
    /// they changed, and keeps each that the queue took.
    fn follow(&mut self, emitter: &AudioEmitter, position: Vec2, voice: VoiceId, audio: &Audio) {
        if bits(position) != bits(self.position)
            && sent(audio, |audio| {
                audio.set_voice(voice, VoiceChange::Position(position));
            })
            .is_some()
        {
            self.position = position;
        }
        let (now, then) = (emitter.params, &mut self.emitter.params);
        if now.volume.to_bits() != then.volume.to_bits()
            && sent(audio, |audio| {
                audio.set_voice(voice, VoiceChange::Volume(now.volume));
            })
            .is_some()
        {
            then.volume = now.volume;
        }
        if now.pitch.to_bits() != then.pitch.to_bits()
            && sent(audio, |audio| {
                audio.set_voice(voice, VoiceChange::Pitch(now.pitch));
            })
            .is_some()
        {
            then.pitch = now.pitch;
        }
    }
}

/// `nadir::sound`, first in the frame's `Extract` stage, after the game's
/// camera: puts the listener where it is drawn, starts, moves, changes and
/// stops the emitters, sends the track of the music area the listener is
/// in, and calls [`Audio::update`].
///
/// The listener and each playing emitter are placed when they are not
/// where the queue last took them, so a frame where nothing moves sends
/// nothing; a move, a play, a stop or a change the full queue dropped is
/// sent again the next frame. It changes no reflected component, and
/// allocates nothing once its table and its marker's storage hold the
/// emitters.
pub(crate) fn sound(world: &World, frame: &Frame) {
    let audio = world.resource::<Audio>();
    let assets = world.resource::<AssetServer>();
    let mut state = world.resource_mut::<AudioState>();
    let transforms = world.read::<Transform>();
    let previous = world.read::<PrevTransform>();
    // Where an entity is drawn: as `nadir::extract_sprites` draws it.
    let at = |entity| {
        let current = transforms.get(entity)?.position;
        Some(previous.get(entity).map_or(current, |previous| {
            previous.0.position.lerp(current, frame.alpha)
        }))
    };
    let listener = world
        .read::<AudioListener>()
        .entities()
        .iter()
        .find_map(|&entity| at(entity))
        .unwrap_or_else(|| world.resource::<RenderFrame>().camera.position);
    state.listener = listener;
    if state.placed.map(bits) != Some(bits(listener))
        && sent(&audio, |audio| audio.set_listener(listener)).is_some()
    {
        state.placed = Some(listener);
    }
    state.emitters(world, &audio, &assets, &at);
    state.music(world, &audio, &assets, &at);
    audio.update();
}

/// Sends what `send` sends, and gives back what it returns if the queue
/// took it: if the count of dropped commands did not rise.
fn sent<T>(audio: &Audio, send: impl FnOnce(&Audio) -> T) -> Option<T> {
    let dropped = audio.stats().dropped;
    let value = send(audio);
    (audio.stats().dropped == dropped).then_some(value)
}

/// A position's bits, to compare, so that one that is not a number is
/// where it was.
fn bits(position: Vec2) -> [u32; 2] {
    [position.x, position.y].map(f32::to_bits)
}

/// Whether the change from `then` to `now` needs a new voice: another
/// sound, bus, loop, priority or limit to its voices.
fn restarts(now: &AudioEmitter, then: &AudioEmitter) -> bool {
    let (a, b) = (&now.params, &then.params);
    now.sound != then.sound
        || a.bus != b.bus
        || a.looped != b.looped
        || a.priority != b.priority
        || a.max_instances != b.max_instances
}

/// Whether two emitters are the same, their numbers bit for bit, so that
/// one with a volume that is not a number is the same as itself.
fn same(a: &AudioEmitter, b: &AudioEmitter) -> bool {
    let bits = |emitter: &AudioEmitter| {
        let params = &emitter.params;
        [params.volume, params.pitch, params.pan].map(f32::to_bits)
    };
    !restarts(a, b) && bits(a) == bits(b)
}

/// Whether two areas are the same, bit for bit.
fn same_area(a: &MusicArea, b: &MusicArea) -> bool {
    let bits = |area: &MusicArea| [area.half.x, area.half.y].map(f32::to_bits);
    a.music == b.music && a.crossfade_ms == b.crossfade_ms && bits(a) == bits(b)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::f32::consts::{FRAC_PI_4, TAU};
    use std::num::NonZeroU32;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use nadir_assets::{AssetError, AssetPath, FinalizeCtx, OnFailure, Source};
    use nadir_audio::{AudioStats, Bus, PlayParams, Sound, SoundData};
    use nadir_codec::{kpcm, qoa};
    use nadir_core::{GenIndex, log};

    use super::*;
    use crate::{App, Config, Volumes};

    /// One tick's length, as a frame: it runs one tick.
    const TICK: Duration = Duration::from_nanos(16_666_667);
    /// The rate the tests render at: the output's.
    const RATE: u32 = 48_000;
    /// The frames of sound a frame of one tick renders: a 60th of a second.
    const FRAME: usize = 800;
    /// The frames an emitter takes to fade out as it stops, and a frame
    /// more: 100 ms.
    const STOPPED: usize = 7;

    /// Files in memory, which a test can add to after the server is made.
    #[derive(Clone, Default)]
    struct Files(Arc<Mutex<HashMap<String, Vec<u8>>>>);

    impl Files {
        fn insert(&self, path: &str, bytes: Vec<u8>) {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(path.to_string(), bytes);
        }
    }

    impl Source for Files {
        fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(path.as_str())
                .cloned()
                .ok_or(AssetError::NotFound)
        }
    }

    /// A sound file of a second of `level` in every sample, mono, at the
    /// output's rate.
    fn constant_file(level: f32) -> Vec<u8> {
        let header = kpcm::Header {
            channels: 1,
            sample_rate: RATE,
            frames: RATE,
        };
        let sample = (level * 32768.0) as i16;
        kpcm::encode(&header, &vec![sample; RATE as usize]).expect("a sound file")
    }

    /// A track file of `samples`, mono, at the output's rate.
    fn track_file(samples: &[i16]) -> Vec<u8> {
        let header = qoa::Header {
            channels: 1,
            sample_rate: RATE,
            samples: samples.len() as u32,
        };
        qoa::encode(&header, samples).expect("a QOA file")
    }

    /// A second of a tone of `hz` at `level`, a whole number of cycles, so
    /// that it loops without a seam: 16 bits, at the output's rate.
    fn tone(hz: f32, level: f32) -> Vec<i16> {
        (0..RATE)
            .map(|n| ((TAU * hz * n as f32 / RATE as f32).sin() * level * 32767.0) as i16)
            .collect()
    }

    /// A game whose mixer the test renders, a frame of sound after each
    /// frame, offline, and whose assets are files in memory.
    struct Game {
        app: App,
        mixer: Mixer,
        files: Files,
    }

    impl Game {
        fn new() -> Self {
            Self::with(Config::default())
        }

        fn with(config: Config) -> Self {
            Self::of(App::new(config))
        }

        fn of(mut app: App) -> Self {
            let mixer = app.take_mixer().expect("no frame has run");
            let files = Files::default();
            // No placeholders, with `dev` or without, as a release build.
            let server = AssetServer::with_source(files.clone(), OnFailure::Nothing);
            app.world_mut().insert_resource(server);
            Self { app, mixer, files }
        }

        fn assets(&self) -> nadir_ecs::ResMut<'_, AssetServer> {
            self.app.world().resource_mut::<AssetServer>()
        }

        /// A sound of `level` at `path`, made now: see [`constant_file`].
        fn constant(&mut self, path: &str, level: f32) -> Handle<Sound> {
            self.files.insert(path, constant_file(level));
            self.assets()
                .load_now(path, &mut FinalizeCtx::new())
                .expect("the sound loads")
        }

        /// A track of `samples` at `path`, mono at the output's rate, made
        /// now.
        fn track(&mut self, path: &str, samples: &[i16]) -> Handle<Music> {
            self.files.insert(path, track_file(samples));
            self.assets()
                .load_now(path, &mut FinalizeCtx::new())
                .expect("the track loads")
        }

        /// An entity at `position`, with `component`.
        fn spawn<T: Component>(&mut self, position: Vec2, component: T) -> Entity {
            self.app
                .world_mut()
                .spawn()
                .with(Transform::at(position))
                .with(component)
                .id()
        }

        fn place(&self, entity: Entity, position: Vec2) {
            let mut transforms = self.app.world().write::<Transform>();
            transforms.get_mut(entity).expect("placed").position = position;
        }

        fn camera(&self, position: Vec2) {
            self.app
                .world()
                .resource_mut::<RenderFrame>()
                .camera
                .position = position;
        }

        fn emitter(&self, entity: Entity) -> std::cell::RefMut<'_, AudioEmitter> {
            std::cell::RefMut::map(self.app.world().write::<AudioEmitter>(), |emitters| {
                emitters.get_mut(entity).expect("an emitter")
            })
        }

        /// Runs a frame `elapsed` after the last, then renders a frame of
        /// the sound it sent, stereo, into `out`.
        fn frame_into(&mut self, elapsed: Duration, out: &mut [f32]) {
            self.app.update(elapsed, &[]);
            self.mixer.render(out, 2, RATE);
        }

        /// Runs a frame of a tick, and gives back its sound.
        fn frame(&mut self) -> Vec<f32> {
            let mut out = vec![0.0; FRAME * 2];
            self.frame_into(TICK, &mut out);
            out
        }

        /// Runs `frames` frames of a tick, and gives back the last's sound.
        fn frames(&mut self, frames: usize) -> Vec<f32> {
            let mut out = vec![0.0; FRAME * 2];
            for _ in 0..frames {
                self.frame_into(TICK, &mut out);
            }
            out
        }

        fn stats(&self) -> AudioStats {
            self.app.world().resource::<Audio>().stats()
        }

        fn music_area(&self) -> Option<Entity> {
            self.app.world().resource::<AudioState>().music_area()
        }

        /// Fills the mixer's queue to the brim, so that whatever comes
        /// next is dropped until the mixer renders.
        fn fill_queue(&self) {
            let audio = self.app.world().resource::<Audio>();
            let dropped = audio.stats().dropped;
            while audio.stats().dropped == dropped {
                audio.set_bus_volume(Bus::Ui, 1.0, 0);
            }
        }
    }

    /// What a mono sound of `level` at `offset` from the listener is heard
    /// as, left and right, by `AudioConfig::default`: panned by the
    /// constant-power law by its offset to the side over 480 pixels, and
    /// fading linearly from 64 pixels away to silence at 800.
    fn heard(level: f32, offset: Vec2) -> [f32; 2] {
        let pan = (offset.x / 480.0).clamp(-1.0, 1.0);
        let distance = offset.length();
        let gain = ((800.0 - distance) / (800.0 - 64.0)).clamp(0.0, 1.0);
        let (sin, cos) = ((pan + 1.0) * FRAC_PI_4).sin_cos();
        [level * gain * cos, level * gain * sin]
    }

    /// Checks that the last frame of `out` is `expected`, left and right.
    #[track_caller]
    fn assert_heard(out: &[f32], expected: [f32; 2]) {
        let found = [out[out.len() - 2], out[out.len() - 1]];
        let near = (0..2).all(|side| (found[side] - expected[side]).abs() < 1e-3);
        assert!(near, "heard {found:?}, not {expected:?}");
    }

    /// The root mean square of the left channel of `out`'s `frames`
    /// frames from `from`.
    fn rms(out: &[f32], from: usize, frames: usize) -> f32 {
        let left = out[from * 2..(from + frames) * 2].iter().step_by(2);
        (left.map(|x| x * x).sum::<f32>() / frames as f32).sqrt()
    }

    /// The records of the log so far that hold every one of `parts`.
    fn logged(parts: &[&str]) -> usize {
        log::tail(log::RING_CAPACITY)
            .iter()
            .filter(|record| parts.iter().all(|part| record.contains(part)))
            .count()
    }

    #[test]
    fn the_mixer_output_renders_the_mixer_and_counts_its_underruns() {
        let (audio, mixer) = nadir_audio::new(AudioConfig::default()).expect("the default");
        let mut output = MixerOutput(mixer);
        let sound = Sound::new(SoundData::new(1, RATE, vec![0.5; 4_800]).expect("a sound"));
        audio.play_sound(&sound, PlayParams::default());
        let mut out = vec![0.0; 480 * 2];
        output.render(&mut out, 2, RATE);
        assert_heard(&out, heard(0.5, Vec2::ZERO));
        // One channel: the mean of the two.
        let mut mono = vec![0.0; 480];
        output.render(&mut mono, 1, RATE);
        assert!((mono[479] - heard(0.5, Vec2::ZERO)[0]).abs() < 1e-3);
        output.underrun();
        output.underrun();
        assert_eq!((audio.stats().underruns, audio.stats().rate), (2, RATE));
    }

    #[test]
    fn the_first_update_drops_a_mixer_not_taken_and_the_audio_does_nothing() {
        let mut taken = App::new(Config::default());
        assert!(taken.take_mixer().is_some());
        assert!(taken.take_mixer().is_none(), "taken once");

        let mut app = App::new(Config::default());
        app.update(TICK, &[]);
        assert!(app.take_mixer().is_none(), "dropped");
        let world = app.world();
        let audio = world.resource::<Audio>();
        let sound = Sound::new(SoundData::new(1, RATE, vec![0.5; 480]).expect("a sound"));
        // Nothing is queued, so nothing fills the queue or is dropped.
        for _ in 0..2_000 {
            audio.play_sound(&sound, PlayParams::default());
            audio.set_listener(Vec2::ONE);
        }
        // A play of nothing is counted still.
        let nothing = Handle::new(GenIndex::new(7, NonZeroU32::MIN));
        audio.play(
            &world.resource::<AssetServer>(),
            nothing,
            PlayParams::default(),
        );
        audio.update();
        let expected = AudioStats {
            unplayed: 1,
            ..AudioStats::default()
        };
        assert_eq!(audio.stats(), expected);
        assert_eq!(
            Arc::strong_count(sound.data()),
            1,
            "nothing holds the sound"
        );
    }

    #[test]
    fn the_listener_is_the_first_audio_listener_where_it_is_drawn_or_else_the_camera() {
        let mut game = Game::new();
        let hum = game.constant("audio/hum.kpcm", 0.5);
        game.spawn(Vec2::new(100.0, 0.0), AudioEmitter::looped(hum));
        // The camera at the origin hears it to the right, and past it, to
        // the left.
        assert_heard(&game.frames(2), heard(0.5, Vec2::new(100.0, 0.0)));
        game.camera(Vec2::new(300.0, 0.0));
        assert_heard(&game.frames(2), heard(0.5, Vec2::new(-200.0, 0.0)));

        // A listener with no Transform is passed over; the next is where
        // it is drawn: halfway from its PrevTransform, half a tick on.
        game.app.world_mut().spawn().with(AudioListener);
        let listener = game.spawn(Vec2::new(200.0, 0.0), AudioListener);
        game.app
            .world_mut()
            .write::<PrevTransform>()
            .insert(listener, PrevTransform(Transform::at(Vec2::ZERO)));
        let mut out = vec![0.0; FRAME * 2];
        game.frame_into(TICK / 2, &mut out);
        assert_heard(&out, heard(0.5, Vec2::ZERO));
        // Once a tick has copied its Transform into it, where it is.
        assert_heard(&game.frames(2), heard(0.5, Vec2::new(-100.0, 0.0)));
        // Gone, the camera hears again.
        game.app.world_mut().despawn(listener);
        assert_heard(&game.frames(2), heard(0.5, Vec2::new(-200.0, 0.0)));
    }

    #[test]
    fn an_emitter_waits_for_its_sound_and_follows_its_entity() {
        let mut game = Game::new();
        game.files.insert("audio/later.kpcm", constant_file(0.5));
        let later = game.assets().load::<Sound>("audio/later.kpcm");
        let entity = game.spawn(Vec2::new(100.0, 0.0), AudioEmitter::looped(later));
        // Still loading: nothing plays, and no play is lost.
        for _ in 0..3 {
            assert!(game.frame().iter().all(|&x| x == 0.0));
        }
        assert_eq!((game.stats().active_voices, game.stats().unplayed), (0, 0));
        game.assets().finish_all_loads(&mut FinalizeCtx::new());
        assert_heard(&game.frame(), heard(0.5, Vec2::new(100.0, 0.0)));
        // Where its entity goes, in the frame after.
        for x in [-240.0, 0.0, 400.0] {
            game.place(entity, Vec2::new(x, 30.0));
            assert_heard(&game.frame(), heard(0.5, Vec2::new(x, 30.0)));
        }
        assert_eq!(game.stats().active_voices, 1);
    }

    #[test]
    fn an_emitter_changed_takes_a_new_volume_live_and_restarts_for_a_new_sound() {
        let mut game = Game::new();
        let hum = game.constant("audio/hum.kpcm", 0.5);
        let buzz = game.constant("audio/buzz.kpcm", 0.25);
        let entity = game.spawn(Vec2::ZERO, AudioEmitter::looped(hum));
        assert_heard(&game.frames(2), heard(0.5, Vec2::ZERO));
        // The voice takes the volume and the pitch as it plays: no other
        // voice starts, and the first goes on.
        game.emitter(entity).params.volume = 0.5;
        game.emitter(entity).params.pitch = 0.5;
        assert_heard(&game.frame(), heard(0.25, Vec2::ZERO));
        assert_eq!(game.stats().active_voices, 1);
        // Not a number, it is silent, and a number again, heard.
        game.emitter(entity).params.volume = f32::NAN;
        assert_heard(&game.frames(2), [0.0, 0.0]);
        game.emitter(entity).params.volume = 0.5;
        assert_heard(&game.frame(), heard(0.25, Vec2::ZERO));

        // Another sound: the old voice fades out as the new one starts.
        game.emitter(entity).sound = buzz;
        game.frame();
        assert_eq!(game.stats().active_voices, 2);
        assert_heard(&game.frames(STOPPED), heard(0.125, Vec2::ZERO));
        assert_eq!(game.stats().active_voices, 1);
        // So does another bus, or a component put in its place.
        game.emitter(entity).params.bus = Bus::Sfx;
        game.frame();
        assert_eq!(game.stats().active_voices, 2);
        game.app
            .world_mut()
            .write::<AudioEmitter>()
            .insert(entity, AudioEmitter::looped(hum));
        assert_heard(&game.frames(STOPPED * 2), heard(0.5, Vec2::ZERO));
        assert_eq!(game.stats().active_voices, 1);
    }

    #[test]
    fn an_emitter_stops_when_it_goes_or_is_far_and_plays_again_when_near() {
        let mut game = Game::new();
        let hum = game.constant("audio/hum.kpcm", 0.5);
        let far = game.spawn(Vec2::new(100.0, 0.0), AudioEmitter::looped(hum));
        let removed = game.spawn(Vec2::new(-100.0, 0.0), AudioEmitter::looped(hum));
        let despawned = game.spawn(Vec2::new(0.0, 100.0), AudioEmitter::looped(hum));
        game.frames(2);
        assert_eq!(game.stats().active_voices, 3);

        game.app.world_mut().write::<AudioEmitter>().remove(removed);
        game.app.world_mut().despawn(despawned);
        game.frames(STOPPED);
        assert_eq!(game.stats().active_voices, 1);
        assert_heard(&game.frame(), heard(0.5, Vec2::new(100.0, 0.0)));

        // Silent from 800 pixels, it plays on to 864, and stops past it.
        game.place(far, Vec2::new(850.0, 0.0));
        assert_heard(&game.frames(STOPPED), [0.0, 0.0]);
        assert_eq!(game.stats().active_voices, 1);
        game.place(far, Vec2::new(0.0, 870.0));
        game.frames(STOPPED);
        assert_eq!(game.stats().active_voices, 0);
        // It starts only within 800 pixels.
        game.place(far, Vec2::new(810.0, 0.0));
        game.frames(2);
        assert_eq!(game.stats().active_voices, 0);
        game.place(far, Vec2::new(-200.0, 0.0));
        assert_heard(&game.frame(), heard(0.5, Vec2::new(-200.0, 0.0)));
        assert_eq!(game.stats().active_voices, 1);

        // Given back its emitter, the other plays again.
        game.app
            .world_mut()
            .write::<AudioEmitter>()
            .insert(removed, AudioEmitter::looped(hum));
        game.frame();
        assert_eq!(game.stats().active_voices, 2);
        // Without its Transform, it stops.
        game.app.world_mut().write::<Transform>().remove(removed);
        game.frames(STOPPED);
        assert_eq!(game.stats().active_voices, 1);
    }

    #[test]
    fn a_sound_played_to_its_end_is_given_back_by_the_frame_after() {
        let mut game = Game::new();
        let blip = game.constant("audio/blip.kpcm", 0.5);
        let held = |game: &Game| Arc::strong_count(game.assets().get(blip).expect("loaded").data());
        game.spawn(Vec2::ZERO, AudioEmitter::new(blip, PlayParams::default()));
        game.frames(2);
        assert_eq!(held(&game), 2, "its voice holds it");
        // A second long, it ends within 61 frames: the mixer gives it back,
        // and the frame after drops it.
        game.frames(70);
        assert_eq!(held(&game), 1, "the asset server alone holds it");
    }

    #[test]
    fn an_emitter_warned_of_is_warned_of_again_only_once_it_goes_or_changes() {
        let mut game = Game::new();
        let sound = {
            let mut assets = game.assets();
            let sound = assets.load::<Sound>("audio/warned-still.kpcm");
            assets.finish_all_loads(&mut FinalizeCtx::new());
            sound
        };
        let entity = game.spawn(Vec2::ZERO, AudioEmitter::looped(sound));
        let warned = || logged(&["warned-still.kpcm", "AudioEmitter"]);
        game.frames(2);
        assert_eq!(warned(), 1);
        // Without its Transform, and with it again, it is the same emitter.
        game.app.world_mut().write::<Transform>().remove(entity);
        game.frames(2);
        game.app
            .world_mut()
            .write::<Transform>()
            .insert(entity, Transform::at(Vec2::ZERO));
        game.frames(2);
        assert_eq!(warned(), 1);
        // Taken away and given back, it is tried again.
        game.app.world_mut().write::<AudioEmitter>().remove(entity);
        game.frame();
        game.app
            .world_mut()
            .write::<AudioEmitter>()
            .insert(entity, AudioEmitter::looped(sound));
        game.frames(2);
        assert_eq!(warned(), 2);
    }

    #[test]
    fn five_looped_emitters_of_one_sound_all_sound() {
        let mut game = Game::new();
        let hum = game.constant("audio/hum.kpcm", 0.1);
        for _ in 0..5 {
            game.spawn(Vec2::ZERO, AudioEmitter::looped(hum));
        }
        assert_heard(&game.frames(2), heard(0.5, Vec2::ZERO));
        assert_eq!(game.stats().active_voices, 5);
        // With the default limit, 4 at once, the fifth takes the first's
        // voice.
        let mut game = Game::new();
        let hum = game.constant("audio/hum.kpcm", 0.1);
        let looped = PlayParams {
            looped: true,
            ..PlayParams::default()
        };
        for _ in 0..5 {
            game.spawn(Vec2::ZERO, AudioEmitter::new(hum, looped));
        }
        assert_heard(&game.frames(2), heard(0.4, Vec2::ZERO));
        assert_eq!(game.stats().active_voices, 4);
    }

    #[test]
    fn the_music_crossfades_when_the_listener_crosses_into_another_area() {
        let mut game = Game::new();
        let tone = game.track("audio/music/tone.qoa", &tone(400.0, 0.25));
        let quiet = game.track("audio/music/quiet.qoa", &[0; RATE as usize]);
        let listener = game.spawn(Vec2::ZERO, AudioListener);
        let mut area = MusicArea::new(tone, Vec2::splat(100.0));
        area.crossfade_ms = 600;
        let loud = game.spawn(Vec2::ZERO, area);
        area.music = quiet;
        let still = game.spawn(Vec2::new(300.0, 0.0), area);
        // The tone fades in over 600 ms, then plays on: 480 frames are 4 of
        // its cycles, whose RMS is the tone's.
        let out = game.frames(45);
        let full = rms(&out, FRAME - 480, 480);
        assert!(full > 0.17, "{full}");
        assert_eq!(game.music_area(), Some(loud));

        // Into the other: the tone fades out linearly over 600 ms, 28,800
        // frames, from the next frame's sound.
        game.place(listener, Vec2::new(300.0, 0.0));
        let fade = 28_800;
        let mut out = vec![0.0; 37 * FRAME * 2];
        for frame in out.chunks_mut(FRAME * 2) {
            game.frame_into(TICK, frame);
        }
        assert_eq!(game.music_area(), Some(still));
        let at = |from: usize| rms(&out, from, 480) / full;
        let start = at(0);
        let half = at(fade / 2 - 240);
        let end = at(fade);
        assert!(
            (start - (1.0 - 240.0 / fade as f32)).abs() < 0.02,
            "{start}"
        );
        assert!((half - 0.5).abs() < 0.02, "{half}");
        // The quiet track alone, as QOA decodes its silence: within a bit.
        assert!(end < 1e-3, "{end}");
    }

    #[test]
    fn outside_every_area_the_music_plays_on_and_coming_back_carries_it_on() {
        let mut game = Game::new();
        let tone = game.track("audio/music/tone.qoa", &tone(400.0, 0.25));
        let listener = game.spawn(Vec2::ZERO, AudioListener);
        let mut area = MusicArea::new(tone, Vec2::new(100.0, 50.0));
        area.crossfade_ms = 100;
        let meadow = game.spawn(Vec2::ZERO, area);
        let out = game.frames(10);
        let full = rms(&out, FRAME - 480, 480);
        assert_eq!(game.music_area(), Some(meadow));
        let heard_full = |out: &[f32]| (rms(out, FRAME - 480, 480) / full - 1.0).abs() < 0.02;

        // Just outside, below: the music plays on.
        game.place(listener, Vec2::new(0.0, 51.0));
        for _ in 0..10 {
            assert!(heard_full(&game.frame()));
            assert_eq!(game.music_area(), None);
        }
        // On its edge, it is in it again: the track is sent again, and the
        // mixer carries on with it, with no dip.
        game.place(listener, Vec2::new(100.0, 50.0));
        for _ in 0..10 {
            assert!(heard_full(&game.frame()));
            assert_eq!(game.music_area(), Some(meadow));
        }
        // Its area gone, the music plays on too.
        game.app.world_mut().write::<MusicArea>().remove(meadow);
        assert!(heard_full(&game.frame()));
        assert_eq!(game.music_area(), None);
        game.app
            .world_mut()
            .write::<MusicArea>()
            .insert(meadow, area);
        game.frame();
        assert_eq!(game.music_area(), Some(meadow));
        game.app.world_mut().despawn(meadow);
        assert!(heard_full(&game.frame()));
        assert_eq!(game.music_area(), None);
    }

    #[test]
    fn where_areas_overlap_the_music_changes_only_on_leaving_the_one_playing() {
        let mut game = Game::new();
        let low = game.track("audio/music/low.qoa", &tone(200.0, 0.25));
        let high = game.track("audio/music/high.qoa", &tone(800.0, 0.25));
        // The camera hears: these areas meet from x = 50 to 100.
        let west = game.spawn(Vec2::ZERO, MusicArea::new(low, Vec2::splat(100.0)));
        let east = game.spawn(
            Vec2::new(150.0, 0.0),
            MusicArea::new(high, Vec2::splat(100.0)),
        );
        let mut heard_in = |x: f32| {
            game.camera(Vec2::new(x, 0.0));
            game.frame();
            game.music_area()
        };
        assert_eq!(heard_in(0.0), Some(west));
        assert_eq!(heard_in(80.0), Some(west));
        assert_eq!(heard_in(200.0), Some(east));
        assert_eq!(heard_in(80.0), Some(east));
        assert_eq!(heard_in(-20.0), Some(west));
        // Where both hold it and neither played, the first.
        assert_eq!(heard_in(1_000.0), None);
        assert_eq!(heard_in(80.0), Some(west));
    }

    #[test]
    fn where_an_area_whose_track_did_not_load_overlaps_another_the_other_plays() {
        let mut game = Game::new();
        let tone = game.track("audio/music/tone.qoa", &tone(400.0, 0.25));
        let missing = {
            let mut assets = game.assets();
            let missing = assets.load::<Music>("audio/music/skipped.qoa");
            assets.finish_all_loads(&mut FinalizeCtx::new());
            missing
        };
        // The camera at the origin is in both, and the first's track did
        // not load.
        game.spawn(Vec2::ZERO, MusicArea::new(missing, Vec2::splat(100.0)));
        let meadow = game.spawn(
            Vec2::new(50.0, 0.0),
            MusicArea::new(tone, Vec2::splat(100.0)),
        );
        game.frames(2);
        assert_eq!(game.music_area(), Some(meadow));
        assert_eq!(logged(&["skipped.qoa", "MusicArea"]), 1);
    }

    #[test]
    fn the_area_playing_despawned_is_named_no_more_while_the_next_ones_track_loads() {
        let mut game = Game::new();
        let tone = game.track("audio/music/tone.qoa", &tone(400.0, 0.25));
        game.files
            .insert("audio/music/later.qoa", track_file(&[0; RATE as usize]));
        let later = game.assets().load::<Music>("audio/music/later.qoa");
        // The camera at the origin is in both.
        let west = game.spawn(Vec2::ZERO, MusicArea::new(tone, Vec2::splat(100.0)));
        let east = game.spawn(
            Vec2::new(50.0, 0.0),
            MusicArea::new(later, Vec2::splat(100.0)),
        );
        game.frames(2);
        assert_eq!(game.music_area(), Some(west));
        game.app.world_mut().despawn(west);
        game.frames(2);
        assert_eq!(game.music_area(), None);
        // Once its track has loaded, the other's plays.
        game.assets().finish_all_loads(&mut FinalizeCtx::new());
        game.frame();
        assert_eq!(game.music_area(), Some(east));
    }

    #[test]
    fn a_play_a_move_and_a_track_dropped_by_a_full_queue_are_sent_again() {
        let mut game = Game::new();
        let hum = game.constant("audio/hum.kpcm", 0.5);
        let tone = game.track("audio/music/tone.qoa", &tone(400.0, 0.25));
        game.fill_queue();
        let entity = game.spawn(Vec2::new(100.0, 0.0), AudioEmitter::looped(hum));
        // Its play is dropped: the mixer catches up, and plays nothing.
        game.app.update(TICK, &[]);
        let mut out = vec![0.0; FRAME * 2];
        game.mixer.render(&mut out, 2, RATE);
        assert_eq!(game.stats().active_voices, 0);
        assert_heard(&game.frame(), heard(0.5, Vec2::new(100.0, 0.0)));
        assert_eq!(game.stats().active_voices, 1);

        // A move dropped is heard a frame late.
        game.fill_queue();
        game.place(entity, Vec2::new(-100.0, 0.0));
        game.app.update(TICK, &[]);
        game.mixer.render(&mut out, 2, RATE);
        assert_heard(&out, heard(0.5, Vec2::new(100.0, 0.0)));
        assert_heard(&game.frame(), heard(0.5, Vec2::new(-100.0, 0.0)));
        // And the listener's.
        game.fill_queue();
        game.camera(Vec2::new(-50.0, 0.0));
        game.app.update(TICK, &[]);
        game.mixer.render(&mut out, 2, RATE);
        assert_heard(&out, heard(0.5, Vec2::new(-100.0, 0.0)));
        assert_heard(&game.frame(), heard(0.5, Vec2::new(-50.0, 0.0)));

        // So is a track.
        game.app.world_mut().despawn(entity);
        game.frames(STOPPED);
        game.fill_queue();
        let mut area = MusicArea::new(tone, Vec2::splat(100.0));
        area.crossfade_ms = 2;
        let meadow = game.spawn(Vec2::ZERO, area);
        game.app.update(TICK, &[]);
        assert_eq!(game.music_area(), None);
        game.mixer.render(&mut out, 2, RATE);
        assert_eq!(rms(&out, 0, FRAME), 0.0);
        let out = game.frame();
        assert_eq!(game.music_area(), Some(meadow));
        assert!(rms(&out, FRAME - 480, 480) > 0.17);
    }

    #[test]
    fn a_frame_where_nothing_moves_sends_nothing() {
        // The mixer never renders: the queue only fills.
        let mut game = Game::new();
        let hum = game.constant("audio/hum.kpcm", 0.5);
        let tone = game.track("audio/music/tone.qoa", &tone(400.0, 0.25));
        game.spawn(Vec2::ZERO, MusicArea::new(tone, Vec2::splat(100.0)));
        let moving = game.spawn(Vec2::new(100.0, 0.0), AudioEmitter::looped(hum));
        for x in [-100.0, 200.0] {
            game.spawn(Vec2::new(x, 50.0), AudioEmitter::looped(hum));
        }
        // Sent each frame, the listener's and the emitters' positions would
        // fill the half of the queue they may take within 130 frames.
        for _ in 0..1_000 {
            game.app.update(TICK, &[]);
        }
        assert_eq!(game.stats().dropped, 0);
        // One that moves sends a position each frame, and the queue takes
        // them until it is half full: it holds the four bus volumes
        // `App::new` set, the listener, three plays where they were played,
        // and a track.
        for frame in 0..600 {
            game.place(moving, Vec2::new(frame as f32, 0.0));
            game.app.update(TICK, &[]);
        }
        assert_eq!(game.stats().dropped, 600 - (512 - 9));
    }

    #[test]
    fn the_configured_volumes_scale_what_is_heard() {
        let volumes = Volumes {
            ambience: 50,
            sfx: 0,
            ..Volumes::default()
        };
        let mut game = Game::with(Config {
            volumes,
            ..Config::default()
        });
        let hum = game.constant("audio/hum.kpcm", 0.5);
        // On the ambience bus, at a quarter of the gain; on the effects
        // bus, at none.
        game.spawn(Vec2::ZERO, AudioEmitter::looped(hum));
        let effect = PlayParams {
            looped: true,
            ..PlayParams::default()
        };
        game.spawn(Vec2::ZERO, AudioEmitter::new(hum, effect));
        assert_heard(&game.frames(2), heard(0.125, Vec2::ZERO));
        assert_eq!(game.stats().active_voices, 2);
    }

    #[test]
    fn a_sound_or_a_track_that_did_not_load_is_warned_of_once_until_it_changes() {
        let mut game = Game::new();
        let (sound, track) = {
            let mut assets = game.assets();
            let sound = assets.load::<Sound>("audio/warned-once.kpcm");
            let track = assets.load::<Music>("audio/music/warned-once.qoa");
            assets.finish_all_loads(&mut FinalizeCtx::new());
            (sound, track)
        };
        let emitter = game.spawn(Vec2::ZERO, AudioEmitter::looped(sound));
        let area = game.spawn(Vec2::ZERO, MusicArea::new(track, Vec2::splat(10.0)));
        let unknown = Handle::new(GenIndex::new(1_000, NonZeroU32::MIN));
        let stranger = game.spawn(Vec2::ZERO, AudioEmitter::looped(unknown));
        let sounds = ["warned-once.kpcm", "AudioEmitter"];
        let tracks = ["warned-once.qoa", "MusicArea"];
        let holds =
            format!("{stranger:?}'s AudioEmitter plays a sound the asset server does not hold");
        let warned = || (logged(&sounds), logged(&tracks), logged(&[&holds]));
        game.frames(5);
        assert_eq!(warned(), (1, 1, 1));
        assert_eq!(game.music_area(), None);
        assert_eq!(game.stats().unplayed, 0);
        // Changed, each is tried again: still not loaded, warned of again.
        game.emitter(emitter).params.volume = 0.5;
        game.app
            .world()
            .write::<MusicArea>()
            .get_mut(area)
            .expect("an area")
            .half = Vec2::splat(20.0);
        game.frames(5);
        assert_eq!(warned(), (2, 2, 1));
        assert_eq!(game.stats().active_voices, 0);
    }

    #[test]
    fn the_area_playing_plays_on_when_its_music_changes_to_a_track_that_did_not_load() {
        let mut game = Game::new();
        let tone = game.track("audio/music/tone.qoa", &tone(400.0, 0.25));
        let quiet = game.track("audio/music/quiet.qoa", &[0; RATE as usize]);
        let missing = {
            let mut assets = game.assets();
            let missing = assets.load::<Music>("audio/music/plays-on.qoa");
            assets.finish_all_loads(&mut FinalizeCtx::new());
            missing
        };
        let mut area = MusicArea::new(tone, Vec2::splat(100.0));
        area.crossfade_ms = 100;
        let meadow = game.spawn(Vec2::ZERO, area);
        let out = game.frames(10);
        let full = rms(&out, FRAME - 480, 480);
        let play = |game: &Game, music: Handle<Music>| {
            let mut areas = game.app.world().write::<MusicArea>();
            areas.get_mut(meadow).expect("an area").music = music;
        };
        // Warned of once, the area is the one playing still, and its old
        // track plays on.
        play(&game, missing);
        for _ in 0..10 {
            let out = game.frame();
            assert!((rms(&out, FRAME - 480, 480) / full - 1.0).abs() < 0.02);
            assert_eq!(game.music_area(), Some(meadow));
        }
        assert_eq!(logged(&["plays-on.qoa", "MusicArea"]), 1);
        // Changed again, it sends its new track.
        play(&game, quiet);
        let out = game.frames(10);
        assert!(rms(&out, FRAME - 480, 480) < 1e-3);
        assert_eq!(game.music_area(), Some(meadow));
    }

    /// Emitters, a listener and two areas that move in fixed paths: a
    /// pass of frames is the same each time.
    #[cfg(feature = "dev")]
    mod steady {
        use nadir_core::time::Tick;
        use nadir_ecs::FixedStage;
        use nadir_platform::{AllocCounts, thread_alloc_counts};

        use super::*;

        /// The ticks of a pass: the frames, one tick each.
        const PERIOD: u64 = 120;
        /// How many emitters move about.
        const EMITTERS: usize = 40;

        /// A path: `centre + swing · sin(phase + 2π · tick / PERIOD)`.
        #[derive(Clone, Copy)]
        struct Path {
            centre: Vec2,
            swing: Vec2,
            phase: f32,
        }

        impl Component for Path {}

        /// Puts each entity with a [`Path`] where its path is this tick.
        fn follow(world: &World, tick: &Tick) {
            let turn = (tick.index % PERIOD) as f32 / PERIOD as f32 * TAU;
            let paths = world.read::<Path>();
            let mut transforms = world.write::<Transform>();
            for (entity, path) in paths.iter() {
                if let Some(transform) = transforms.get_mut(entity) {
                    transform.position = path.centre + path.swing * (path.phase + turn).sin();
                }
            }
        }

        fn scene() -> Game {
            let app = App::new(Config::default()).add_fixed_system(
                FixedStage::Gameplay,
                "follow",
                follow,
            );
            let mut game = Game::of(app);
            game.app.world_mut().register::<Path>();
            let hum = game.constant("audio/hum.kpcm", 0.01);
            let buzz = game.constant("audio/buzz.kpcm", 0.01);
            let low = game.track("audio/music/low.qoa", &tone(200.0, 0.25));
            let high = game.track("audio/music/high.qoa", &tone(800.0, 0.25));
            // The listener crosses from the west's area to the east's and
            // back.
            let listener = Path {
                centre: Vec2::ZERO,
                swing: Vec2::new(300.0, 0.0),
                phase: 0.0,
            };
            let start = |path: Path| path.centre + path.swing * path.phase.sin();
            let world = game.app.world_mut();
            world
                .spawn()
                .with(Transform::at(start(listener)))
                .with(listener)
                .with(AudioListener);
            for (x, music) in [(-200.0, low), (200.0, high)] {
                world
                    .spawn()
                    .with(Transform::at(Vec2::new(x, 0.0)))
                    .with(MusicArea::new(music, Vec2::new(200.0, 400.0)));
            }
            // The emitters, in and out of range of it, drawn between ticks.
            for index in 0..EMITTERS {
                let way = Vec2::from_angle(TAU * index as f32 / EMITTERS as f32);
                let path = Path {
                    centre: way * 800.0,
                    swing: way * 250.0,
                    phase: index as f32,
                };
                let sound = if index % 2 == 0 { hum } else { buzz };
                let at = Transform::at(start(path));
                world
                    .spawn()
                    .with(at)
                    .with(PrevTransform(at))
                    .with(path)
                    .with(AudioEmitter::looped(sound));
            }
            game
        }

        /// What a pass did: the fewest and the most voices at the end of a
        /// frame, and how many times the area whose track plays changed.
        #[derive(Debug, Default, PartialEq)]
        struct Seen {
            fewest: u32,
            most: u32,
            changes: u32,
        }

        /// Runs a pass, and gives back what it allocated, and what it did.
        fn pass(game: &mut Game, out: &mut [f32]) -> (AllocCounts, Seen) {
            let mut seen = Seen {
                fewest: u32::MAX,
                ..Seen::default()
            };
            let mut area = game.music_area();
            let before = thread_alloc_counts();
            for _ in 0..PERIOD {
                game.frame_into(TICK, out);
                let voices = game.stats().active_voices;
                seen.fewest = seen.fewest.min(voices);
                seen.most = seen.most.max(voices);
                if game.music_area() != area {
                    seen.changes += 1;
                    area = game.music_area();
                }
            }
            (thread_alloc_counts().since(before), seen)
        }

        #[test]
        fn a_steady_frame_of_moving_emitters_a_listener_and_areas_allocates_nothing() {
            let mut game = scene();
            let mut out = vec![0.0; FRAME * 2];
            let (first, _) = pass(&mut game, &mut out);
            assert_ne!(first, AllocCounts::default(), "the first grows");
            let (_, second) = pass(&mut game, &mut out);
            let (third, seen) = pass(&mut game, &mut out);
            assert_eq!(third, AllocCounts::default(), "the steady pass allocated");
            // Each pass, emitters came into range and went out of it, and
            // the listener crossed from one area to the other and back.
            assert_eq!(seen, second);
            assert!(
                seen.fewest < seen.most && seen.most < EMITTERS as u32,
                "{seen:?}"
            );
            assert_eq!(seen.changes, 2);
            assert_eq!(game.stats().dropped, 0);
        }
    }
}
