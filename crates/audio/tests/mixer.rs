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

//! The mixer's offline tests, as `docs/subsystems/audio.md` lists them:
//! sounds and tracks written as the cook writes them, loaded through an
//! asset server and played by their handles, commands in, frames rendered
//! into a buffer, and the frames compared with what they should be. No
//! device: the test calls `Mixer::render` itself, as the platform's audio
//! thread will.

mod common;

use std::f32::consts::SQRT_2;
use std::f64::consts::TAU;
use std::sync::Arc;

use common::{Library, loaded};
use nadir_assets::{Handle, Scope};
use nadir_audio::{
    Audio, AudioConfig, AudioError, Bus, Mixer, Music, MusicData, PlayParams, Sound, SoundData,
    VoiceChange,
};
use nadir_codec::{pcm, qoa};
use nadir_core::Vec2;

const RATE: u32 = 48_000;

/// The samples of the 2 ms every voice fades in over, at [`RATE`].
const FADE_IN: usize = 96;

/// The samples of the 2 ms every voice fades in over, at 44.1 kHz: 88.2,
/// rounded up.
const FADE_IN_44_1: usize = 89;

/// `cos(π/4)`: each side's gain for a mono sound at the centre.
const CENTRE: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn fresh() -> (Audio, Mixer) {
    mixer_with(AudioConfig::default())
}

fn mixer_with(config: AudioConfig) -> (Audio, Mixer) {
    nadir_audio::new(config).expect("a valid config")
}

/// Renders `frames` stereo frames at `rate` in callbacks of `callback`
/// frames.
fn render_at(mixer: &mut Mixer, frames: usize, rate: u32, callback: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    for part in out.chunks_mut(callback * 2) {
        mixer.render(part, 2, rate);
    }
    out
}

/// Renders `frames` stereo frames at [`RATE`] in callbacks of 256 frames.
fn render(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    render_at(mixer, frames, RATE, 256)
}

fn left(out: &[f32]) -> Vec<f32> {
    out.chunks(2).map(|frame| frame[0]).collect()
}

/// A mono sound of `frames` frames at `level`, at [`RATE`], loaded in
/// `scope`: it plays [`loaded`] of `level`.
fn constant_in(lib: &mut Library, scope: Scope, level: f32, frames: usize) -> Handle<Sound> {
    lib.sound_in(scope, 1, RATE, &vec![pcm::to_i16(level); frames])
}

fn looped() -> PlayParams {
    PlayParams {
        looped: true,
        ..PlayParams::default()
    }
}

fn close(a: f32, b: f32, epsilon: f32) -> bool {
    (a - b).abs() <= epsilon
}

#[test]
fn the_pan_law_keeps_a_mono_sound_s_power_and_puts_each_pan_where_it_belongs() {
    let mut lib = Library::new();
    let sound = lib.constant(0.5, 1000);
    let level = loaded(0.5);
    for step in -10..=10 {
        let pan = step as f32 / 10.0;
        let (audio, mut mixer) = fresh();
        audio.play(&lib.server, sound, PlayParams { pan, ..looped() });
        let out = render(&mut mixer, 200);
        let (l, r) = (out[150 * 2], out[150 * 2 + 1]);
        let theta = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        assert!(
            close(l * l + r * r, level * level, 1e-6),
            "pan {pan}: {l}, {r}"
        );
        assert!(close(l, level * theta.cos(), 1e-6), "pan {pan}: {l}");
        assert!(close(r, level * theta.sin(), 1e-6), "pan {pan}: {r}");
        match step {
            -10 => assert!(close(l, level, 1e-6) && close(r, 0.0, 1e-6), "{l}, {r}"),
            0 => assert_eq!(l, r),
            10 => assert!(close(l, 0.0, 1e-6) && close(r, level, 1e-6), "{l}, {r}"),
            _ => assert_eq!(l > r, pan < 0.0, "pan {pan}: {l}, {r}"),
        }
    }
}

#[test]
fn a_stereo_sound_is_balanced_at_unity_and_never_above() {
    let mut lib = Library::new();
    let sound = lib.sound_f32(2, RATE, &[0.5, -0.25].repeat(1000));
    let (left_level, right_level) = (loaded(0.5), loaded(-0.25));
    for step in -10..=10 {
        let pan = step as f32 / 10.0;
        let (audio, mut mixer) = fresh();
        audio.play(&lib.server, sound, PlayParams { pan, ..looped() });
        let out = render(&mut mixer, 200);
        let (l, r) = (out[150 * 2] / left_level, out[150 * 2 + 1] / right_level);
        assert!(l <= 1.0 && r <= 1.0, "pan {pan}: {l}, {r}");
        if pan <= 0.0 {
            assert_eq!(l, 1.0, "pan {pan}: the near side stays as it is");
        }
        if pan >= 0.0 {
            assert_eq!(r, 1.0, "pan {pan}: the near side stays as it is");
        }
        // The far side follows √2 · (cos θ, sin θ).
        let theta = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        if pan > 0.0 {
            assert!(close(l, SQRT_2 * theta.cos(), 1e-6), "pan {pan}: {l}");
        }
        if pan < 0.0 {
            assert!(close(r, SQRT_2 * theta.sin(), 1e-6), "pan {pan}: {r}");
        }
        if pan == -1.0 {
            assert!(close(r, 0.0, 1e-6), "{r}");
        }
        if pan == 1.0 {
            assert!(close(l, 0.0, 1e-6), "{l}");
        }
    }
}

#[test]
fn a_sound_at_a_position_is_panned_and_attenuated_from_the_listener() {
    // The defaults: fully to one side 480 px off, full volume to 64 px,
    // silent from 800.
    let mut lib = Library::new();
    let sound = lib.constant(0.5, 1000);
    let level = loaded(0.5);
    let cases = [
        // The position from a listener at (100, 100), and the gain.
        (Vec2::new(0.0, 0.0), 1.0),
        (Vec2::new(0.0, 64.0), 1.0),
        (Vec2::new(0.0, 432.0), 0.5),
        (Vec2::new(-240.0, -320.0), (800.0 - 400.0) / 736.0),
        (Vec2::new(0.0, 800.0), 0.0),
        (Vec2::new(1e4, 0.0), 0.0),
        (Vec2::new(-480.0, 0.0), (800.0 - 480.0) / 736.0),
        (Vec2::new(600.0, 0.0), (800.0 - 600.0) / 736.0),
    ];
    for (offset, gain) in cases {
        let (audio, mut mixer) = fresh();
        let listener = Vec2::new(100.0, 100.0);
        audio.set_listener(listener);
        audio.play_at(&lib.server, sound, listener + offset, looped());
        let out = render(&mut mixer, 200);
        let (l, r) = (out[150 * 2], out[150 * 2 + 1]);
        let pan = (offset.x / 480.0).clamp(-1.0, 1.0);
        let theta = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        assert!(
            close(l, level * gain * theta.cos(), 1e-6),
            "{offset:?}: {l}"
        );
        assert!(
            close(r, level * gain * theta.sin(), 1e-6),
            "{offset:?}: {r}"
        );
        if offset.x < 0.0 && gain > 0.0 {
            assert!(
                l > r,
                "{offset:?}: a sound to the left is louder on the left"
            );
        }
    }
}

