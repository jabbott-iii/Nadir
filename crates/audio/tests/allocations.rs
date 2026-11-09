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

//! `Mixer::render` allocates and frees nothing, counted by the platform's
//! `dev` allocation counter (a dev-dependency installs it): through steals,
//! refusals, voices that end, music across its frames, its loop and its
//! crossfades, a change of rate, and a main thread that does not take back
//! what the mixer is done with; and through each place the mixer gives a
//! sound or a track back: a voice ending beside the one it took the place
//! of, a music handoff finished, cancelled by a stop or by the track coming
//! back, and the same track played twice.
//!
//! Each test loads its sounds and tracks in an asset server of its own,
//! plays them from their handles, and unloads each as soon as it is
//! queued, checking that the server has let go of it, so that the mixer
//! holds the last reference: a sound dropped on the audio thread would be
//! freed there, and counted.

mod common;

use std::sync::{Arc, Weak};

use nadir_assets::{Handle, Scope};
use nadir_audio::{AudioConfig, Mixer, Music, MusicData, PlayParams, Sound, SoundData};
use nadir_codec::pcm;
use nadir_core::Vec2;
use nadir_platform::thread_alloc_counts;

use common::Library;

/// The mixer, and what its renders have allocated and freed.
struct Counted {
    mixer: Mixer,
    out: Vec<f32>,
    renders: u32,
    allocations: u64,
    deallocations: u64,
}

impl Counted {
    /// Renders `frames` frames of `channels` channels at `rate`, in
    /// callbacks of `callback` frames, counting each call alone.
    fn render(&mut self, frames: usize, channels: usize, rate: u32, callback: usize) {
        let mut done = 0;
        while done < frames {
            let len = callback.min(frames - done);
            let out = &mut self.out[..len * channels];
            let before = thread_alloc_counts();
            self.mixer.render(out, channels, rate);
            let counted = thread_alloc_counts().since(before);
            self.allocations += counted.allocations;
            self.deallocations += counted.deallocations;
            self.renders += 1;
            done += len;
        }
    }

    fn stereo(&mut self, frames: usize) {
        self.render(frames, 2, 48_000, 256);
    }
}

/// The sounds and tracks a test loads, in a server of its own, of which
/// the test keeps only handles and weak references.
struct Loads {
    lib: Library,
    sounds: Vec<Weak<SoundData>>,
    tracks: Vec<Weak<MusicData>>,
}

impl Loads {
    fn new() -> Self {
        Self {
            lib: Library::new(),
            sounds: Vec::new(),
            tracks: Vec::new(),
        }
    }

    /// A sound at `level`, of `channels` and `frames` at 48 kHz, loaded in
    /// `scope`, and a weak reference to its samples.
    fn sound(
        &mut self,
        scope: Scope,
        level: f32,
        channels: u8,
        frames: usize,
    ) -> (Handle<Sound>, Weak<SoundData>) {
        let samples = vec![pcm::to_i16(level); frames * usize::from(channels)];
        let sound = self.lib.sound_in(scope, channels, 48_000, &samples);
        let weak = Arc::downgrade(self.lib.get(sound).data());
        self.sounds.push(Weak::clone(&weak));
        (sound, weak)
    }

    /// A stereo track at `level`, of `frames` at `rate`, loaded in `scope`,
    /// and a weak reference to it.
    fn track(
        &mut self,
        scope: Scope,
        level: i16,
        frames: u32,
        rate: u32,
    ) -> (Handle<Music>, Weak<MusicData>) {
        let samples = vec![level; frames as usize * 2];
        let track = self.lib.track_in(scope, 2, rate, &samples);
        let weak = Arc::downgrade(self.lib.get_track(track).data());
        self.tracks.push(Weak::clone(&weak));
        (track, weak)
    }

    /// Unloads `scope`, where `loaded` were loaded: the server lets go of
    /// each. Returns how many references to each are left, all of them the
    /// mixer's: one for each play of it the mixer has taken or has queued,
    /// as it frees nothing before `Audio::update` takes them back.
    fn unload<T>(&mut self, scope: Scope, loaded: &[Weak<T>]) -> Vec<usize> {
        self.lib.unload(scope);
        loaded.iter().map(Weak::strong_count).collect()
    }
}

