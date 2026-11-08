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
//! The tests let go of each sound as soon as it is queued, so that the
//! mixer holds the last reference: a sound dropped on the audio thread
//! would be freed there, and counted.

use std::sync::{Arc, Weak};

use nadir_audio::{AudioConfig, Mixer, MusicData, PlayParams, SoundData};
use nadir_codec::qoa;
use nadir_core::Vec2;
use nadir_platform::thread_alloc_counts;

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

/// A sound, of which the test keeps only a weak reference.
fn sound(
    level: f32,
    channels: u8,
    frames: usize,
    kept: &mut Vec<Weak<SoundData>>,
) -> Arc<SoundData> {
    let samples = vec![level; frames * usize::from(channels)];
    let sound = Arc::new(SoundData::new(channels, 48_000, samples).expect("a valid sound"));
    kept.push(Arc::downgrade(&sound));
    sound
}

fn track(level: i16, frames: u32, rate: u32, kept: &mut Vec<Weak<MusicData>>) -> Arc<MusicData> {
    let header = qoa::Header {
        channels: 2,
        sample_rate: rate,
        samples: frames,
    };
    let file = qoa::encode(&header, &vec![level; frames as usize * 2]).expect("a valid track");
    let track = Arc::new(MusicData::new(file).expect("a valid track"));
    kept.push(Arc::downgrade(&track));
    track
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
    let mut sounds = Vec::new();
    let mut tracks = Vec::new();
    let mut voices = Vec::new();
    let looped = PlayParams {
        looped: true,
        ..PlayParams::default()
    };

    // From the very first callback, with nothing to do.
    counted.stereo(256);

    // More voices than slots: steals, a refusal, a stop of a voice that has
    // only just taken a slot, and sounds that end on their own.
    for (i, priority) in [5u8, 3, 3, 7, 4, 0].into_iter().enumerate() {
        let sound = sound(0.01 * i as f32, 1 + (i % 2) as u8, 2000, &mut sounds);
        let voice = audio.play_at(
            &sound,
            Vec2::new(i as f32 * 100.0, 0.0),
            PlayParams { priority, ..looped },
        );
        voices.push(voice);
        if i == 4 {
            audio.stop(voice, 0);
        }
    }
    counted.stereo(3000);
    // A sound's instances: the oldest gives way.
    let shared = sound(0.02, 1, 30_000, &mut sounds);
    for _ in 0..3 {
        audio.play(
            &shared,
            PlayParams {
                max_instances: 2,
                priority: 255,
                ..PlayParams::default()
            },
        );
        counted.stereo(300);
    }
    drop(shared);
    // A loop shorter than its step; short sounds that end.
    let tiny = sound(0.05, 1, 1, &mut sounds);
    voices.push(audio.play(
        &tiny,
        PlayParams {
            pitch: 16.0,
            priority: 255,
            ..looped
        },
    ));
    drop(tiny);
    for _ in 0..8 {
        let short = sound(0.01, 2, 37, &mut sounds);
        audio.play(
            &short,
            PlayParams {
                priority: 255,
                ..PlayParams::default()
            },
        );
    }
    counted.stereo(2000);

    // Music: across frames and the loop, a crossfade, a third track during
    // it, the first brought back, and a stop to the end of its fade.
    let first = track(4000, 6000, 48_000, &mut tracks);
    audio.play_music(&first, 0);
    counted.stereo(15_000);
    let second = track(-4000, 20_000, 44_100, &mut tracks);
    audio.play_music(&second, 200);
    counted.stereo(2000);
    let third = track(2000, 7000, 22_050, &mut tracks);
    audio.play_music(&third, 200);
    counted.stereo(100);
    audio.play_music(&first, 100);
    counted.stereo(3000);
    drop((second, third));
    audio.stop_music(20);
    counted.stereo(2000);
    audio.play_music(&first, 0);
    drop(first);
    counted.stereo(500);

    // Other rates, channel counts and callback sizes.
    counted.render(4410, 2, 44_100, 441);
    counted.render(5000, 1, 22_050, 4096);
    counted.render(9000, 3, 96_000, 8192);

    // A main thread that does not call `update` for a long while: what the
    // mixer is done with fills the return queue, then its own room, and
    // then it takes no new sound until the main thread catches up.
    for round in 0..20 {
        for _ in 0..30 {
            let short = sound(0.01, 1, 10, &mut sounds);
            audio.play(
                &short,
                PlayParams {
                    priority: (round * 13 % 256) as u8,
                    ..PlayParams::default()
                },
            );
        }
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
    let alive = sounds
        .iter()
        .filter(|sound| sound.strong_count() > 0)
        .count()
        + tracks
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
        sounds.iter().all(|sound| sound.strong_count() == 0),
        "a sound never came back"
    );
    assert!(tracks.iter().all(|track| track.strong_count() == 0));
    // Each a buffer of samples, and the track's file.
    assert!(freed >= alive as u64, "{freed} freed for {alive}");
}

/// What only the mixer holds once it is queued, as a game that lets go of
/// a sound as soon as it plays it: a sound or track the audio thread let
/// go of would be freed there.
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
    let mut sounds = Vec::new();
    let mut tracks = Vec::new();
    let mut voices = Vec::new();
    let short = |counted: &mut Counted, frames| counted.render(frames, 2, 48_000, 64);
    let mut music = |level| track(level, 20_000, 48_000, &mut tracks);

    // A crossfade, then a third track: the quieter voice hands off to it
    // after 2 ms; the same track again changes nothing; and the track
    // handed off goes back with no one else holding it.
    audio.play_music(&music(1000), 0);
    short(&mut counted, 640);
    audio.play_music(&music(-1000), 500);
    short(&mut counted, 640);
    let third = music(2000);
    audio.play_music(&third, 500);
    audio.play_music(&third, 500);
    drop(third);
    short(&mut counted, 640);
    // A fourth while the third fades in, and the music stopped before the
    // handoff to it is done: the fourth never starts.
    audio.play_music(&music(-2000), 500);
    audio.stop_music(10);
    short(&mut counted, 64 * 40);
    // A track brought back from a handoff away from it: the track that was
    // to follow never starts.
    let fifth = music(1500);
    audio.play_music(&fifth, 0);
    short(&mut counted, 640);
    let sixth = music(-1500);
    audio.play_music(&sixth, 500);
    short(&mut counted, 64);
    audio.play_music(&music(500), 500);
    audio.play_music(&sixth, 500);
    drop((fifth, sixth));
    short(&mut counted, 64 * 40);

    // A sound shorter than a steal's fade, ending while the voice it took
    // the place of still fades.
    for _ in 0..4 {
        let long = sound(0.01, 1, 48_000, &mut sounds);
        voices.push(audio.play(
            &long,
            PlayParams {
                priority: 0,
                looped: true,
                ..PlayParams::default()
            },
        ));
    }
    short(&mut counted, 640);
    let brief = sound(0.01, 1, 37, &mut sounds);
    audio.play(
        &brief,
        PlayParams {
            priority: 255,
            ..PlayParams::default()
        },
    );
    drop(brief);
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
    assert!(sounds.iter().all(|sound| sound.strong_count() == 0));
    assert!(tracks.iter().all(|track| track.strong_count() == 0));
}