#[test]
fn a_voice_at_a_position_that_is_not_finite_is_silent() {
    let mut lib = Library::new();
    let sound = lib.constant(0.5, 1000);
    let level = loaded(0.5);
    for position in [
        Vec2::new(f32::NAN, 0.0),
        Vec2::new(0.0, f32::INFINITY),
        Vec2::new(f32::NEG_INFINITY, f32::NAN),
    ] {
        let (audio, mut mixer) = fresh();
        audio.play_at(&lib.server, sound, position, looped());
        assert!(
            render(&mut mixer, 300).iter().all(|&x| x == 0.0),
            "{position:?}"
        );
        // Nor does a listener that is not finite move.
        let voice = audio.play_at(&lib.server, sound, Vec2::ZERO, looped());
        audio.set_listener(Vec2::new(f32::NAN, 0.0));
        let out = render(&mut mixer, 300);
        assert!(close(out[299 * 2], level * CENTRE, 1e-6), "{position:?}");
        audio.set_voice(voice, VoiceChange::Position(position));
        let out = render(&mut mixer, 2000);
        assert!(out[1500 * 2..].iter().all(|&x| x == 0.0), "{position:?}");
    }
}

#[test]
fn a_move_reaches_the_voice_over_16_ms_whatever_the_callback() {
    let mut lib = Library::new();
    let sound = lib.constant(0.5, 1000);
    let level = loaded(0.5);
    for callback in [16, 100, 4096] {
        let (audio, mut mixer) = fresh();
        let voice = audio.play_at(&lib.server, sound, Vec2::ZERO, looped());
        let before = render_at(&mut mixer, 1024, RATE, callback);
        assert!(close(before[1023 * 2], level * CENTRE, 1e-6));
        // Fully right, 480 px away: left silent.
        audio.set_voice(voice, VoiceChange::Position(Vec2::new(480.0, 0.0)));
        let after = left(&render_at(&mut mixer, 2000, RATE, callback));
        // 16 ms at 48 kHz.
        let smooth = 768;
        for (k, &l) in after.iter().enumerate().take(smooth) {
            let expected = level * CENTRE * (1.0 - (k + 1) as f32 / smooth as f32);
            assert!(
                close(l, expected, 1e-5),
                "callback {callback}, {k}: {l} not {expected}"
            );
        }
        assert!(after[smooth - 1..].iter().all(|&l| close(l, 0.0, 1e-6)));
    }
}

#[test]
fn a_voice_fades_in_over_2_ms_and_out_over_its_fade_and_then_ends() {
    let (audio, mut mixer) = fresh();
    let mut lib = Library::new();
    let sound = lib.constant(0.5, 100_000);
    let level = loaded(0.5);
    let voice = audio.play(&lib.server, sound, PlayParams::default());
    let start = render(&mut mixer, 1000);
    for (k, frame) in start.chunks(2).enumerate() {
        let fade = ((k + 1) as f32 / FADE_IN as f32).min(1.0);
        for &x in frame {
            assert!(close(x, level * CENTRE * fade, 1e-6), "{k}: {frame:?}");
        }
    }
    // 100 ms: 4,800 samples.
    audio.stop(voice, 100);
    let out = render(&mut mixer, 6000);
    for (k, frame) in out.chunks(2).enumerate() {
        let fade = (1.0 - (k + 1) as f32 / 4800.0).max(0.0);
        for &x in frame {
            assert!(close(x, level * CENTRE * fade, 1e-6), "{k}: {frame:?}");
        }
    }
    let stop = left(&out);
    assert_eq!(stop[4799], 0.0, "the fade lands on 0");
    assert_eq!(audio.stats().active_voices, 0);
    // A stop of 0 ms takes 2.
    let voice = audio.play(&lib.server, sound, PlayParams::default());
    render(&mut mixer, 1000);
    audio.stop(voice, 0);
    let stop = left(&render(&mut mixer, 200));
    for (k, &l) in stop.iter().enumerate() {
        let fade = (1.0 - (k + 1) as f32 / FADE_IN as f32).max(0.0);
        assert!(close(l, level * CENTRE * fade, 1e-6), "{k}: {l}");
    }
}

#[test]
fn a_bus_s_volume_ramps_and_multiplies_each_voice_s() {
    let (audio, mut mixer) = fresh();
    let mut lib = Library::new();
    let sound = lib.constant(0.25, 1000);
    let level = loaded(0.25);
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            bus: Bus::Ui,
            volume: 0.5,
            ..looped()
        },
    );
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            bus: Bus::Sfx,
            pan: -1.0,
            ..looped()
        },
    );
    render(&mut mixer, 1000);
    // 50 ms: 2,400 samples, from 1 to 0.4, on the Ui bus only.
    audio.set_bus_volume(Bus::Ui, 0.4, 50);
    let out = render(&mut mixer, 3000);
    for k in 0..3000 {
        let bus = 1.0 - 0.6 * ((k + 1) as f32 / 2400.0).min(1.0);
        let (l, r) = (out[k * 2], out[k * 2 + 1]);
        // The Sfx voice is all on the left; the Ui voice at the centre.
        assert!(close(r, level * 0.5 * CENTRE * bus, 1e-6), "{k}: {r}");
        assert!(
            close(l, level + level * 0.5 * CENTRE * bus, 1e-6),
            "{k}: {l}"
        );
    }
    assert!(close(out[2999 * 2 + 1], level * 0.5 * 0.4 * CENTRE, 1e-7));
}

/// Plays sounds whose levels are powers of two, so that the mix says which
/// play: bit `i` of the level, in 1,024ths, is sound `i`'s. Each level is
/// a whole number of 16-bit steps, so that it loads as it is.
struct Marked {
    lib: Library,
    sounds: Vec<Handle<Sound>>,
}

impl Marked {
    fn new(count: usize) -> Self {
        let mut lib = Library::new();
        let sounds = (0..count)
            .map(|i| {
                let level = (1 << i) as f32 / 1024.0;
                assert_eq!(loaded(level), level);
                lib.constant(level, 1000)
            })
            .collect();
        Self { lib, sounds }
    }

    fn play(&self, audio: &Audio, i: usize, priority: u8) -> nadir_audio::VoiceId {
        audio.play(
            &self.lib.server,
            self.sounds[i],
            PlayParams {
                priority,
                pan: -1.0,
                ..looped()
            },
        )
    }