#[test]
fn render_allocates_and_frees_nothing_whatever_it_is_asked() {
    let config = AudioConfig {
        voices: 4,
        commands: 64,
        ..AudioConfig::default()
    };
    let (audio, mixer) = nadir_audio::new(config).unwrap();
    let mut counted = Counted {
        mixer,
        out: vec![0.0; 8192 * 3],
        renders: 0,
        allocations: 0,
        deallocations: 0,
    };
    let mut loads = Loads::new();
    let mut voices = Vec::new();
    let looped = PlayParams {
        looped: true,
        ..PlayParams::default()
    };

    // From the very first callback, with nothing to do.
    counted.stereo(256);

    // More voices than slots: steals, a refusal, a stop of a voice that has
    // only just taken a slot, and sounds that end on their own.
    let mut group = Vec::new();
    for (i, priority) in [5u8, 3, 3, 7, 4, 0].into_iter().enumerate() {
        let (sound, weak) = loads.sound(Scope::Transient, 0.01 * i as f32, 1 + (i % 2) as u8, 2000);
        group.push(weak);
        let voice = audio.play_at(
            &loads.lib.server,
            sound,
            Vec2::new(i as f32 * 100.0, 0.0),
            PlayParams { priority, ..looped },
        );
        voices.push(voice);
        if i == 4 {
            audio.stop(voice, 0);
        }
    }
    assert_eq!(loads.unload(Scope::Transient, &group), [1; 6]);
    counted.stereo(3000);
    // A sound's instances: the oldest gives way.
    let (shared, weak) = loads.sound(Scope::Transient, 0.02, 1, 30_000);
    for _ in 0..3 {
        audio.play(
            &loads.lib.server,
            shared,
            PlayParams {
                max_instances: 2,
                priority: 255,
                ..PlayParams::default()
            },
        );
        counted.stereo(300);
    }
    // Each of its three plays, playing, fading or done.
    assert_eq!(loads.unload(Scope::Transient, &[weak]), [3]);
    // A loop shorter than its step; short sounds that end.
    let (tiny, weak) = loads.sound(Scope::Transient, 0.05, 1, 1);
    voices.push(audio.play(
        &loads.lib.server,
        tiny,
        PlayParams {
            pitch: 16.0,
            priority: 255,
            ..looped
        },
    ));
    assert_eq!(loads.unload(Scope::Transient, &[weak]), [1]);
    let mut group = Vec::new();
    for _ in 0..8 {
        let (short, weak) = loads.sound(Scope::Transient, 0.01, 2, 37);
        group.push(weak);
        audio.play(
            &loads.lib.server,
            short,
            PlayParams {
                priority: 255,
                ..PlayParams::default()
            },
        );
    }
    assert_eq!(loads.unload(Scope::Transient, &group), [1; 8]);
    counted.stereo(2000);

    // Music: across frames and the loop, a crossfade, a third track during
    // it, the first brought back, and a stop to the end of its fade.
    let (first, first_weak) = loads.track(Scope::Level, 4000, 6000, 48_000);
    audio.play_music(&loads.lib.server, first, 0);
    counted.stereo(15_000);
    let (second, second_weak) = loads.track(Scope::Transient, -4000, 20_000, 44_100);
    audio.play_music(&loads.lib.server, second, 200);
    counted.stereo(2000);
    let (third, third_weak) = loads.track(Scope::Transient, 2000, 7000, 22_050);
    audio.play_music(&loads.lib.server, third, 200);
    counted.stereo(100);
    audio.play_music(&loads.lib.server, first, 100);
    counted.stereo(3000);
    assert_eq!(
        loads.unload(Scope::Transient, &[second_weak, third_weak]),
        [1, 1]
    );
    audio.stop_music(20);
    counted.stereo(2000);
    audio.play_music(&loads.lib.server, first, 0);
    // Each of its three plays.
    assert_eq!(loads.unload(Scope::Level, &[first_weak]), [3]);
    counted.stereo(500);

    // Other rates, channel counts and callback sizes.
    counted.render(4410, 2, 44_100, 441);
    counted.render(5000, 1, 22_050, 4096);
    counted.render(9000, 3, 96_000, 8192);

    // A main thread that does not call `update` for a long while: what the
    // mixer is done with fills the return queue, then its own room, and
    // then it takes no new sound until the main thread catches up.
    for round in 0..20 {
        let mut group = Vec::new();
        let mut queued = Vec::new();
        for _ in 0..30 {
            let (short, weak) = loads.sound(Scope::Transient, 0.01, 1, 10);
            group.push(weak);
            let dropped = audio.stats().dropped;
            audio.play(
                &loads.lib.server,
                short,
                PlayParams {
                    priority: (round * 13 % 256) as u8,
                    ..PlayParams::default()
                },
            );
            // A play the full queue turned away was dropped here, on the
            // main thread: the server held the last reference to it.
            queued.push(usize::from(audio.stats().dropped == dropped));
        }
        assert_eq!(
            loads.unload(Scope::Transient, &group),
            queued,
            "round {round}"
        );
        counted.stereo(512);
    }
    assert!(counted.renders > 150, "{} renders", counted.renders);
    assert_eq!(
        (counted.allocations, counted.deallocations),
        (0, 0),
        "render allocated or freed"
    );

    // Back on the main thread: everything comes back, and is freed here,
    // where the counter sees it.
    let alive = loads
        .sounds
        .iter()
        .filter(|sound| sound.strong_count() > 0)
        .count()
        + loads
            .tracks
            .iter()
            .filter(|track| track.strong_count() > 0)
            .count();
    assert!(alive > 0);
    let mut freed = 0;
    let mut catch_up = |counted: &mut Counted| {
        let before = thread_alloc_counts();
        audio.update();
        freed += thread_alloc_counts().since(before).deallocations;
        counted.stereo(512);
    };
    // The queue is still full of plays: they go first.
    for _ in 0..20 {
        catch_up(&mut counted);
    }
    audio.stop_music(0);
    for &voice in &voices {
        audio.stop(voice, 0);
    }
    for _ in 0..20 {
        catch_up(&mut counted);
    }
    assert_eq!(counted.deallocations, 0, "render freed");
    assert!(
        loads.sounds.iter().all(|sound| sound.strong_count() == 0),
        "a sound never came back"
    );
    assert!(loads.tracks.iter().all(|track| track.strong_count() == 0));
    // Each a buffer of samples, and the track's file.
    assert!(freed >= alive as u64, "{freed} freed for {alive}");
}

