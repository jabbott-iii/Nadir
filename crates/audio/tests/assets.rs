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

//! The sound and music assets: files written as the cook writes them,
//! loaded through the asset server on its workers, and played through the
//! mixer offline; their conversion to the load rate, their limits, their
//! refusals, their placeholders, and what a handle with nothing under it
//! plays.

mod common;

use std::sync::Arc;

use nadir_assets::{Asset, AssetError, DecodeCtx, FinalizeCtx, LoadState, OnFailure, Scope};
use nadir_audio::{
    Audio, AudioConfig, AudioError, LOAD_RATE, Mixer, Music, MusicData, PlayParams, Sound,
    SoundData, VoiceChange,
};
use nadir_codec::{CodecError, Limits, kpcm, pcm, qoa};
use nadir_core::{Vec2, log};
use nadir_platform::AudioOutputConfig;

use common::Library;

const RATE: u32 = 48_000;

fn fresh() -> (Audio, Mixer) {
    nadir_audio::new(AudioConfig::default()).expect("the default config")
}

/// Renders `frames` stereo frames at [`RATE`] in callbacks of 256 frames.
fn render(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    for part in out.chunks_mut(512) {
        mixer.render(part, 2, RATE);
    }
    out
}

fn looped() -> PlayParams {
    PlayParams {
        looped: true,
        ..PlayParams::default()
    }
}

/// `frames` frames of `channels`, 16-bit: a tone of `hz` at `rate`, each
/// channel an octave above the one before, below full scale.
fn tone(channels: u8, rate: u32, frames: usize, hz: f64) -> Vec<i16> {
    (0..frames)
        .flat_map(|n| {
            (0..channels).map(move |c| {
                let t = n as f64 / f64::from(rate);
                (0.5 * (std::f64::consts::TAU * hz * f64::from(1 << c) * t).sin() * 32767.0) as i16
            })
        })
        .collect()
}

/// `samples` with a quiet end, as a sound that does not loop has: 2 ms of
/// silence, and 36 frames at least, after them. Only such a sound is
/// converted as it loads.
fn quiet_end(mut samples: Vec<i16>, channels: u8, rate: u32) -> Vec<i16> {
    let frames = (rate as usize / 500).max(36);
    samples.extend(std::iter::repeat_n(0, frames * usize::from(channels)));
    samples
}

/// `samples` converted as a loading sound is: as a sound that does not
/// loop, then clamped.
fn converted(samples: &[f32], channels: u8, rate: u32) -> Vec<f32> {
    pcm::resample(
        samples,
        usize::from(channels),
        rate,
        LOAD_RATE,
        false,
        Limits::default(),
    )
    .expect("a resample")
    .into_iter()
    .map(|x| x.clamp(-1.0, 1.0))
    .collect()
}

/// The largest step from a frame to the next of `channel`, played looped
/// past the end of `out`'s first loop.
fn largest_step(out: &[f32], channel: usize) -> f32 {
    let side: Vec<f32> = out.iter().skip(channel).step_by(2).copied().collect();
    side.windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max)
}

fn as_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&x| f32::from(x) / 32768.0).collect()
}

fn kpcm_file(channels: u8, rate: u32, samples: &[i16]) -> Vec<u8> {
    let header = kpcm::Header {
        channels,
        sample_rate: rate,
        frames: (samples.len() / usize::from(channels)) as u32,
    };
    kpcm::encode(&header, samples).expect("a sound file")
}

fn qoa_file(channels: u8, rate: u32, samples: &[i16]) -> Vec<u8> {
    let header = qoa::Header {
        channels,
        sample_rate: rate,
        samples: (samples.len() / usize::from(channels)) as u32,
    };
    qoa::encode(&header, samples).expect("a QOA file")
}