    /// The sounds playing at the end of `out`, past any fade.
    fn playing(out: &[f32]) -> u32 {
        let level = out[out.len() - 2] * 1024.0;
        assert!(
            close(level, level.round(), 1e-3),
            "{level}: a fade not done"
        );
        level.round() as u32
    }
}

#[test]
fn a_new_voice_steals_the_lowest_priority_then_the_oldest_or_does_not_play() {
    let (audio, mut mixer) = mixer_with(AudioConfig {
        voices: 4,
        ..AudioConfig::default()
    });
    let marked = Marked::new(10);
    for (i, priority) in [5, 3, 3, 7].into_iter().enumerate() {
        marked.play(&audio, i, priority);
    }
    assert_eq!(Marked::playing(&render(&mut mixer, 1000)), 0b1111);
    // Priority 4 takes the older voice of priority 3.
    marked.play(&audio, 4, 4);
    let out = render(&mut mixer, 1000);
    assert_eq!(Marked::playing(&out), 0b1_1101);
    // The stolen voice fades out over 5 ms, the new one in over 2.
    for k in 0..400 {
        let new = ((k + 1) as f32 / FADE_IN as f32).min(1.0);
        let old = (1.0 - (k + 1) as f32 / 240.0).max(0.0);
        let expected = (0b1101 as f32 + 2.0 * old + 16.0 * new) / 1024.0;
        assert!(close(out[k * 2], expected, 1e-6), "{k}: {}", out[k * 2]);
    }
    assert_eq!(audio.stats().active_voices, 4);
    // Priority 3 takes the other voice of priority 3.
    marked.play(&audio, 5, 3);
    assert_eq!(Marked::playing(&render(&mut mixer, 1000)), 0b11_1001);
    // Priority 2 finds none as low, and does not play.
    marked.play(&audio, 6, 2);
    assert_eq!(Marked::playing(&render(&mut mixer, 1000)), 0b11_1001);
    // Priority 9 takes the lowest, of priority 3.
    marked.play(&audio, 7, 9);
    assert_eq!(Marked::playing(&render(&mut mixer, 1000)), 0b1001_1001);
    // Priority 200 takes the lowest, voice 4 of priority 4.
    let fading = marked.play(&audio, 8, 200);
    assert_eq!(Marked::playing(&render(&mut mixer, 1000)), 0b1_1000_1001);
    // A voice fading out goes first, whatever its priority: priority 0
    // takes voice 8, of priority 200, as it fades.
    audio.stop(fading, 10_000);
    render(&mut mixer, 100);
    marked.play(&audio, 9, 0);
    let out = render(&mut mixer, 1000);
    assert_eq!(
        Marked::playing(&out),
        0b10_1000_1001,
        "voice 8, fading out, gave its place to voice 9"
    );
}

#[test]
fn a_sound_plays_as_many_times_at_once_as_its_max_instances_the_oldest_giving_way() {
    // A rising ramp, a 16-bit step a frame: what a voice plays says how
    // long it has played.
    let ramp: Vec<i16> = (0..=i16::MAX).collect();
    let mut lib = Library::new();
    let sound = lib.sound(1, RATE, &ramp);
    let params = PlayParams {
        max_instances: 2,
        pan: -1.0,
        ..PlayParams::default()
    };
    let (audio, mut mixer) = fresh();
    audio.play(&lib.server, sound, params);
    render(&mut mixer, 512);
    audio.play(&lib.server, sound, params);
    render(&mut mixer, 512);
    audio.play(&lib.server, sound, params);
    let out = left(&render(&mut mixer, 1024));
    // The first gave way: the second has played 512 frames more than the
    // third.
    let t = 1023;
    let expected = ((t + 512) + t) as f32 / 32768.0;
    assert!(close(out[t], expected, 1e-6), "{} not {expected}", out[t]);
    assert_eq!(audio.stats().active_voices, 2);
    // 0 counts as 1.
    let (audio, mut mixer) = fresh();
    for _ in 0..3 {
        audio.play(
            &lib.server,
            sound,
            PlayParams {
                max_instances: 0,
                ..params
            },
        );
        render(&mut mixer, 512);
    }
    assert_eq!(audio.stats().active_voices, 1);
}

/// `cycles` periods of a cosine at `amplitude` in `len` frames.
fn cosine(cycles: usize, len: usize, amplitude: f64) -> Vec<f32> {
    (0..len)
        .map(|n| (amplitude * (TAU * (cycles * n) as f64 / len as f64).cos()) as f32)
        .collect()
}

/// The source position of output frame `m` for a step through the source
/// of `ratio` frames a frame: in 32.32, as the mixer counts.
fn position(m: usize, ratio: f64) -> f64 {
    let step = (ratio * 4_294_967_296.0).round();
    m as f64 * step / 4_294_967_296.0
}

#[test]
fn a_looped_sound_passes_its_loop_point_without_a_click() {
    // Ten periods of a cosine, which is 0.5 at the loop point, in 441
    // frames at 48 kHz, played at 44.1 kHz at pitch 1.37: the loop is never
    // a whole number of steps.
    let mut lib = Library::new();
    let sound = lib.sound_f32(1, RATE, &cosine(10, 441, 0.5));
    assert_ne!(
        lib.get(sound).data().sample_rate(),
        44_100,
        "the mixer resamples it"
    );
    let (audio, mut mixer) = fresh();
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            pitch: 1.37,
            pan: -1.0,
            ..looped()
        },
    );
    let out = left(&render_at(&mut mixer, 10_000, 44_100, 256));
    let ratio = 48_000.0 / 44_100.0 * f64::from(1.37f32);
    let mut largest = 0.0f32;
    for m in FADE_IN_44_1..out.len() {
        // The cosine, continuous across the loop. Its 16 bits are within
        // 2^-16 of it, well inside linear interpolation's error.
        let expected = 0.5 * (TAU * 10.0 * position(m, ratio) / 441.0).cos();
        let error = (f64::from(out[m]) - expected).abs();
        assert!(error < 2e-3, "{m}: {} not {expected}", out[m]);
        largest = largest.max((out[m] - out[m - 1]).abs());
    }
    // No step between samples larger than the cosine's own, give or take.
    let slope = 0.5 * TAU * 10.0 * ratio / 441.0;
    assert!(f64::from(largest) < 1.1 * slope, "{largest} > {slope}");
}

#[test]
fn a_looped_sound_shorter_than_its_step_still_loops() {
    let mut lib = Library::new();
    let sound = lib.sound_f32(1, RATE, &[0.25]);
    let (audio, mut mixer) = fresh();
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            pitch: 16.0,
            pan: -1.0,
            ..looped()
        },
    );
    let out = left(&render(&mut mixer, 1000));
    assert!(out[FADE_IN..].iter().all(|&l| l == loaded(0.25)));
}