/// What only the mixer holds once it is queued, as a game that unloads a
/// sound as soon as it plays it: a sound or track the audio thread let go
/// of would be freed there.
#[test]
fn render_frees_nothing_when_the_mixer_holds_the_last_reference() {
    let config = AudioConfig {
        voices: 4,
        ..AudioConfig::default()
    };
    let (audio, mixer) = nadir_audio::new(config).expect("the config is valid");
    let mut counted = Counted {
        mixer,
        out: vec![0.0; 8192 * 2],
        renders: 0,
        allocations: 0,
        deallocations: 0,
    };
    let mut loads = Loads::new();
    let mut voices = Vec::new();
    let short = |counted: &mut Counted, frames| counted.render(frames, 2, 48_000, 64);
    let music = |loads: &mut Loads, level| loads.track(Scope::Transient, level, 20_000, 48_000);

    // A crossfade, then a third track: the quieter voice hands off to it
    // after 2 ms; the same track again changes nothing; and the track
    // handed off goes back with no one else holding it.
    let (track, weak) = music(&mut loads, 1000);
    audio.play_music(&loads.lib.server, track, 0);
    assert_eq!(loads.unload(Scope::Transient, &[weak]), [1]);
    short(&mut counted, 640);
    let (track, weak) = music(&mut loads, -1000);
    audio.play_music(&loads.lib.server, track, 500);
    assert_eq!(loads.unload(Scope::Transient, &[weak]), [1]);
    short(&mut counted, 640);
    let (third, weak) = music(&mut loads, 2000);
    audio.play_music(&loads.lib.server, third, 500);
    audio.play_music(&loads.lib.server, third, 500);
    // Each of its two plays.
    assert_eq!(loads.unload(Scope::Transient, &[weak]), [2]);
    short(&mut counted, 640);
    // A fourth while the third fades in, and the music stopped before the
    // handoff to it is done: the fourth never starts.
    let (track, weak) = music(&mut loads, -2000);
    audio.play_music(&loads.lib.server, track, 500);
    audio.stop_music(10);
    assert_eq!(loads.unload(Scope::Transient, &[weak]), [1]);
    short(&mut counted, 64 * 40);
    // A track brought back from a handoff away from it: the track that was
    // to follow never starts.
    let (fifth, fifth_weak) = music(&mut loads, 1500);
    audio.play_music(&loads.lib.server, fifth, 0);
    short(&mut counted, 640);
    let (sixth, sixth_weak) = music(&mut loads, -1500);
    audio.play_music(&loads.lib.server, sixth, 500);
    short(&mut counted, 64);
    let (track, weak) = music(&mut loads, 500);
    audio.play_music(&loads.lib.server, track, 500);
    audio.play_music(&loads.lib.server, sixth, 500);
    // The sixth's two plays.
    assert_eq!(
        loads.unload(Scope::Transient, &[fifth_weak, sixth_weak, weak]),
        [1, 2, 1]
    );
    short(&mut counted, 64 * 40);

    // A sound shorter than a steal's fade, ending while the voice it took
    // the place of still fades.
    let mut long = Vec::new();
    for _ in 0..4 {
        let (sound, weak) = loads.sound(Scope::Transient, 0.01, 1, 48_000);
        long.push(weak);
        voices.push(audio.play(
            &loads.lib.server,
            sound,
            PlayParams {
                priority: 0,
                looped: true,
                ..PlayParams::default()
            },
        ));
    }
    assert_eq!(loads.unload(Scope::Transient, &long), [1; 4]);
    short(&mut counted, 640);
    let (brief, weak) = loads.sound(Scope::Transient, 0.01, 1, 37);
    audio.play(
        &loads.lib.server,
        brief,
        PlayParams {
            priority: 255,
            ..PlayParams::default()
        },
    );
    assert_eq!(loads.unload(Scope::Transient, &[weak]), [1]);
    short(&mut counted, 640);

    assert_eq!(
        (counted.allocations, counted.deallocations),
        (0, 0),
        "render allocated or freed"
    );
    audio.stop_music(0);
    for &voice in &voices {
        audio.stop(voice, 0);
    }
    for _ in 0..10 {
        short(&mut counted, 512);
        audio.update();
    }
    assert_eq!(counted.deallocations, 0, "render freed");
    assert!(loads.sounds.iter().all(|sound| sound.strong_count() == 0));
    assert!(loads.tracks.iter().all(|track| track.strong_count() == 0));
}