#[test]
fn cooked_sounds_and_music_load_through_the_server_and_play_through_the_mixer() {
    let mut lib = Library::new();
    let step = tone(1, RATE, 2400, 440.0);
    let chime = tone(2, RATE, 3000, 660.0);
    let theme = tone(2, RATE, 3 * qoa::FRAME_LEN + 100, 220.0);
    lib.files
        .insert("audio/sfx/step.kpcm", kpcm_file(1, RATE, &step));
    lib.files
        .insert("audio/sfx/chime.qoa", qoa_file(2, RATE, &chime));
    lib.files
        .insert("audio/music/theme.qoa", qoa_file(2, RATE, &theme));
    // On the workers, as a game loads them, and finished on this thread.
    let step_handle = lib.server.load::<Sound>("audio/sfx/step.kpcm");
    let chime_handle = lib.server.load::<Sound>("audio/sfx/chime.qoa");
    let theme_handle = lib.server.load::<Music>("audio/music/theme.qoa");
    assert_eq!(lib.server.state(step_handle), Some(LoadState::Loading));
    lib.server.finish_all_loads(&mut FinalizeCtx::new());
    for state in [
        lib.server.state(step_handle),
        lib.server.state(chime_handle),
        lib.server.state(theme_handle),
    ] {
        assert_eq!(state, Some(LoadState::Loaded));
    }

    // A sound file plays its samples as they are; a QOA sound, the samples
    // QOA decodes.
    let step_data = lib.get(step_handle).data();
    assert_eq!((step_data.channels(), step_data.sample_rate()), (1, RATE));
    assert_eq!(step_data.samples(), as_f32(&step));
    let chime_file = qoa_file(2, RATE, &chime);
    let decoded = qoa::decode(&chime_file, Limits::default()).expect("the file");
    let chime_data = lib.get(chime_handle).data();
    assert_eq!((chime_data.channels(), chime_data.sample_rate()), (2, RATE));
    assert_eq!(chime_data.samples(), as_f32(&decoded.samples));
    let theme_file = qoa_file(2, RATE, &theme);
    assert_eq!(lib.get_track(theme_handle).data().bytes(), theme_file);

    // Played from their handles, they mix as the same sounds made in code.
    let (audio, mut mixer) = fresh();
    audio.play(&lib.server, step_handle, PlayParams::default());
    audio.play_at(
        &lib.server,
        chime_handle,
        Vec2::new(150.0, 0.0),
        PlayParams::default(),
    );
    audio.play_music(&lib.server, theme_handle, 50);
    let out = render(&mut mixer, 6000);
    assert!(out.iter().any(|&x| x != 0.0));

    let (reference, mut reference_mixer) = fresh();
    let step_sound =
        Sound::new(SoundData::new(1, RATE, as_f32(&step)).expect("the step's samples"));
    let chime_sound =
        Sound::new(SoundData::new(2, RATE, as_f32(&decoded.samples)).expect("the chime's samples"));
    let theme_music = Music::new(MusicData::new(theme_file).expect("the theme"));
    reference.play_sound(&step_sound, PlayParams::default());
    reference.play_sound_at(&chime_sound, Vec2::new(150.0, 0.0), PlayParams::default());
    reference.play_track(&theme_music, 50);
    assert_eq!(out, render(&mut reference_mixer, 6000));
    assert_eq!(audio.stats().unplayed, 0);
}