#[test]
fn a_sine_resampled_from_48_khz_to_44_1_keeps_its_frequency_and_amplitude() {
    let sine: Vec<f32> = (0..48_000)
        .map(|n| (0.5 * (TAU * 1000.0 * n as f64 / 48_000.0).sin()) as f32)
        .collect();
    let mut lib = Library::new();
    let sound = lib.sound_f32(1, RATE, &sine);
    assert_ne!(
        lib.get(sound).data().sample_rate(),
        44_100,
        "the mixer resamples it"
    );
    let (audio, mut mixer) = fresh();
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            pan: -1.0,
            ..PlayParams::default()
        },
    );
    // A second of it: 44,100 frames out.
    let out = left(&render_at(&mut mixer, 44_100, 44_100, 256));
    let span = &out[FADE_IN_44_1..43_100];
    let crossings = span
        .windows(2)
        .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
        .count();
    let expected = 2.0 * 1000.0 * span.len() as f64 / 44_100.0;
    assert!(
        (crossings as f64 - expected).abs() <= 2.0,
        "{crossings} zero crossings, not {expected}"
    );
    let peak = span.iter().fold(0.0f32, |peak, &x| peak.max(x.abs()));
    let db = 20.0 * (f64::from(peak) / 0.5).log10();
    assert!(db.abs() <= 0.5, "peak {peak}: {db} dB");
    // Interpolated, not the nearest sample: within linear
    // interpolation's error of the sine itself, which its 16 bits are
    // within 2^-16 of.
    for (m, &x) in out.iter().enumerate().take(43_100).skip(FADE_IN_44_1) {
        let expected = 0.5 * (TAU * 1000.0 * position(m, 48_000.0 / 44_100.0) / 48_000.0).sin();
        assert!(
            (f64::from(x) - expected).abs() < 2e-3,
            "{m}: {x} not {expected}"
        );
    }
}

#[test]
fn a_change_of_rate_keeps_a_voice_s_frequency() {
    let sine: Vec<f32> = (0..48)
        .map(|n| (0.5 * (TAU * n as f64 / 48.0).sin()) as f32)
        .collect();
    let mut lib = Library::new();
    let sound = lib.sound_f32(1, RATE, &sine);
    let (audio, mut mixer) = fresh();
    audio.play(&lib.server, sound, looped());
    let crossings = |out: &[f32]| {
        left(out)
            .windows(2)
            .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
            .count()
    };
    let first = render_at(&mut mixer, 24_000, 48_000, 480);
    let second = render_at(&mut mixer, 22_050, 44_100, 441);
    // Half a second each: 1,000 crossings of a 1 kHz tone.
    assert!(
        crossings(&first).abs_diff(1000) <= 2,
        "{}",
        crossings(&first)
    );
    assert!(
        crossings(&second).abs_diff(1000) <= 2,
        "{}",
        crossings(&second)
    );
    assert_eq!(audio.stats().rate, 44_100);
}

/// A QOA track of `frames` frames of `channels` channels at `rate`: a mix
/// of tones below −6 dBFS, loaded, and the samples its file decodes to.
fn track(lib: &mut Library, channels: u8, rate: u32, frames: u32) -> (Handle<Music>, Vec<i16>) {
    let samples: Vec<i16> = (0..frames)
        .flat_map(|n| {
            (0..channels).map(move |c| {
                let t = f64::from(n) / f64::from(rate);
                let x = 0.3 * (TAU * 220.0 * f64::from(c + 1) * t).sin()
                    + 0.15 * (TAU * 1375.0 * t).sin();
                (x * 32767.0) as i16
            })
        })
        .collect();
    let music = lib.track(channels, rate, &samples);
    (music, decoded(lib, music))
}

/// The samples the file of the track under `music` decodes to.
fn decoded(lib: &Library, music: Handle<Music>) -> Vec<i16> {
    let file = lib.get_track(music).data().bytes();
    qoa::decode(file, nadir_codec::Limits::default())
        .expect("the file loaded")
        .samples
}

#[test]
fn music_plays_the_samples_qoa_decodes_across_its_frames_and_its_loop() {
    let mut lib = Library::new();
    for channels in [1u8, 2] {
        // Two frames and part of a third, played past the end.
        let frames = 2 * qoa::FRAME_LEN as u32 + 777;
        let (music, decoded) = track(&mut lib, channels, RATE, frames);
        let (audio, mut mixer) = fresh();
        audio.play_music(&lib.server, music, 0);
        let out = render(&mut mixer, 30_000);
        let channels = usize::from(channels);
        for m in 0..30_000 {
            let n = m % frames as usize;
            // From the first frame, through its fade-in, on both sides.
            let fade = ((m + 1) as f32 / FADE_IN as f32).min(1.0);
            for side in 0..2 {
                let sample = f32::from(decoded[n * channels + side.min(channels - 1)]) / 32768.0;
                let expected = sample * fade;
                assert_eq!(
                    out[m * 2 + side],
                    expected,
                    "{channels} channels, frame {m}"
                );
            }
        }
    }
}

#[test]
fn music_resampled_from_44_1_khz_interpolates_across_frames_and_its_loop() {
    // The last frame holds 1 sample, which a step of 0.92 can land past.
    let frames = 2 * qoa::FRAME_LEN as u32 + 1;
    let mut lib = Library::new();
    let (music, decoded) = track(&mut lib, 2, 44_100, frames);
    assert_ne!(
        lib.get_track(music).data().header().sample_rate,
        RATE,
        "a track keeps its rate: the mixer resamples it"
    );
    let (audio, mut mixer) = fresh();
    audio.play_music(&lib.server, music, 0);
    let out = render(&mut mixer, 40_000);
    let step = (44_100.0 / 48_000.0 * 4_294_967_296.0f64).round() as u64;
    let len = u64::from(frames);
    for m in FADE_IN..40_000 {
        let at = m as u64 * step;
        let (index, t) = (
            (at >> 32) % len,
            (at & 0xffff_ffff) as f32 / 4_294_967_296.0,
        );
        for side in 0..2 {
            let a = f32::from(decoded[index as usize * 2 + side]);
            let b = f32::from(decoded[((index + 1) % len) as usize * 2 + side]);
            let expected = (a * (1.0 - t) + b * t) / 32768.0;
            assert!(
                close(out[m * 2 + side], expected, 1e-6),
                "frame {m}: {} not {expected}",
                out[m * 2 + side]
            );
        }
    }
}

/// A track of a level that stays the same, which QOA keeps nearly as it
/// is, loaded in `scope`.
fn level_in(lib: &mut Library, scope: Scope, value: f32, frames: u32) -> Handle<Music> {
    let sample = (value * 32768.0) as i16;
    lib.track_in(scope, 1, RATE, &vec![sample; frames as usize])
}

/// A track of a level that stays the same, loaded for good.
fn level(lib: &mut Library, value: f32, frames: u32) -> Handle<Music> {
    level_in(lib, Scope::Global, value, frames)
}

