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

//! The mixer's benchmark: a callback of 32 voices and a music track.

use nadir_audio::{AudioConfig, Music, MusicData, PlayParams, Sound, SoundData};
use nadir_codec::qoa;
use nadir_core::{Pcg32, Vec2};

use crate::harness::Bencher;

/// The device's rate.
const RATE: u32 = 48_000;

/// The frames of a callback: 10.7 ms at [`RATE`], a common period.
const FRAMES: usize = 512;

/// `mixer_32_voices`: one callback of [`FRAMES`] stereo frames with 32
/// voices looping and a QOA track streaming. A quarter of the voices each
/// are mono at the device's rate, mono at a position and pitched up 10%,
/// stereo at 44.1 kHz (resampled), and mono at 22.05 kHz at a position, so
/// that most voices resample and half are panned by position; the track is
/// stereo at the device's rate. A callback has 10.7 ms to be ready.
pub(crate) fn voices(bencher: &mut Bencher) {
    let mut rng = Pcg32::new(0x6d69_7865, 0);
    let mut noise = |len: usize| -> Vec<f32> { (0..len).map(|_| rng.range(-0.5..0.5)).collect() };
    // Not handles: a loaded sound is converted to 48 kHz, and these must keep their rates.
    let sounds = [
        SoundData::new(1, RATE, noise(48_000)),
        SoundData::new(1, RATE, noise(30_000)),
        SoundData::new(2, 44_100, noise(44_100 * 2)),
        SoundData::new(1, 22_050, noise(22_050)),
    ]
    .map(|sound| Sound::new(sound.expect("a valid sound")));
    let (audio, mut mixer) =
        nadir_audio::new(AudioConfig::default()).expect("the default config is valid");
    audio.set_listener(Vec2::new(400.0, 300.0));
    for voice in 0..32 {
        let kind = voice % 4;
        let params = PlayParams {
            looped: true,
            volume: 0.03,
            pitch: if kind == 1 { 1.1 } else { 1.0 },
            max_instances: 8,
            ..PlayParams::default()
        };
        let sound = &sounds[kind];
        if kind % 2 == 1 {
            let at = Vec2::new(voice as f32 * 25.0, 300.0 + voice as f32 * 5.0);
            audio.play_sound_at(sound, at, params);
        } else {
            audio.play_sound(sound, params);
        }
    }
    audio.play_track(&track(), 0);
    let mut out = vec![0.0; FRAMES * 2];
    // The commands, and the music's first frame.
    mixer.render(&mut out, 2, RATE);
    assert_eq!(audio.stats().active_voices, 32, "every voice plays");
    bencher.iter(|| {
        mixer.render(&mut out, 2, RATE);
        out[0]
    });
}

/// A track of 3 s of stereo noise at [`RATE`], below −6 dBFS.
fn track() -> Music {
    let mut rng = Pcg32::new(0x6d75_7369, 0);
    let frames = 3 * RATE;
    let samples: Vec<i16> = (0..frames * 2)
        .map(|_| rng.range(-12_000..12_000i32) as i16)
        .collect();
    let header = qoa::Header {
        channels: 2,
        sample_rate: RATE,
        samples: frames,
    };
    let file = qoa::encode(&header, &samples).expect("the track's header is valid");
    Music::new(MusicData::new(file).expect("a valid track"))
}