#[test]
fn a_sound_at_another_rate_loads_at_the_load_rate_as_the_codec_resamples_it() {
    assert_eq!(LOAD_RATE, AudioOutputConfig::default().sample_rate);
    let mut lib = Library::new();
    for (channels, rate) in [
        (1, 22_050),
        (2, 44_100),
        (1, 8_000),
        (2, 96_000),
        (2, 384_000),
    ] {
        let samples = quiet_end(
            tone(channels, rate, rate as usize / 20, 440.0),
            channels,
            rate,
        );
        let handle = lib.sound(channels, rate, &samples);
        let data = lib.get(handle).data();
        assert_eq!(
            (data.channels(), data.sample_rate()),
            (channels, LOAD_RATE),
            "{rate} Hz"
        );
        assert_eq!(
            data.samples(),
            converted(&as_f32(&samples), channels, rate),
            "{rate} Hz"
        );
    }
    // A QOA sound: the samples QOA decodes, converted. QOA's error rings
    // into the silence after a tone, so it ends in 20 ms of it, as a sound
    // that fades out does.
    for (channels, rate) in [(1, 22_050), (2, 44_100)] {
        let mut samples = tone(channels, rate, rate as usize / 20, 330.0);
        samples.extend(vec![0; usize::from(channels) * rate as usize / 50]);
        let file = qoa_file(channels, rate, &samples);
        let path = format!("audio/sfx/qoa-{rate}.qoa");
        lib.files.insert(&path, file.clone());
        let handle = lib
            .server
            .load_now::<Sound>(&path, &mut FinalizeCtx::new())
            .expect("a sound");
        let decoded = qoa::decode(&file, Limits::default())
            .expect("the file")
            .samples;
        let data = lib.get(handle).data();
        assert_eq!(
            (data.channels(), data.sample_rate()),
            (channels, LOAD_RATE),
            "{rate} Hz"
        );
        assert_eq!(
            data.samples(),
            converted(&as_f32(&decoded), channels, rate),
            "{rate} Hz"
        );
    }
    // At the load rate, below 8,000 Hz, and as a sound made in code, a
    // sound keeps its rate and its samples, though it ends in quiet; so
    // does one whose quiet end, 2.5 ms at 8 kHz, is shorter than the
    // resampler's reach of 36 frames.
    let ramp = |frames: i32| -> Vec<i16> {
        (0..frames)
            .map(|i| (i * 97 % 20_000 - 10_000) as i16)
            .collect()
    };
    for rate in [LOAD_RATE, 7_999, 1] {
        let samples = quiet_end(ramp(300), 1, rate);
        let handle = lib.sound(1, rate, &samples);
        let data = lib.get(handle).data();
        assert_eq!(data.sample_rate(), rate);
        assert_eq!(data.samples(), as_f32(&samples), "{rate} Hz");
    }
    let mut short_gap = ramp(300);
    short_gap.extend([0; 20]);
    let handle = lib.sound(1, 8_000, &short_gap);
    assert_eq!(lib.get(handle).data().sample_rate(), 8_000);
    let made = Sound::new(SoundData::new(1, 22_050, vec![0.25; 10]).expect("a sound"));
    assert_eq!(made.data().sample_rate(), 22_050);
}

#[test]
fn a_sound_that_ends_loud_may_be_a_loop_and_keeps_its_rate_and_its_seam() {
    let mut lib = Library::new();
    // Seamless loops at 22,050 Hz: 50 turns of 441 Hz, one cut where it
    // ends loud, one cut at a zero, whose last 2 ms are loud still.
    let turns = |shift: usize| -> Vec<i16> {
        (0..2500)
            .map(|i| {
                let phase = (i + shift) as f64 * 441.0 / 22_050.0;
                (0.5 * (std::f64::consts::TAU * phase).sin() * 32767.0) as i16
            })
            .collect()
    };
    for (cut, samples) in [("loud", turns(0)), ("at a zero", turns(1))] {
        let last = *samples.last().expect("samples");
        assert_eq!(last == 0, cut == "at a zero", "cut {cut}: {last}");
        assert!(
            samples[samples.len() - 44..]
                .iter()
                .any(|x| x.unsigned_abs() > 128)
        );
        let handle = lib.sound(1, 22_050, &samples);
        let data = lib.get(handle).data();
        assert_eq!(data.sample_rate(), 22_050, "cut {cut}");
        assert_eq!(data.samples(), as_f32(&samples), "cut {cut}");
        // Looped, it steps across its seam as across any other point.
        let (audio, mut mixer) = fresh();
        audio.play(
            &lib.server,
            handle,
            PlayParams {
                pan: -1.0,
                ..looped()
            },
        );
        let out = render(&mut mixer, 5443 + 400);
        let tone = largest_step(&out[200 * 2..5000 * 2], 0);
        let seam = largest_step(&out[5000 * 2..5843 * 2], 0);
        assert!(
            seam <= tone * 1.01,
            "cut {cut}: {seam} at the seam, {tone} elsewhere"
        );
    }
}