#[test]
fn play_music_crossfades_linearly_from_one_track_to_the_next() {
    let mut lib = Library::new();
    let (a, b) = (
        level(&mut lib, 0.25, 20_000),
        level(&mut lib, -0.125, 20_000),
    );
    let (audio, mut mixer) = fresh();
    audio.play_music(&lib.server, a, 0);
    render(&mut mixer, 1000);
    // 100 ms: 4,800 samples.
    audio.play_music(&lib.server, b, 100);
    let out = left(&render(&mut mixer, 6000));
    for (k, &l) in out.iter().enumerate() {
        let t = ((k + 1) as f32 / 4800.0).min(1.0);
        let expected = 0.25 * (1.0 - t) - 0.125 * t;
        assert!(close(l, expected, 2e-3), "{k}: {l} not {expected}");
    }
    // The same track again changes nothing.
    audio.play_music(&lib.server, b, 100);
    let out = left(&render(&mut mixer, 1000));
    assert!(out.iter().all(|&l| close(l, -0.125, 2e-3)));
}

#[test]
fn a_track_still_sounding_fades_back_in_rather_than_starting_again() {
    // Longer than the test, so that neither loops: a QOA file's first
    // samples, which its predictor has yet to learn, are not quite the
    // level.
    let mut lib = Library::new();
    let (a, b) = (
        level(&mut lib, 0.3, 200_000),
        level(&mut lib, -0.3, 200_000),
    );
    let (audio, mut mixer) = fresh();
    audio.play_music(&lib.server, a, 0);
    render(&mut mixer, 1000);
    // Halfway through a crossfade of a second, back to the first track:
    // it rises from where it is, and nothing restarts.
    audio.play_music(&lib.server, b, 1000);
    let half = left(&render(&mut mixer, 24_000));
    assert!(close(half[23_999], 0.0, 3e-3), "{}", half[23_999]);
    audio.play_music(&lib.server, a, 1000);
    let back = left(&render(&mut mixer, 60_000));
    let mut last = half[23_999];
    for (k, &l) in back.iter().enumerate() {
        assert!(l >= last - 1e-3, "{k}: {l} after {last}: a dip");
        last = l;
    }
    assert!(close(back[59_999], 0.3, 3e-3), "{}", back[59_999]);
    // Stopped, then played again: one copy, fading back in from where its
    // fade-out had reached.
    audio.stop_music(3000);
    let fading = left(&render(&mut mixer, 48_000));
    assert!(close(fading[47_999], 0.2, 3e-3), "{}", fading[47_999]);
    audio.play_music(&lib.server, a, 1000);
    let again = left(&render(&mut mixer, 60_000));
    let mut last = fading[47_999];
    for (k, &l) in again.iter().enumerate() {
        assert!(l >= last - 1e-3 && l <= 0.3 + 3e-3, "{k}: {l} after {last}");
        last = l;
    }
}

#[test]
fn stop_music_fades_the_track_out_and_ends_it() {
    let mut lib = Library::new();
    let a = level_in(&mut lib, Scope::Transient, 0.3, 50_000);
    let track = Arc::downgrade(lib.get_track(a).data());
    let (audio, mut mixer) = fresh();
    audio.play_music(&lib.server, a, 0);
    // The server lets go of it: the play holds the last reference.
    lib.unload(Scope::Transient);
    assert_eq!(track.strong_count(), 1, "only the queue holds the track");
    render(&mut mixer, 1000);
    audio.stop_music(50);
    let out = left(&render(&mut mixer, 3000));
    for (k, &l) in out.iter().enumerate() {
        let expected = 0.3 * (1.0 - (k + 1) as f32 / 2400.0).max(0.0);
        assert!(close(l, expected, 2e-3), "{k}: {l} not {expected}");
    }
    render(&mut mixer, 256);
    audio.update();
    assert_eq!(track.strong_count(), 0, "the track came back to be dropped");
}

#[test]
fn the_limiter_keeps_the_master_below_full_scale_and_quiet_mixes_as_they_are() {
    let (audio, mut mixer) = fresh();
    let mut lib = Library::new();
    for i in 0..48 {
        let sound = lib.constant(if i % 2 == 0 { 1.0 } else { 0.999 }, 1000);
        audio.play(
            &lib.server,
            sound,
            PlayParams {
                volume: f32::MAX,
                max_instances: 255,
                ..looped()
            },
        );
    }
    audio.set_bus_volume(Bus::Sfx, f32::INFINITY, 0);
    let out = render(&mut mixer, 2000);
    assert!(out.iter().all(|x| x.is_finite() && x.abs() < 1.0));
    assert!(out[1999 * 2] > 0.999);
    // Below −1 dBFS, untouched.
    let (audio, mut mixer) = fresh();
    let sound = lib.constant(0.89, 1000);
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            pan: -1.0,
            ..looped()
        },
    );
    assert_eq!(render(&mut mixer, 200)[199 * 2], loaded(0.89));
}

#[test]
fn what_the_mixer_is_done_with_comes_back_to_be_dropped_round_after_round() {
    // Room for 2 · (2 + 2) + 4 = 12: five rounds of 12 sounds each.
    let (audio, mut mixer) = mixer_with(AudioConfig {
        voices: 2,
        commands: 4,
        ..AudioConfig::default()
    });
    let mut lib = Library::new();
    for round in 0..5 {
        let sounds: Vec<_> = (0..12)
            .map(|_| constant_in(&mut lib, Scope::Transient, 0.1, 10))
            .collect();
        let held: Vec<_> = sounds
            .iter()
            .map(|&sound| Arc::downgrade(lib.get(sound).data()))
            .collect();
        for four in sounds.chunks(4) {
            for &sound in four {
                audio.play(&lib.server, sound, PlayParams::default());
            }
            render(&mut mixer, 256);
        }
        // The server lets go of them: only the mixer holds them, or what
        // it hands back.
        lib.unload(Scope::Transient);
        for data in &held {
            assert_eq!(data.strong_count(), 1, "round {round}: held once");
        }
        render(&mut mixer, 256);
        audio.update();
        for data in &held {
            assert_eq!(data.strong_count(), 0, "round {round}");
        }
    }
}

#[test]
fn a_full_queue_drops_changes_first_and_then_everything() {
    let (audio, mut mixer) = mixer_with(AudioConfig {
        commands: 8,
        ..AudioConfig::default()
    });
    let mut lib = Library::new();
    let sound = lib.constant(0.1, 10_000);
    // The references to its samples: the server's, and the plays'.
    let held = |lib: &Library| Arc::strong_count(lib.get(sound).data());
    for _ in 0..6 {
        audio.set_listener(Vec2::ZERO);
    }
    assert_eq!(audio.stats().dropped, 2, "changes keep to half the queue");
    for _ in 0..6 {
        audio.play(&lib.server, sound, PlayParams::default());
    }
    assert_eq!(audio.stats().dropped, 4);
    assert_eq!(
        held(&lib),
        1 + 4,
        "the server's and the four queued: the dropped plays let go of it"
    );
    render(&mut mixer, 256);
    assert_eq!(audio.stats().active_voices, 4);
    // With the mixer gone, nothing is queued, and nothing is held but by
    // the server.
    drop(mixer);
    audio.play(&lib.server, sound, PlayParams::default());
    assert_eq!(held(&lib), 1, "a command was queued");
    audio.update();
    assert_eq!(audio.stats().dropped, 4);
    assert_eq!(held(&lib), 1);
}

#[test]
fn render_fills_one_channel_with_the_mean_and_more_with_silence() {
    let mut lib = Library::new();
    let sound = lib.sound_f32(2, RATE, &[0.5, -0.25].repeat(1000));
    let (l, r) = (loaded(0.5), loaded(-0.25));
    for channels in [1, 3, 6] {
        let (audio, mut mixer) = fresh();
        audio.play(&lib.server, sound, looped());
        // Two whole frames short of 200, and a part of a frame.
        let mut out = vec![1.0; 200 * channels + 1];
        mixer.render(&mut out, channels, RATE);
        let frame = &out[150 * channels..151 * channels];
        if channels == 1 {
            assert!(close(frame[0], (l + r) / 2.0, 1e-6), "{frame:?}");
        } else {
            assert!(close(frame[0], l, 1e-6) && close(frame[1], r, 1e-6));
            assert!(frame[2..].iter().all(|&x| x == 0.0), "{frame:?}");
        }
        if channels > 1 {
            assert_eq!(out[200 * channels], 0.0, "the part of a frame is silent");
        }
    }
    // No channels, or a rate of 0: silence, and nothing moves.
    let (audio, mut mixer) = fresh();
    audio.play(&lib.server, sound, PlayParams::default());
    let mut out = vec![1.0; 16];
    mixer.render(&mut out, 0, RATE);
    mixer.render(&mut out, 2, 0);
    assert!(out.iter().all(|&x| x == 0.0));
    // The voice had not started: its first frame is the first of its fade.
    mixer.render(&mut out[..2], 2, RATE);
    assert_eq!(out[..2], [l / FADE_IN as f32, r / FADE_IN as f32]);
}

#[test]
fn a_callback_larger_than_the_mixer_s_parts_is_mixed_in_parts() {
    let mut lib = Library::new();
    let sound = lib.constant(0.5, 100_000);
    let (audio, mut mixer) = fresh();
    audio.play(&lib.server, sound, PlayParams::default());
    let whole = render_at(&mut mixer, 20_000, RATE, 20_000);
    let (audio, mut mixer) = fresh();
    audio.play(&lib.server, sound, PlayParams::default());
    let parts = render_at(&mut mixer, 20_000, RATE, 100);
    assert_eq!(whole, parts);
}

#[test]
fn sounds_and_tracks_are_refused_when_the_mixer_cannot_play_them() {
    let cases = [
        (
            SoundData::new(3, RATE, vec![0.0; 3]),
            AudioError::Channels { found: 3 },
        ),
        (
            SoundData::new(0, RATE, vec![]),
            AudioError::Channels { found: 0 },
        ),
        (
            SoundData::new(1, 0, vec![0.0]),
            AudioError::SampleRate { found: 0 },
        ),
        (
            SoundData::new(1, 384_001, vec![0.0]),
            AudioError::SampleRate { found: 384_001 },
        ),
        (SoundData::new(2, RATE, vec![]), AudioError::NoSamples),
        (
            SoundData::new(2, RATE, vec![0.0; 3]),
            AudioError::PartialFrame {
                samples: 3,
                channels: 2,
            },
        ),
        (
            SoundData::new(1, RATE, vec![0.0, 1.0, -1.0, 1.0001]),
            AudioError::OutOfRange { index: 3 },
        ),
        (
            SoundData::new(1, RATE, vec![f32::NAN]),
            AudioError::OutOfRange { index: 0 },
        ),
    ];
    for (result, error) in cases {
        assert_eq!(result, Err(error));
    }
    assert!(SoundData::new(2, 384_000, vec![-1.0, 1.0]).is_ok());
    let header = |channels, sample_rate| qoa::Header {
        channels,
        sample_rate,
        samples: 10,
    };
    let three = qoa::encode(&header(3, RATE), &[0; 30]).unwrap();
    assert_eq!(
        MusicData::new(three),
        Err(AudioError::Channels { found: 3 })
    );
    let fast = qoa::encode(&header(1, 500_000), &[0; 10]).unwrap();
    assert_eq!(
        MusicData::new(fast),
        Err(AudioError::SampleRate { found: 500_000 })
    );
    let mut damaged = qoa::encode(&header(1, RATE), &[0; 10]).unwrap();
    // The frame's channel count.
    damaged[8] = 2;
    assert!(matches!(
        MusicData::new(damaged),
        Err(AudioError::Decode(_))
    ));
    assert!(matches!(
        MusicData::new(b"qoaf".to_vec()),
        Err(AudioError::Decode(_))
    ));
}

#[test]
fn a_config_out_of_range_is_refused() {
    let base = AudioConfig::default();
    let cases = [
        (AudioConfig { voices: 0, ..base }, "voices"),
        (
            AudioConfig {
                voices: 1025,
                ..base
            },
            "voices",
        ),
        (
            AudioConfig {
                commands: 1,
                ..base
            },
            "commands",
        ),
        (
            AudioConfig {
                pan_width: 0.0,
                ..base
            },
            "pan_width",
        ),
        (
            AudioConfig {
                pan_width: f32::INFINITY,
                ..base
            },
            "pan_width",
        ),
        (
            AudioConfig {
                min_dist: -1.0,
                ..base
            },
            "min_dist",
        ),
        (
            AudioConfig {
                min_dist: f32::NAN,
                ..base
            },
            "min_dist",
        ),
        (
            AudioConfig {
                max_dist: 64.0,
                ..base
            },
            "max_dist",
        ),
        (
            AudioConfig {
                max_dist: f32::INFINITY,
                ..base
            },
            "max_dist",
        ),
    ];
    for (config, name) in cases {
        match nadir_audio::new(config) {
            Err(AudioError::Config { field, .. }) => assert_eq!(field, name),
            other => panic!("{name}: {other:?}"),
        }
    }
}

#[test]
fn errors_say_what_is_wrong() {
    let cases = [
        (
            AudioError::Config {
                field: "voices",
                expected: "from 1 to 1024",
            },
            "the audio config's `voices` must be from 1 to 1024",
        ),
        (
            AudioError::Channels { found: 3 },
            "a sound of 3 channels: the mixer plays 1 or 2",
        ),
        (
            AudioError::SampleRate { found: 0 },
            "a sound at 0 Hz: the mixer plays rates of 1 to 384000 Hz",
        ),
        (AudioError::TooLong, "a sound of more than 2^31 frames"),
        (AudioError::NoSamples, "a sound has no samples"),
        (
            AudioError::PartialFrame {
                samples: 3,
                channels: 2,
            },
            "3 samples are not a whole number of frames of 2 channels",
        ),
        (
            AudioError::OutOfRange { index: 7 },
            "sample 7 is outside -1 to 1, or not a number",
        ),
    ];
    for (error, text) in cases {
        assert_eq!(error.to_string(), text);
    }
    let decode = MusicData::new(b"qoaf".to_vec()).unwrap_err();
    assert!(decode.to_string().starts_with("QOA error"), "{decode}");
    assert!(std::error::Error::source(&decode).is_some());
}