#[test]
fn a_converted_sound_past_full_scale_is_clipped_and_told() {
    // A square wave at full scale rings past it, resampled.
    let mut lib = Library::new();
    let square: Vec<i16> = (0..2205)
        .map(|i| if (i / 25) % 2 == 0 { 32767 } else { -32768 })
        .collect();
    let square = quiet_end(square, 1, 22_050);
    let path = "audio/sfx/clipped-square.kpcm";
    lib.files.insert(path, kpcm_file(1, 22_050, &square));
    let handle = lib
        .server
        .load_now::<Sound>(path, &mut FinalizeCtx::new())
        .expect("a sound");
    let samples = lib.get(handle).data().samples();
    assert!(samples.iter().all(|x| x.abs() <= 1.0));
    assert!(samples.iter().any(|x| x.abs() == 1.0));
    assert!(
        log::tail(log::RING_CAPACITY)
            .iter()
            .any(|line| line.contains(path) && line.contains("clipped")),
        "no warning names the file"
    );
}

#[test]
fn a_file_the_mixer_cannot_play_fails_to_load_and_says_why() {
    let mut lib = Library::new();
    let three = qoa_file(3, RATE, &[0; 300]);
    let fast = qoa_file(1, 500_000, &[0; 100]);
    let step = kpcm_file(1, RATE, &[0; 100]);
    let theme = qoa_file(1, RATE, &[0; 100]);
    for (path, bytes) in [
        ("audio/sfx/three.qoa", &three),
        ("audio/sfx/fast.qoa", &fast),
        ("audio/music/three.qoa", &three),
        ("audio/music/fast.qoa", &fast),
        ("audio/sfx/cut.kpcm", &step[..step.len() - 1].to_vec()),
        ("audio/music/cut.qoa", &theme[..theme.len() - 1].to_vec()),
    ] {
        lib.files.insert(path, bytes.clone());
    }
    let sounds = [
        lib.server.load::<Sound>("audio/sfx/three.qoa"),
        lib.server.load::<Sound>("audio/sfx/fast.qoa"),
        lib.server.load::<Sound>("audio/sfx/cut.kpcm"),
        lib.server.load::<Sound>("audio/sfx/step.wav"),
    ];
    let tracks = [
        lib.server.load::<Music>("audio/music/three.qoa"),
        lib.server.load::<Music>("audio/music/fast.qoa"),
        lib.server.load::<Music>("audio/music/cut.qoa"),
        lib.server.load::<Music>("audio/music/step.kpcm"),
    ];
    lib.server.finish_all_loads(&mut FinalizeCtx::new());
    let channels = AssetError::Invalid(AudioError::Channels { found: 3 }.to_string());
    let rate = AssetError::Invalid(AudioError::SampleRate { found: 500_000 }.to_string());
    for (handle, expected) in sounds.iter().zip([&channels, &rate]) {
        assert_eq!(lib.server.state(*handle), Some(LoadState::Failed));
        assert_eq!(lib.server.error(*handle), Some(expected));
    }
    for (handle, expected) in tracks.iter().zip([&channels, &rate]) {
        assert_eq!(lib.server.state(*handle), Some(LoadState::Failed));
        assert_eq!(lib.server.error(*handle), Some(expected));
    }
    assert!(matches!(
        lib.server.error(sounds[2]),
        Some(AssetError::Decode(CodecError::Kpcm { .. }))
    ));
    assert!(matches!(
        lib.server.error(tracks[2]),
        Some(AssetError::Decode(CodecError::Qoa { .. }))
    ));
    // A sound is a `.kpcm` or a `.qoa`; music, a `.qoa`.
    assert_eq!(
        lib.server.error(sounds[3]),
        Some(&AssetError::Extension {
            expected: &["kpcm", "qoa"]
        })
    );
    assert_eq!(
        lib.server.error(tracks[3]),
        Some(&AssetError::Extension { expected: &["qoa"] })
    );
}