#[test]
fn a_voice_fading_out_is_taken_before_any_other() {
    let (audio, mut mixer) = mixer_with(AudioConfig {
        voices: 2,
        ..AudioConfig::default()
    });
    let marked = Marked::new(3);
    let fading = marked.play(&audio, 0, 200);
    marked.play(&audio, 1, 3);
    render(&mut mixer, 1000);
    audio.stop(fading, 10_000);
    render(&mut mixer, 100);
    // Voice 1, of priority 3, could be taken too; voice 0, fading, goes.
    marked.play(&audio, 2, 4);
    assert_eq!(Marked::playing(&render(&mut mixer, 1000)), 0b110);
}

#[test]
fn a_voice_fading_out_is_not_one_of_its_sound_s_instances() {
    let mut lib = Library::new();
    let sound = lib.constant(0.25, 1000);
    let level = loaded(0.25);
    let params = PlayParams {
        max_instances: 1,
        pan: -1.0,
        ..looped()
    };
    let (audio, mut mixer) = fresh();
    let first = audio.play(&lib.server, sound, params);
    render(&mut mixer, 1000);
    audio.stop(first, 1000);
    render(&mut mixer, 100);
    audio.play(&lib.server, sound, params);
    let out = left(&render(&mut mixer, 1000));
    // The first goes on fading over its second, beside the second.
    let expected = level * (1.0 - 1100.0 / 48_000.0) + level;
    assert!(close(out[999], expected, 1e-3), "{}", out[999]);
}

#[test]
fn a_change_reaches_only_its_own_voice() {
    let mut lib = Library::new();
    let sound = lib.constant(0.25, 1000);
    let (audio, mut mixer) = fresh();
    let left_voice = audio.play(
        &lib.server,
        sound,
        PlayParams {
            pan: -1.0,
            ..looped()
        },
    );
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            pan: 1.0,
            ..looped()
        },
    );
    render(&mut mixer, 1000);
    audio.set_voice(left_voice, VoiceChange::Volume(0.0));
    let out = render(&mut mixer, 2000);
    assert!(out.chunks(2).all(|frame| frame[1] == loaded(0.25)));
    assert!(close(out[1999 * 2], 0.0, 1e-7));
}

#[test]
fn queued_sounds_are_let_go_once_the_mixer_is_gone() {
    let mut lib = Library::new();
    let sound = constant_in(&mut lib, Scope::Transient, 0.25, 1000);
    let data = Arc::downgrade(lib.get(sound).data());
    let (audio, mixer) = fresh();
    audio.play(&lib.server, sound, PlayParams::default());
    // The server lets go of it: the play holds the last reference.
    lib.unload(Scope::Transient);
    assert_eq!(data.strong_count(), 1, "only the queue holds the sound");
    drop(mixer);
    audio.update();
    assert_eq!(data.strong_count(), 0);
}

#[test]
fn underruns_are_counted_as_the_device_reports_them() {
    let (audio, mixer) = fresh();
    for _ in 0..3 {
        mixer.count_underrun();
    }
    assert_eq!((audio.stats().underruns, audio.stats().dropped), (3, 0));
}

#[test]
fn a_change_of_a_bus_s_volume_in_0_ms_takes_2() {
    let (audio, mut mixer) = fresh();
    let mut lib = Library::new();
    let sound = lib.constant(0.5, 1000);
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            pan: -1.0,
            ..looped()
        },
    );
    render(&mut mixer, 1000);
    audio.set_bus_volume(Bus::Sfx, 0.0, 0);
    let out = left(&render(&mut mixer, 200));
    for (k, &l) in out.iter().enumerate() {
        let expected = loaded(0.5) * (1.0 - (k + 1) as f32 / FADE_IN as f32).max(0.0);
        assert!(close(l, expected, 1e-6), "{k}: {l}");
    }
}

#[test]
fn a_voice_stolen_counts_twice_while_it_fades() {
    let (audio, mut mixer) = mixer_with(AudioConfig {
        voices: 1,
        ..AudioConfig::default()
    });
    let mut lib = Library::new();
    let sound = lib.constant(0.1, 1000);
    audio.play(&lib.server, sound, looped());
    render(&mut mixer, 300);
    let sound = lib.constant(0.1, 1000);
    audio.play(&lib.server, sound, looped());
    render_at(&mut mixer, 100, RATE, 100);
    assert_eq!(audio.stats().active_voices, 2);
    render(&mut mixer, 300);
    assert_eq!(audio.stats().active_voices, 1);
}

#[test]
fn a_voice_stopped_and_stolen_at_once_keeps_its_shorter_fade() {
    let (audio, mut mixer) = mixer_with(AudioConfig {
        voices: 1,
        ..AudioConfig::default()
    });
    let mut lib = Library::new();
    let sound = lib.constant(0.5, 1000);
    let first = audio.play(
        &lib.server,
        sound,
        PlayParams {
            pan: -1.0,
            ..looped()
        },
    );
    render(&mut mixer, 300);
    audio.stop(first, 0);
    let sound = lib.constant(0.5, 1000);
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            pan: 1.0,
            ..looped()
        },
    );
    let out = left(&render(&mut mixer, 300));
    // The new voice, hard right, leaves only `cos(π/2)` of itself on the
    // left.
    assert!(
        out[FADE_IN - 1..].iter().all(|&l| close(l, 0.0, 1e-7)),
        "{:?}",
        &out[90..110]
    );
}

#[test]
fn a_stop_keeps_the_sooner_end_of_two() {
    let (audio, mut mixer) = fresh();
    let mut lib = Library::new();
    let sound = lib.constant(0.5, 1000);
    let voice = audio.play(
        &lib.server,
        sound,
        PlayParams {
            pan: -1.0,
            ..looped()
        },
    );
    render(&mut mixer, 1000);
    // 50 ms, 2,400 samples; then a stop of 5 s does not lengthen it.
    audio.stop(voice, 50);
    let first = left(&render(&mut mixer, 1200));
    audio.stop(voice, 5000);
    let second = left(&render(&mut mixer, 1300));
    for (k, &l) in first.iter().chain(&second).enumerate() {
        let expected = loaded(0.5) * (1.0 - (k + 1) as f32 / 2400.0).max(0.0);
        assert!(close(l, expected, 1e-6), "{k}: {l}");
    }
    assert_eq!(audio.stats().active_voices, 0);
}