#[test]
fn a_sound_is_held_to_the_limits_at_four_bytes_a_sample_and_a_track_at_its_file() {
    let limit = |max_decoded_bytes| Limits {
        max_decoded_bytes,
        ..Limits::default()
    };
    let path = "audio/sfx/a.kpcm";
    // 1,000 samples take 4,000 bytes as `f32`.
    let file = kpcm_file(1, RATE, &[100; 1000]);
    assert!(Sound::decode(&file, &DecodeCtx::new(path, limit(4000))).is_ok());
    assert!(matches!(
        Sound::decode(&file, &DecodeCtx::new(path, limit(3999))),
        Err(AssetError::Decode(CodecError::Kpcm { .. }))
    ));
    let qoa_path = "audio/sfx/a.qoa";
    let qoa_sound = qoa_file(1, RATE, &[100; 1000]);
    assert!(Sound::decode(&qoa_sound, &DecodeCtx::new(qoa_path, limit(4000))).is_ok());
    assert!(matches!(
        Sound::decode(&qoa_sound, &DecodeCtx::new(qoa_path, limit(3999))),
        Err(AssetError::Decode(CodecError::Qoa { .. }))
    ));
    // Converted from 22,050 Hz, 1,000 frames become 2,177: 8,708 bytes.
    let low = kpcm_file(1, 22_050, &[100; 1000]);
    assert!(Sound::decode(&low, &DecodeCtx::new(path, limit(8708))).is_ok());
    match Sound::decode(&low, &DecodeCtx::new(path, limit(8707))) {
        Err(AssetError::Invalid(why)) => {
            assert!(
                why.starts_with("converting it from 22050 Hz to 48000 Hz"),
                "{why}"
            );
        }
        other => panic!("{other:?}"),
    }
    // QOA's channels are refused from its header, before anything is
    // decoded: so even when its samples would pass the limits.
    let three = qoa_file(3, RATE, &[100; 3000]);
    assert_eq!(
        Sound::decode(&three, &DecodeCtx::new(qoa_path, limit(100))).err(),
        Some(AssetError::Invalid(
            AudioError::Channels { found: 3 }.to_string()
        ))
    );
    // A track is held to its file's size, before it is copied.
    let theme = qoa_file(1, RATE, &[100; 1000]);
    let music_path = "audio/music/theme.qoa";
    assert!(Music::decode(&theme, &DecodeCtx::new(music_path, limit(theme.len()))).is_ok());
    assert_eq!(
        Music::decode(&theme, &DecodeCtx::new(music_path, limit(theme.len() - 1))),
        Err(AssetError::Invalid(format!(
            "it takes {} bytes, more than the limit of {}",
            theme.len(),
            theme.len() - 1
        )))
    );
}