#[test]
fn a_volume_or_pitch_that_is_not_a_number_is_taken_safely() {
    let (audio, mut mixer) = fresh();
    let mut lib = Library::new();
    let sound = lib.constant(0.5, 1000);
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            volume: f32::NAN,
            pan: -1.0,
            ..looped()
        },
    );
    let sound = lib.constant(0.5, 1000);
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            pan: 1.0,
            ..looped()
        },
    );
    let out = render(&mut mixer, 300);
    assert!(close(out[299 * 2], 0.0, 1e-7) && out[299 * 2 + 1] == loaded(0.5));
    // A pitch of 0 or less, or not a number, plays at 1: a ramp in whole
    // 16-bit steps at 48 kHz, rendered at 44.1 kHz, so that the step is
    // the rates' ratio times the pitch, not the pitch alone.
    let ramp: Vec<i16> = (0..1000).map(|n| n * 3).collect();
    let sound = lib.sound(1, RATE, &ramp);
    assert_ne!(
        lib.get(sound).data().sample_rate(),
        44_100,
        "the mixer resamples it"
    );
    let play = |pitch| {
        let (audio, mut mixer) = fresh();
        audio.play(
            &lib.server,
            sound,
            PlayParams {
                pitch,
                ..PlayParams::default()
            },
        );
        render_at(&mut mixer, 1200, 44_100, 256)
    };
    let one = play(1.0);
    for pitch in [0.0, -3.0, f32::NAN] {
        assert_eq!(play(pitch), one, "pitch {pitch}");
    }
}

#[test]
fn a_resampled_sound_ends_toward_silence() {
    // 200 frames at 48 kHz, played at 96 kHz: every other frame out falls
    // halfway between two in, and the last halfway to silence.
    let mut lib = Library::new();
    let sound = lib.constant(0.5, 200);
    assert_ne!(
        lib.get(sound).data().sample_rate(),
        96_000,
        "the mixer resamples it"
    );
    let (audio, mut mixer) = fresh();
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            pan: -1.0,
            ..PlayParams::default()
        },
    );
    let out = left(&render_at(&mut mixer, 500, 96_000, 256));
    let level = loaded(0.5);
    assert_eq!((out[397], out[398], out[399]), (level, level, level / 2.0));
    assert!(out[400..].iter().all(|&l| l == 0.0));
}

#[test]
fn stop_music_silences_every_track_within_its_fade() {
    let mut lib = Library::new();
    let (a, b) = (
        level_in(&mut lib, Scope::Transient, 0.3, 100_000),
        level_in(&mut lib, Scope::Transient, -0.2, 100_000),
    );
    let (held_a, held_b) = (
        Arc::downgrade(lib.get_track(a).data()),
        Arc::downgrade(lib.get_track(b).data()),
    );
    let (audio, mut mixer) = fresh();
    audio.play_music(&lib.server, a, 0);
    render(&mut mixer, 1000);
    audio.play_music(&lib.server, b, 1000);
    // The server lets go of them: only the mixer holds the first, and the
    // queue the second.
    lib.unload(Scope::Transient);
    assert_eq!((held_a.strong_count(), held_b.strong_count()), (1, 1));
    render(&mut mixer, 1000);
    // 50 ms: the track still fading out of the crossfade goes as fast.
    audio.stop_music(50);
    let out = left(&render(&mut mixer, 3000));
    assert!(out[2400..].iter().all(|&l| l == 0.0), "{}", out[2400]);
    render(&mut mixer, 256);
    audio.update();
    assert_eq!((held_a.strong_count(), held_b.strong_count()), (0, 0));
}

#[test]
fn a_third_track_in_a_crossfade_takes_the_quieter_voice_2_ms_later() {
    let mut lib = Library::new();
    let tracks = [0.25, -0.125, 0.1].map(|value| {
        let music = level(&mut lib, value, 20_000);
        let samples: Vec<f64> = decoded(&lib, music)
            .iter()
            .map(|&x| f64::from(x) / 32768.0)
            .collect();
        (music, samples)
    });
    let [(a, sa), (b, sb), (c, sc)] = &tracks;
    let (audio, mut mixer) = fresh();
    audio.play_music(&lib.server, *a, 0);
    render(&mut mixer, 1000);
    // B fades in over 100 ms, A out; 50 frames in, C, with a crossfade of
    // 100 ms too: B, the quieter, falls silent over 2 ms, not 100, and C
    // starts there, fading in over 100 ms.
    audio.play_music(&lib.server, *b, 100);
    render_at(&mut mixer, 50, RATE, 50);
    audio.play_music(&lib.server, *c, 100);
    let out = left(&render(&mut mixer, 400));
    for (k, &l) in out.iter().enumerate() {
        let m = 1050 + k;
        let a = sa[m] * (1.0 - (m - 999) as f64 / 4800.0);
        let b = if k < 96 {
            sb[m - 1000] * (50.0 / 4800.0) * (1.0 - (k + 1) as f64 / 96.0)
        } else {
            0.0
        };
        let c = if k >= 96 {
            sc[k - 96] * ((k - 95) as f64 / 4800.0).min(1.0)
        } else {
            0.0
        };
        let expected = a + b + c;
        assert!(
            (f64::from(l) - expected).abs() < 1e-5,
            "{k}: {l} not {expected}"
        );
    }
}

#[test]
fn a_track_is_checked_to_its_last_frame() {
    let header = qoa::Header {
        channels: 1,
        sample_rate: RATE,
        samples: qoa::FRAME_LEN as u32 + 10,
    };
    let mut file = qoa::encode(&header, &vec![0; qoa::FRAME_LEN + 10]).expect("a valid track");
    // The second frame's channels: after the header and a whole first
    // frame of 256 slices.
    file[8 + 8 + 16 + 256 * 8] = 2;
    assert!(matches!(MusicData::new(file), Err(AudioError::Decode(_))));
}

#[test]
fn a_fade_is_never_shorter_than_it_says() {
    // 2 ms at 44.1 kHz is 88.2 samples: 89, not 88. The sound is at
    // 48 kHz, as sounds load: the fade counts the output's samples.
    let mut lib = Library::new();
    let sound = lib.constant(0.5, 1000);
    assert_ne!(
        lib.get(sound).data().sample_rate(),
        44_100,
        "the mixer resamples it"
    );
    let (audio, mut mixer) = fresh();
    audio.play(
        &lib.server,
        sound,
        PlayParams {
            pan: -1.0,
            ..looped()
        },
    );
    let out = left(&render_at(&mut mixer, 200, 44_100, 100));
    for (k, &l) in out.iter().enumerate() {
        let expected = loaded(0.5) * ((k + 1) as f32 / 89.0).min(1.0);
        assert!(close(l, expected, 1e-6), "{k}: {l} not {expected}");
    }
}