#[test]
fn with_placeholders_a_failed_sound_plays_as_silence_and_a_failed_track_fades_the_music_out() {
    let mut lib = Library::with(OnFailure::Placeholder);
    let missing = lib.server.load::<Sound>("audio/sfx/missing.kpcm");
    let other = lib.server.load::<Sound>("audio/sfx/other.kpcm");
    let missing_track = lib.server.load::<Music>("audio/music/missing.qoa");
    lib.server.finish_all_loads(&mut FinalizeCtx::new());
    assert_eq!(lib.server.state(missing), Some(LoadState::Failed));
    // A placeholder for each, so that each counts its own instances.
    assert!(!Arc::ptr_eq(lib.get(missing).data(), lib.get(other).data()));
    assert_eq!(lib.server.state(missing_track), Some(LoadState::Failed));
    let silence = lib.get(missing).data();
    assert_eq!(
        (silence.channels(), silence.sample_rate(), silence.samples()),
        (1, LOAD_RATE, &[0.0][..])
    );
    let header = *lib.get_track(missing_track).data().header();
    assert_eq!(
        header,
        qoa::Header {
            channels: 1,
            sample_rate: LOAD_RATE,
            samples: qoa::FRAME_LEN as u32
        }
    );

    let theme = lib.track(1, RATE, &tone(1, RATE, 48_000, 220.0));
    let (audio, mut mixer) = fresh();
    let voice = audio.play(&lib.server, missing, looped());
    audio.play_music(&lib.server, theme, 2);
    let out = render(&mut mixer, 4800);
    assert_eq!(audio.stats().active_voices, 1, "the silence holds a voice");
    assert!(out.iter().any(|&x| x.abs() > 0.1), "the theme plays");
    // The failed track: the music fades out, to its silence.
    audio.play_music(&lib.server, missing_track, 10);
    render(&mut mixer, 4800);
    let out = render(&mut mixer, 4800);
    assert!(
        out.iter().all(|&x| x.abs() < 1e-4),
        "{:?}",
        out.iter().copied().fold(0.0f32, |a, x| a.max(x.abs()))
    );
    assert_eq!(audio.stats().unplayed, 0);
    audio.stop(voice, 2);
    render(&mut mixer, 480);
    assert_eq!(audio.stats().active_voices, 0);
}

#[test]
fn a_handle_with_nothing_under_it_plays_nothing_and_is_counted() {
    let mut lib = Library::new();
    // Failed, with no placeholder.
    let missing = lib.server.load::<Sound>("audio/sfx/missing.kpcm");
    let missing_track = lib.server.load::<Music>("audio/music/missing.qoa");
    lib.server.finish_all_loads(&mut FinalizeCtx::new());
    assert_eq!(lib.server.state(missing), Some(LoadState::Failed));
    // Unloaded.
    let gone = lib.sound_in(Scope::Level, 1, RATE, &[16_000; 1000]);
    lib.unload(Scope::Level);
    assert_eq!(lib.server.state(gone), None);
    let theme = lib.track(1, RATE, &tone(1, RATE, 9600, 220.0));
    // Still loading: on a worker, not finished.
    lib.files
        .insert("audio/sfx/later.kpcm", kpcm_file(1, RATE, &[16_000; 1000]));
    let later = lib.server.load::<Sound>("audio/sfx/later.kpcm");
    assert_eq!(lib.server.state(later), Some(LoadState::Loading));

    let hum = lib.constant(0.25, 1000);
    let (audio, mut mixer) = fresh();
    audio.play_music(&lib.server, theme, 2);
    let real = audio.play(&lib.server, hum, looped());
    let mut ids = vec![real];
    for handle in [missing, gone, later] {
        let voice = audio.play(&lib.server, handle, looped());
        // It names no voice, the real one least: changing and stopping it
        // do nothing.
        audio.set_voice(voice, VoiceChange::Volume(2.0));
        audio.stop(voice, 2);
        ids.push(voice);
    }
    ids.push(audio.play_at(&lib.server, missing, Vec2::new(0.0, 0.0), looped()));
    audio.play_music(&lib.server, missing_track, 2);
    let out = render(&mut mixer, 4800);
    assert_eq!(audio.stats().unplayed, 5);
    assert_eq!(audio.stats().active_voices, 1);
    // Each names a voice of its own, never used again.
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "{ids:?}");
    // The theme and the hum play as they would alone.
    let (alone, mut alone_mixer) = fresh();
    alone.play_music(&lib.server, theme, 2);
    alone.play(&lib.server, hum, looped());
    assert_eq!(out, render(&mut alone_mixer, 4800));
    lib.server.finish_all_loads(&mut FinalizeCtx::new());
}

#[test]
fn the_same_track_loaded_again_goes_on_as_it_is() {
    let mut lib = Library::new();
    let samples = tone(2, RATE, 3 * qoa::FRAME_LEN, 220.0);
    let first = lib.track_in(Scope::Level, 2, RATE, &samples);
    let path = lib.server.path(first).expect("a path").to_string();
    let (audio, mut mixer) = fresh();
    audio.play_music(&lib.server, first, 100);
    let mut out = render(&mut mixer, 4800);
    // The next level loads the same file again, a new asset.
    lib.unload(Scope::Level);
    let again = lib
        .server
        .load_now_in::<Music>(&path, Scope::Level, &mut FinalizeCtx::new())
        .expect("the track");
    assert_ne!(again, first);
    let weak = Arc::downgrade(lib.get_track(again).data());
    audio.play_music(&lib.server, again, 100);
    out.extend(render(&mut mixer, 4800));
    // The mixer handed the second load back, to be dropped here.
    assert_eq!(weak.strong_count(), 2);
    audio.update();
    assert_eq!(weak.strong_count(), 1, "only the server holds it");
    // As if it had been played once.
    let (once, mut once_mixer) = fresh();
    once.play_music(&lib.server, again, 100);
    assert_eq!(out, render(&mut once_mixer, 9600));
}

#[test]
fn the_same_track_loaded_again_while_it_waits_to_start_goes_on_as_it_is() {
    // A third track in a crossfade waits 2 ms for the quieter voice: the
    // same file loaded again meanwhile changes nothing.
    let mut lib = Library::new();
    let a = lib.track(1, RATE, &tone(1, RATE, 48_000, 220.0));
    let b = lib.track(1, RATE, &tone(1, RATE, 48_000, 330.0));
    let samples = tone(1, RATE, 48_000, 495.0);
    let c = lib.track(1, RATE, &samples);
    let again = lib.track(1, RATE, &samples);
    assert_ne!(c, again);
    let run = |twice: bool| -> Vec<f32> {
        let (audio, mut mixer) = fresh();
        audio.play_music(&lib.server, a, 50);
        let mut out = render(&mut mixer, 4800);
        audio.play_music(&lib.server, b, 100);
        out.extend(render(&mut mixer, 480));
        audio.play_music(&lib.server, c, 100);
        if twice {
            // Even with a crossfade of its own: C goes on as it is.
            audio.play_music(&lib.server, again, 10);
        }
        out.extend(render(&mut mixer, 9600));
        out
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn a_sound_reloaded_plays_its_new_samples_from_its_next_play() {
    let mut lib = Library::new();
    let path = "audio/sfx/blip.kpcm";
    lib.files.insert(path, kpcm_file(1, RATE, &[8192; 1000]));
    let blip = lib
        .server
        .load_now::<Sound>(path, &mut FinalizeCtx::new())
        .expect("a sound");
    let (audio, mut mixer) = fresh();
    let old = audio.play(&lib.server, blip, looped());
    render(&mut mixer, 480);
    lib.files.insert(path, kpcm_file(1, RATE, &[16_384; 1000]));
    assert_eq!(lib.server.reload(path), 1);
    lib.server.finish_all_loads(&mut FinalizeCtx::new());
    assert_eq!(
        lib.server.drain_swapped::<Sound>().collect::<Vec<_>>(),
        [blip]
    );
    assert_eq!(lib.get(blip).data().samples()[0], 0.5);
    // The old voice plays on until it stops; the next plays the new.
    let out = render(&mut mixer, 480);
    assert!((out[200 * 2] - 0.25 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    audio.stop(old, 2);
    audio.play(&lib.server, blip, looped());
    let out = render(&mut mixer, 480);
    assert!((out[400 * 2] - 0.5 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
}
